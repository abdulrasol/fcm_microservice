//! Generic JWT identity provider (Supabase, Auth0, Keycloak, a custom backend).
//!
//! The algorithm is pinned by the configuration: the token header must name one
//! of the configured algorithms, signing keys are never taken from the token
//! header, and an HS256 secret is read from the environment only.

use super::jwks::{cached_key, JwkProvider, JwksCache, JwksError, MAX_KID_LENGTH};
use super::{
    claim_at, uid_from_claim, Identity, IdentityError, IdentityProvider, MAX_TOKEN_LENGTH,
};
use crate::config::JwtAlgorithm;
use async_trait::async_trait;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde_json::Value;
use std::sync::{Arc, Mutex};

const CLOCK_LEEWAY_SECONDS: u64 = 60;
const MIN_HS256_SECRET_BYTES: usize = 32;

/// Reads an environment variable by name.
///
/// Tests provide their own implementation instead of changing the process
/// environment, so they stay independent of each other.
pub trait EnvSource: Send + Sync {
    fn get(&self, name: &str) -> Option<String>;
}

/// The real process environment.
pub struct SystemEnv;

impl EnvSource for SystemEnv {
    fn get(&self, name: &str) -> Option<String> {
        std::env::var(name).ok().filter(|value| !value.is_empty())
    }
}

/// The JWT settings of an app's identity configuration.
#[derive(Debug, Clone)]
pub struct JwtProviderConfig {
    pub algorithms: Vec<JwtAlgorithm>,
    pub jwks_url: Option<String>,
    pub hs256_secret_env: Option<String>,
    pub issuer: String,
    pub audience: String,
    pub uid_claim: String,
    pub name_claim: Option<String>,
    pub max_age_seconds: u64,
}

pub struct JwtProvider {
    config: JwtProviderConfig,
    env: Arc<dyn EnvSource>,
    jwks: Arc<dyn JwkProvider>,
    cache: Mutex<JwksCache>,
}

impl JwtProvider {
    pub fn new(
        config: &JwtProviderConfig,
        env: Arc<dyn EnvSource>,
        jwks: Arc<dyn JwkProvider>,
    ) -> Self {
        Self {
            config: config.clone(),
            env,
            jwks,
            cache: Mutex::new(JwksCache::new()),
        }
    }

    /// Maps a token header algorithm to the configured one, if it is allowed.
    fn configured_algorithm(&self, header: Algorithm) -> Option<Algorithm> {
        self.config.algorithms.iter().find_map(|configured| {
            let candidate = match configured {
                JwtAlgorithm::RS256 => Algorithm::RS256,
                JwtAlgorithm::ES256 => Algorithm::ES256,
                JwtAlgorithm::HS256 => Algorithm::HS256,
            };
            (candidate == header).then_some(candidate)
        })
    }

    fn hs256_key(&self) -> Result<DecodingKey, IdentityError> {
        let name = self.config.hs256_secret_env.as_deref();
        let secret = name
            .and_then(|name| self.env.get(name))
            .filter(|value| value.len() >= MIN_HS256_SECRET_BYTES);
        match secret {
            Some(secret) => Ok(DecodingKey::from_secret(secret.as_bytes())),
            None => {
                tracing::error!(
                    secret_env = name.unwrap_or("<unset>"),
                    "the HS256 identity secret is missing or shorter than 32 bytes"
                );
                Err(IdentityError::Unavailable(
                    "identity secret is not configured".to_string(),
                ))
            }
        }
    }
}

#[async_trait]
impl IdentityProvider for JwtProvider {
    async fn verify(&self, token: &str) -> Result<Identity, IdentityError> {
        if token.len() > MAX_TOKEN_LENGTH {
            return Err(IdentityError::Invalid("token is too long"));
        }

        let header = decode_header(token).map_err(|_| IdentityError::Invalid("malformed token"))?;
        let algorithm = self
            .configured_algorithm(header.alg)
            .ok_or(IdentityError::Invalid("unsupported token algorithm"))?;

        let key = match algorithm {
            Algorithm::HS256 => self.hs256_key()?,
            Algorithm::RS256 | Algorithm::ES256 => {
                let kid = match header.kid.as_deref() {
                    Some(kid) if kid.is_empty() || kid.len() > MAX_KID_LENGTH => {
                        return Err(IdentityError::Invalid("token has no valid kid"));
                    }
                    kid => kid,
                };
                cached_key(&self.cache, self.jwks.as_ref(), kid)
                    .await
                    .map_err(map_jwks_error)?
                    .ok_or(IdentityError::Invalid("unknown token signing key"))?
            }
            _ => return Err(IdentityError::Invalid("unsupported token algorithm")),
        };

        let mut validation = Validation::new(algorithm);
        validation.leeway = CLOCK_LEEWAY_SECONDS;
        validation.set_required_spec_claims(&["exp"]);
        // `iss` and `aud` are checked below so both shapes of `aud` (string or
        // array) and the exact issuer message stay under our control.
        validation.validate_aud = false;

        let data = decode::<Value>(token, &key, &validation)
            .map_err(|_| IdentityError::Invalid("invalid token"))?;
        let claims = data.claims;

        if claims.get("iss").and_then(Value::as_str) != Some(self.config.issuer.as_str()) {
            return Err(IdentityError::Invalid("invalid token issuer"));
        }
        if !audience_matches(claims.get("aud"), &self.config.audience) {
            return Err(IdentityError::Invalid("invalid token audience"));
        }

        let now = jsonwebtoken::get_current_timestamp();
        let exp =
            numeric_claim(&claims, "exp")?.ok_or(IdentityError::Invalid("token is missing exp"))?;
        if now > exp.saturating_add(CLOCK_LEEWAY_SECONDS) {
            return Err(IdentityError::Invalid("token has expired"));
        }
        if let Some(nbf) = numeric_claim(&claims, "nbf")? {
            if nbf > now.saturating_add(CLOCK_LEEWAY_SECONDS) {
                return Err(IdentityError::Invalid("token is not valid yet"));
            }
        }
        if let Some(iat) = numeric_claim(&claims, "iat")? {
            if iat > now.saturating_add(CLOCK_LEEWAY_SECONDS) {
                return Err(IdentityError::Invalid("token was issued in the future"));
            }
            if now.saturating_sub(iat) > self.config.max_age_seconds {
                return Err(IdentityError::Invalid(
                    "token is older than max_age_seconds",
                ));
            }
        }

        let uid = claim_at(&claims, &self.config.uid_claim)
            .ok_or(IdentityError::Invalid("token has no usable uid"))
            .and_then(uid_from_claim)?;

        let email = claims
            .get("email")
            .and_then(Value::as_str)
            .map(ToString::to_string);
        let name = self
            .config
            .name_claim
            .as_deref()
            .and_then(|path| claim_at(&claims, path))
            .and_then(Value::as_str)
            .map(ToString::to_string);

        Ok(Identity {
            uid,
            email,
            name,
            claims,
        })
    }
}

fn map_jwks_error(error: JwksError) -> IdentityError {
    match error {
        JwksError::Expired => IdentityError::Unavailable(
            "JWT signing keys expired and refresh is throttled".to_string(),
        ),
        JwksError::Fetch(reason) => {
            IdentityError::Unavailable(format!("JWT signing keys unavailable: {reason}"))
        }
    }
}

/// A numeric claim. A present but non-numeric claim is rejected instead of being
/// ignored, so a malformed `iat`/`nbf` cannot bypass the checks.
fn numeric_claim(claims: &Value, name: &str) -> Result<Option<u64>, IdentityError> {
    match claims.get(name) {
        None => Ok(None),
        Some(Value::Number(number)) => number
            .as_u64()
            .map(Some)
            .ok_or(IdentityError::Invalid("token has no usable timestamp")),
        Some(_) => Err(IdentityError::Invalid("token has no usable timestamp")),
    }
}

fn audience_matches(aud: Option<&Value>, expected: &str) -> bool {
    match aud {
        Some(Value::String(value)) => value == expected,
        Some(Value::Array(values)) => values.iter().any(|value| value.as_str() == Some(expected)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::jwks::MIN_JWK_REFRESH;
    use crate::identity::MAX_TOKEN_LENGTH;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine as _;
    use jsonwebtoken::jwk::Jwk;
    use jsonwebtoken::{encode, EncodingKey, Header};
    use rand::rngs::OsRng;
    use rsa::pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding};
    use rsa::RsaPrivateKey;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    const ISSUER: &str = "https://auth.example.com/";
    const AUDIENCE: &str = "client-mobile-app";
    const SECRET_ENV: &str = "CLIENT_JWT_SECRET";
    const HS256_SECRET: &str = "a-very-long-test-secret-value-1234567890";

    struct TestKeys {
        private_pem: String,
        decoding: DecodingKey,
    }

    fn generate_keys() -> TestKeys {
        let private = RsaPrivateKey::new(&mut OsRng, 2048).expect("generate RSA key");
        let private_pem = private
            .to_pkcs8_pem(LineEnding::LF)
            .expect("private key PEM")
            .to_string();
        let public_pem = private
            .to_public_key()
            .to_public_key_pem(LineEnding::LF)
            .expect("public key PEM");
        TestKeys {
            private_pem,
            decoding: DecodingKey::from_rsa_pem(public_pem.as_bytes()).expect("decoding key"),
        }
    }

    fn encoding_key(keys: &TestKeys) -> EncodingKey {
        EncodingKey::from_rsa_pem(keys.private_pem.as_bytes()).expect("encoding key")
    }

    fn rs256_config() -> JwtProviderConfig {
        JwtProviderConfig {
            algorithms: vec![JwtAlgorithm::RS256],
            jwks_url: Some("https://auth.example.com/jwks.json".to_string()),
            hs256_secret_env: None,
            issuer: ISSUER.to_string(),
            audience: AUDIENCE.to_string(),
            uid_claim: "sub".to_string(),
            name_claim: Some("name".to_string()),
            max_age_seconds: 3600,
        }
    }

    fn hs256_config() -> JwtProviderConfig {
        JwtProviderConfig {
            algorithms: vec![JwtAlgorithm::HS256],
            jwks_url: None,
            hs256_secret_env: Some(SECRET_ENV.to_string()),
            issuer: ISSUER.to_string(),
            audience: AUDIENCE.to_string(),
            uid_claim: "sub".to_string(),
            name_claim: None,
            max_age_seconds: 3600,
        }
    }

    struct StaticEnv {
        values: HashMap<String, String>,
    }

    impl StaticEnv {
        fn empty() -> Self {
            Self {
                values: HashMap::new(),
            }
        }

        fn with(name: &str, value: &str) -> Self {
            Self {
                values: HashMap::from([(name.to_string(), value.to_string())]),
            }
        }
    }

    impl EnvSource for StaticEnv {
        fn get(&self, name: &str) -> Option<String> {
            self.values.get(name).cloned()
        }
    }

    struct StaticJwks {
        batches: Mutex<Vec<HashMap<String, DecodingKey>>>,
        calls: AtomicUsize,
    }

    impl StaticJwks {
        fn new(batches: Vec<HashMap<String, DecodingKey>>) -> Self {
            Self {
                batches: Mutex::new(batches),
                calls: AtomicUsize::new(0),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl JwkProvider for StaticJwks {
        async fn fetch_keys(&self) -> Result<super::super::jwks::FetchedJwks, String> {
            let index = self.calls.fetch_add(1, Ordering::SeqCst);
            let batches = self.batches.lock().expect("batches lock");
            let keys = batches
                .get(index.min(batches.len().saturating_sub(1)))
                .cloned()
                .unwrap_or_default();
            Ok(super::super::jwks::FetchedJwks {
                keys,
                max_age: Duration::from_secs(3600),
            })
        }
    }

    struct FailingJwks;

    #[async_trait]
    impl JwkProvider for FailingJwks {
        async fn fetch_keys(&self) -> Result<super::super::jwks::FetchedJwks, String> {
            Err("the endpoint is down".to_string())
        }
    }

    fn jwks_with(kid: &str, keys: &TestKeys) -> HashMap<String, DecodingKey> {
        HashMap::from([(kid.to_string(), keys.decoding.clone())])
    }

    fn rs256_provider(
        config: JwtProviderConfig,
        env: StaticEnv,
        jwks: Arc<dyn JwkProvider>,
    ) -> JwtProvider {
        JwtProvider::new(&config, Arc::new(env), jwks)
    }

    fn payload() -> serde_json::Map<String, Value> {
        let now = jsonwebtoken::get_current_timestamp();
        json!({
            "sub": "uid-1",
            "iss": ISSUER,
            "aud": AUDIENCE,
            "exp": now + 3600,
            "iat": now - 10,
            "name": "Test User",
            "email": "user@example.com",
        })
        .as_object()
        .cloned()
        .expect("claims object")
    }

    fn rs256_token(
        keys: &TestKeys,
        kid: Option<&str>,
        mutate: impl FnOnce(&mut serde_json::Map<String, Value>),
    ) -> String {
        let mut claims = payload();
        mutate(&mut claims);
        let mut header = Header::new(Algorithm::RS256);
        header.kid = kid.map(ToString::to_string);
        encode(&header, &claims, &encoding_key(keys)).expect("encode token")
    }

    fn hs256_token(
        secret: &str,
        mutate: impl FnOnce(&mut serde_json::Map<String, Value>),
    ) -> String {
        let mut claims = payload();
        mutate(&mut claims);
        let header = Header::new(Algorithm::HS256);
        encode(
            &header,
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .expect("encode token")
    }

    fn expect_invalid(result: Result<Identity, IdentityError>) {
        match result {
            Err(IdentityError::Invalid(_)) => {}
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    fn expect_unavailable(result: Result<Identity, IdentityError>) {
        match result {
            Err(IdentityError::Unavailable(_)) => {}
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn jwt_rs256_valid_token_accepted() {
        let keys = generate_keys();
        let jwks = Arc::new(StaticJwks::new(vec![jwks_with("k1", &keys)]));
        let provider = rs256_provider(rs256_config(), StaticEnv::empty(), jwks);
        let identity = provider
            .verify(&rs256_token(&keys, Some("k1"), |_| {}))
            .await
            .expect("a valid token must be accepted");
        assert_eq!(identity.uid, "uid-1");
        assert_eq!(identity.email.as_deref(), Some("user@example.com"));
        assert_eq!(identity.name.as_deref(), Some("Test User"));
    }

    #[tokio::test]
    async fn jwt_audience_array_accepted() {
        let keys = generate_keys();
        let jwks = Arc::new(StaticJwks::new(vec![jwks_with("k1", &keys)]));
        let provider = rs256_provider(rs256_config(), StaticEnv::empty(), jwks);
        let token = rs256_token(&keys, Some("k1"), |claims| {
            claims.insert("aud".into(), json!(["other-app", AUDIENCE]));
        });
        provider
            .verify(&token)
            .await
            .expect("an audience array containing the configured value must pass");
    }

    #[tokio::test]
    async fn jwt_wrong_issuer_rejected() {
        let keys = generate_keys();
        let jwks = Arc::new(StaticJwks::new(vec![jwks_with("k1", &keys)]));
        let provider = rs256_provider(rs256_config(), StaticEnv::empty(), jwks);
        let token = rs256_token(&keys, Some("k1"), |claims| {
            claims.insert("iss".into(), json!("https://evil.example.com/"));
        });
        expect_invalid(provider.verify(&token).await);
    }

    #[tokio::test]
    async fn jwt_wrong_audience_rejected() {
        let keys = generate_keys();
        let jwks = Arc::new(StaticJwks::new(vec![jwks_with("k1", &keys)]));
        let provider = rs256_provider(rs256_config(), StaticEnv::empty(), jwks);
        let token = rs256_token(&keys, Some("k1"), |claims| {
            claims.insert("aud".into(), json!("another-app"));
        });
        expect_invalid(provider.verify(&token).await);
    }

    #[tokio::test]
    async fn jwt_expired_rejected() {
        let keys = generate_keys();
        let jwks = Arc::new(StaticJwks::new(vec![jwks_with("k1", &keys)]));
        let provider = rs256_provider(rs256_config(), StaticEnv::empty(), jwks);
        let token = rs256_token(&keys, Some("k1"), |claims| {
            claims.insert("exp".into(), json!(1000));
        });
        expect_invalid(provider.verify(&token).await);
    }

    #[tokio::test]
    async fn jwt_future_iat_rejected() {
        let keys = generate_keys();
        let jwks = Arc::new(StaticJwks::new(vec![jwks_with("k1", &keys)]));
        let provider = rs256_provider(rs256_config(), StaticEnv::empty(), jwks);
        let token = rs256_token(&keys, Some("k1"), |claims| {
            claims.insert(
                "iat".into(),
                json!(jsonwebtoken::get_current_timestamp() + 3600),
            );
        });
        expect_invalid(provider.verify(&token).await);
    }

    #[tokio::test]
    async fn jwt_older_than_max_age_rejected() {
        let keys = generate_keys();
        let jwks = Arc::new(StaticJwks::new(vec![jwks_with("k1", &keys)]));
        let provider = rs256_provider(rs256_config(), StaticEnv::empty(), jwks);
        let token = rs256_token(&keys, Some("k1"), |claims| {
            claims.insert(
                "iat".into(),
                json!(jsonwebtoken::get_current_timestamp() - 3601),
            );
        });
        expect_invalid(provider.verify(&token).await);
    }

    #[tokio::test]
    async fn jwt_alg_none_rejected() {
        let keys = generate_keys();
        let jwks = Arc::new(StaticJwks::new(vec![jwks_with("k1", &keys)]));
        let provider = rs256_provider(rs256_config(), StaticEnv::empty(), jwks);
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD.encode(Value::Object(payload()).to_string());
        let token = format!("{header}.{payload}.");
        expect_invalid(provider.verify(&token).await);
    }

    #[tokio::test]
    async fn jwt_hs256_token_against_rs256_provider_rejected() {
        let keys = generate_keys();
        let jwks = Arc::new(StaticJwks::new(vec![jwks_with("k1", &keys)]));
        let provider = rs256_provider(rs256_config(), StaticEnv::empty(), jwks);
        let token = hs256_token(HS256_SECRET, |_| {});
        expect_invalid(provider.verify(&token).await);
    }

    #[tokio::test]
    async fn jwt_rs256_token_against_hs256_provider_rejected() {
        let keys = generate_keys();
        let provider = JwtProvider::new(
            &hs256_config(),
            Arc::new(StaticEnv::with(SECRET_ENV, HS256_SECRET)),
            Arc::new(StaticJwks::new(vec![jwks_with("k1", &keys)])),
        );
        let token = rs256_token(&keys, Some("k1"), |_| {});
        expect_invalid(provider.verify(&token).await);
    }

    #[tokio::test]
    async fn jwt_hs256_valid_accepted() {
        let provider = JwtProvider::new(
            &hs256_config(),
            Arc::new(StaticEnv::with(SECRET_ENV, HS256_SECRET)),
            Arc::new(FailingJwks),
        );
        let identity = provider
            .verify(&hs256_token(HS256_SECRET, |_| {}))
            .await
            .expect("a valid HS256 token must be accepted");
        assert_eq!(identity.uid, "uid-1");
        assert_eq!(identity.name, None);
    }

    #[tokio::test]
    async fn jwt_hs256_missing_secret_env_is_unavailable() {
        let provider = JwtProvider::new(
            &hs256_config(),
            Arc::new(StaticEnv::empty()),
            Arc::new(FailingJwks),
        );
        expect_unavailable(provider.verify(&hs256_token(HS256_SECRET, |_| {})).await);
    }

    #[tokio::test]
    async fn jwt_short_hs256_secret_is_unavailable() {
        let short = "short-secret-0123456789012345";
        assert!(short.len() < 32);
        let provider = JwtProvider::new(
            &hs256_config(),
            Arc::new(StaticEnv::with(SECRET_ENV, short)),
            Arc::new(FailingJwks),
        );
        expect_unavailable(provider.verify(&hs256_token(short, |_| {})).await);
    }

    #[tokio::test]
    async fn jwt_nested_uid_claim() {
        let keys = generate_keys();
        let jwks = Arc::new(StaticJwks::new(vec![jwks_with("k1", &keys)]));
        let mut config = rs256_config();
        config.uid_claim = "user.id".to_string();
        let provider = rs256_provider(config, StaticEnv::empty(), jwks);
        let token = rs256_token(&keys, Some("k1"), |claims| {
            claims.insert("user".into(), json!({ "id": "uid-9" }));
        });
        let identity = provider
            .verify(&token)
            .await
            .expect("the nested uid claim must be read");
        assert_eq!(identity.uid, "uid-9");
    }

    #[tokio::test]
    async fn jwt_numeric_uid_claim_converted() {
        let keys = generate_keys();
        let jwks = Arc::new(StaticJwks::new(vec![jwks_with("k1", &keys)]));
        let provider = rs256_provider(rs256_config(), StaticEnv::empty(), jwks);
        let token = rs256_token(&keys, Some("k1"), |claims| {
            claims.insert("sub".into(), json!(42));
        });
        let identity = provider
            .verify(&token)
            .await
            .expect("a numeric uid must be converted");
        assert_eq!(identity.uid, "42");
    }

    #[tokio::test]
    async fn jwt_missing_uid_claim_rejected() {
        let keys = generate_keys();
        let jwks = Arc::new(StaticJwks::new(vec![jwks_with("k1", &keys)]));
        let provider = rs256_provider(rs256_config(), StaticEnv::empty(), jwks);
        let token = rs256_token(&keys, Some("k1"), |claims| {
            claims.remove("sub");
        });
        expect_invalid(provider.verify(&token).await);
    }

    #[tokio::test]
    async fn jwt_uid_too_long_rejected() {
        let keys = generate_keys();
        let jwks = Arc::new(StaticJwks::new(vec![jwks_with("k1", &keys)]));
        let provider = rs256_provider(rs256_config(), StaticEnv::empty(), jwks);
        let token = rs256_token(&keys, Some("k1"), |claims| {
            claims.insert("sub".into(), json!("a".repeat(129)));
        });
        expect_invalid(provider.verify(&token).await);
    }

    #[tokio::test]
    async fn jwt_oversized_token_rejected() {
        let keys = generate_keys();
        let jwks = Arc::new(StaticJwks::new(vec![jwks_with("k1", &keys)]));
        let provider = rs256_provider(rs256_config(), StaticEnv::empty(), jwks);
        let token = "a".repeat(MAX_TOKEN_LENGTH + 1);
        expect_invalid(provider.verify(&token).await);
    }

    #[tokio::test]
    async fn jwt_unknown_kid_refreshes_once_per_minute() {
        let keys = generate_keys();
        let first = jwks_with("k1", &keys);
        let mut second = first.clone();
        second.insert("k2".to_string(), keys.decoding.clone());
        let jwks = Arc::new(StaticJwks::new(vec![first, second]));
        let provider = rs256_provider(rs256_config(), StaticEnv::empty(), jwks.clone());

        // Warm the cache with k1 (refresh #1).
        provider
            .verify(&rs256_token(&keys, Some("k1"), |_| {}))
            .await
            .expect("k1 must be accepted");
        assert_eq!(jwks.calls(), 1);

        // Pretend the cache was loaded a while ago, then use a rotated kid.
        {
            let mut cache = provider.cache.lock().expect("cache lock");
            cache.last_refresh = Instant::now() - MIN_JWK_REFRESH;
        }
        provider
            .verify(&rs256_token(&keys, Some("k2"), |_| {}))
            .await
            .expect("a rotated kid must be accepted after one refresh");
        assert_eq!(jwks.calls(), 2);

        // A second unknown kid must not trigger another refresh inside the window.
        expect_invalid(
            provider
                .verify(&rs256_token(&keys, Some("k3"), |_| {}))
                .await,
        );
        assert_eq!(jwks.calls(), 2);
    }

    #[tokio::test]
    async fn jwt_missing_kid_uses_sole_key() {
        let keys = generate_keys();
        let jwks = Arc::new(StaticJwks::new(vec![jwks_with("k1", &keys)]));
        let provider = rs256_provider(rs256_config(), StaticEnv::empty(), jwks);
        provider
            .verify(&rs256_token(&keys, None, |_| {}))
            .await
            .expect("a sole key must be used when the token has no kid");
    }

    #[tokio::test]
    async fn jwt_jwks_fetch_failure_is_unavailable() {
        let keys = generate_keys();
        let provider = JwtProvider::new(
            &rs256_config(),
            Arc::new(StaticEnv::empty()),
            Arc::new(FailingJwks),
        );
        expect_unavailable(
            provider
                .verify(&rs256_token(&keys, Some("k1"), |_| {}))
                .await,
        );
    }

    #[tokio::test]
    async fn jwt_header_supplied_key_is_ignored() {
        let keys = generate_keys();
        let attacker = generate_keys();
        let jwks = Arc::new(StaticJwks::new(vec![jwks_with("k1", &keys)]));
        let provider = rs256_provider(rs256_config(), StaticEnv::empty(), jwks);

        // The header claims the attacker's key under the legitimate kid; the
        // token is signed with the attacker's key. Only the JWKS key may count.
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some("k1".to_string());
        header.jwk = Some(
            Jwk::from_encoding_key(&encoding_key(&attacker), Algorithm::RS256)
                .expect("attacker JWK"),
        );
        let claims = Value::Object(payload());
        let token = encode(&header, &claims, &encoding_key(&attacker)).expect("encode token");
        expect_invalid(provider.verify(&token).await);
    }
}

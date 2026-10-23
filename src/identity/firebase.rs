//! Firebase ID-token verification.
//!
//! Tokens are verified locally (RS256) against Google's public JWKs, so the
//! service account private key never signs user tokens. The rules are the ones
//! the service used before the identity module existed; only the code moved.

use super::jwks::{cached_key, warmup_cache, JwkProvider, JwksCache, JwksError, MAX_KID_LENGTH};
use super::{is_valid_uid, Identity, IdentityError, MAX_TOKEN_LENGTH};
use jsonwebtoken::{decode, decode_header, Algorithm, Validation};
use serde::Deserialize;
use serde_json::Value;
use std::sync::{Arc, Mutex};

pub(crate) const SECURETOKEN_ISSUER_PREFIX: &str = "https://securetoken.google.com/";
const CLOCK_LEEWAY_SECONDS: u64 = 60;

#[derive(Debug, Deserialize)]
struct RawClaims {
    sub: Option<String>,
    aud: Option<Value>,
    iss: Option<String>,
    exp: Option<u64>,
    iat: Option<u64>,
    auth_time: Option<u64>,
    email: Option<String>,
}

/// Verifies Firebase ID tokens for any project.
///
/// A verifier owns one JWKS cache. All Firebase apps share the Google endpoint,
/// so the registry builds one verifier and reuses it through [`Arc`].
pub struct FirebaseVerifier {
    jwks: Arc<dyn JwkProvider>,
    cache: Arc<Mutex<JwksCache>>,
}

impl FirebaseVerifier {
    /// Builds a verifier with its own empty cache.
    pub fn new(jwks: Arc<dyn JwkProvider>) -> Self {
        Self::with_cache(jwks, Arc::new(Mutex::new(JwksCache::new())))
    }

    /// Builds a verifier around an existing cache, so Firebase apps can share
    /// one downloaded Google key set.
    pub fn with_cache(jwks: Arc<dyn JwkProvider>, cache: Arc<Mutex<JwksCache>>) -> Self {
        Self { jwks, cache }
    }

    /// Warms the shared Google JWKS cache if it is cold or expired.
    pub async fn warmup(&self) -> Result<(), JwksError> {
        warmup_cache(&self.cache, self.jwks.as_ref()).await
    }

    pub async fn verify(&self, token: &str, project_id: &str) -> Result<Identity, IdentityError> {
        if token.len() > MAX_TOKEN_LENGTH {
            return Err(IdentityError::Invalid("token is too long"));
        }

        let header =
            decode_header(token).map_err(|_| IdentityError::Invalid("malformed ID token"))?;
        if header.alg != Algorithm::RS256 {
            return Err(IdentityError::Invalid(
                "unsupported token algorithm; RS256 is required",
            ));
        }
        let kid = header
            .kid
            .filter(|kid| !kid.is_empty() && kid.len() <= MAX_KID_LENGTH)
            .ok_or(IdentityError::Invalid("token has no valid kid"))?;

        let key = cached_key(&self.cache, self.jwks.as_ref(), Some(&kid))
            .await
            .map_err(|error| match error {
                JwksError::Expired => IdentityError::Unavailable(
                    "Firebase signing keys expired and refresh is throttled".to_string(),
                ),
                JwksError::Fetch(reason) => IdentityError::Unavailable(format!(
                    "Firebase signing keys unavailable: {reason}"
                )),
            })?
            .ok_or(IdentityError::Invalid("unknown token signing key"))?;

        let mut validation = Validation::new(Algorithm::RS256);
        validation.leeway = CLOCK_LEEWAY_SECONDS;
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        validation.set_issuer(&[format!("{SECURETOKEN_ISSUER_PREFIX}{project_id}")]);
        validation.set_audience(&[project_id]);

        let data = decode::<Value>(token, &key, &validation)
            .map_err(|_| IdentityError::Invalid("invalid ID token"))?;
        let claims = data.claims;
        // Deserialize after validation so claim types keep the same strictness
        // the typed decoder had, while `Identity::claims` keeps every claim.
        let raw: RawClaims = serde_json::from_value(claims.clone())
            .map_err(|_| IdentityError::Invalid("invalid ID token"))?;

        let now = jsonwebtoken::get_current_timestamp();
        let exp = raw
            .exp
            .ok_or(IdentityError::Invalid("token is missing exp"))?;
        if now > exp.saturating_add(CLOCK_LEEWAY_SECONDS) {
            return Err(IdentityError::Invalid("token has expired"));
        }
        let iat = raw
            .iat
            .ok_or(IdentityError::Invalid("token is missing iat"))?;
        if iat > now.saturating_add(CLOCK_LEEWAY_SECONDS) {
            return Err(IdentityError::Invalid("token was issued in the future"));
        }
        let auth_time = raw
            .auth_time
            .ok_or(IdentityError::Invalid("token is missing auth_time"))?;
        if auth_time > now {
            return Err(IdentityError::Invalid("token auth_time is in the future"));
        }

        let uid = raw
            .sub
            .filter(|sub| is_valid_uid(sub) && !sub.trim().is_empty())
            .ok_or(IdentityError::Invalid("token has no valid sub"))?;

        // These checks are redundant with `Validation` but make the contract
        // explicit if the validation setup is ever changed.
        let expected_iss = format!("{SECURETOKEN_ISSUER_PREFIX}{project_id}");
        if raw.iss.as_deref() != Some(expected_iss.as_str()) {
            return Err(IdentityError::Invalid("invalid token issuer"));
        }
        if !audience_matches(raw.aud.as_ref(), project_id) {
            return Err(IdentityError::Invalid("invalid token audience"));
        }

        Ok(Identity {
            uid,
            email: raw.email,
            name: None,
            claims,
        })
    }
}

fn audience_matches(aud: Option<&Value>, project_id: &str) -> bool {
    match aud {
        Some(Value::String(value)) => value == project_id,
        Some(Value::Array(values)) => values
            .iter()
            .any(|value| value.as_str() == Some(project_id)),
        _ => false,
    }
}

/// One Firebase app: a project id plus the verifier shared by all Firebase apps.
pub struct FirebaseProvider {
    project_id: String,
    verifier: Arc<FirebaseVerifier>,
}

impl FirebaseProvider {
    pub fn new(project_id: impl Into<String>, verifier: Arc<FirebaseVerifier>) -> Self {
        Self {
            project_id: project_id.into(),
            verifier,
        }
    }
}

#[async_trait::async_trait]
impl super::IdentityProvider for FirebaseProvider {
    async fn verify(&self, token: &str) -> Result<Identity, IdentityError> {
        self.verifier.verify(token, &self.project_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::jwks::FetchedJwks;
    use crate::identity::IdentityProvider;
    use jsonwebtoken::{encode, DecodingKey, EncodingKey, Header};
    use rand::rngs::OsRng;
    use rsa::pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding};
    use rsa::RsaPrivateKey;
    use serde_json::json;
    use std::collections::HashMap;
    use std::time::Duration;

    const PROJECT: &str = "test-project";

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

    struct StaticJwks {
        keys: HashMap<String, DecodingKey>,
    }

    #[async_trait::async_trait]
    impl JwkProvider for StaticJwks {
        async fn fetch_keys(&self) -> Result<FetchedJwks, String> {
            Ok(FetchedJwks {
                keys: self.keys.clone(),
                max_age: Duration::from_secs(3600),
            })
        }
    }

    fn provider(keys: &TestKeys) -> FirebaseProvider {
        let jwks = Arc::new(StaticJwks {
            keys: HashMap::from([("k1".to_string(), keys.decoding.clone())]),
        });
        FirebaseProvider::new(PROJECT, Arc::new(FirebaseVerifier::new(jwks)))
    }

    fn token(keys: &TestKeys, audience: &str) -> String {
        let now = jsonwebtoken::get_current_timestamp();
        let payload = json!({
            "sub": "uid-1",
            "aud": audience,
            "iss": format!("{SECURETOKEN_ISSUER_PREFIX}{PROJECT}"),
            "exp": now + 3600,
            "iat": now - 10,
            "auth_time": now - 10,
            "email": "user@example.com",
        });
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some("k1".to_string());
        encode(
            &header,
            &payload,
            &EncodingKey::from_rsa_pem(keys.private_pem.as_bytes()).expect("encoding key"),
        )
        .expect("encode token")
    }

    #[tokio::test]
    async fn firebase_provider_returns_identity_with_email() {
        let keys = generate_keys();
        let identity = provider(&keys)
            .verify(&token(&keys, PROJECT))
            .await
            .expect("a valid token must be accepted");
        assert_eq!(identity.uid, "uid-1");
        assert_eq!(identity.email.as_deref(), Some("user@example.com"));
        assert_eq!(identity.name, None);
        assert_eq!(
            identity.claims["iss"],
            json!(format!("{SECURETOKEN_ISSUER_PREFIX}{PROJECT}"))
        );
    }

    #[tokio::test]
    async fn firebase_provider_rejects_wrong_audience() {
        let keys = generate_keys();
        let error = provider(&keys)
            .verify(&token(&keys, "other-project"))
            .await
            .expect_err("a token for another project must be rejected");
        assert!(matches!(error, IdentityError::Invalid(_)));
    }
}

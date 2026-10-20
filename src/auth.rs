//! Firebase ID-token verification and authorization helpers.
//!
//! Design notes:
//! - ID tokens are verified locally (RS256) against Google's public JWKs, so the
//!   service account private key never signs user tokens.
//! - JWKs are cached in memory and refreshed according to `Cache-Control: max-age`.
//!   An unknown `kid` triggers at most one refresh per [`MIN_JWK_REFRESH`].
//! - Admin status is read from Firestore `users/{uid}` using the per-app service
//!   account with the `datastore` scope. Results are cached for 60 seconds.
//! - The service key and the legacy API key are compared in constant time and are
//!   never logged.

use crate::credentials::AuthMap;
use async_trait::async_trait;
use axum::http::{header, HeaderMap, StatusCode};
use jsonwebtoken::jwk::{JwkSet, KeyAlgorithm};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq;

use crate::credentials::ProjectIdMap;

/// Google's public keys for Firebase Auth ID tokens.
const JWK_URL: &str =
    "https://www.googleapis.com/service_accounts/v1/jwk/securetoken@system.gserviceaccount.com";
const SECURETOKEN_ISSUER_PREFIX: &str = "https://securetoken.google.com/";
const CLOCK_LEEWAY_SECONDS: u64 = 60;
const MAX_TOKEN_LENGTH: usize = 8192;
const MAX_KID_LENGTH: usize = 128;
const MAX_UID_LENGTH: usize = 128;
const ADMIN_CACHE_TTL: Duration = Duration::from_secs(60);
const ADMIN_CACHE_MAX_ENTRIES: usize = 4096;
const MIN_JWK_REFRESH: Duration = Duration::from_secs(60);
const DEFAULT_JWK_MAX_AGE: Duration = Duration::from_secs(60 * 60);
const MAX_JWK_MAX_AGE: Duration = Duration::from_secs(6 * 60 * 60);
const MIN_JWK_MAX_AGE: Duration = Duration::from_secs(60);
const FIRESTORE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminIdentity {
    pub uid: String,
    pub email: Option<String>,
}

impl AdminIdentity {
    pub fn service() -> Self {
        Self {
            uid: "service".to_string(),
            email: None,
        }
    }

    pub fn legacy() -> Self {
        Self {
            uid: "legacy".to_string(),
            email: None,
        }
    }

    pub fn is_service(&self) -> bool {
        self.uid == "service"
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserIdentity {
    pub uid: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    Unauthorized(&'static str),
    Forbidden(String),
    Unavailable(String),
    AppNotFound(String),
}

impl AuthError {
    pub fn status(&self) -> StatusCode {
        match self {
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            Self::AppNotFound(_) => StatusCode::NOT_FOUND,
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::Unauthorized(reason) => format!("Unauthorized: {reason}"),
            Self::Forbidden(reason) => format!("Forbidden: {reason}"),
            Self::Unavailable(reason) => format!("Service unavailable: {reason}"),
            Self::AppNotFound(app) => {
                format!("App '{app}' not found in credentials folder.")
            }
        }
    }

    pub fn into_response(self) -> (StatusCode, String) {
        let status = self.status();
        (status, self.message())
    }
}

#[derive(Clone, Default)]
pub struct AuthConfig {
    /// Server-to-server key. `None` disables the service-key path.
    pub service_api_key: Option<String>,
    /// Legacy `API_KEY` value. Only used while `legacy_enabled` is true.
    pub legacy_api_key: Option<String>,
    /// When false (default), `API_KEY` is never accepted.
    pub legacy_enabled: bool,
}

pub struct FetchedJwks {
    pub keys: HashMap<String, DecodingKey>,
    pub max_age: Duration,
}

#[async_trait]
pub trait JwkProvider: Send + Sync {
    async fn fetch_keys(&self) -> Result<FetchedJwks, String>;
}

#[async_trait]
pub trait FirestoreReader: Send + Sync {
    /// Returns `Ok(None)` when the user document does not exist. Any transport,
    /// authentication or permission problem must be returned as `Err`, so callers
    /// can fail closed with `503`.
    async fn read_user(
        &self,
        app: &str,
        project_id: &str,
        uid: &str,
    ) -> Result<Option<Value>, String>;
}

pub struct GoogleJwkProvider {
    client: reqwest::Client,
}

impl GoogleJwkProvider {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }
}

#[async_trait]
impl JwkProvider for GoogleJwkProvider {
    async fn fetch_keys(&self) -> Result<FetchedJwks, String> {
        let response = self
            .client
            .get(JWK_URL)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|_| "could not reach the Google JWK endpoint".to_string())?;

        let status = response.status();
        if !status.is_success() {
            return Err(format!("Google JWK endpoint returned HTTP {status}"));
        }

        let max_age = response
            .headers()
            .get(reqwest::header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_cache_max_age)
            .unwrap_or(DEFAULT_JWK_MAX_AGE)
            .clamp(MIN_JWK_MAX_AGE, MAX_JWK_MAX_AGE);

        let set: JwkSet = response
            .json()
            .await
            .map_err(|_| "Google JWK response was not valid JSON".to_string())?;

        let mut keys = HashMap::new();
        for jwk in set.keys {
            if jwk.common.key_algorithm != Some(KeyAlgorithm::RS256) {
                continue;
            }
            let Some(kid) = jwk.common.key_id.clone() else {
                continue;
            };
            if kid.len() > MAX_KID_LENGTH {
                continue;
            }
            if let Ok(key) = DecodingKey::from_jwk(&jwk) {
                keys.insert(kid, key);
            }
        }

        if keys.is_empty() {
            return Err("Google JWK response contained no usable RS256 keys".to_string());
        }

        Ok(FetchedJwks { keys, max_age })
    }
}

pub struct FirestoreHttp {
    client: reqwest::Client,
    auth_map: AuthMap,
}

impl FirestoreHttp {
    pub fn new(client: reqwest::Client, auth_map: AuthMap) -> Self {
        Self { client, auth_map }
    }
}

#[async_trait]
impl FirestoreReader for FirestoreHttp {
    async fn read_user(
        &self,
        app: &str,
        project_id: &str,
        uid: &str,
    ) -> Result<Option<Value>, String> {
        if !is_valid_uid(uid) {
            return Err("refusing to read a Firestore document for an invalid uid".to_string());
        }
        let encoded_uid = encode_path_segment(uid);

        let auth = {
            let map = self.auth_map.read().await;
            map.get(app).cloned()
        }
        .ok_or_else(|| format!("no service account is loaded for app '{app}'"))?;

        let token = auth
            .token(&["https://www.googleapis.com/auth/datastore"])
            .await
            .map_err(|_| {
                "could not obtain a service-account access token for Firestore".to_string()
            })?;
        let access_token = token
            .token()
            .ok_or_else(|| "service account returned an empty access token".to_string())?;

        let url = format!(
            "https://firestore.googleapis.com/v1/projects/{project_id}/databases/(default)/documents/users/{encoded_uid}"
        );
        let response = self
            .client
            .get(url)
            .bearer_auth(access_token)
            .timeout(FIRESTORE_TIMEOUT)
            .send()
            .await
            .map_err(|_| "could not reach Firestore".to_string())?;

        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(
                "Firestore denied the request; grant the app service account a role that can read Firestore (for example roles/datastore.viewer) in the Firebase project"
                    .to_string(),
            );
        }
        if !status.is_success() {
            return Err(format!("Firestore returned HTTP {status}"));
        }

        response
            .json::<Value>()
            .await
            .map(Some)
            .map_err(|_| "Firestore returned an invalid JSON document".to_string())
    }
}

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

#[derive(Debug, Clone)]
struct VerifiedClaims {
    uid: String,
    email: Option<String>,
}

struct JwkCache {
    keys: HashMap<String, DecodingKey>,
    expires_at: Instant,
    last_refresh: Instant,
}

struct AdminCache {
    entries: HashMap<(String, String), (bool, Instant)>,
}

pub struct AuthService {
    project_map: ProjectIdMap,
    jwks: Arc<dyn JwkProvider>,
    firestore: Arc<dyn FirestoreReader>,
    config: AuthConfig,
    jwk_cache: Mutex<JwkCache>,
    admin_cache: Mutex<AdminCache>,
}

impl AuthService {
    pub fn new(
        project_map: ProjectIdMap,
        jwks: Arc<dyn JwkProvider>,
        firestore: Arc<dyn FirestoreReader>,
        config: AuthConfig,
    ) -> Self {
        Self {
            project_map,
            jwks,
            firestore,
            config,
            jwk_cache: Mutex::new(JwkCache {
                keys: HashMap::new(),
                expires_at: Instant::now(),
                last_refresh: Instant::now() - MIN_JWK_REFRESH,
            }),
            admin_cache: Mutex::new(AdminCache {
                entries: HashMap::new(),
            }),
        }
    }

    /// Pure constant-time comparison of two secrets.
    pub fn verify_service_key(&self, headers: &HeaderMap) -> bool {
        let Some(expected) = self.config.service_api_key.as_deref() else {
            return false;
        };
        if expected.is_empty() {
            return false;
        }
        match bearer_token(headers) {
            Some(provided) => constant_time_eq(provided, expected),
            None => false,
        }
    }

    fn verify_legacy_key(&self, headers: &HeaderMap) -> bool {
        if !self.config.legacy_enabled {
            return false;
        }
        let Some(expected) = self.config.legacy_api_key.as_deref() else {
            return false;
        };
        if expected.is_empty() {
            return false;
        }
        bearer_token(headers)
            .map(|provided| constant_time_eq(provided, expected))
            .unwrap_or(false)
    }

    /// Admin authentication for routes that carry an `app`.
    pub async fn authenticate_admin(
        &self,
        headers: &HeaderMap,
        app: &str,
    ) -> Result<AdminIdentity, AuthError> {
        if self.verify_service_key(headers) {
            return Ok(AdminIdentity::service());
        }

        let project_id = self.project_for(app).await?;
        let claims = self.verify_id_token(headers, &project_id).await?;
        if !self.uid_is_admin(app, &project_id, &claims.uid).await? {
            return Err(AuthError::Forbidden(
                "the Firebase user is not an admin for this app".to_string(),
            ));
        }

        Ok(AdminIdentity {
            uid: claims.uid,
            email: claims.email,
        })
    }

    /// Admin authentication for `/send` with the temporary legacy-key fallback.
    /// The legacy key is never accepted for any other route.
    pub async fn authenticate_send(
        &self,
        headers: &HeaderMap,
        app: &str,
    ) -> Result<AdminIdentity, AuthError> {
        if self.verify_service_key(headers) {
            return Ok(AdminIdentity::service());
        }

        if self.verify_legacy_key(headers) {
            tracing::warn!(
                "Legacy API_KEY accepted for /send; migrate the admin app to Firebase ID tokens"
            );
            return Ok(AdminIdentity::legacy());
        }

        self.authenticate_admin(headers, app).await
    }

    /// Returns a valid ID-token identity for any user (no role check).
    /// Invalid or missing tokens (including unrecognised legacy keys) yield `None`.
    pub async fn authenticate_user(&self, headers: &HeaderMap, app: &str) -> Option<UserIdentity> {
        let project_id = self.project_for(app).await.ok()?;
        self.verify_id_token(headers, &project_id)
            .await
            .ok()
            .map(|claims| UserIdentity { uid: claims.uid })
    }

    /// Admin authentication for `/apps` and app-less admin queries.
    ///
    /// The token audience (unverified) only selects candidate apps from the
    /// server-side project map; the signature is always verified against that
    /// app's project before any admin lookup. Returns the identity and the list of
    /// apps where the user is an admin (`None` means the service key: all apps).
    pub async fn authenticate_admin_any(
        &self,
        headers: &HeaderMap,
    ) -> Result<(AdminIdentity, Option<Vec<String>>), AuthError> {
        if self.verify_service_key(headers) {
            return Ok((AdminIdentity::service(), None));
        }

        let token = bearer_token(headers).ok_or(AuthError::Unauthorized("missing bearer token"))?;
        if token.len() > MAX_TOKEN_LENGTH {
            return Err(AuthError::Unauthorized("token is too long"));
        }

        let audience = unverified_audience(token)
            .ok_or(AuthError::Unauthorized("token has no usable audience"))?;

        let candidates: Vec<String> = {
            let map = self.project_map.read().await;
            let mut apps: Vec<String> = map
                .iter()
                .filter(|(_, project)| project.as_str() == audience)
                .map(|(app, _)| app.clone())
                .collect();
            apps.sort();
            apps
        };

        if candidates.is_empty() {
            return Err(AuthError::Unauthorized(
                "token audience does not match a registered app",
            ));
        }

        let claims = self.verify_token(token, &audience).await?;
        let mut admin_apps = Vec::new();
        for app in candidates {
            if self.uid_is_admin(&app, &audience, &claims.uid).await? {
                admin_apps.push(app);
            }
        }

        if admin_apps.is_empty() {
            return Err(AuthError::Forbidden(
                "the Firebase user is not an admin for any registered app".to_string(),
            ));
        }

        Ok((
            AdminIdentity {
                uid: claims.uid,
                email: claims.email,
            },
            Some(admin_apps),
        ))
    }

    /// Role check for an already verified uid. Used by topic subscription to allow
    /// admins to use privileged topics.
    pub async fn is_admin(&self, app: &str, uid: &str) -> Result<bool, AuthError> {
        let project_id = self.project_for(app).await?;
        self.uid_is_admin(app, &project_id, uid).await
    }

    async fn project_for(&self, app: &str) -> Result<String, AuthError> {
        let map = self.project_map.read().await;
        match map.get(app) {
            Some(project) if !project.trim().is_empty() => Ok(project.clone()),
            _ => Err(AuthError::AppNotFound(app.to_string())),
        }
    }

    async fn verify_id_token(
        &self,
        headers: &HeaderMap,
        project_id: &str,
    ) -> Result<VerifiedClaims, AuthError> {
        let token = bearer_token(headers).ok_or(AuthError::Unauthorized("missing bearer token"))?;
        self.verify_token(token, project_id).await
    }

    async fn verify_token(
        &self,
        token: &str,
        project_id: &str,
    ) -> Result<VerifiedClaims, AuthError> {
        if token.len() > MAX_TOKEN_LENGTH {
            return Err(AuthError::Unauthorized("token is too long"));
        }

        let header =
            decode_header(token).map_err(|_| AuthError::Unauthorized("malformed ID token"))?;
        if header.alg != Algorithm::RS256 {
            return Err(AuthError::Unauthorized(
                "unsupported token algorithm; RS256 is required",
            ));
        }
        let kid = header
            .kid
            .filter(|kid| !kid.is_empty() && kid.len() <= MAX_KID_LENGTH)
            .ok_or(AuthError::Unauthorized("token has no valid kid"))?;

        let key = self
            .decoding_key(&kid)
            .await?
            .ok_or(AuthError::Unauthorized("unknown token signing key"))?;

        let mut validation = Validation::new(Algorithm::RS256);
        validation.leeway = CLOCK_LEEWAY_SECONDS;
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        validation.set_issuer(&[format!("{SECURETOKEN_ISSUER_PREFIX}{project_id}")]);
        validation.set_audience(&[project_id]);

        let data = decode::<RawClaims>(token, &key, &validation)
            .map_err(|_| AuthError::Unauthorized("invalid ID token"))?;
        let claims = data.claims;

        let now = jsonwebtoken::get_current_timestamp();
        let exp = claims
            .exp
            .ok_or(AuthError::Unauthorized("token is missing exp"))?;
        if now > exp.saturating_add(CLOCK_LEEWAY_SECONDS) {
            return Err(AuthError::Unauthorized("token has expired"));
        }
        let iat = claims
            .iat
            .ok_or(AuthError::Unauthorized("token is missing iat"))?;
        if iat > now.saturating_add(CLOCK_LEEWAY_SECONDS) {
            return Err(AuthError::Unauthorized("token was issued in the future"));
        }
        let auth_time = claims
            .auth_time
            .ok_or(AuthError::Unauthorized("token is missing auth_time"))?;
        if auth_time > now {
            return Err(AuthError::Unauthorized("token auth_time is in the future"));
        }

        let uid = claims
            .sub
            .filter(|sub| is_valid_uid(sub) && !sub.trim().is_empty())
            .ok_or(AuthError::Unauthorized("token has no valid sub"))?;

        // These checks are redundant with `Validation` but make the contract
        // explicit if the validation setup is ever changed.
        let expected_iss = format!("{SECURETOKEN_ISSUER_PREFIX}{project_id}");
        if claims.iss.as_deref() != Some(expected_iss.as_str()) {
            return Err(AuthError::Unauthorized("invalid token issuer"));
        }
        if !audience_matches(claims.aud.as_ref(), project_id) {
            return Err(AuthError::Unauthorized("invalid token audience"));
        }

        Ok(VerifiedClaims {
            uid,
            email: claims.email,
        })
    }

    async fn decoding_key(&self, kid: &str) -> Result<Option<DecodingKey>, AuthError> {
        let now = Instant::now();
        let (cached, expired) = {
            let cache = lock(&self.jwk_cache);
            (cache.keys.get(kid).cloned(), now >= cache.expires_at)
        };
        if !expired && cached.is_some() {
            return Ok(cached);
        }

        let may_refresh = {
            let cache = lock(&self.jwk_cache);
            now.duration_since(cache.last_refresh) >= MIN_JWK_REFRESH
        };
        if !may_refresh {
            return if expired {
                Err(AuthError::Unavailable(
                    "Firebase signing keys expired and refresh is throttled".to_string(),
                ))
            } else {
                Ok(None)
            };
        }

        {
            let mut cache = lock(&self.jwk_cache);
            cache.last_refresh = now;
        }
        self.refresh_keys().await?;

        let cache = lock(&self.jwk_cache);
        Ok(cache.keys.get(kid).cloned())
    }

    async fn refresh_keys(&self) -> Result<(), AuthError> {
        let fetched = self.jwks.fetch_keys().await.map_err(|reason| {
            AuthError::Unavailable(format!("Firebase signing keys unavailable: {reason}"))
        })?;

        let max_age = fetched.max_age.clamp(MIN_JWK_MAX_AGE, MAX_JWK_MAX_AGE);
        let mut cache = lock(&self.jwk_cache);
        cache.keys = fetched.keys;
        cache.expires_at = Instant::now() + max_age;
        Ok(())
    }

    async fn uid_is_admin(
        &self,
        app: &str,
        project_id: &str,
        uid: &str,
    ) -> Result<bool, AuthError> {
        let key = (app.to_string(), uid.to_string());
        {
            let cache = lock(&self.admin_cache);
            if let Some((is_admin, expires_at)) = cache.entries.get(&key) {
                if Instant::now() < *expires_at {
                    return Ok(*is_admin);
                }
            }
        }

        let document = self
            .firestore
            .read_user(app, project_id, uid)
            .await
            .map_err(|reason| {
                AuthError::Unavailable(format!("admin role lookup failed: {reason}"))
            })?;
        let is_admin = document.as_ref().map(role_is_admin).unwrap_or(false);

        let mut cache = lock(&self.admin_cache);
        let now = Instant::now();
        cache.entries.retain(|_, (_, expires_at)| now < *expires_at);
        if cache.entries.len() >= ADMIN_CACHE_MAX_ENTRIES {
            cache.entries.clear();
        }
        cache.entries.insert(key, (is_admin, now + ADMIN_CACHE_TTL));

        Ok(is_admin)
    }
}

/// Access level resolved for a topic subscription request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopicAccess {
    Anonymous,
    User,
    Admin,
}

/// Anonymous clients may only use public topics. Authenticated users additionally
/// get the role topics. Admins may use every valid topic.
pub fn topic_allowed(topic: &str, access: TopicAccess) -> bool {
    match access {
        TopicAccess::Admin => true,
        TopicAccess::User => {
            is_public_topic(topic) || matches!(topic, "users" | "companies" | "technicals")
        }
        TopicAccess::Anonymous => is_public_topic(topic),
    }
}

/// `all`, `country_<two letters>` and `lang_<two letters>` (strict).
pub fn is_public_topic(topic: &str) -> bool {
    if topic == "all" {
        return true;
    }
    if let Some(code) = topic.strip_prefix("country_") {
        return is_two_ascii_letters(code);
    }
    if let Some(code) = topic.strip_prefix("lang_") {
        return is_two_ascii_letters(code);
    }
    false
}

fn is_two_ascii_letters(value: &str) -> bool {
    value.len() == 2 && value.bytes().all(|byte| byte.is_ascii_alphabetic())
}

impl AuthService {
    /// Topic-subscription authorization.
    ///
    /// - A valid ID token is accepted as a normal user (role topics + public).
    /// - Missing or invalid ID tokens are treated as anonymous (public topics only),
    ///   so a bad header is ignored instead of rejecting the request.
    /// - Privileged topics require an admin; a Firestore failure returns `503`.
    pub async fn authorize_topic(
        &self,
        headers: &HeaderMap,
        app: &str,
        topic: &str,
    ) -> Result<(TopicAccess, Option<UserIdentity>), AuthError> {
        if self.verify_service_key(headers) {
            return Ok((TopicAccess::Admin, None));
        }

        if let Some(user) = self.authenticate_user(headers, app).await {
            if topic_allowed(topic, TopicAccess::User) {
                return Ok((TopicAccess::User, Some(user)));
            }
            if self.is_admin(app, &user.uid).await? {
                return Ok((TopicAccess::Admin, Some(user)));
            }
            return Err(AuthError::Forbidden(format!(
                "topic '{topic}' requires an admin ID token"
            )));
        }

        if topic_allowed(topic, TopicAccess::Anonymous) {
            Ok((TopicAccess::Anonymous, None))
        } else {
            Err(AuthError::Forbidden(format!(
                "topic '{topic}' requires an authenticated admin"
            )))
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Constant-time comparison that does not short-circuit on length: both inputs
/// are hashed first, then the fixed-size digests are compared.
pub fn constant_time_eq(provided: &str, expected: &str) -> bool {
    let provided_digest = Sha256::digest(provided.as_bytes());
    let expected_digest = Sha256::digest(expected.as_bytes());
    provided_digest.ct_eq(&expected_digest).into()
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.trim().split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = token.trim();
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

fn unverified_audience(token: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct AudOnly {
        aud: Value,
    }

    let data = jsonwebtoken::dangerous::insecure_decode::<AudOnly>(token).ok()?;
    match data.claims.aud {
        Value::String(aud) => Some(aud),
        Value::Array(values) => values.first()?.as_str().map(ToString::to_string),
        _ => None,
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

fn role_is_admin(document: &Value) -> bool {
    document
        .get("fields")
        .and_then(|fields| fields.get("role"))
        .and_then(|role| role.get("stringValue"))
        .and_then(Value::as_str)
        == Some("admin")
}

pub(crate) fn is_valid_uid(uid: &str) -> bool {
    !uid.is_empty() && uid.len() <= MAX_UID_LENGTH
}

/// Percent-encode a Firestore document id safely (slashes must not create new
/// path segments).
fn encode_path_segment(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

pub(crate) fn parse_cache_max_age(value: &str) -> Option<Duration> {
    value.split(',').find_map(|directive| {
        let (name, value) = directive.trim().split_once('=')?;
        if !name.trim().eq_ignore_ascii_case("max-age") {
            return None;
        }
        value.trim().parse::<u64>().ok().map(Duration::from_secs)
    })
}

/// Client IP for rate limiting. `X-Forwarded-For` is only trusted behind a proxy.
pub fn client_ip(headers: &HeaderMap, peer: Option<SocketAddr>, trust_proxy: bool) -> String {
    if trust_proxy {
        if let Some(forwarded) = headers
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok())
        {
            if let Some(first) = forwarded.split(',').next() {
                let candidate = first.trim();
                if !candidate.is_empty() && candidate.len() <= 64 {
                    return candidate.to_string();
                }
            }
        }
    }
    peer.map(|addr| addr.ip().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};
    use rand::rngs::OsRng;
    use rsa::pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding};
    use rsa::RsaPrivateKey;
    use serde_json::json;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::OnceLock;
    use tokio::sync::RwLock;

    const PROJECT: &str = "test-project";
    const APP: &str = "test-app";

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

    fn keys() -> &'static TestKeys {
        static KEYS: OnceLock<TestKeys> = OnceLock::new();
        KEYS.get_or_init(generate_keys)
    }

    fn other_keys() -> &'static TestKeys {
        static KEYS: OnceLock<TestKeys> = OnceLock::new();
        KEYS.get_or_init(generate_keys)
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
        async fn fetch_keys(&self) -> Result<FetchedJwks, String> {
            let index = self.calls.fetch_add(1, Ordering::SeqCst);
            let batches = self.batches.lock().expect("batches lock");
            let keys = batches
                .get(index.min(batches.len().saturating_sub(1)))
                .cloned()
                .unwrap_or_default();
            Ok(FetchedJwks {
                keys,
                max_age: Duration::from_secs(3600),
            })
        }
    }

    struct FakeFirestore {
        role: Option<&'static str>,
        fail: AtomicBool,
    }

    impl FakeFirestore {
        fn with_role(role: Option<&'static str>) -> Self {
            Self {
                role,
                fail: AtomicBool::new(false),
            }
        }
    }

    #[async_trait]
    impl FirestoreReader for FakeFirestore {
        async fn read_user(
            &self,
            _app: &str,
            _project_id: &str,
            _uid: &str,
        ) -> Result<Option<Value>, String> {
            if self.fail.load(Ordering::SeqCst) {
                return Err("permission denied".to_string());
            }
            Ok(self.role.map(|role| {
                json!({
                    "name": "projects/test/databases/(default)/documents/users/uid",
                    "fields": { "role": { "stringValue": role } }
                })
            }))
        }
    }

    fn project_map() -> ProjectIdMap {
        Arc::new(RwLock::new(HashMap::from([(
            APP.to_string(),
            PROJECT.to_string(),
        )])))
    }

    fn service(jwks: Arc<dyn JwkProvider>, firestore: Arc<dyn FirestoreReader>) -> AuthService {
        AuthService::new(
            project_map(),
            jwks,
            firestore,
            AuthConfig {
                service_api_key: Some("service-secret".to_string()),
                legacy_api_key: Some("legacy-secret".to_string()),
                legacy_enabled: true,
            },
        )
    }

    fn jwks_with(kid: &str, key: DecodingKey) -> HashMap<String, DecodingKey> {
        HashMap::from([(kid.to_string(), key)])
    }

    fn claims(uid: &str, kid: &str, key: &TestKeys) -> String {
        token_with(uid, kid, key, |_| {})
    }

    fn token_with(
        uid: &str,
        kid: &str,
        key: &TestKeys,
        mutate: impl FnOnce(&mut serde_json::Map<String, Value>),
    ) -> String {
        let now = jsonwebtoken::get_current_timestamp();
        let mut payload = json!({
            "sub": uid,
            "aud": PROJECT,
            "iss": format!("{SECURETOKEN_ISSUER_PREFIX}{PROJECT}"),
            "exp": now + 3600,
            "iat": now - 10,
            "auth_time": now - 10,
            "email": "admin@example.com",
        });
        mutate(payload.as_object_mut().expect("claims object"));
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(kid.to_string());
        encode(
            &header,
            &payload,
            &EncodingKey::from_rsa_pem(key.private_pem.as_bytes()).expect("encoding key"),
        )
        .expect("encode token")
    }

    fn bearer(token: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {token}").parse().expect("header value"),
        );
        headers
    }

    async fn expect_unauthorized(result: Result<AdminIdentity, AuthError>) {
        match result {
            Err(AuthError::Unauthorized(_)) => {}
            other => panic!("expected Unauthorized, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn accepts_valid_admin_token() {
        let jwks = Arc::new(StaticJwks::new(vec![jwks_with(
            "k1",
            keys().decoding.clone(),
        )]));
        let service = service(jwks, Arc::new(FakeFirestore::with_role(Some("admin"))));
        let identity = service
            .authenticate_admin(&bearer(&claims("uid-1", "k1", keys())), APP)
            .await
            .expect("valid admin token");
        assert_eq!(identity.uid, "uid-1");
        assert_eq!(identity.email.as_deref(), Some("admin@example.com"));
    }

    #[tokio::test]
    async fn rejects_expired_token() {
        let jwks = Arc::new(StaticJwks::new(vec![jwks_with(
            "k1",
            keys().decoding.clone(),
        )]));
        let service = service(jwks, Arc::new(FakeFirestore::with_role(Some("admin"))));
        let token = token_with("uid-1", "k1", keys(), |claims| {
            claims.insert("exp".into(), json!(1000));
        });
        expect_unauthorized(service.authenticate_admin(&bearer(&token), APP).await).await;
    }

    #[tokio::test]
    async fn rejects_wrong_issuer_and_audience() {
        let jwks = Arc::new(StaticJwks::new(vec![jwks_with(
            "k1",
            keys().decoding.clone(),
        )]));
        let service = service(jwks, Arc::new(FakeFirestore::with_role(Some("admin"))));
        let wrong_iss = token_with("uid-1", "k1", keys(), |claims| {
            claims.insert("iss".into(), json!("https://securetoken.google.com/other"));
        });
        expect_unauthorized(service.authenticate_admin(&bearer(&wrong_iss), APP).await).await;

        let wrong_aud = token_with("uid-1", "k1", keys(), |claims| {
            claims.insert("aud".into(), json!("other-project"));
        });
        expect_unauthorized(service.authenticate_admin(&bearer(&wrong_aud), APP).await).await;
    }

    #[tokio::test]
    async fn rejects_signature_from_another_key() {
        let jwks = Arc::new(StaticJwks::new(vec![jwks_with(
            "k1",
            keys().decoding.clone(),
        )]));
        let service = service(jwks, Arc::new(FakeFirestore::with_role(Some("admin"))));
        let token = claims("uid-1", "k1", other_keys());
        expect_unauthorized(service.authenticate_admin(&bearer(&token), APP).await).await;
    }

    #[tokio::test]
    async fn rejects_alg_none() {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine as _;

        let now = jsonwebtoken::get_current_timestamp();
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD.encode(
            json!({
                "sub": "uid-1",
                "aud": PROJECT,
                "iss": format!("{SECURETOKEN_ISSUER_PREFIX}{PROJECT}"),
                "exp": now + 3600,
                "iat": now - 10,
                "auth_time": now - 10,
            })
            .to_string(),
        );
        let token = format!("{header}.{payload}.");

        let jwks = Arc::new(StaticJwks::new(vec![jwks_with(
            "k1",
            keys().decoding.clone(),
        )]));
        let service = service(jwks, Arc::new(FakeFirestore::with_role(Some("admin"))));
        expect_unauthorized(service.authenticate_admin(&bearer(&token), APP).await).await;
    }

    #[tokio::test]
    async fn unknown_kid_refreshes_keys_once_then_throttles() {
        let first = jwks_with("k1", keys().decoding.clone());
        let mut second = first.clone();
        second.insert("k2".to_string(), keys().decoding.clone());
        let jwks = Arc::new(StaticJwks::new(vec![first, second]));
        let service = service(
            jwks.clone(),
            Arc::new(FakeFirestore::with_role(Some("admin"))),
        );

        // Warm the cache with k1 (refresh #1).
        service
            .authenticate_admin(&bearer(&claims("uid-1", "k1", keys())), APP)
            .await
            .expect("warm cache");
        assert_eq!(jwks.calls(), 1);

        // Pretend the cache was loaded a while ago, then use a rotated kid.
        {
            let mut cache = lock(&service.jwk_cache);
            cache.last_refresh = Instant::now() - MIN_JWK_REFRESH;
        }
        service
            .authenticate_admin(&bearer(&claims("uid-1", "k2", keys())), APP)
            .await
            .expect("rotated kid accepted after refresh");
        assert_eq!(jwks.calls(), 2);

        // A second unknown kid must not trigger another refresh inside the window.
        let unknown = token_with("uid-1", "k3", keys(), |_| {});
        expect_unauthorized(service.authenticate_admin(&bearer(&unknown), APP).await).await;
        assert_eq!(jwks.calls(), 2);
    }

    #[tokio::test]
    async fn non_admin_role_is_forbidden() {
        let jwks = Arc::new(StaticJwks::new(vec![jwks_with(
            "k1",
            keys().decoding.clone(),
        )]));
        let service = service(jwks, Arc::new(FakeFirestore::with_role(Some("user"))));
        match service
            .authenticate_admin(&bearer(&claims("uid-1", "k1", keys())), APP)
            .await
        {
            Err(AuthError::Forbidden(_)) => {}
            other => panic!("expected Forbidden, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn missing_user_document_is_not_admin() {
        let jwks = Arc::new(StaticJwks::new(vec![jwks_with(
            "k1",
            keys().decoding.clone(),
        )]));
        let service = service(jwks, Arc::new(FakeFirestore::with_role(None)));
        match service
            .authenticate_admin(&bearer(&claims("uid-1", "k1", keys())), APP)
            .await
        {
            Err(AuthError::Forbidden(_)) => {}
            other => panic!("expected Forbidden, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn firestore_failure_fails_closed_with_503() {
        let jwks = Arc::new(StaticJwks::new(vec![jwks_with(
            "k1",
            keys().decoding.clone(),
        )]));
        let firestore = Arc::new(FakeFirestore::with_role(Some("admin")));
        let service = service(jwks, firestore.clone());
        firestore.fail.store(true, Ordering::SeqCst);
        match service
            .authenticate_admin(&bearer(&claims("uid-1", "k1", keys())), APP)
            .await
        {
            Err(AuthError::Unavailable(_)) => {}
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn authenticate_user_ignores_invalid_and_legacy_tokens() {
        let jwks = Arc::new(StaticJwks::new(vec![jwks_with(
            "k1",
            keys().decoding.clone(),
        )]));
        let service = service(jwks, Arc::new(FakeFirestore::with_role(Some("user"))));

        assert!(service
            .authenticate_user(&bearer("legacy-secret"), APP)
            .await
            .is_none());
        assert!(service
            .authenticate_user(&bearer(&claims("uid-1", "k1", keys())), APP)
            .await
            .is_some());
    }

    #[tokio::test]
    async fn service_key_is_constant_time_and_enables_admin() {
        let jwks = Arc::new(StaticJwks::new(vec![jwks_with(
            "k1",
            keys().decoding.clone(),
        )]));
        let service = service(jwks, Arc::new(FakeFirestore::with_role(Some("user"))));

        assert!(service.verify_service_key(&bearer("service-secret")));
        assert!(!service.verify_service_key(&bearer("service-secret-wrong")));
        let identity = service
            .authenticate_admin(&bearer("service-secret"), APP)
            .await
            .expect("service key is admin");
        assert!(identity.is_service());
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
    }

    #[tokio::test]
    async fn legacy_key_only_works_when_enabled_and_only_for_send() {
        let jwks = Arc::new(StaticJwks::new(vec![jwks_with(
            "k1",
            keys().decoding.clone(),
        )]));
        let service = service(jwks, Arc::new(FakeFirestore::with_role(Some("user"))));

        let identity = service
            .authenticate_send(&bearer("legacy-secret"), APP)
            .await
            .expect("legacy key allowed on send");
        assert_eq!(identity.uid, "legacy");

        expect_unauthorized(
            service
                .authenticate_admin(&bearer("legacy-secret"), APP)
                .await,
        )
        .await;
    }

    #[tokio::test]
    async fn rejects_future_iat_auth_time_and_empty_subject() {
        let jwks = Arc::new(StaticJwks::new(vec![jwks_with(
            "k1",
            keys().decoding.clone(),
        )]));
        let service = service(jwks, Arc::new(FakeFirestore::with_role(Some("admin"))));

        let future_iat = token_with("uid-1", "k1", keys(), |claims| {
            claims.insert(
                "iat".into(),
                json!(jsonwebtoken::get_current_timestamp() + 3600),
            );
        });
        expect_unauthorized(service.authenticate_admin(&bearer(&future_iat), APP).await).await;

        let future_auth_time = token_with("uid-1", "k1", keys(), |claims| {
            claims.insert(
                "auth_time".into(),
                json!(jsonwebtoken::get_current_timestamp() + 3600),
            );
        });
        expect_unauthorized(
            service
                .authenticate_admin(&bearer(&future_auth_time), APP)
                .await,
        )
        .await;

        let empty_subject = token_with("uid-1", "k1", keys(), |claims| {
            claims.insert("sub".into(), json!(""));
        });
        expect_unauthorized(
            service
                .authenticate_admin(&bearer(&empty_subject), APP)
                .await,
        )
        .await;
    }

    #[tokio::test]
    async fn anonymous_topic_access_is_limited_to_public_topics() {
        let jwks = Arc::new(StaticJwks::new(vec![jwks_with(
            "k1",
            keys().decoding.clone(),
        )]));
        let service = service(jwks, Arc::new(FakeFirestore::with_role(Some("user"))));
        let headers = HeaderMap::new();

        for topic in ["all", "country_us", "country_IQ", "lang_ar", "lang_EN"] {
            let (access, user) = service
                .authorize_topic(&headers, APP, topic)
                .await
                .unwrap_or_else(|error| panic!("{topic} should be public: {error:?}"));
            assert_eq!(access, TopicAccess::Anonymous);
            assert!(user.is_none());
        }

        for topic in [
            "admins",
            "users",
            "companies",
            "technicals",
            "country_usa",
            "lang_a1",
        ] {
            match service.authorize_topic(&headers, APP, topic).await {
                Err(AuthError::Forbidden(_)) => {}
                other => panic!("{topic} should be forbidden for anonymous, got {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn authenticated_user_gets_role_topics_but_not_admin_topics() {
        let jwks = Arc::new(StaticJwks::new(vec![jwks_with(
            "k1",
            keys().decoding.clone(),
        )]));
        let service = service(jwks, Arc::new(FakeFirestore::with_role(Some("user"))));
        let headers = bearer(&claims("uid-1", "k1", keys()));

        let (access, user) = service
            .authorize_topic(&headers, APP, "users")
            .await
            .expect("users topic for authenticated user");
        assert_eq!(access, TopicAccess::User);
        assert_eq!(user.map(|value| value.uid).as_deref(), Some("uid-1"));

        match service.authorize_topic(&headers, APP, "admins").await {
            Err(AuthError::Forbidden(_)) => {}
            other => panic!("expected Forbidden for admins topic, got {other:?}"),
        }
        match service.authorize_topic(&headers, APP, "custom_topic").await {
            Err(AuthError::Forbidden(_)) => {}
            other => panic!("expected Forbidden for custom topic, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn admin_user_gets_admin_topics() {
        let jwks = Arc::new(StaticJwks::new(vec![jwks_with(
            "k1",
            keys().decoding.clone(),
        )]));
        let service = service(jwks, Arc::new(FakeFirestore::with_role(Some("admin"))));
        let headers = bearer(&claims("uid-admin", "k1", keys()));

        let (access, _) = service
            .authorize_topic(&headers, APP, "admins")
            .await
            .expect("admin topic for admin user");
        assert_eq!(access, TopicAccess::Admin);
    }

    #[tokio::test]
    async fn invalid_legacy_header_is_ignored_for_public_topics() {
        let jwks = Arc::new(StaticJwks::new(vec![jwks_with(
            "k1",
            keys().decoding.clone(),
        )]));
        let service = service(jwks, Arc::new(FakeFirestore::with_role(Some("admin"))));
        let headers = bearer("legacy-secret");

        let (access, _) = service
            .authorize_topic(&headers, APP, "all")
            .await
            .expect("legacy key is not a user token and public topic is allowed");
        assert_eq!(access, TopicAccess::Anonymous);

        match service.authorize_topic(&headers, APP, "admins").await {
            Err(AuthError::Forbidden(_)) => {}
            other => panic!("expected Forbidden for admins topic, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn service_key_can_use_privileged_topics() {
        let jwks = Arc::new(StaticJwks::new(vec![jwks_with(
            "k1",
            keys().decoding.clone(),
        )]));
        let service = service(jwks, Arc::new(FakeFirestore::with_role(Some("user"))));
        let (access, user) = service
            .authorize_topic(&bearer("service-secret"), APP, "admins")
            .await
            .expect("service key is admin");
        assert_eq!(access, TopicAccess::Admin);
        assert!(user.is_none());
    }

    #[test]
    fn public_topic_pattern_is_strict() {
        assert!(is_public_topic("all"));
        assert!(is_public_topic("country_us"));
        assert!(is_public_topic("lang_ar"));
        assert!(!is_public_topic("country_usa"));
        assert!(!is_public_topic("country_u1"));
        assert!(!is_public_topic("lang_"));
        assert!(!is_public_topic("ALL"));
        assert!(!is_public_topic("admins"));
        assert!(topic_allowed("users", TopicAccess::User));
        assert!(!topic_allowed("users", TopicAccess::Anonymous));
        assert!(topic_allowed("anything", TopicAccess::Admin));
    }

    #[test]
    fn parses_cache_control_max_age() {
        assert_eq!(
            parse_cache_max_age("public, max-age=7200, must-revalidate"),
            Some(Duration::from_secs(7200))
        );
        assert_eq!(parse_cache_max_age("no-store"), None);
        assert_eq!(parse_cache_max_age("max-age=oops"), None);
    }

    #[test]
    fn encodes_firestore_path_segments() {
        assert_eq!(encode_path_segment("abcDEF123_-"), "abcDEF123_-");
        assert_eq!(encode_path_segment("a/b"), "a%2Fb");
        assert_eq!(encode_path_segment("../../etc"), "..%2F..%2Fetc");
        assert_eq!(encode_path_segment("a b@c"), "a%20b%40c");
        assert!(is_valid_uid("abc"));
        assert!(!is_valid_uid(""));
        assert!(!is_valid_uid(&"a".repeat(MAX_UID_LENGTH + 1)));
    }
}

//! Authorization helpers built on top of the identity providers.
//!
//! Design notes:
//! - Tokens are verified by the provider chosen per app from its stored
//!   configuration. An app with no configuration keeps the historical Firebase
//!   behavior.
//! - The role is read from `identity.role`: a token claim or a data-source
//!   document. An app with no configuration keeps `users/{uid}.role` with
//!   `admin` as the only admin role.
//! - The service key and the legacy API key are compared in constant time and are
//!   never logged.

use crate::appkeys::{AppKeyStore, KeyCheck, TrustedSender, APP_KEY_PREFIX};
use crate::config::store::ConfigStore;
use crate::config::{IdentityProvider as ProviderConfig, RoleSource, WebSubscribe};
use crate::credentials::ProjectIdMap;
use crate::datasource::registry::DataSourceRegistry;
use crate::datasource::DataSource;
use crate::identity::registry::IdentityRegistry;
use crate::identity::{Identity, IdentityError, IdentityProvider, MAX_TOKEN_LENGTH};
use crate::roles::{
    is_admin_role, legacy_policy, topic_matches, IdentityPolicy, NoSource, RoleError, RoleResolver,
};
use axum::http::{header, HeaderMap, StatusCode};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use subtle::ConstantTimeEq;

/// The privileged topics that signed-in users may always use when an app has no
/// stored configuration (the historical behavior).
pub const LEGACY_SIGNED_IN_TOPICS: [&str; 3] = ["users", "companies", "technicals"];

/// Never verify one token against more than this many candidate apps.
const MAX_CANDIDATE_APPS: usize = 8;

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

    /// True when the request was accepted because of the legacy API key.
    pub fn is_legacy(&self) -> bool {
        self.uid == "legacy"
    }

    /// The identity of an application key.
    pub fn app_key(id: i64) -> Self {
        Self {
            uid: TrustedSender::app_key_uid(id),
            email: None,
        }
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

pub struct AuthService {
    project_map: ProjectIdMap,
    identity: Arc<IdentityRegistry>,
    datasource: Arc<DataSourceRegistry>,
    config: Arc<ConfigStore>,
    auth_config: AuthConfig,
    roles: RoleResolver,
    /// The application keys, when the service knows them.
    app_keys: Option<Arc<AppKeyStore>>,
}

impl AuthService {
    pub fn new(
        project_map: ProjectIdMap,
        identity: Arc<IdentityRegistry>,
        datasource: Arc<DataSourceRegistry>,
        config: Arc<ConfigStore>,
        auth_config: AuthConfig,
    ) -> Self {
        Self {
            project_map,
            identity,
            datasource,
            config,
            auth_config,
            roles: RoleResolver::new(),
            app_keys: None,
        }
    }

    /// The same service with the application keys of the tool.
    pub fn with_app_keys(mut self, app_keys: Arc<AppKeyStore>) -> Self {
        self.app_keys = Some(app_keys);
        self
    }

    /// The application keys of the tool, when the service knows them.
    pub fn app_keys(&self) -> Option<&Arc<AppKeyStore>> {
        self.app_keys.as_ref()
    }

    /// The sender a bearer token stands for, if it is a trusted one.
    ///
    /// The service key is trusted for every app, an application key only for
    /// its own app. A key of another app is refused with `403` and an unknown
    /// or revoked key with `401`.
    pub async fn trusted_sender(
        &self,
        headers: &HeaderMap,
        app: &str,
    ) -> Result<Option<TrustedSender>, AuthError> {
        if self.verify_service_key(headers) {
            return Ok(Some(TrustedSender::Service));
        }
        let Some(store) = self.app_keys.as_ref() else {
            return Ok(None);
        };
        let Some(token) = bearer_token(headers) else {
            return Ok(None);
        };
        if !token.starts_with(APP_KEY_PREFIX) {
            return Ok(None);
        }
        match store.check(token, app).await {
            KeyCheck::Valid { id } => Ok(Some(TrustedSender::AppKey { id })),
            KeyCheck::WrongApp => Err(AuthError::Forbidden("Forbidden".to_string())),
            KeyCheck::Invalid => Err(AuthError::Unauthorized("invalid application key")),
            KeyCheck::NotAKey => Ok(None),
        }
    }

    /// Pure constant-time comparison of two secrets.
    pub fn verify_service_key(&self, headers: &HeaderMap) -> bool {
        let Some(expected) = self.auth_config.service_api_key.as_deref() else {
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
        if !self.auth_config.legacy_enabled {
            return false;
        }
        let Some(expected) = self.auth_config.legacy_api_key.as_deref() else {
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
        match self.trusted_sender(headers, app).await? {
            Some(TrustedSender::Service) => return Ok(AdminIdentity::service()),
            Some(TrustedSender::AppKey { id }) => return Ok(AdminIdentity::app_key(id)),
            None => {}
        }

        let (policy, version) = self.policy_for(app);
        let identity = self.verify_identity(headers, app).await?;
        let role = self.resolve_role(app, version, &policy, &identity).await?;
        if !is_admin_role(role.as_deref(), &policy.admin_roles) {
            return Err(AuthError::Forbidden(
                "the user is not an admin for this app".to_string(),
            ));
        }

        Ok(AdminIdentity {
            uid: identity.uid,
            email: identity.email,
        })
    }

    /// Admin authentication for `/send` with the temporary legacy-key fallback.
    /// The legacy key is never accepted for any other route.
    pub async fn authenticate_send(
        &self,
        headers: &HeaderMap,
        app: &str,
    ) -> Result<AdminIdentity, AuthError> {
        match self.trusted_sender(headers, app).await? {
            Some(TrustedSender::Service) => return Ok(AdminIdentity::service()),
            Some(TrustedSender::AppKey { id }) => return Ok(AdminIdentity::app_key(id)),
            None => {}
        }

        if self.verify_legacy_key(headers) {
            tracing::warn!(
                "Legacy API_KEY accepted for /send; migrate the admin app to Firebase ID tokens"
            );
            return Ok(AdminIdentity::legacy());
        }

        self.authenticate_admin(headers, app).await
    }

    /// Returns a valid identity for any user (no role check).
    /// Invalid or missing tokens (including unrecognised legacy keys) yield `None`.
    ///
    /// The topic route uses [`Self::authorize_topic`], which needs the full
    /// identity; this entry point stays public for the routes wired in T6.
    #[allow(dead_code)]
    pub async fn authenticate_user(&self, headers: &HeaderMap, app: &str) -> Option<UserIdentity> {
        let token = bearer_token(headers)?;
        let provider = self.identity.provider_for(app).await.ok()?;
        provider
            .verify(token)
            .await
            .ok()
            .map(|identity| UserIdentity { uid: identity.uid })
    }

    /// The verified identity of a user-facing route caller (T6 devices).
    ///
    /// A missing or invalid token is `401`, an unavailable key source is `503`
    /// and an app with neither configuration nor credentials is `404`. Service
    /// and legacy keys are not identities, so they never authenticate here.
    pub async fn authenticate_identity(
        &self,
        headers: &HeaderMap,
        app: &str,
    ) -> Result<Identity, AuthError> {
        self.verify_identity(headers, app).await
    }

    /// Admin authentication for `/apps` and app-less admin queries.
    ///
    /// Candidate apps come from the server-side project map. A Firebase app (or
    /// an app without configuration) is selected by the unverified `aud` claim,
    /// a JWT app by the unverified `iss` claim. The signature is always verified
    /// against each candidate app before any role lookup. At most
    /// [`MAX_CANDIDATE_APPS`] apps are tried. Returns the identity and the list
    /// of apps where the user is an admin (`None` means the service key: all
    /// apps).
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

        let selection = unverified_selection(token).ok_or(AuthError::Unauthorized(
            "token has no usable audience or issuer",
        ))?;
        let mut candidates = self.candidate_apps(&selection).await;
        if candidates.is_empty() {
            return Err(AuthError::Unauthorized(
                "token audience or issuer does not match a registered app",
            ));
        }
        if candidates.len() > MAX_CANDIDATE_APPS {
            tracing::warn!(
                candidates = candidates.len(),
                limit = MAX_CANDIDATE_APPS,
                "too many candidate apps for this token; verifying the first ones only"
            );
            candidates.truncate(MAX_CANDIDATE_APPS);
        }

        let mut verified = Vec::new();
        for app in &candidates {
            let provider = self
                .identity
                .provider_for(app)
                .await
                .map_err(map_identity_error)?;
            match provider.verify(token).await {
                Ok(identity) => verified.push((app.clone(), identity)),
                Err(IdentityError::Invalid(_)) => {}
                Err(IdentityError::Unavailable(reason)) => {
                    return Err(AuthError::Unavailable(reason))
                }
            }
        }
        if verified.is_empty() {
            return Err(AuthError::Unauthorized(
                "token is not valid for any registered app",
            ));
        }

        let mut admin_apps = Vec::new();
        let mut first_admin = None;
        for (app, identity) in verified {
            if self.is_admin_identity(&app, &identity).await? {
                if first_admin.is_none() {
                    first_admin = Some(identity);
                }
                admin_apps.push(app);
            }
        }

        match first_admin {
            Some(identity) => Ok((
                AdminIdentity {
                    uid: identity.uid,
                    email: identity.email,
                },
                Some(admin_apps),
            )),
            None => Err(AuthError::Forbidden(
                "the user is not an admin for any registered app".to_string(),
            )),
        }
    }

    /// Role check for an already verified identity. Used by topic subscription to
    /// allow admins to use privileged topics.
    pub async fn is_admin_identity(
        &self,
        app: &str,
        identity: &Identity,
    ) -> Result<bool, AuthError> {
        let (policy, version) = self.policy_for(app);
        let role = self.resolve_role(app, version, &policy, identity).await?;
        Ok(is_admin_role(role.as_deref(), &policy.admin_roles))
    }

    /// The role of a verified identity, for template senders (T8).
    ///
    /// A missing data source or role configuration is not an error here: the
    /// caller decides whether the template requires a role.
    pub async fn resolve_sender_role(
        &self,
        app: &str,
        identity: &Identity,
    ) -> Result<Option<String>, AuthError> {
        let (policy, version) = self.policy_for(app);
        self.resolve_role(app, version, &policy, identity).await
    }

    /// The role policy and configuration version of an app, or the legacy policy
    /// for an app that has no stored configuration.
    fn policy_for(&self, app: &str) -> (IdentityPolicy, i64) {
        match self.config.get(app) {
            Some(snapshot) => (snapshot.config.identity.clone(), snapshot.version),
            None => (legacy_policy(), 0),
        }
    }

    async fn resolve_role(
        &self,
        app: &str,
        version: i64,
        policy: &IdentityPolicy,
        identity: &Identity,
    ) -> Result<Option<String>, AuthError> {
        let source: Arc<dyn DataSource> = match &policy.role {
            RoleSource::Claim { .. } => Arc::new(NoSource),
            RoleSource::DataSource { .. } => {
                self.datasource.source_for(app).await.map_err(|error| {
                    tracing::warn!(
                        app = %app,
                        error = ?error,
                        "the data source for the role lookup is unavailable"
                    );
                    AuthError::Unavailable("role lookup failed".to_string())
                })?
            }
        };
        self.roles
            .role(app, version, policy, identity, source.as_ref())
            .await
            .map_err(|RoleError::Unavailable(reason)| AuthError::Unavailable(reason))
    }

    /// Verifies a bearer token with the provider configured for `app`.
    async fn verify_identity(&self, headers: &HeaderMap, app: &str) -> Result<Identity, AuthError> {
        let token = bearer_token(headers).ok_or(AuthError::Unauthorized("missing bearer token"))?;
        let provider = self.configured_provider(app).await?;
        provider.verify(token).await.map_err(map_identity_error)
    }

    /// Returns the provider for `app` and keeps the historical `404` for an app
    /// that has neither configuration nor loaded credentials.
    async fn configured_provider(&self, app: &str) -> Result<Arc<dyn IdentityProvider>, AuthError> {
        if self.config.get(app).is_none() {
            let known = {
                let projects = self.project_map.read().await;
                projects
                    .get(app)
                    .is_some_and(|project| !project.trim().is_empty())
            };
            if !known {
                return Err(AuthError::AppNotFound(app.to_string()));
            }
        }
        self.identity
            .provider_for(app)
            .await
            .map_err(map_identity_error)
    }

    async fn candidate_apps(&self, selection: &TokenSelection) -> Vec<String> {
        let projects = { self.project_map.read().await.clone() };
        let mut apps: Vec<String> = projects.keys().cloned().collect();
        // Alphabetical order keeps the truncation deterministic.
        apps.sort();
        apps.retain(|app| match self.config.get(app) {
            Some(snapshot) => match &snapshot.config.identity.provider {
                ProviderConfig::Jwt { issuer, .. } => {
                    selection.issuer.as_deref() == Some(issuer.as_str())
                }
                ProviderConfig::Firebase => {
                    audience_matches_project(&projects, app, selection.audience.as_deref())
                }
            },
            None => audience_matches_project(&projects, app, selection.audience.as_deref()),
        });
        apps
    }
}

fn map_identity_error(error: IdentityError) -> AuthError {
    match error {
        IdentityError::Invalid(reason) => AuthError::Unauthorized(reason),
        IdentityError::Unavailable(reason) => AuthError::Unavailable(reason),
    }
}

fn audience_matches_project(
    projects: &HashMap<String, String>,
    app: &str,
    audience: Option<&str>,
) -> bool {
    let Some(audience) = audience else {
        return false;
    };
    projects
        .get(app)
        .is_some_and(|project| project.trim() == audience)
}

/// Access level resolved for a topic subscription request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopicAccess {
    Anonymous,
    User,
    Admin,
}

/// Anonymous clients may only use topics allowed by `web_subscribe.anonymous`.
/// Authenticated users additionally get `web_subscribe.signed_in`. Admins may
/// use every valid topic.
///
/// `None` means the app has no stored configuration: the historical behavior is
/// kept exactly (`all`, `country_xx`, `lang_xx` and the three role topics for
/// signed-in users).
pub fn topic_allowed(
    topic: &str,
    access: TopicAccess,
    web_subscribe: Option<&WebSubscribe>,
) -> bool {
    match access {
        TopicAccess::Admin => true,
        TopicAccess::User => match web_subscribe {
            None => is_public_topic(topic) || LEGACY_SIGNED_IN_TOPICS.contains(&topic),
            Some(web_subscribe) => web_subscribe
                .anonymous
                .iter()
                .chain(web_subscribe.signed_in.iter())
                .any(|pattern| topic_matches(pattern, topic)),
        },
        TopicAccess::Anonymous => match web_subscribe {
            None => is_public_topic(topic),
            Some(web_subscribe) => web_subscribe
                .anonymous
                .iter()
                .any(|pattern| topic_matches(pattern, topic)),
        },
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
    /// - The service key is accepted as an admin.
    /// - A valid identity may use the `web_subscribe` patterns for signed-in
    ///   users; an admin role also gets every valid topic.
    /// - Missing or invalid tokens are treated as anonymous, so a bad header is
    ///   ignored instead of rejecting the request.
    /// - A role lookup failure returns `503` only when the role is needed.
    pub async fn authorize_topic(
        &self,
        headers: &HeaderMap,
        app: &str,
        topic: &str,
    ) -> Result<(TopicAccess, Option<UserIdentity>), AuthError> {
        match self.trusted_sender(headers, app).await? {
            Some(TrustedSender::Service) => return Ok((TopicAccess::Admin, None)),
            // An application key is an admin of its own app.
            Some(TrustedSender::AppKey { .. }) => return Ok((TopicAccess::Admin, None)),
            None => {}
        }

        let snapshot = self.config.get(app);
        let web_subscribe = snapshot
            .as_ref()
            .map(|snapshot| &snapshot.config.web_subscribe);

        if let Some(identity) = self.authenticated_identity(headers, app).await {
            if topic_allowed(topic, TopicAccess::User, web_subscribe) {
                return Ok((TopicAccess::User, Some(UserIdentity { uid: identity.uid })));
            }
            if self.is_admin_identity(app, &identity).await? {
                return Ok((TopicAccess::Admin, Some(UserIdentity { uid: identity.uid })));
            }
            return Err(AuthError::Forbidden(format!(
                "topic '{topic}' requires an admin token"
            )));
        }

        if topic_allowed(topic, TopicAccess::Anonymous, web_subscribe) {
            Ok((TopicAccess::Anonymous, None))
        } else {
            Err(AuthError::Forbidden(format!(
                "topic '{topic}' requires a signed-in user"
            )))
        }
    }

    /// Best-effort verification for topic subscription: any failure means the
    /// caller is anonymous (the historical behavior).
    async fn authenticated_identity(&self, headers: &HeaderMap, app: &str) -> Option<Identity> {
        let token = bearer_token(headers)?;
        let provider = self.identity.provider_for(app).await.ok()?;
        provider.verify(token).await.ok()
    }
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

struct TokenSelection {
    audience: Option<String>,
    issuer: Option<String>,
}

fn unverified_selection(token: &str) -> Option<TokenSelection> {
    #[derive(Deserialize)]
    struct SelectionClaims {
        #[serde(default)]
        aud: Option<Value>,
        #[serde(default)]
        iss: Option<String>,
    }

    let data = jsonwebtoken::dangerous::insecure_decode::<SelectionClaims>(token).ok()?;
    let audience = match data.claims.aud {
        Some(Value::String(audience)) => Some(audience),
        Some(Value::Array(values)) => values.first()?.as_str().map(ToString::to_string),
        Some(_) => None,
        None => None,
    };
    let issuer = data.claims.iss.filter(|issuer| !issuer.is_empty());
    if audience.is_none() && issuer.is_none() {
        return None;
    }
    Some(TokenSelection { audience, issuer })
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
    use crate::config::store::ConfigStore;
    use crate::config::AppConfig;
    use crate::credentials::AuthMap;
    use crate::datasource::test_support::{json as json_response, TestServer};
    use crate::identity::firebase::FirebaseVerifier;
    use crate::identity::is_valid_uid;
    use crate::identity::jwks::{
        parse_cache_max_age, FetchedJwks, JwkProvider, JwksCache, MIN_JWK_REFRESH,
    };
    use crate::identity::jwt::EnvSource;
    use crate::identity::registry::IdentityRegistry;
    use crate::identity::MAX_UID_LENGTH;
    use async_trait::async_trait;
    use jsonwebtoken::{encode, Algorithm, DecodingKey, EncodingKey, Header};
    use rand::rngs::OsRng;
    use rsa::pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding};
    use rsa::RsaPrivateKey;
    use serde_json::{json, Map, Value};
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::collections::HashMap;
    use std::str::FromStr;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Mutex, OnceLock};
    use tokio::sync::RwLock;

    const APP: &str = "test-app";
    const PROJECT: &str = "test-project";
    const ISSUER: &str = "https://issuer.example.com/";
    const AUDIENCE: &str = "test-audience";
    const JWT_SECRET_ENV: &str = "CLIENT_JWT_SECRET";
    const JWT_SECRET: &str = "jwt-test-secret-jwt-test-secret-0123456789";
    const RESOLVER_SECRET_ENV: &str = "RESOLVER_SECRET";
    const RESOLVER_SECRET: &str = "resolver-secret-resolver-secret-0001";

    struct FakeEnv {
        values: HashMap<String, String>,
    }

    impl FakeEnv {
        fn new(pairs: &[(&str, &str)]) -> Self {
            Self {
                values: pairs
                    .iter()
                    .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
                    .collect(),
            }
        }

        fn jwt() -> Self {
            Self::new(&[
                (JWT_SECRET_ENV, JWT_SECRET),
                (RESOLVER_SECRET_ENV, RESOLVER_SECRET),
            ])
        }
    }

    impl EnvSource for FakeEnv {
        fn get(&self, name: &str) -> Option<String> {
            self.values.get(name).cloned()
        }
    }

    /// A data source that answers from memory, for the legacy role document.
    struct RoleDocSource {
        doc: Option<Value>,
        fail: bool,
    }

    #[async_trait]
    impl DataSource for RoleDocSource {
        async fn get_doc(
            &self,
            _path: &str,
        ) -> Result<Option<crate::datasource::Doc>, crate::datasource::DataError> {
            if self.fail {
                return Err(crate::datasource::DataError::Unavailable(
                    "the backend is down".to_string(),
                ));
            }
            Ok(self.doc.clone())
        }

        async fn members(
            &self,
            _collection: &str,
            _field: &str,
            _value: &str,
            _limit: usize,
        ) -> Result<Vec<String>, crate::datasource::DataError> {
            Err(crate::datasource::DataError::Unavailable(
                "unused in auth tests".to_string(),
            ))
        }
    }

    fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
        mutex
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    async fn memory_store() -> Arc<ConfigStore> {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .expect("the options must parse")
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("the in-memory database must open");
        crate::db::migrate(&pool)
            .await
            .expect("the migration must run");
        Arc::new(
            ConfigStore::new(pool)
                .await
                .expect("the config store must open"),
        )
    }

    async fn save_app(store: &ConfigStore, app: &str, sections: &[(&str, Value)]) {
        store
            .create_app(app, "tester")
            .await
            .expect("the app must be created");
        let mut version = 1;
        for (section, value) in sections {
            version = store
                .save_section(app, section, value.clone(), "tester", version)
                .await
                .expect("the section must be saved");
        }
    }

    fn auth_service(
        store: Arc<ConfigStore>,
        projects: &[(&str, &str)],
        env: Arc<FakeEnv>,
    ) -> AuthService {
        let project_map: ProjectIdMap = Arc::new(RwLock::new(
            projects
                .iter()
                .map(|(app, project)| ((*app).to_string(), (*project).to_string()))
                .collect(),
        ));
        let auth_map: AuthMap = Arc::new(RwLock::new(HashMap::new()));
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("the test client must build");
        let env: Arc<dyn EnvSource> = env;
        let identity = Arc::new(IdentityRegistry::new(
            store.clone(),
            project_map.clone(),
            client.clone(),
            env.clone(),
        ));
        let datasource = Arc::new(DataSourceRegistry::new(
            store.clone(),
            project_map.clone(),
            auth_map,
            client,
            env,
        ));
        AuthService::new(
            project_map,
            identity,
            datasource,
            store,
            AuthConfig {
                service_api_key: Some("service-secret".to_string()),
                legacy_api_key: Some("legacy-secret".to_string()),
                legacy_enabled: true,
            },
        )
    }

    fn jwt_identity(role: Value, admin_roles: &[&str]) -> Value {
        json!({
            "provider": {
                "type": "jwt",
                "algorithms": ["HS256"],
                "hs256_secret_env": JWT_SECRET_ENV,
                "issuer": ISSUER,
                "audience": AUDIENCE,
                "uid_claim": "sub",
                "max_age_seconds": 3600
            },
            "role": role,
            "admin_roles": admin_roles
        })
    }

    fn claim_role(claim: &str) -> Value {
        json!({ "source": "claim", "claim": claim })
    }

    fn legacy_web_subscribe() -> Value {
        json!({
            "anonymous": ["all", "country_*", "lang_*"],
            "signed_in": ["users", "companies", "technicals"]
        })
    }

    async fn claim_service(admin_roles: &[&str]) -> AuthService {
        let store = memory_store().await;
        save_app(
            &store,
            APP,
            &[("identity", jwt_identity(claim_role("role"), admin_roles))],
        )
        .await;
        auth_service(store, &[(APP, PROJECT)], Arc::new(FakeEnv::jwt()))
    }

    async fn unconfigured_service() -> AuthService {
        auth_service(
            memory_store().await,
            &[(APP, PROJECT)],
            Arc::new(FakeEnv::jwt()),
        )
    }

    fn claims_value(issuer: &str, audience: &str, uid: &str, role: &str) -> Value {
        let now = jsonwebtoken::get_current_timestamp();
        json!({
            "sub": uid,
            "aud": audience,
            "iss": issuer,
            "exp": now + 3600,
            "iat": now - 10,
            "role": role,
            "email": "admin@example.com",
        })
    }

    fn sign_claims(secret: &str, claims: &Value) -> String {
        encode(
            &Header::new(Algorithm::HS256),
            claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .expect("the token must encode")
    }

    fn token(uid: &str, role: &str) -> String {
        sign_claims(JWT_SECRET, &claims_value(ISSUER, AUDIENCE, uid, role))
    }

    fn token_with(uid: &str, role: &str, mutate: impl FnOnce(&mut Map<String, Value>)) -> String {
        let mut claims = claims_value(ISSUER, AUDIENCE, uid, role);
        mutate(claims.as_object_mut().expect("the claims object"));
        sign_claims(JWT_SECRET, &claims)
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

    async fn expect_forbidden(result: Result<AdminIdentity, AuthError>) {
        match result {
            Err(AuthError::Forbidden(_)) => {}
            other => panic!("expected Forbidden, got {other:?}"),
        }
    }

    // ---------- Firebase verifier guards (kept from the old AuthService) ----------

    struct TestKeys {
        private_pem: String,
        decoding: DecodingKey,
    }

    fn generate_keys() -> TestKeys {
        let private = RsaPrivateKey::new(&mut OsRng, 2048).expect("RSA key");
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
                max_age: std::time::Duration::from_secs(3600),
            })
        }
    }

    fn jwks_with(kid: &str, key: DecodingKey) -> HashMap<String, DecodingKey> {
        HashMap::from([(kid.to_string(), key)])
    }

    fn firebase_token_with(
        uid: &str,
        kid: &str,
        key: &TestKeys,
        mutate: impl FnOnce(&mut Map<String, Value>),
    ) -> String {
        let now = jsonwebtoken::get_current_timestamp();
        let mut payload = json!({
            "sub": uid,
            "aud": PROJECT,
            "iss": format!("https://securetoken.google.com/{PROJECT}"),
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

    fn firebase_token(uid: &str, kid: &str, key: &TestKeys) -> String {
        firebase_token_with(uid, kid, key, |_| {})
    }

    // ---------- AuthService tests ----------

    #[tokio::test]
    async fn accepts_valid_admin_token() {
        let service = claim_service(&["admin"]).await;
        let identity = service
            .authenticate_admin(&bearer(&token("uid-1", "admin")), APP)
            .await
            .expect("valid admin token");
        assert_eq!(identity.uid, "uid-1");
        assert_eq!(identity.email.as_deref(), Some("admin@example.com"));
    }

    #[tokio::test]
    async fn rejects_expired_token() {
        let service = claim_service(&["admin"]).await;
        let expired = token_with("uid-1", "admin", |claims| {
            claims.insert("exp".into(), json!(1000));
        });
        expect_unauthorized(service.authenticate_admin(&bearer(&expired), APP).await).await;
    }

    #[tokio::test]
    async fn rejects_wrong_issuer_and_audience() {
        let service = claim_service(&["admin"]).await;
        let wrong_iss = token_with("uid-1", "admin", |claims| {
            claims.insert("iss".into(), json!("https://other.example.com/"));
        });
        expect_unauthorized(service.authenticate_admin(&bearer(&wrong_iss), APP).await).await;

        let wrong_aud = token_with("uid-1", "admin", |claims| {
            claims.insert("aud".into(), json!("other-audience"));
        });
        expect_unauthorized(service.authenticate_admin(&bearer(&wrong_aud), APP).await).await;
    }

    #[tokio::test]
    async fn rejects_signature_from_another_key() {
        let service = claim_service(&["admin"]).await;
        let token = sign_claims(
            "another-secret-another-secret-0123456789",
            &claims_value(ISSUER, AUDIENCE, "uid-1", "admin"),
        );
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
                "aud": AUDIENCE,
                "iss": ISSUER,
                "exp": now + 3600,
                "iat": now - 10,
            })
            .to_string(),
        );
        let token = format!("{header}.{payload}.");

        let service = claim_service(&["admin"]).await;
        expect_unauthorized(service.authenticate_admin(&bearer(&token), APP).await).await;
    }

    #[tokio::test]
    async fn unknown_kid_refreshes_keys_once_then_throttles() {
        let first = jwks_with("k1", keys().decoding.clone());
        let mut second = first.clone();
        second.insert("k2".to_string(), keys().decoding.clone());
        let jwks = Arc::new(StaticJwks::new(vec![first, second]));
        let cache = Arc::new(Mutex::new(JwksCache::new()));
        let verifier = FirebaseVerifier::with_cache(jwks.clone(), cache.clone());

        // Warm the cache with k1 (refresh #1).
        let identity = verifier
            .verify(&firebase_token("uid-1", "k1", keys()), PROJECT)
            .await
            .expect("warm cache");
        assert_eq!(identity.uid, "uid-1");
        assert_eq!(jwks.calls(), 1);

        // Pretend the cache was loaded a while ago, then use a rotated kid.
        {
            let mut cache = lock(&cache);
            cache.last_refresh = std::time::Instant::now() - MIN_JWK_REFRESH;
        }
        verifier
            .verify(&firebase_token("uid-1", "k2", keys()), PROJECT)
            .await
            .expect("rotated kid accepted after refresh");
        assert_eq!(jwks.calls(), 2);

        // A second unknown kid must not trigger another refresh inside the window.
        let unknown = firebase_token("uid-1", "k3", keys());
        let error = verifier
            .verify(&unknown, PROJECT)
            .await
            .expect_err("an unknown kid must be rejected");
        assert!(matches!(error, IdentityError::Invalid(_)));
        assert_eq!(jwks.calls(), 2);
    }

    #[tokio::test]
    async fn non_admin_role_is_forbidden() {
        let service = claim_service(&["admin"]).await;
        expect_forbidden(
            service
                .authenticate_admin(&bearer(&token("uid-1", "user")), APP)
                .await,
        )
        .await;
    }

    #[tokio::test]
    async fn missing_user_document_is_not_admin() {
        let server = TestServer::start(vec![json_response(404, "Not Found", "")]).await;
        let service = data_source_service(&server.base_url).await;
        expect_forbidden(
            service
                .authenticate_admin(&bearer(&token("uid-1", "admin")), APP)
                .await,
        )
        .await;
    }

    #[tokio::test]
    async fn role_lookup_failure_is_503_not_403() {
        let server = TestServer::start(vec![json_response(500, "Internal Server Error", "")]).await;
        let service = data_source_service(&server.base_url).await;
        match service
            .authenticate_admin(&bearer(&token("uid-1", "admin")), APP)
            .await
        {
            Err(AuthError::Unavailable(_)) => {}
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn authenticate_user_ignores_invalid_and_legacy_tokens() {
        let service = claim_service(&["admin"]).await;

        assert!(service
            .authenticate_user(&bearer("legacy-secret"), APP)
            .await
            .is_none());
        assert!(service
            .authenticate_user(&bearer(&token("uid-1", "user")), APP)
            .await
            .is_some());
    }

    #[tokio::test]
    async fn service_key_is_constant_time_and_enables_admin() {
        let service = unconfigured_service().await;

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
        let service = unconfigured_service().await;

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
        let service = claim_service(&["admin"]).await;

        let future_iat = token_with("uid-1", "admin", |claims| {
            claims.insert(
                "iat".into(),
                json!(jsonwebtoken::get_current_timestamp() + 3600),
            );
        });
        expect_unauthorized(service.authenticate_admin(&bearer(&future_iat), APP).await).await;

        let empty_subject = token_with("uid-1", "admin", |claims| {
            claims.insert("sub".into(), json!(""));
        });
        expect_unauthorized(
            service
                .authenticate_admin(&bearer(&empty_subject), APP)
                .await,
        )
        .await;

        // Firebase ID tokens carry `auth_time`; the legacy provider still rejects
        // a future value even though JWT providers have no such claim.
        let jwks = Arc::new(StaticJwks::new(vec![jwks_with(
            "k1",
            keys().decoding.clone(),
        )]));
        let verifier = FirebaseVerifier::new(jwks);
        let future_auth_time = firebase_token_with("uid-1", "k1", keys(), |claims| {
            claims.insert(
                "auth_time".into(),
                json!(jsonwebtoken::get_current_timestamp() + 3600),
            );
        });
        let error = verifier
            .verify(&future_auth_time, PROJECT)
            .await
            .expect_err("a future auth_time must be rejected");
        assert!(matches!(error, IdentityError::Invalid(_)));
    }

    #[tokio::test]
    async fn anonymous_topic_access_is_limited_to_public_topics() {
        let service = unconfigured_service().await;
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
        let store = memory_store().await;
        save_app(
            &store,
            APP,
            &[
                ("identity", jwt_identity(claim_role("role"), &["admin"])),
                ("web_subscribe", legacy_web_subscribe()),
            ],
        )
        .await;
        let service = auth_service(store, &[(APP, PROJECT)], Arc::new(FakeEnv::jwt()));
        let headers = bearer(&token("uid-1", "user"));

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
        let service = claim_service(&["admin"]).await;
        let headers = bearer(&token("uid-admin", "admin"));

        let (access, _) = service
            .authorize_topic(&headers, APP, "admins")
            .await
            .expect("admin topic for admin user");
        assert_eq!(access, TopicAccess::Admin);
    }

    #[tokio::test]
    async fn invalid_legacy_header_is_ignored_for_public_topics() {
        let service = unconfigured_service().await;
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
        let service = unconfigured_service().await;
        let (access, user) = service
            .authorize_topic(&bearer("service-secret"), APP, "admins")
            .await
            .expect("service key is admin");
        assert_eq!(access, TopicAccess::Admin);
        assert!(user.is_none());
    }

    #[tokio::test]
    async fn unconfigured_app_keeps_legacy_admin_behavior() {
        let service = unconfigured_service().await;
        let (policy, version) = service.policy_for(APP);
        assert_eq!(version, 0);
        assert_eq!(policy, AppConfig::default_firebase().identity);
        assert!(matches!(&policy.provider, ProviderConfig::Firebase));
        assert_eq!(policy.admin_roles, vec!["admin".to_string()]);
        match &policy.role {
            RoleSource::DataSource {
                doc,
                field,
                cache_seconds,
            } => {
                assert_eq!(doc, "users/{uid}");
                assert_eq!(field, "role");
                assert_eq!(*cache_seconds, 60);
            }
            other => panic!("expected the legacy data source role, got {other:?}"),
        }

        let identity = Identity {
            uid: "uid-1".to_string(),
            email: None,
            name: None,
            claims: json!({}),
        };
        let admin = RoleDocSource {
            doc: Some(json!({ "role": "admin" })),
            fail: false,
        };
        let role = service
            .roles
            .role(APP, version, &policy, &identity, &admin)
            .await
            .expect("the legacy role document must be read");
        assert!(is_admin_role(role.as_deref(), &policy.admin_roles));

        let missing = RoleDocSource {
            doc: None,
            fail: false,
        };
        let other_identity = Identity {
            uid: "uid-2".to_string(),
            email: None,
            name: None,
            claims: json!({}),
        };
        let role = service
            .roles
            .role(APP, version, &policy, &other_identity, &missing)
            .await
            .expect("a missing legacy document is not an error");
        assert!(!is_admin_role(role.as_deref(), &policy.admin_roles));
    }

    #[tokio::test]
    async fn unconfigured_app_keeps_legacy_topics() {
        let service = unconfigured_service().await;
        let headers = HeaderMap::new();

        for topic in ["all", "country_us", "lang_ar"] {
            let (access, _) = service
                .authorize_topic(&headers, APP, topic)
                .await
                .expect("the legacy public topics must stay public");
            assert_eq!(access, TopicAccess::Anonymous);
        }
        for topic in ["users", "companies", "technicals", "admins"] {
            match service.authorize_topic(&headers, APP, topic).await {
                Err(AuthError::Forbidden(_)) => {}
                other => panic!("{topic} must stay forbidden, got {other:?}"),
            }
        }

        let (access, _) = service
            .authorize_topic(&bearer("legacy-secret"), APP, "all")
            .await
            .expect("an invalid token is anonymous");
        assert_eq!(access, TopicAccess::Anonymous);
    }

    #[tokio::test]
    async fn configured_claim_role_grants_admin() {
        let service = claim_service(&["admin"]).await;
        let identity = service
            .authenticate_admin(&bearer(&token("uid-1", "admin")), APP)
            .await
            .expect("the claim role must grant admin");
        assert_eq!(identity.uid, "uid-1");
    }

    #[tokio::test]
    async fn configured_data_source_role_grants_admin() {
        let server = TestServer::start(vec![json_response(200, "OK", r#"{"role":"admin"}"#)]).await;
        let service = data_source_service(&server.base_url).await;
        let identity = service
            .authenticate_admin(&bearer(&token("uid-1", "user")), APP)
            .await
            .expect("the data source role must grant admin");
        assert_eq!(identity.uid, "uid-1");
    }

    #[tokio::test]
    async fn custom_admin_roles_list_is_honored() {
        let service = claim_service(&["manager"]).await;
        service
            .authenticate_admin(&bearer(&token("uid-1", "manager")), APP)
            .await
            .expect("the configured admin role is accepted");
        expect_forbidden(
            service
                .authenticate_admin(&bearer(&token("uid-1", "admin")), APP)
                .await,
        )
        .await;
    }

    #[tokio::test]
    async fn web_subscribe_custom_patterns_allow_and_deny() {
        let store = memory_store().await;
        save_app(
            &store,
            APP,
            &[
                ("identity", jwt_identity(claim_role("role"), &["admin"])),
                (
                    "web_subscribe",
                    json!({
                        "anonymous": ["all", "country_*"],
                        "signed_in": ["users", "cat_*"]
                    }),
                ),
            ],
        )
        .await;
        let service = auth_service(store, &[(APP, PROJECT)], Arc::new(FakeEnv::jwt()));
        let user = bearer(&token("uid-1", "user"));

        for topic in ["all", "country_us", "users", "cat_books"] {
            let (access, _) = service
                .authorize_topic(&user, APP, topic)
                .await
                .expect("the configured pattern must allow the topic");
            assert_eq!(access, TopicAccess::User, "{topic}");
        }
        for topic in ["admins", "lang_ar"] {
            match service.authorize_topic(&user, APP, topic).await {
                Err(AuthError::Forbidden(_)) => {}
                other => panic!("{topic} must be forbidden, got {other:?}"),
            }
        }

        let anonymous = HeaderMap::new();
        for topic in ["all", "country_us"] {
            let (access, _) = service
                .authorize_topic(&anonymous, APP, topic)
                .await
                .expect("the anonymous pattern must allow the topic");
            assert_eq!(access, TopicAccess::Anonymous, "{topic}");
        }
        match service.authorize_topic(&anonymous, APP, "users").await {
            Err(AuthError::Forbidden(_)) => {}
            other => panic!("users must be forbidden for anonymous, got {other:?}"),
        }

        let admin = bearer(&token("uid-admin", "admin"));
        let (access, _) = service
            .authorize_topic(&admin, APP, "admins")
            .await
            .expect("an admin role allows every valid topic");
        assert_eq!(access, TopicAccess::Admin);
    }

    #[tokio::test]
    async fn legacy_hardcoded_topics_not_applied_to_configured_app() {
        let store = memory_store().await;
        save_app(
            &store,
            APP,
            &[
                ("identity", jwt_identity(claim_role("role"), &["admin"])),
                (
                    "web_subscribe",
                    json!({ "anonymous": [], "signed_in": ["promos"] }),
                ),
            ],
        )
        .await;
        let service = auth_service(store, &[(APP, PROJECT)], Arc::new(FakeEnv::jwt()));
        let user = bearer(&token("uid-1", "user"));

        match service.authorize_topic(&user, APP, "users").await {
            Err(AuthError::Forbidden(_)) => {}
            other => panic!("users must not be inherited, got {other:?}"),
        }
        let (access, _) = service
            .authorize_topic(&user, APP, "promos")
            .await
            .expect("the configured topic must be allowed");
        assert_eq!(access, TopicAccess::User);
    }

    #[tokio::test]
    async fn authenticate_admin_any_jwt_app_selected_by_issuer() {
        const OTHER_APP: &str = "other-app";
        const OTHER_ISSUER: &str = "https://issuer-b.example.com/";
        const OTHER_SECRET_ENV: &str = "OTHER_JWT_SECRET";
        const OTHER_SECRET: &str = "other-jwt-secret-other-jwt-secret-012345";

        let store = memory_store().await;
        save_app(
            &store,
            APP,
            &[("identity", jwt_identity(claim_role("role"), &["admin"]))],
        )
        .await;
        let mut other_identity = jwt_identity(claim_role("role"), &["admin"]);
        other_identity["provider"]["issuer"] = json!(OTHER_ISSUER);
        other_identity["provider"]["hs256_secret_env"] = json!(OTHER_SECRET_ENV);
        save_app(&store, OTHER_APP, &[("identity", other_identity)]).await;

        let env = Arc::new(FakeEnv::new(&[
            (JWT_SECRET_ENV, JWT_SECRET),
            (OTHER_SECRET_ENV, OTHER_SECRET),
        ]));
        let service = auth_service(store, &[(APP, PROJECT), (OTHER_APP, "other-project")], env);
        let token = token("uid-1", "admin");

        let (identity, apps) = service
            .authenticate_admin_any(&bearer(&token))
            .await
            .expect("the JWT issuer selects the app");
        assert_eq!(identity.uid, "uid-1");
        assert_eq!(apps, Some(vec![APP.to_string()]));
    }

    #[tokio::test]
    async fn authenticate_admin_any_limits_candidates() {
        let store = memory_store().await;
        let mut projects = Vec::new();
        for index in 0..10 {
            let app = format!("app-{index:02}");
            save_app(
                &store,
                &app,
                &[("identity", jwt_identity(claim_role("role"), &["admin"]))],
            )
            .await;
            projects.push((app, PROJECT));
        }
        let project_refs: Vec<(&str, &str)> = projects
            .iter()
            .map(|(app, project)| (app.as_str(), *project))
            .collect();
        let service = auth_service(store, project_refs.as_slice(), Arc::new(FakeEnv::jwt()));

        let (_, apps) = service
            .authenticate_admin_any(&bearer(&token("uid-1", "admin")))
            .await
            .expect("the first candidate apps are verified");
        let apps = apps.expect("the service key was not used");
        assert_eq!(apps.len(), MAX_CANDIDATE_APPS);
        assert_eq!(apps.first().map(String::as_str), Some("app-00"));
        assert_eq!(apps.last().map(String::as_str), Some("app-07"));
    }

    #[tokio::test]
    async fn jwt_app_admin_end_to_end() {
        let service = claim_service(&["admin"]).await;
        let token = token("uid-1", "admin");

        let identity = service
            .authenticate_admin(&bearer(&token), APP)
            .await
            .expect("the HS256 provider from the fake environment must verify");
        assert_eq!(identity.uid, "uid-1");

        let send = service
            .authenticate_send(&bearer(&token), APP)
            .await
            .expect("a verified admin can send");
        assert_eq!(send.uid, "uid-1");

        let (access, user) = service
            .authorize_topic(&bearer(&token), APP, "admins")
            .await
            .expect("an admin can use every valid topic");
        assert_eq!(access, TopicAccess::Admin);
        assert_eq!(user.map(|value| value.uid).as_deref(), Some("uid-1"));
    }

    async fn data_source_service(base_url: &str) -> AuthService {
        let store = memory_store().await;
        let identity = json!({
            "provider": {
                "type": "jwt",
                "algorithms": ["HS256"],
                "hs256_secret_env": JWT_SECRET_ENV,
                "issuer": ISSUER,
                "audience": AUDIENCE,
                "uid_claim": "sub",
                "max_age_seconds": 3600
            },
            "role": {
                "source": "data_source",
                "doc": "users/{uid}",
                "field": "role",
                "cache_seconds": 60
            },
            "admin_roles": ["admin"]
        });
        save_app(
            &store,
            APP,
            &[
                ("identity", identity),
                (
                    "data_source",
                    json!({
                        "type": "http",
                        "base_url": format!("{base_url}/internal/fcm"),
                        "secret_env": RESOLVER_SECRET_ENV,
                        "timeout_ms": 3000
                    }),
                ),
            ],
        )
        .await;
        auth_service(store, &[(APP, PROJECT)], Arc::new(FakeEnv::jwt()))
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
        assert!(topic_allowed("users", TopicAccess::User, None));
        assert!(!topic_allowed("users", TopicAccess::Anonymous, None));
        assert!(topic_allowed("anything", TopicAccess::Admin, None));
    }

    #[test]
    fn parses_cache_control_max_age() {
        assert_eq!(
            parse_cache_max_age("public, max-age=7200, must-revalidate"),
            Some(std::time::Duration::from_secs(7200))
        );
        assert_eq!(parse_cache_max_age("no-store"), None);
        assert_eq!(parse_cache_max_age("max-age=oops"), None);
    }

    #[test]
    fn encodes_firestore_path_segments() {
        assert_eq!(
            crate::datasource::percent_encode("abcDEF123_-"),
            "abcDEF123_-"
        );
        assert_eq!(crate::datasource::percent_encode("a/b"), "a%2Fb");
        assert_eq!(
            crate::datasource::percent_encode("../../etc"),
            "..%2F..%2Fetc"
        );
        assert_eq!(crate::datasource::percent_encode("a b@c"), "a%20b%40c");
        assert!(is_valid_uid("abc"));
        assert!(!is_valid_uid(""));
        assert!(!is_valid_uid(&"a".repeat(MAX_UID_LENGTH + 1)));
    }
}

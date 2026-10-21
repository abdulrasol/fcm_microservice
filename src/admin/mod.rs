//! Dashboard admin API (T2): accounts, sessions and the CSRF guard.
//!
//! Everything here lives under `/api/v1/admin` and answers with the JSON error
//! envelope `{"error": "<code>", "message": "<text>"}` instead of the plain
//! text used by the rest of the tool.

pub mod apps;
pub mod auth;
pub mod campaigns;
pub mod diagnostics;
pub mod history;
pub mod keys;
pub mod password;
pub mod session;
pub mod templates;
pub mod users;

use crate::config::store::ConfigError;
use crate::config::validate::Issue;
use crate::ratelimit::RateLimiter;
use crate::AppState;
use axum::extract::{FromRequest, Request, State};
use axum::http::{header, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::MethodRouter;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::sync::Arc;
use std::time::Duration;

/// Failed logins allowed per email and per IP inside the window.
const LOGIN_FAILURE_LIMIT: usize = 5;
const LOGIN_FAILURE_WINDOW: Duration = Duration::from_secs(15 * 60);

/// `X-Requested-With` value the dashboard sends on every mutating request.
pub const CSRF_HEADER_VALUE: &str = "fcm-dashboard";

/// Shared state of the admin router.
pub struct ToolAuth {
    pub pool: SqlitePool,
    pub cookie: session::CookieConfig,
    pub login_limiter: RateLimiter,
    /// Hash of a random throwaway password, used when the email is unknown.
    pub dummy_hash: String,
    pub allowed_origins: Vec<String>,
}

impl ToolAuth {
    pub async fn new(pool: SqlitePool) -> Result<Arc<Self>, String> {
        let dummy_hash = password::hash_password(&session::generate_token()).await?;
        let allowed_origins = std::env::var("CORS_ALLOWED_ORIGINS")
            .unwrap_or_default()
            .split(',')
            .map(|entry| entry.trim().to_string())
            .filter(|entry| !entry.is_empty())
            .collect();

        Ok(Arc::new(Self {
            pool,
            cookie: session::CookieConfig::from_env(),
            login_limiter: RateLimiter::new(LOGIN_FAILURE_LIMIT, LOGIN_FAILURE_WINDOW),
            dummy_hash,
            allowed_origins,
        }))
    }
}

/// The admin error envelope: `{"error": "<code>", "message": "<text>"}`.
///
/// A few documented responses have their own shape (`invalid`, `conflict`,
/// `recently_used`); `body` carries those instead of the envelope.
#[derive(Debug)]
pub struct AdminError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
    body: Option<Value>,
}

impl AdminError {
    fn new(status: StatusCode, code: &'static str, message: &str) -> Self {
        Self {
            status,
            code,
            message: message.to_string(),
            body: None,
        }
    }

    fn with_body(status: StatusCode, code: &'static str, body: Value) -> Self {
        Self {
            status,
            code,
            message: String::new(),
            body: Some(body),
        }
    }

    /// No cookie, an unknown session or a deleted account: all look the same.
    pub fn unauthorized_missing() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "a valid session is required",
        )
    }

    /// One message for every credential failure, so accounts cannot be probed.
    pub fn unauthorized_credentials() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "invalid email or password",
        )
    }

    pub fn unauthorized(message: &str) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthorized", message)
    }

    pub fn forbidden(message: &str) -> Self {
        Self::new(StatusCode::FORBIDDEN, "forbidden", message)
    }

    pub fn password_change_required() -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "password_change_required",
            "the password must be changed before using the dashboard",
        )
    }

    pub fn invalid_request(message: &str) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_request", message)
    }

    /// A stored configuration/template was rejected by the validators.
    pub fn invalid(issues: Vec<Issue>) -> Self {
        let issues: Vec<Value> = issues
            .into_iter()
            .map(|issue| json!({ "path": issue.path, "message": issue.message }))
            .collect();
        Self::with_body(
            StatusCode::BAD_REQUEST,
            "invalid",
            json!({ "error": "invalid", "issues": issues }),
        )
    }

    /// Someone else changed the version first.
    pub fn conflict(current_version: i64) -> Self {
        Self::with_body(
            StatusCode::CONFLICT,
            "conflict",
            json!({ "error": "conflict", "current_version": current_version }),
        )
    }

    pub fn not_found(message: &str) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }

    /// A template used inside the last 30 days cannot be deleted (plan 3.5).
    pub fn recently_used() -> Self {
        Self::with_body(
            StatusCode::CONFLICT,
            "recently_used",
            json!({
                "error": "recently_used",
                "hint": "disable the template instead",
            }),
        )
    }

    pub fn unavailable(message: &str) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, "unavailable", message)
    }

    pub fn too_many_requests() -> Self {
        Self::new(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "too many requests; retry later",
        )
    }

    pub fn rate_limited() -> Self {
        Self::new(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "too many failed attempts; retry later",
        )
    }

    /// Internal failures never expose database details to the client.
    pub fn internal(message: &str) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", message)
    }

    pub fn from_db(context: &str, error: sqlx::Error) -> Self {
        tracing::error!("admin: {} failed: {}", context, error);
        Self::internal("the request could not be completed")
    }
}

impl IntoResponse for AdminError {
    fn into_response(self) -> Response {
        let body = self
            .body
            .unwrap_or_else(|| json!({ "error": self.code, "message": self.message }));
        (self.status, Json(body)).into_response()
    }
}

/// Maps a `ConfigStore` failure to the dashboard's documented error shapes.
pub fn map_config_error(error: ConfigError) -> AdminError {
    match error {
        ConfigError::Invalid(issues) => AdminError::invalid(issues),
        ConfigError::Conflict { current_version } => AdminError::conflict(current_version),
        ConfigError::NotFound => AdminError::not_found("not found"),
        ConfigError::Db(error) => AdminError::from_db("the configuration store", error),
    }
}

/// What a route needs beyond a valid session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Need {
    /// Owners and managers of the app.
    AnyMember,
    /// Owners only (identity, data source, devices, App Check, keys, import...).
    Owner,
}

/// The single authorization helper used by every dashboard route (plan 3.11).
///
/// The caller's role is checked first, then the app binding, then existence. A
/// manager who is not bound to an app always sees `403`, even when the app does
/// not exist, so app names cannot be enumerated. An owner gets `404` for an
/// unknown app.
pub async fn authorize(
    state: &AppState,
    user: &auth::AuthedUser,
    app: &str,
    need: Need,
) -> Result<(), AdminError> {
    if need == Need::Owner {
        user.require_owner()?;
    }
    user.require_app(app)?;

    let configured = state.config.get(app).is_some();
    let has_credentials = state.project_map.read().await.contains_key(app);
    if configured || has_credentials {
        Ok(())
    } else {
        Err(AdminError::not_found("the app does not exist"))
    }
}

/// A JSON body extractor that rejects with the admin error envelope instead of
/// axum's plain-text rejection.
pub struct AdminJson<T>(pub T);

#[axum::async_trait]
impl<S, T> FromRequest<S> for AdminJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = AdminError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Json(value) = Json::<T>::from_request(request, state)
            .await
            .map_err(|_| AdminError::invalid_request("the request body is not valid JSON"))?;
        Ok(AdminJson(value))
    }
}

/// Rejects state-changing requests that a browser did not send from the
/// dashboard: a custom header (not settable cross-site without CORS) plus an
/// `Origin` that is either the request host or an explicitly allowed origin.
async fn csrf_guard(State(state): State<Arc<AppState>>, request: Request, next: Next) -> Response {
    if matches!(
        request.method(),
        &Method::GET | &Method::HEAD | &Method::OPTIONS
    ) {
        return next.run(request).await;
    }

    let headers = request.headers();
    let header_ok = headers
        .get("x-requested-with")
        .and_then(|value| value.to_str().ok())
        == Some(CSRF_HEADER_VALUE);
    if !header_ok {
        return AdminError::forbidden("the X-Requested-With header must be fcm-dashboard")
            .into_response();
    }

    if let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    {
        let host = headers
            .get(header::HOST)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        let scheme = headers
            .get("x-forwarded-proto")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("http");
        let same_origin = !host.is_empty() && origin == format!("{}://{}", scheme, host);
        let listed = state
            .tool_auth
            .allowed_origins
            .iter()
            .any(|allowed| allowed == origin);
        if !same_origin && !listed {
            return AdminError::forbidden("the Origin header is not allowed").into_response();
        }
    }

    next.run(request).await
}

/// One admin route: its method, its path, whether it needs a dashboard
/// session, and its handler.
pub(crate) type AdminRoute = (
    &'static str,
    &'static str,
    bool,
    MethodRouter<Arc<AppState>>,
);

/// Every admin route in one table.
///
/// The table is the single source of truth: the router is built from it and
/// the permission matrix test enumerates it, so a route cannot be added
/// without a test covering it.
pub(crate) fn admin_routes() -> Vec<AdminRoute> {
    vec![
        ("POST", "/auth/login", false, post(auth::login)),
        ("POST", "/auth/logout", false, post(auth::logout)),
        ("GET", "/auth/me", true, get(auth::me)),
        ("POST", "/auth/password", true, post(auth::change_password)),
        ("POST", "/auth/reauth", true, post(auth::reauth)),
        ("GET", "/apps", true, get(apps::list_apps)),
        ("GET", "/apps/:app", true, get(apps::get_app)),
        ("POST", "/apps/:app/init", true, post(apps::init_app)),
        ("PUT", "/apps/:app/:section", true, put(apps::put_section)),
        (
            "GET",
            "/apps/:app/templates",
            true,
            get(templates::list_templates),
        ),
        (
            "POST",
            "/apps/:app/templates",
            true,
            post(templates::create_template),
        ),
        (
            "POST",
            "/apps/:app/templates/validate",
            true,
            post(templates::validate_definition),
        ),
        (
            "GET",
            "/apps/:app/templates/:key",
            true,
            get(templates::get_template),
        ),
        (
            "PUT",
            "/apps/:app/templates/:key",
            true,
            put(templates::update_template),
        ),
        (
            "DELETE",
            "/apps/:app/templates/:key",
            true,
            delete(templates::delete_template),
        ),
        (
            "PUT",
            "/apps/:app/templates/:key/enabled",
            true,
            put(templates::set_enabled),
        ),
        (
            "POST",
            "/apps/:app/templates/:key/preview",
            true,
            post(templates::preview),
        ),
        (
            "POST",
            "/apps/:app/templates/:key/test-send",
            true,
            post(templates::test_send),
        ),
        ("GET", "/apps/:app/versions", true, get(apps::list_versions)),
        (
            "GET",
            "/apps/:app/versions/:id",
            true,
            get(apps::get_version),
        ),
        (
            "POST",
            "/apps/:app/versions/:id/restore",
            true,
            post(apps::restore_version),
        ),
        ("GET", "/apps/:app/export", true, get(apps::export)),
        ("POST", "/apps/:app/import", true, post(apps::import)),
        ("GET", "/audit", true, get(apps::audit)),
        (
            "GET",
            "/apps/:app/campaigns",
            true,
            get(campaigns::list_campaigns),
        ),
        // The static path of the campaign list would shadow the `campaigns`
        // configuration section, so the section save is registered here too.
        (
            "PUT",
            "/apps/:app/campaigns",
            true,
            put(apps::put_campaigns_section),
        ),
        (
            "POST",
            "/apps/:app/campaigns/:id/approve",
            true,
            post(campaigns::approve),
        ),
        (
            "POST",
            "/apps/:app/campaigns/:id/reject",
            true,
            post(campaigns::reject),
        ),
        (
            "GET",
            "/apps/:app/blocks",
            true,
            get(campaigns::list_blocks),
        ),
        (
            "POST",
            "/apps/:app/blocks",
            true,
            post(campaigns::block_entity),
        ),
        (
            "DELETE",
            "/apps/:app/blocks/:entity_id",
            true,
            delete(campaigns::unblock_entity),
        ),
        (
            "GET",
            "/apps/:app/history",
            true,
            get(history::list_history),
        ),
        (
            "GET",
            "/apps/:app/history/:id",
            true,
            get(history::history_detail),
        ),
        ("POST", "/apps/:app/send", true, post(history::free_send)),
        (
            "GET",
            "/apps/:app/devices/stats",
            true,
            get(history::device_stats),
        ),
        ("GET", "/users", true, get(users::list_users)),
        ("POST", "/users", true, post(users::create_user)),
        ("PUT", "/users/:id", true, put(users::update_user)),
        (
            "POST",
            "/users/:id/reset-password",
            true,
            post(users::reset_password),
        ),
        ("DELETE", "/users/:id", true, delete(users::delete_user)),
        ("PUT", "/auth/test-uid", true, put(auth::set_test_uid)),
        (
            "POST",
            "/apps/:app/identity/test",
            true,
            post(diagnostics::identity_test),
        ),
        (
            "POST",
            "/apps/:app/data_source/test",
            true,
            post(diagnostics::data_source_test),
        ),
        ("GET", "/apps/:app/keys", true, get(keys::list_keys)),
        ("POST", "/apps/:app/keys", true, post(keys::create_key)),
        (
            "DELETE",
            "/apps/:app/keys/:id",
            true,
            delete(keys::revoke_key),
        ),
    ]
}

/// Merges the routes of the same path into one method router.
fn merge_routes(routes: Vec<AdminRoute>) -> Vec<(&'static str, MethodRouter<Arc<AppState>>)> {
    let mut merged: Vec<(&'static str, MethodRouter<Arc<AppState>>)> = Vec::new();
    for (_, path, _, method_router) in routes {
        match merged.iter_mut().find(|(known, _)| *known == path) {
            Some((_, known)) => {
                let taken = std::mem::replace(known, MethodRouter::new());
                *known = taken.merge(method_router);
            }
            None => merged.push((path, method_router)),
        }
    }
    merged
}

/// The admin routes. The caller nests this under `/admin`.
pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    let mut router = Router::new();
    for (path, method_router) in merge_routes(admin_routes()) {
        router = router.route(path, method_router);
    }
    router.layer(axum::middleware::from_fn_with_state(state, csrf_guard))
}

/// Deletes expired sessions every hour; the task dies with the process.
pub fn spawn_session_cleanup(tool_auth: Arc<ToolAuth>) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(3600));
        // The first tick of an interval completes immediately; skip it so the
        // cleanup runs after an hour, not during startup.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            match session::delete_expired(&tool_auth.pool).await {
                Ok(0) => {}
                Ok(deleted) => tracing::info!("admin: removed {} expired session(s)", deleted),
                Err(error) => tracing::error!("admin: session cleanup failed: {}", error),
            }
        }
    });
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::admin::auth::Role;
    use crate::auth::{AuthConfig, AuthService};
    use crate::config::store::ConfigStore;
    use crate::credentials::{AuthMap, ProjectIdMap};
    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::HeaderValue;
    use http_body_util::BodyExt;
    use reqwest::Client;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::collections::HashMap;
    use std::net::SocketAddr;
    use std::str::FromStr;
    use tokio::sync::RwLock;
    use tower::ServiceExt;

    pub(crate) const PASSWORD: &str = "correct horse battery 2026";
    const NEW_PASSWORD: &str = "a brand new passphrase 2026";
    const DASHBOARD_ORIGIN: &str = "http://localhost:5173";

    pub(crate) async fn memory_pool() -> SqlitePool {
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
        pool
    }

    pub(crate) async fn test_state(pool: SqlitePool) -> Arc<AppState> {
        test_state_with_env(pool, None, None).await
    }

    /// A state whose data-source environment and FCM transport can be replaced.
    pub(crate) async fn test_state_with_env(
        pool: SqlitePool,
        env: Option<Arc<dyn crate::identity::jwt::EnvSource>>,
        fcm: Option<Arc<dyn crate::send::FcmTransport>>,
    ) -> Arc<AppState> {
        let client = Client::builder()
            .build()
            .expect("the test client must build");
        let auth_map: AuthMap = Arc::new(RwLock::new(HashMap::new()));
        let project_map: ProjectIdMap = Arc::new(RwLock::new(HashMap::new()));
        let config = Arc::new(
            ConfigStore::new(pool.clone())
                .await
                .expect("the config store must open"),
        );
        let env = env.unwrap_or_else(|| Arc::new(crate::identity::jwt::SystemEnv));
        let identity = Arc::new(crate::identity::registry::IdentityRegistry::new(
            config.clone(),
            project_map.clone(),
            client.clone(),
            env.clone(),
        ));
        let datasource = Arc::new(crate::datasource::registry::DataSourceRegistry::new(
            config.clone(),
            project_map.clone(),
            auth_map.clone(),
            client.clone(),
            env,
        ));
        let app_keys = Arc::new(crate::appkeys::AppKeyStore::new(pool.clone()));
        let auth = AuthService::new(
            project_map.clone(),
            identity.clone(),
            datasource.clone(),
            config.clone(),
            AuthConfig {
                service_api_key: None,
                legacy_api_key: None,
                legacy_enabled: false,
            },
        )
        .with_app_keys(app_keys);
        let tool_auth = Arc::new(ToolAuth {
            pool: pool.clone(),
            cookie: session::CookieConfig::with_hours(true, 12),
            login_limiter: RateLimiter::new(LOGIN_FAILURE_LIMIT, LOGIN_FAILURE_WINDOW),
            dummy_hash: password::hash_password(&session::generate_token())
                .await
                .expect("the dummy hash must build"),
            allowed_origins: vec![DASHBOARD_ORIGIN.to_string()],
        });

        let fcm = fcm.unwrap_or_else(|| {
            Arc::new(crate::send::HttpFcm::new(client.clone(), auth_map.clone()))
        });

        Arc::new(AppState {
            db: pool.clone(),
            auth_map,
            project_map,
            client,
            auth,
            trust_proxy: false,
            topics_limiter: RateLimiter::new(30, Duration::from_secs(60)),
            auth_fail_limiter: RateLimiter::new(10, Duration::from_secs(60)),
            devices: Arc::new(crate::devices::DeviceStore::new(pool)),
            devices_limiter: RateLimiter::new(20, Duration::from_secs(60)),
            config,
            tool_auth,
            identity,
            datasource,
            fcm,
            role_directory: Arc::new(crate::devices::RoleDirectory::new()),
            template_rate: Arc::new(crate::db::TemplateRateLimiter::new()),
        })
    }

    pub(crate) fn app(state: Arc<AppState>) -> Router {
        router(state.clone()).with_state(state)
    }

    pub(crate) fn build_request(
        method: Method,
        uri: &str,
        body: Option<serde_json::Value>,
    ) -> Request {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("x-requested-with", CSRF_HEADER_VALUE);
        let body = match body {
            Some(value) => {
                builder = builder.header(header::CONTENT_TYPE, "application/json");
                Body::from(value.to_string())
            }
            None => Body::empty(),
        };
        let mut request = builder.body(body).expect("the request must build");
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 40_000))));
        request
    }

    pub(crate) fn with_cookie(mut request: Request, name: &str, token: &str) -> Request {
        let value =
            HeaderValue::from_str(&format!("{}={}", name, token)).expect("a valid cookie header");
        request.headers_mut().insert(header::COOKIE, value);
        request
    }

    pub(crate) async fn read_json(response: Response) -> serde_json::Value {
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("the body must be readable")
            .to_bytes();
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    }

    fn set_cookie_of(response: &Response) -> String {
        response
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .expect("the response must set a cookie")
            .to_string()
    }

    pub(crate) fn session_token(response: &Response) -> String {
        session::read_cookie(
            [set_cookie_of(response).as_str()],
            session::SECURE_COOKIE_NAME,
        )
        .expect("the cookie must carry the token")
    }

    pub(crate) async fn insert_user(
        pool: &SqlitePool,
        email: &str,
        role: &str,
        must_change_password: bool,
    ) -> i64 {
        let hash = password::hash_password(PASSWORD)
            .await
            .expect("hashing must work");
        sqlx::query(
            "INSERT INTO tool_users (email, password_hash, role, must_change_password) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(email)
        .bind(&hash)
        .bind(role)
        .bind(if must_change_password { 1_i64 } else { 0_i64 })
        .execute(pool)
        .await
        .expect("the user must be inserted");

        sqlx::query_scalar("SELECT id FROM tool_users WHERE email = ?")
            .bind(email)
            .fetch_one(pool)
            .await
            .expect("the id must exist")
    }

    pub(crate) async fn login_request(app: &Router, email: &str, password: &str) -> Response {
        let body = json!({ "email": email, "password": password });
        app.clone()
            .oneshot(build_request(Method::POST, "/auth/login", Some(body)))
            .await
            .expect("the login must run")
    }

    pub(crate) async fn send(app: &Router, request: Request) -> Response {
        app.clone()
            .oneshot(request)
            .await
            .expect("the request must run")
    }

    fn authed_user(role: Role, apps: Vec<String>) -> auth::AuthedUser {
        auth::AuthedUser {
            user_id: 1,
            email: "user@example.com".to_string(),
            role,
            apps,
            must_change_password: false,
            session_id_hash: "hash".to_string(),
            reauth_at: None,
        }
    }

    #[tokio::test]
    async fn login_success_sets_cookie_and_returns_user_without_hash() {
        let pool = memory_pool().await;
        insert_user(&pool, "owner@example.com", "owner", false).await;
        let app = app(test_state(pool.clone()).await);

        let response = login_request(&app, "owner@example.com", PASSWORD).await;
        assert_eq!(response.status(), StatusCode::OK);

        let cookie = set_cookie_of(&response);
        assert!(cookie.starts_with("__Host-fcm_session="));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("Secure"));
        assert!(cookie.contains("SameSite=Strict"));
        assert!(cookie.contains("Path=/"));
        assert!(cookie.contains("Max-Age=43200"));
        let token = session_token(&response);

        let body = read_json(response).await;
        let user = body["user"]
            .as_object()
            .expect("the user must be an object")
            .clone();
        assert_eq!(user.len(), 5, "only the documented fields are returned");
        assert_eq!(user["email"], "owner@example.com");
        assert_eq!(user["role"], "owner");
        assert_eq!(user["must_change_password"], false);
        assert!(user["apps"].is_array());
        let text = body.to_string();
        assert!(!text.contains("password_hash"));
        assert!(!text.contains("test_uid"));
        assert!(!text.contains(&token));

        let stored: String = sqlx::query_scalar("SELECT id_hash FROM tool_sessions")
            .fetch_one(&pool)
            .await
            .expect("the session must exist");
        assert_eq!(stored, session::hash_token(&token));
        assert_ne!(stored, token);
    }

    #[tokio::test]
    async fn login_wrong_password_and_unknown_email_return_identical_401() {
        let pool = memory_pool().await;
        insert_user(&pool, "owner@example.com", "owner", false).await;
        let app = app(test_state(pool).await);

        let wrong = login_request(&app, "owner@example.com", "wrong password 2026").await;
        assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
        let wrong_body = read_json(wrong).await;

        let unknown = login_request(&app, "nobody@example.com", "wrong password 2026").await;
        assert_eq!(unknown.status(), StatusCode::UNAUTHORIZED);
        let unknown_body = read_json(unknown).await;

        assert_eq!(wrong_body, unknown_body);
        assert_eq!(wrong_body["error"], "unauthorized");
    }

    #[tokio::test]
    async fn login_disabled_user_returns_401() {
        let pool = memory_pool().await;
        insert_user(&pool, "owner@example.com", "owner", false).await;
        sqlx::query("UPDATE tool_users SET disabled = 1")
            .execute(&pool)
            .await
            .expect("the account must be disabled");
        let app = app(test_state(pool).await);

        let response = login_request(&app, "owner@example.com", PASSWORD).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = read_json(response).await;
        assert_eq!(body["error"], "unauthorized");
        assert_eq!(body["message"], "invalid email or password");
    }

    #[tokio::test]
    async fn sixth_failed_login_returns_429_without_checking_password() {
        let pool = memory_pool().await;
        insert_user(&pool, "owner@example.com", "owner", false).await;
        let app = app(test_state(pool.clone()).await);

        for _ in 0..LOGIN_FAILURE_LIMIT {
            let response = login_request(&app, "owner@example.com", "wrong password 2026").await;
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }

        // The sixth attempt carries the correct password: it must still be
        // refused before any hashing happens.
        let response = login_request(&app, "owner@example.com", PASSWORD).await;
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let body = read_json(response).await;
        assert_eq!(body["error"], "rate_limited");

        let sessions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tool_sessions")
            .fetch_one(&pool)
            .await
            .expect("the count must exist");
        assert_eq!(sessions, 0, "no session may be created after the limit");
    }

    #[tokio::test]
    async fn login_without_csrf_header_is_403() {
        let pool = memory_pool().await;
        insert_user(&pool, "owner@example.com", "owner", false).await;
        let app = app(test_state(pool).await);

        let body = json!({ "email": "owner@example.com", "password": PASSWORD }).to_string();
        let mut request = Request::builder()
            .method(Method::POST)
            .uri("/auth/login")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .expect("the request must build");
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 40_000))));

        let response = send(&app, request).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(read_json(response).await["error"], "forbidden");
    }

    #[tokio::test]
    async fn login_with_foreign_origin_is_403() {
        let pool = memory_pool().await;
        insert_user(&pool, "owner@example.com", "owner", false).await;
        let app = app(test_state(pool).await);

        let body = json!({ "email": "owner@example.com", "password": PASSWORD }).to_string();
        let mut request = Request::builder()
            .method(Method::POST)
            .uri("/auth/login")
            .header("x-requested-with", CSRF_HEADER_VALUE)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ORIGIN, "http://evil.example")
            .body(Body::from(body))
            .expect("the request must build");
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 40_000))));

        let response = send(&app, request).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(read_json(response).await["error"], "forbidden");

        // The configured dashboard origin is accepted by the same guard.
        let body = json!({ "email": "owner@example.com", "password": PASSWORD }).to_string();
        let mut allowed = Request::builder()
            .method(Method::POST)
            .uri("/auth/login")
            .header("x-requested-with", CSRF_HEADER_VALUE)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ORIGIN, DASHBOARD_ORIGIN)
            .body(Body::from(body))
            .expect("the request must build");
        allowed
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 40_000))));
        assert_eq!(send(&app, allowed).await.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn me_requires_session() {
        let pool = memory_pool().await;
        insert_user(&pool, "owner@example.com", "owner", false).await;
        let app = app(test_state(pool).await);

        let response = send(&app, build_request(Method::GET, "/auth/me", None)).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(read_json(response).await["error"], "unauthorized");
    }

    #[tokio::test]
    async fn must_change_password_blocks_other_routes() {
        let pool = memory_pool().await;
        insert_user(&pool, "owner@example.com", "owner", true).await;
        let app = app(test_state(pool).await);
        let name = session::SECURE_COOKIE_NAME;

        let login = login_request(&app, "owner@example.com", PASSWORD).await;
        assert_eq!(login.status(), StatusCode::OK);
        let token = session_token(&login);

        let me = send(
            &app,
            with_cookie(build_request(Method::GET, "/auth/me", None), name, &token),
        )
        .await;
        assert_eq!(me.status(), StatusCode::OK);
        assert_eq!(read_json(me).await["must_change_password"], true);

        let body = json!({ "password": PASSWORD });
        let reauth = send(
            &app,
            with_cookie(
                build_request(Method::POST, "/auth/reauth", Some(body)),
                name,
                &token,
            ),
        )
        .await;
        assert_eq!(reauth.status(), StatusCode::FORBIDDEN);
        assert_eq!(read_json(reauth).await["error"], "password_change_required");
    }

    #[tokio::test]
    async fn change_password_clears_flag_and_kills_other_sessions() {
        let pool = memory_pool().await;
        let user_id = insert_user(&pool, "owner@example.com", "owner", true).await;
        let app = app(test_state(pool.clone()).await);
        let name = session::SECURE_COOKIE_NAME;

        let first = login_request(&app, "owner@example.com", PASSWORD).await;
        let first_token = session_token(&first);
        let second = login_request(&app, "owner@example.com", PASSWORD).await;
        let second_token = session_token(&second);
        assert_ne!(first_token, second_token);

        let body = json!({ "current_password": PASSWORD, "new_password": NEW_PASSWORD });
        let change = send(
            &app,
            with_cookie(
                build_request(Method::POST, "/auth/password", Some(body)),
                name,
                &first_token,
            ),
        )
        .await;
        assert_eq!(change.status(), StatusCode::NO_CONTENT);

        let flag: i64 =
            sqlx::query_scalar("SELECT must_change_password FROM tool_users WHERE id = ?")
                .bind(user_id)
                .fetch_one(&pool)
                .await
                .expect("the flag must exist");
        assert_eq!(flag, 0);

        let sessions: Vec<String> = sqlx::query_scalar("SELECT id_hash FROM tool_sessions")
            .fetch_all(&pool)
            .await
            .expect("the sessions must be listed");
        assert_eq!(sessions, vec![session::hash_token(&first_token)]);

        let gone = send(
            &app,
            with_cookie(
                build_request(Method::GET, "/auth/me", None),
                name,
                &second_token,
            ),
        )
        .await;
        assert_eq!(gone.status(), StatusCode::UNAUTHORIZED);

        let me = send(
            &app,
            with_cookie(
                build_request(Method::GET, "/auth/me", None),
                name,
                &first_token,
            ),
        )
        .await;
        assert_eq!(me.status(), StatusCode::OK);
        assert_eq!(read_json(me).await["must_change_password"], false);
    }

    #[tokio::test]
    async fn change_password_wrong_current_is_401() {
        let pool = memory_pool().await;
        insert_user(&pool, "owner@example.com", "owner", false).await;
        let app = app(test_state(pool).await);
        let name = session::SECURE_COOKIE_NAME;

        let login = login_request(&app, "owner@example.com", PASSWORD).await;
        let token = session_token(&login);

        let body = json!({
            "current_password": "not the current password",
            "new_password": NEW_PASSWORD,
        });
        let response = send(
            &app,
            with_cookie(
                build_request(Method::POST, "/auth/password", Some(body)),
                name,
                &token,
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(read_json(response).await["error"], "unauthorized");
    }

    #[tokio::test]
    async fn change_password_policy_violation_is_400() {
        let pool = memory_pool().await;
        insert_user(&pool, "owner@example.com", "owner", false).await;
        let app = app(test_state(pool).await);
        let name = session::SECURE_COOKIE_NAME;

        let login = login_request(&app, "owner@example.com", PASSWORD).await;
        let token = session_token(&login);

        let body = json!({
            "current_password": PASSWORD,
            "new_password": "short",
        });
        let response = send(
            &app,
            with_cookie(
                build_request(Method::POST, "/auth/password", Some(body)),
                name,
                &token,
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(read_json(response).await["error"], "invalid_request");
    }

    #[tokio::test]
    async fn logout_deletes_session_and_clears_cookie() {
        let pool = memory_pool().await;
        insert_user(&pool, "owner@example.com", "owner", false).await;
        let app = app(test_state(pool.clone()).await);
        let name = session::SECURE_COOKIE_NAME;

        let login = login_request(&app, "owner@example.com", PASSWORD).await;
        let token = session_token(&login);

        let logout = send(
            &app,
            with_cookie(
                build_request(Method::POST, "/auth/logout", None),
                name,
                &token,
            ),
        )
        .await;
        assert_eq!(logout.status(), StatusCode::NO_CONTENT);
        let cookie = set_cookie_of(&logout);
        assert!(cookie.contains("Max-Age=0"));
        assert!(cookie.starts_with("__Host-fcm_session=;"));

        let sessions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tool_sessions")
            .fetch_one(&pool)
            .await
            .expect("the count must exist");
        assert_eq!(sessions, 0);

        let me = send(
            &app,
            with_cookie(build_request(Method::GET, "/auth/me", None), name, &token),
        )
        .await;
        assert_eq!(me.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn reauth_sets_timestamp() {
        let pool = memory_pool().await;
        insert_user(&pool, "owner@example.com", "owner", false).await;
        let app = app(test_state(pool.clone()).await);
        let name = session::SECURE_COOKIE_NAME;

        let login = login_request(&app, "owner@example.com", PASSWORD).await;
        let token = session_token(&login);

        let body = json!({ "password": PASSWORD });
        let response = send(
            &app,
            with_cookie(
                build_request(Method::POST, "/auth/reauth", Some(body)),
                name,
                &token,
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        let reauth_at: Option<i64> =
            sqlx::query_scalar("SELECT reauth_at FROM tool_sessions WHERE id_hash = ?")
                .bind(session::hash_token(&token))
                .fetch_one(&pool)
                .await
                .expect("the session must exist");
        assert!(reauth_at.is_some(), "reauth_at must be set");

        let body = json!({ "password": "not the password" });
        let wrong = send(
            &app,
            with_cookie(
                build_request(Method::POST, "/auth/reauth", Some(body)),
                name,
                &token,
            ),
        )
        .await;
        assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn require_reauth_rejects_stale_and_missing() {
        let missing = authed_user(Role::Owner, Vec::new());
        let error = missing
            .require_reauth(300)
            .expect_err("a missing reauth must be rejected");
        assert_eq!(error.code, "forbidden");
        assert_eq!(error.message, "reauth_required");

        let stale = auth::AuthedUser {
            reauth_at: Some(session::now_epoch() - 400),
            ..authed_user(Role::Owner, Vec::new())
        };
        assert_eq!(
            stale
                .require_reauth(300)
                .expect_err("a stale reauth must be rejected")
                .message,
            "reauth_required"
        );

        let fresh = auth::AuthedUser {
            reauth_at: Some(session::now_epoch() - 10),
            ..authed_user(Role::Owner, Vec::new())
        };
        assert!(fresh.require_reauth(300).is_ok());
    }

    #[test]
    fn require_app_manager_scope() {
        let manager = authed_user(Role::Manager, vec!["watt".to_string()]);
        assert!(manager.require_app("watt").is_ok());
        let error = manager
            .require_app("other")
            .expect_err("a manager cannot reach another app");
        assert_eq!(error.code, "forbidden");

        let owner = authed_user(Role::Owner, Vec::new());
        assert!(owner.require_app("watt").is_ok());
        assert!(owner.require_app("anything").is_ok());
    }

    #[test]
    fn require_owner_rejects_manager() {
        let manager = authed_user(Role::Manager, Vec::new());
        assert_eq!(
            manager
                .require_owner()
                .expect_err("a manager is not an owner")
                .code,
            "forbidden"
        );
        assert!(authed_user(Role::Owner, Vec::new()).require_owner().is_ok());
    }

    #[tokio::test]
    async fn audit_rows_written_without_secrets() {
        let pool = memory_pool().await;
        insert_user(&pool, "owner@example.com", "owner", false).await;
        let app = app(test_state(pool.clone()).await);
        let name = session::SECURE_COOKIE_NAME;

        let login = login_request(&app, "owner@example.com", PASSWORD).await;
        let token = session_token(&login);
        let _ = login_request(&app, "owner@example.com", "wrong password 2026").await;
        let _ = login_request(&app, "nobody@example.com", "wrong password 2026").await;

        let body = json!({ "password": PASSWORD });
        let _ = send(
            &app,
            with_cookie(
                build_request(Method::POST, "/auth/reauth", Some(body)),
                name,
                &token,
            ),
        )
        .await;

        let body = json!({ "current_password": PASSWORD, "new_password": NEW_PASSWORD });
        let _ = send(
            &app,
            with_cookie(
                build_request(Method::POST, "/auth/password", Some(body)),
                name,
                &token,
            ),
        )
        .await;

        let details: Vec<Option<String>> = sqlx::query_scalar("SELECT detail FROM audit_log")
            .fetch_all(&pool)
            .await
            .expect("the audit rows must be listed");
        assert!(!details.is_empty(), "the actions must be audited");
        for detail in details.into_iter().flatten() {
            assert!(
                !detail.contains(PASSWORD),
                "detail leaked a password: {}",
                detail
            );
            assert!(
                !detail.contains(NEW_PASSWORD),
                "detail leaked the new password: {}",
                detail
            );
            assert!(!detail.contains(&token), "detail leaked a session token");
            assert!(
                !detail.contains("owner@example.com"),
                "detail leaked a raw email: {}",
                detail
            );
        }

        let actions: Vec<String> = sqlx::query_scalar("SELECT action FROM audit_log ORDER BY id")
            .fetch_all(&pool)
            .await
            .expect("the actions must be listed");
        for action in [
            "admin.login",
            "admin.login_failed",
            "admin.reauth",
            "admin.password_changed",
        ] {
            assert!(
                actions.iter().any(|value| value == action),
                "missing {}",
                action
            );
        }
    }

    #[tokio::test]
    async fn expired_session_is_rejected_and_cleaned() {
        let pool = memory_pool().await;
        let user_id = insert_user(&pool, "owner@example.com", "owner", false).await;
        let cookie = session::CookieConfig::with_hours(true, 12);
        let created =
            session::create_session(&pool, &cookie, user_id, Some("127.0.0.1"), Some("test"))
                .await
                .expect("the session must be created");
        assert!(session::load_session(&pool, &created.token)
            .await
            .expect("the session must load")
            .is_some());

        sqlx::query("UPDATE tool_sessions SET expires_at = ? WHERE id_hash = ?")
            .bind(session::now_epoch() - 1)
            .bind(session::hash_token(&created.token))
            .execute(&pool)
            .await
            .expect("the session must expire");

        assert!(session::load_session(&pool, &created.token)
            .await
            .expect("the session must load")
            .is_none());
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tool_sessions")
            .fetch_one(&pool)
            .await
            .expect("the count must exist");
        assert_eq!(count, 0, "the expired row is deleted, not only ignored");
    }

    #[tokio::test]
    async fn eleventh_session_evicts_oldest() {
        let pool = memory_pool().await;
        let user_id = insert_user(&pool, "owner@example.com", "owner", false).await;

        // Explicit `created_at` values make "oldest" unambiguous.
        for index in 0..session::MAX_SESSIONS_PER_USER {
            sqlx::query(
                "INSERT INTO tool_sessions (id_hash, user_id, expires_at, created_at) \
                 VALUES (?, ?, ?, ?)",
            )
            .bind(format!("hash_{index}"))
            .bind(user_id)
            .bind(session::now_epoch() + 3600)
            .bind(index)
            .execute(&pool)
            .await
            .expect("the session must be inserted");
        }

        let cookie = session::CookieConfig::with_hours(true, 12);
        session::create_session(&pool, &cookie, user_id, None, None)
            .await
            .expect("the eleventh session must be created");

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tool_sessions WHERE user_id = ?")
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .expect("the count must exist");
        assert_eq!(count, session::MAX_SESSIONS_PER_USER);

        let oldest: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM tool_sessions WHERE id_hash = 'hash_0'")
                .fetch_one(&pool)
                .await
                .expect("the count must exist");
        assert_eq!(oldest, 0, "the oldest session is evicted");

        let second: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM tool_sessions WHERE id_hash = 'hash_1'")
                .fetch_one(&pool)
                .await
                .expect("the count must exist");
        assert_eq!(second, 1, "the newer sessions stay");
    }

    #[tokio::test]
    async fn deleting_user_cascades_sessions() {
        let pool = memory_pool().await;
        let user_id = insert_user(&pool, "owner@example.com", "owner", false).await;
        let cookie = session::CookieConfig::with_hours(true, 12);
        session::create_session(&pool, &cookie, user_id, None, None)
            .await
            .expect("the session must be created");

        sqlx::query("DELETE FROM tool_users WHERE id = ?")
            .bind(user_id)
            .execute(&pool)
            .await
            .expect("the user must be deleted");

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tool_sessions")
            .fetch_one(&pool)
            .await
            .expect("the count must exist");
        assert_eq!(count, 0, "ON DELETE CASCADE needs PRAGMA foreign_keys=ON");
    }

    // ---------- T10-A shared harness ----------

    pub(crate) const APP: &str = "watt";
    pub(crate) const OTHER_APP: &str = "other";
    pub(crate) const DATASOURCE_SECRET_ENV: &str = "ADMIN_TEST_DS_SECRET";
    /// The identity secret of the tests.
    pub(crate) const JWT_SECRET_ENV: &str = "ADMIN_TEST_JWT_SECRET";
    /// A long enough HS256 secret for the tests.
    pub(crate) const JWT_SECRET: &str = "a-very-long-test-secret-value-1234567890";

    pub(crate) struct FakeEnv;

    impl crate::identity::jwt::EnvSource for FakeEnv {
        fn get(&self, name: &str) -> Option<String> {
            if name == DATASOURCE_SECRET_ENV {
                Some("0123456789abcdef0123456789abcdef".to_string())
            } else if name == JWT_SECRET_ENV {
                Some(JWT_SECRET.to_string())
            } else {
                None
            }
        }
    }

    /// Counts FCM calls and remembers the addressed tokens.
    #[derive(Default)]
    pub(crate) struct CountingFcm {
        calls: std::sync::atomic::AtomicUsize,
        tokens: std::sync::Mutex<Vec<String>>,
    }

    impl CountingFcm {
        pub(crate) fn calls(&self) -> usize {
            self.calls.load(std::sync::atomic::Ordering::SeqCst)
        }

        pub(crate) fn tokens(&self) -> Vec<String> {
            self.tokens
                .lock()
                .expect("the token list must be readable")
                .clone()
        }
    }

    #[async_trait::async_trait]
    impl crate::send::FcmTransport for CountingFcm {
        async fn send(
            &self,
            _app: &str,
            _project_id: &str,
            message: serde_json::Value,
        ) -> Result<crate::send::FcmOutcome, crate::send::SendError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if let Some(token) = message
                .pointer("/message/token")
                .and_then(serde_json::Value::as_str)
            {
                self.tokens
                    .lock()
                    .expect("the token list must be writable")
                    .push(token.to_string());
            }
            Ok(crate::send::FcmOutcome::Accepted {
                body: "{}".to_string(),
            })
        }
    }

    /// Creates the configuration and the credentials entry for `app`.
    pub(crate) async fn bind_app(state: &Arc<AppState>, app: &str, project: &str) {
        if state.config.get(app).is_none() {
            state
                .config
                .create_app(app, "tester")
                .await
                .expect("the app configuration must be created");
        }
        state
            .project_map
            .write()
            .await
            .insert(app.to_string(), project.to_string());
    }

    /// Adds credentials without a configuration (`POST /init` scenario).
    pub(crate) async fn add_credentials(state: &Arc<AppState>, app: &str, project: &str) {
        state
            .project_map
            .write()
            .await
            .insert(app.to_string(), project.to_string());
    }

    pub(crate) struct Harness {
        pub state: Arc<AppState>,
        pub pool: SqlitePool,
        pub router: Router,
        pub token: String,
        pub user_id: i64,
        pub fcm: Arc<CountingFcm>,
    }

    impl Harness {
        pub(crate) async fn call(
            &self,
            method: Method,
            uri: &str,
            body: Option<serde_json::Value>,
        ) -> (StatusCode, serde_json::Value) {
            call(&self.router, method, uri, body, Some(&self.token), true).await
        }

        /// Marks the session freshly re-authenticated.
        pub(crate) async fn reauth(&self) {
            sqlx::query("UPDATE tool_sessions SET reauth_at = ? WHERE id_hash = ?")
                .bind(session::now_epoch())
                .bind(session::hash_token(&self.token))
                .execute(&self.pool)
                .await
                .expect("the session must be updated");
        }

        pub(crate) async fn set_test_uid(&self, uid: &str) {
            sqlx::query("UPDATE tool_users SET test_uid = ? WHERE id = ?")
                .bind(uid)
                .bind(self.user_id)
                .execute(&self.pool)
                .await
                .expect("the test uid must be set");
        }

        pub(crate) async fn register_device(&self, uid: &str, token: &str) {
            self.state
                .devices
                .register(APP, uid, token, crate::devices::Platform::Android, 20)
                .await
                .expect("the device must register");
        }

        pub(crate) async fn save_template(&self, key: &str, definition: serde_json::Value) {
            self.state
                .config
                .save_template(APP, key, definition, "tester", None)
                .await
                .expect("the template must save");
        }
    }

    /// Logs in a dashboard account bound to the given apps.
    pub(crate) async fn setup(role: &str, bound: &[&str]) -> Harness {
        setup_with_env(role, bound, None).await
    }

    pub(crate) async fn setup_with_env(
        role: &str,
        bound: &[&str],
        env: Option<Arc<dyn crate::identity::jwt::EnvSource>>,
    ) -> Harness {
        let pool = memory_pool().await;
        let fcm = Arc::new(CountingFcm::default());
        let state = test_state_with_env(pool.clone(), env, Some(fcm.clone())).await;
        bind_app(&state, APP, "project-watt").await;
        bind_app(&state, OTHER_APP, "project-other").await;

        let user_id = insert_user(&pool, "user@example.com", role, false).await;
        for app in bound {
            sqlx::query("INSERT INTO tool_user_apps (user_id, app) VALUES (?, ?)")
                .bind(user_id)
                .bind(app)
                .execute(&pool)
                .await
                .expect("the app binding must be inserted");
        }

        let router = app(state.clone());
        let login = login_request(&router, "user@example.com", PASSWORD).await;
        let token = session_token(&login);
        Harness {
            state,
            pool,
            router,
            token,
            user_id,
            fcm,
        }
    }

    pub(crate) async fn call(
        router: &Router,
        method: Method,
        uri: &str,
        body: Option<serde_json::Value>,
        token: Option<&str>,
        csrf: bool,
    ) -> (StatusCode, serde_json::Value) {
        let response = call_raw(router, method, uri, body, token, csrf).await;
        let status = response.status();
        let body = read_json(response).await;
        (status, body)
    }

    /// Sends a request and returns the whole response, headers included.
    pub(crate) async fn call_raw(
        router: &Router,
        method: Method,
        uri: &str,
        body: Option<serde_json::Value>,
        token: Option<&str>,
        csrf: bool,
    ) -> Response {
        let mut builder = Request::builder().method(method).uri(uri);
        if csrf {
            builder = builder.header("x-requested-with", CSRF_HEADER_VALUE);
        }
        if let Some(token) = token {
            builder = builder.header(
                header::COOKIE,
                format!("{}={}", session::SECURE_COOKIE_NAME, token),
            );
        }
        let body = match body {
            Some(value) => {
                builder = builder.header(header::CONTENT_TYPE, "application/json");
                Body::from(value.to_string())
            }
            None => Body::empty(),
        };
        let mut request = builder.body(body).expect("the request must build");
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 40_000))));
        router
            .clone()
            .oneshot(request)
            .await
            .expect("the request must run")
    }

    /// A minimal admin-only template that passes the validators.
    pub(crate) fn sample_template() -> serde_json::Value {
        json!({
            "description": "Sample admin alert",
            "enabled": true,
            "senders": ["admin"],
            "to": { "role": "admin" },
            "title": "Hello",
            "body": "Body"
        })
    }

    /// Every registered route: its method, its path and whether it needs a
    /// dashboard session. The list comes from the route table, so a new route
    /// is covered by the matrix without touching the test.
    fn registered_routes() -> Vec<(Method, String, bool)> {
        admin_routes()
            .into_iter()
            .map(|(method, path, needs_session, _)| {
                (
                    Method::from_bytes(method.as_bytes()).expect("a known method"),
                    path.to_string(),
                    needs_session,
                )
            })
            .collect()
    }

    /// Every route that requires a dashboard session.
    fn protected_routes() -> Vec<(Method, String)> {
        registered_routes()
            .into_iter()
            .filter(|(_, _, needs_session)| *needs_session)
            .map(|(method, path, _)| (method, path))
            .collect()
    }

    fn section_value(harness: &Harness, section: &str) -> serde_json::Value {
        let snapshot = harness.state.config.get(APP).expect("the app exists");
        match section {
            "identity" => serde_json::to_value(&snapshot.config.identity),
            "data_source" => serde_json::to_value(&snapshot.config.data_source),
            "devices" => serde_json::to_value(&snapshot.config.devices),
            "web_subscribe" => serde_json::to_value(&snapshot.config.web_subscribe),
            "campaigns" => serde_json::to_value(&snapshot.config.campaigns),
            "app_check" => serde_json::to_value(&snapshot.config.app_check),
            other => panic!("unknown section {}", other),
        }
        .expect("the section must serialize")
    }

    #[tokio::test]
    async fn matrix_unauthenticated_is_401_on_every_route() {
        let harness = setup("owner", &[]).await;
        for (method, uri) in protected_routes() {
            let (status, body) =
                call(&harness.router, method.clone(), &uri, None, None, true).await;
            assert_eq!(
                status,
                StatusCode::UNAUTHORIZED,
                "{} {} returned {} ({})",
                method,
                uri,
                status,
                body
            );
        }
    }

    /// One matrix case: the registered route, the concrete request path and
    /// the request body.
    struct MatrixCase {
        method: Method,
        pattern: String,
        path: String,
        body: Option<serde_json::Value>,
    }

    impl MatrixCase {
        fn new(method: Method, pattern: &str, body: Option<serde_json::Value>) -> Self {
            MatrixCase {
                method,
                pattern: pattern.to_string(),
                path: concrete_path(pattern),
                body,
            }
        }

        fn at(
            method: Method,
            pattern: &str,
            path: String,
            body: Option<serde_json::Value>,
        ) -> Self {
            MatrixCase {
                method,
                pattern: pattern.to_string(),
                path,
                body,
            }
        }
    }

    /// A concrete request path for a route pattern.
    fn concrete_path(pattern: &str) -> String {
        pattern
            .replace(":app", APP)
            .replace(":section", "identity")
            .replace(":key", "sample")
            .replace(":id", "1")
    }

    /// The owner matrix: one case per registered route.
    async fn owner_matrix_cases(harness: &Harness) -> Vec<MatrixCase> {
        let (_, document) = harness
            .call(Method::GET, &format!("/apps/{}/export", APP), None)
            .await;

        // The data source check must not reach a real service: the app gets a
        // source whose secret the environment does not hold.
        let version = harness
            .state
            .config
            .get(APP)
            .expect("the app exists")
            .version;
        let _ = harness
            .state
            .config
            .save_section(
                APP,
                "data_source",
                json!({
                    "type": "http",
                    "base_url": "http://127.0.0.1:1",
                    "secret_env": "ADMIN_TEST_MISSING_DS_SECRET"
                }),
                "tester",
                version,
            )
            .await;

        // The account routes act on a manager, so one is created first.
        let (_, created) = harness
            .call(
                Method::POST,
                "/users",
                Some(json!({ "email": "matrix@example.com", "apps": [APP] })),
            )
            .await;
        let manager_id = created["id"].as_i64().expect("an id");

        let mut cases: Vec<MatrixCase> = vec![
            MatrixCase::new(
                Method::POST,
                "/auth/login",
                Some(json!({ "email": "user@example.com", "password": PASSWORD })),
            ),
            MatrixCase::new(Method::GET, "/auth/me", None),
            MatrixCase::new(
                Method::POST,
                "/auth/reauth",
                Some(json!({ "password": PASSWORD })),
            ),
            MatrixCase::new(
                Method::POST,
                "/auth/password",
                Some(json!({
                    "current_password": PASSWORD,
                    "new_password": "new password 2026"
                })),
            ),
            MatrixCase::new(Method::GET, "/apps", None),
            MatrixCase::new(Method::GET, "/apps/:app", None),
            MatrixCase::new(Method::POST, "/apps/:app/init", Some(json!({}))),
            MatrixCase::new(Method::GET, "/apps/:app/templates", None),
            MatrixCase::new(
                Method::POST,
                "/apps/:app/templates",
                Some(json!({ "key": "matrix_new", "definition": sample_template() })),
            ),
            MatrixCase::new(
                Method::POST,
                "/apps/:app/templates/validate",
                Some(json!({ "definition": sample_template() })),
            ),
            MatrixCase::new(Method::GET, "/apps/:app/templates/:key", None),
            MatrixCase::new(
                Method::PUT,
                "/apps/:app/templates/:key",
                Some(json!({ "definition": sample_template(), "expected_version": 1 })),
            ),
            MatrixCase::new(
                Method::PUT,
                "/apps/:app/templates/:key/enabled",
                Some(json!({ "enabled": true })),
            ),
            MatrixCase::new(
                Method::POST,
                "/apps/:app/templates/:key/preview",
                Some(json!({ "params": {}, "sender": { "uid": "u1" } })),
            ),
            MatrixCase::new(
                Method::POST,
                "/apps/:app/templates/:key/test-send",
                Some(json!({ "params": {}, "sender": { "uid": "u1" } })),
            ),
            MatrixCase::new(Method::GET, "/apps/:app/versions", None),
            MatrixCase::new(Method::GET, "/apps/:app/versions/:id", None),
            MatrixCase::new(
                Method::POST,
                "/apps/:app/versions/:id/restore",
                Some(json!({})),
            ),
            MatrixCase::new(Method::GET, "/apps/:app/export", None),
            MatrixCase::new(Method::POST, "/apps/:app/import", Some(document)),
            MatrixCase::at(Method::GET, "/audit", format!("/audit?app={}", APP), None),
            MatrixCase::new(Method::DELETE, "/apps/:app/templates/:key", None),
            MatrixCase::new(Method::GET, "/apps/:app/campaigns", None),
            MatrixCase::new(
                Method::PUT,
                "/apps/:app/campaigns",
                Some(json!({
                    "value": section_value(harness, "campaigns"),
                    "expected_version": 1
                })),
            ),
            MatrixCase::new(
                Method::POST,
                "/apps/:app/campaigns/:id/approve",
                Some(json!({})),
            ),
            MatrixCase::new(
                Method::POST,
                "/apps/:app/campaigns/:id/reject",
                Some(json!({ "note": "matrix" })),
            ),
            MatrixCase::new(Method::GET, "/apps/:app/blocks", None),
            MatrixCase::new(
                Method::POST,
                "/apps/:app/blocks",
                Some(json!({ "entity_id": "matrix" })),
            ),
            MatrixCase::new(Method::DELETE, "/apps/:app/blocks/:entity_id", None),
            MatrixCase::new(Method::GET, "/apps/:app/history", None),
            MatrixCase::new(Method::GET, "/apps/:app/history/:id", None),
            MatrixCase::new(
                Method::POST,
                "/apps/:app/send",
                Some(json!({ "title": "Matrix", "topic": "news" })),
            ),
            MatrixCase::new(Method::GET, "/apps/:app/devices/stats", None),
            MatrixCase::new(Method::GET, "/users", None),
            MatrixCase::new(
                Method::POST,
                "/users",
                Some(json!({ "email": "matrix2@example.com", "apps": [APP] })),
            ),
            MatrixCase::at(
                Method::PUT,
                "/users/:id",
                format!("/users/{}", manager_id),
                Some(json!({ "apps": [APP] })),
            ),
            MatrixCase::at(
                Method::POST,
                "/users/:id/reset-password",
                format!("/users/{}/reset-password", manager_id),
                None,
            ),
            MatrixCase::at(
                Method::DELETE,
                "/users/:id",
                format!("/users/{}", manager_id),
                None,
            ),
            MatrixCase::new(
                Method::PUT,
                "/auth/test-uid",
                Some(json!({ "test_uid": "matrix-user" })),
            ),
            MatrixCase::new(
                Method::POST,
                "/apps/:app/identity/test",
                Some(json!({ "token": "matrix" })),
            ),
            MatrixCase::new(
                Method::POST,
                "/apps/:app/data_source/test",
                Some(json!({ "path": "users/u1" })),
            ),
            MatrixCase::new(Method::GET, "/apps/:app/keys", None),
            MatrixCase::new(
                Method::POST,
                "/apps/:app/keys",
                Some(json!({ "name": "matrix" })),
            ),
            MatrixCase::new(Method::DELETE, "/apps/:app/keys/:id", None),
        ];
        for section in [
            "identity",
            "data_source",
            "devices",
            "web_subscribe",
            "campaigns",
            "app_check",
        ] {
            let version = harness
                .state
                .config
                .get(APP)
                .expect("the app exists")
                .version;
            cases.push(MatrixCase::at(
                Method::PUT,
                "/apps/:app/:section",
                format!("/apps/{}/{}", APP, section),
                Some(json!({ "value": section_value(harness, section), "expected_version": version })),
            ));
        }
        // The last case ends the session, so it must stay at the end.
        cases.push(MatrixCase::new(Method::POST, "/auth/logout", None));
        cases
    }

    /// A route of the matrix: its method and its path pattern.
    type MatrixRoute = (String, String);

    /// The registered routes and the routes the matrix covers.
    fn matrix_coverage(
        cases: &[MatrixCase],
    ) -> (
        std::collections::BTreeSet<MatrixRoute>,
        std::collections::BTreeSet<MatrixRoute>,
    ) {
        let registered: std::collections::BTreeSet<MatrixRoute> = registered_routes()
            .into_iter()
            .map(|(method, pattern, _)| (method.as_str().to_string(), pattern))
            .collect();
        let covered: std::collections::BTreeSet<MatrixRoute> = cases
            .iter()
            .map(|case| (case.method.as_str().to_string(), case.pattern.clone()))
            .collect();
        (registered, covered)
    }

    #[tokio::test]
    async fn matrix_owner_can_reach_every_route() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        harness.set_test_uid("tester").await;
        harness
            .register_device("tester", "token-tester-1-0000000000000000000000000000")
            .await;
        harness.save_template("sample", sample_template()).await;

        let cases = owner_matrix_cases(&harness).await;

        // Every registered route must be covered here, so a new route cannot
        // be added without a test.
        let (registered, covered) = matrix_coverage(&cases);
        assert_eq!(
            covered, registered,
            "the owner matrix must cover every registered route"
        );

        for case in cases {
            let (status, response) = call(
                &harness.router,
                case.method.clone(),
                &case.path,
                case.body,
                Some(&harness.token),
                true,
            )
            .await;
            assert_ne!(
                status,
                StatusCode::UNAUTHORIZED,
                "{} {} was not authenticated: {}",
                case.method,
                case.path,
                response
            );
            assert_ne!(
                status,
                StatusCode::FORBIDDEN,
                "{} {} was refused: {}",
                case.method,
                case.path,
                response
            );
        }
    }

    #[tokio::test]
    async fn malformed_body_unauthenticated_is_401() {
        // The session is checked before the body, so a broken body of an
        // anonymous request is a 401 and never a 400.
        let harness = setup("owner", &[]).await;
        let mut request = Request::builder()
            .method(Method::POST)
            .uri(format!("/apps/{}/identity/test", APP))
            .header("x-requested-with", CSRF_HEADER_VALUE)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("{not json"))
            .expect("the request must build");
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 40_000))));

        let response = harness
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("the request must run");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn matrix_covers_new_routes() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let cases = owner_matrix_cases(&harness).await;
        let (registered, covered) = matrix_coverage(&cases);
        assert_eq!(
            covered, registered,
            "the owner matrix must cover every registered route"
        );

        for pattern in [
            "/apps/:app/campaigns",
            "/apps/:app/campaigns/:id/approve",
            "/apps/:app/campaigns/:id/reject",
            "/apps/:app/blocks",
            "/apps/:app/blocks/:entity_id",
            "/apps/:app/history",
            "/apps/:app/history/:id",
            "/apps/:app/send",
            "/apps/:app/devices/stats",
        ] {
            assert!(
                covered.iter().any(|(_, known)| known == pattern),
                "the matrix must cover {}",
                pattern
            );
        }
    }

    #[tokio::test]
    async fn matrix_includes_users_and_diagnostics_routes() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let cases = owner_matrix_cases(&harness).await;
        let (registered, covered) = matrix_coverage(&cases);
        assert_eq!(
            covered, registered,
            "the owner matrix must cover every registered route"
        );

        for pattern in [
            "/users",
            "/users/:id",
            "/users/:id/reset-password",
            "/auth/test-uid",
            "/apps/:app/identity/test",
            "/apps/:app/data_source/test",
        ] {
            assert!(
                covered.iter().any(|(_, known)| known == pattern),
                "the matrix must cover {}",
                pattern
            );
        }
    }

    #[tokio::test]
    async fn matrix_manager_denied_owner_only_routes() {
        let harness = setup("manager", &[APP]).await;
        harness.reauth().await;
        let routes = vec![
            (Method::POST, "/apps/fresh/init".to_string(), json!({})),
            (
                Method::PUT,
                format!("/apps/{}/identity", APP),
                json!({ "value": {}, "expected_version": 1 }),
            ),
            (
                Method::PUT,
                format!("/apps/{}/data_source", APP),
                json!({ "value": {}, "expected_version": 1 }),
            ),
            (
                Method::PUT,
                format!("/apps/{}/devices", APP),
                json!({ "value": {}, "expected_version": 1 }),
            ),
            (
                Method::PUT,
                format!("/apps/{}/app_check", APP),
                json!({ "value": {}, "expected_version": 1 }),
            ),
            (Method::GET, format!("/apps/{}/export", APP), json!(null)),
            (Method::POST, format!("/apps/{}/import", APP), json!({})),
        ];

        for (method, uri, body) in routes {
            let (status, response) = call(
                &harness.router,
                method.clone(),
                &uri,
                Some(body),
                Some(&harness.token),
                true,
            )
            .await;
            assert_eq!(
                status,
                StatusCode::FORBIDDEN,
                "{} {} was not refused: {}",
                method,
                uri,
                response
            );
        }
    }

    #[tokio::test]
    async fn matrix_manager_allowed_on_own_app_routes() {
        let harness = setup("manager", &[APP]).await;
        harness.reauth().await;
        harness.set_test_uid("tester").await;
        harness
            .register_device("tester", "token-tester-1-0000000000000000000000000000")
            .await;
        harness.save_template("sample", sample_template()).await;

        let mut routes: Vec<(Method, String, Option<serde_json::Value>)> = vec![
            (Method::GET, "/apps".to_string(), None),
            (Method::GET, format!("/apps/{}", APP), None),
            (Method::GET, format!("/apps/{}/templates", APP), None),
            (
                Method::POST,
                format!("/apps/{}/templates", APP),
                Some(json!({ "key": "manager_new", "definition": sample_template() })),
            ),
            (
                Method::POST,
                format!("/apps/{}/templates/validate", APP),
                Some(json!({ "definition": sample_template() })),
            ),
            (Method::GET, format!("/apps/{}/templates/sample", APP), None),
            (
                Method::PUT,
                format!("/apps/{}/templates/sample", APP),
                Some(json!({ "definition": sample_template(), "expected_version": 1 })),
            ),
            (
                Method::PUT,
                format!("/apps/{}/templates/sample/enabled", APP),
                Some(json!({ "enabled": true })),
            ),
            (
                Method::POST,
                format!("/apps/{}/templates/sample/preview", APP),
                Some(json!({ "params": {}, "sender": { "uid": "u1" } })),
            ),
            (
                Method::POST,
                format!("/apps/{}/templates/sample/test-send", APP),
                Some(json!({ "params": {}, "sender": { "uid": "u1" } })),
            ),
            (Method::GET, format!("/apps/{}/versions", APP), None),
            (Method::GET, format!("/audit?app={}", APP), None),
        ];

        for section in ["web_subscribe", "campaigns"] {
            let version = harness
                .state
                .config
                .get(APP)
                .expect("the app exists")
                .version;
            routes.push((
                Method::PUT,
                format!("/apps/{}/{}", APP, section),
                Some(json!({ "value": section_value(&harness, section), "expected_version": version })),
            ));
        }

        for (method, uri, body) in routes {
            let (status, response) = call(
                &harness.router,
                method.clone(),
                &uri,
                body,
                Some(&harness.token),
                true,
            )
            .await;
            assert_ne!(
                status,
                StatusCode::FORBIDDEN,
                "{} {} was refused: {}",
                method,
                uri,
                response
            );
            assert_ne!(status, StatusCode::UNAUTHORIZED);
        }
    }

    #[tokio::test]
    async fn matrix_manager_other_app_is_403_not_404() {
        let harness = setup("manager", &[APP]).await;
        for app in [OTHER_APP, "missing"] {
            let (status, body) = harness
                .call(Method::GET, &format!("/apps/{}", app), None)
                .await;
            assert_eq!(status, StatusCode::FORBIDDEN, "app {}: {}", app, body);
            assert_eq!(body["error"], "forbidden");
        }
    }

    #[tokio::test]
    async fn matrix_must_change_password_blocks_new_routes() {
        let pool = memory_pool().await;
        let state = test_state(pool.clone()).await;
        bind_app(&state, APP, "project-watt").await;
        insert_user(&pool, "pending@example.com", "owner", true).await;
        let router = app(state);
        let login = login_request(&router, "pending@example.com", PASSWORD).await;
        let token = session_token(&login);

        let (status, body) = call(&router, Method::GET, "/apps", None, Some(&token), true).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"], "password_change_required");
    }

    #[tokio::test]
    async fn matrix_missing_csrf_header_is_403_on_writes() {
        let harness = setup("owner", &[]).await;
        let (status, body) = call(
            &harness.router,
            Method::POST,
            &format!("/apps/{}/templates", APP),
            Some(json!({ "key": "sample", "definition": sample_template() })),
            Some(&harness.token),
            false,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"], "forbidden");
    }
}

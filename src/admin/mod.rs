//! Dashboard admin API (T2): accounts, sessions and the CSRF guard.
//!
//! Everything here lives under `/api/v1/admin` and answers with the JSON error
//! envelope `{"error": "<code>", "message": "<text>"}` instead of the plain
//! text used by the rest of the tool.

pub mod auth;
pub mod password;
pub mod session;

use crate::ratelimit::RateLimiter;
use crate::AppState;
use axum::extract::{FromRequest, Request, State};
use axum::http::{header, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::de::DeserializeOwned;
use serde_json::json;
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
#[derive(Debug)]
pub struct AdminError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

impl AdminError {
    fn new(status: StatusCode, code: &'static str, message: &str) -> Self {
        Self {
            status,
            code,
            message: message.to_string(),
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
        let body = json!({ "error": self.code, "message": self.message });
        (self.status, Json(body)).into_response()
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

/// The admin routes. The caller nests this under `/admin`.
pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/me", get(auth::me))
        .route("/auth/password", post(auth::change_password))
        .route("/auth/reauth", post(auth::reauth))
        .layer(axum::middleware::from_fn_with_state(state, csrf_guard))
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
mod tests {
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

    const PASSWORD: &str = "correct horse battery 2026";
    const NEW_PASSWORD: &str = "a brand new passphrase 2026";
    const DASHBOARD_ORIGIN: &str = "http://localhost:5173";

    async fn memory_pool() -> SqlitePool {
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

    async fn test_state(pool: SqlitePool) -> Arc<AppState> {
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
        let identity = Arc::new(crate::identity::registry::IdentityRegistry::new(
            config.clone(),
            project_map.clone(),
            client.clone(),
            Arc::new(crate::identity::jwt::SystemEnv),
        ));
        let datasource = Arc::new(crate::datasource::registry::DataSourceRegistry::new(
            config.clone(),
            project_map.clone(),
            auth_map.clone(),
            client.clone(),
            Arc::new(crate::identity::jwt::SystemEnv),
        ));
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
        );
        let tool_auth = Arc::new(ToolAuth {
            pool: pool.clone(),
            cookie: session::CookieConfig::with_hours(true, 12),
            login_limiter: RateLimiter::new(LOGIN_FAILURE_LIMIT, LOGIN_FAILURE_WINDOW),
            dummy_hash: password::hash_password(&session::generate_token())
                .await
                .expect("the dummy hash must build"),
            allowed_origins: vec![DASHBOARD_ORIGIN.to_string()],
        });

        let fcm = Arc::new(crate::send::HttpFcm::new(client.clone(), auth_map.clone()));

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
        })
    }

    fn app(state: Arc<AppState>) -> Router {
        router(state.clone()).with_state(state)
    }

    fn build_request(method: Method, uri: &str, body: Option<serde_json::Value>) -> Request {
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

    fn with_cookie(mut request: Request, name: &str, token: &str) -> Request {
        let value =
            HeaderValue::from_str(&format!("{}={}", name, token)).expect("a valid cookie header");
        request.headers_mut().insert(header::COOKIE, value);
        request
    }

    async fn read_json(response: Response) -> serde_json::Value {
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

    fn session_token(response: &Response) -> String {
        session::read_cookie(
            [set_cookie_of(response).as_str()],
            session::SECURE_COOKIE_NAME,
        )
        .expect("the cookie must carry the token")
    }

    async fn insert_user(
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

    async fn login_request(app: &Router, email: &str, password: &str) -> Response {
        let body = json!({ "email": email, "password": password });
        app.clone()
            .oneshot(build_request(Method::POST, "/auth/login", Some(body)))
            .await
            .expect("the login must run")
    }

    async fn send(app: &Router, request: Request) -> Response {
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
}

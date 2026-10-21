mod admin;
mod auth;
mod cli;
mod config;
mod credentials;
mod datasource;
mod db;
mod devices;
mod identity;
mod ratelimit;
mod roles;
mod template;
mod topics;
mod validation;

use auth::{AdminIdentity, AuthConfig, AuthError, AuthService};
use axum::{
    extract::{rejection::JsonRejection, ConnectInfo, Query, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use config::store::ConfigStore;
use credentials::{AuthMap, ProjectIdMap};
use datasource::registry::DataSourceRegistry;
use devices::{DeviceError, DeviceStore, Platform};
use identity::jwt::SystemEnv;
use identity::registry::IdentityRegistry;
use ratelimit::RateLimiter;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::Row;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::services::{ServeDir, ServeFile};

const MAX_REQUEST_BODY_BYTES: usize = 64 * 1024;
const TOPICS_RATE_LIMIT_PER_MINUTE: usize = 30;
const AUTH_FAILURE_RATE_LIMIT_PER_MINUTE: usize = 10;
/// Device registrations allowed per uid per minute (T6).
const DEVICES_RATE_LIMIT_PER_MINUTE: usize = 20;
const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(60);

struct AppState {
    db: sqlx::SqlitePool,
    auth_map: AuthMap,
    project_map: ProjectIdMap,
    client: Client,
    auth: AuthService,
    trust_proxy: bool,
    topics_limiter: RateLimiter,
    auth_fail_limiter: RateLimiter,
    /// T1: the configuration store. Used from T7/T8.
    #[allow(dead_code)]
    config: Arc<ConfigStore>,
    /// T2: dashboard accounts and sessions.
    tool_auth: Arc<admin::ToolAuth>,
    /// T6: token ↔ uid registry.
    devices: Arc<DeviceStore>,
    /// T6: per-uid registration limit.
    devices_limiter: RateLimiter,
    /// T3: identity providers per app. Used from T6.
    #[allow(dead_code)]
    identity: Arc<IdentityRegistry>,
    /// T4: data sources per app. Used from T6.
    #[allow(dead_code)]
    datasource: Arc<DataSourceRegistry>,
}

#[derive(Deserialize, Serialize, Debug)]
struct SendNotificationRequest {
    app: String,
    topic: Option<String>,
    token: Option<String>,
    condition: Option<String>,
    title: String,
    body: Option<String>,
    image: Option<String>,
    data: Option<serde_json::Value>,
    analytics_label: Option<String>,
}

#[derive(Deserialize)]
struct TopicRequest {
    app: String,
    name: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let _ = dotenvy::dotenv();

    // CLI first: `import` and `export` must work before the CORS check and
    // before the port is opened. Without arguments the server starts as usual.
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let code = cli::run(&args).await;
        std::process::exit(code);
    }

    let cors = match build_cors_layer() {
        Ok(layer) => layer,
        Err(message) => {
            tracing::error!("{}", message);
            std::process::exit(1);
        }
    };

    let service_api_key = env_optional("SERVICE_API_KEY");
    let legacy_api_key = env_optional("API_KEY");
    let legacy_enabled = env_flag("LEGACY_API_KEY_ENABLED");
    let trust_proxy = env_flag("TRUST_PROXY");

    if service_api_key.is_none() {
        tracing::warn!("SERVICE_API_KEY is not set; service-key authentication is disabled");
    }
    if legacy_enabled && legacy_api_key.is_none() {
        tracing::warn!(
            "LEGACY_API_KEY_ENABLED=true but API_KEY is unset; legacy authentication stays disabled"
        );
    }
    if !legacy_enabled && legacy_api_key.is_some() {
        tracing::warn!(
            "API_KEY is set but LEGACY_API_KEY_ENABLED is not true; it will not be accepted"
        );
    }

    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());

    let db_pool = match db::init_db().await {
        Ok(pool) => pool,
        Err(error) => {
            tracing::error!("Failed to initialize database: {}", error);
            std::process::exit(1);
        }
    };

    let client = match Client::builder()
        .timeout(Duration::from_secs(30))
        // The signed resolver contract never follows a redirect: a redirect
        // would move the request away from the path that was signed.
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            tracing::error!("Failed to build the HTTP client: {}", error);
            std::process::exit(1);
        }
    };

    let auth_map: AuthMap = Arc::new(RwLock::new(HashMap::new()));
    let project_map: ProjectIdMap = Arc::new(RwLock::new(HashMap::new()));

    let creds_dir = PathBuf::from("credentials");
    credentials::scan_credentials_dir(&creds_dir, auth_map.clone(), project_map.clone()).await;
    credentials::watch_credentials_dir(creds_dir.clone(), auth_map.clone(), project_map.clone());

    let config_store = match ConfigStore::new(db_pool.clone()).await {
        Ok(store) => Arc::new(store),
        Err(error) => {
            tracing::error!("Failed to load the config store: {}", error);
            std::process::exit(1);
        }
    };
    tracing::info!(
        "Config store loaded with {} app(s)",
        config_store.app_count()
    );

    let identity_registry = Arc::new(IdentityRegistry::new(
        config_store.clone(),
        project_map.clone(),
        client.clone(),
        Arc::new(SystemEnv),
    ));
    let datasource_registry = Arc::new(DataSourceRegistry::new(
        config_store.clone(),
        project_map.clone(),
        auth_map.clone(),
        client.clone(),
        Arc::new(SystemEnv),
    ));

    let auth = AuthService::new(
        project_map.clone(),
        identity_registry.clone(),
        datasource_registry.clone(),
        config_store.clone(),
        AuthConfig {
            service_api_key,
            legacy_api_key,
            legacy_enabled,
        },
    );

    let tool_auth = match admin::ToolAuth::new(db_pool.clone()).await {
        Ok(tool_auth) => tool_auth,
        Err(message) => {
            tracing::error!("Failed to prepare the dashboard accounts: {}", message);
            std::process::exit(1);
        }
    };
    admin::spawn_session_cleanup(tool_auth.clone());

    let device_store = Arc::new(DeviceStore::new(db_pool.clone()));

    let state = Arc::new(AppState {
        db: db_pool,
        auth_map,
        project_map,
        client,
        auth,
        trust_proxy,
        topics_limiter: RateLimiter::new(TOPICS_RATE_LIMIT_PER_MINUTE, RATE_LIMIT_WINDOW),
        auth_fail_limiter: RateLimiter::new(AUTH_FAILURE_RATE_LIMIT_PER_MINUTE, RATE_LIMIT_WINDOW),
        devices: device_store,
        devices_limiter: RateLimiter::new(DEVICES_RATE_LIMIT_PER_MINUTE, RATE_LIMIT_WINDOW),
        config: config_store,
        tool_auth,
        identity: identity_registry,
        datasource: datasource_registry,
    });

    // Serve static files from "dist", fallback to index.html for SPA routing
    let serve_dir = ServeDir::new("dist").not_found_service(ServeFile::new("dist/index.html"));

    let api_routes = Router::new()
        .route("/apps", get(list_apps))
        .route("/history", get(get_history))
        .route("/topics", get(get_topics).post(add_topic))
        .route("/send", post(send_notification))
        .route("/topics/subscribe", post(topics::subscribe))
        .route("/topics/unsubscribe", post(topics::unsubscribe))
        .route("/devices/register", post(register_device))
        .route("/devices/unregister", post(unregister_device))
        .nest("/admin", admin::router(state.clone()))
        .fallback(api_fallback)
        .layer(RequestBodyLimitLayer::new(MAX_REQUEST_BODY_BYTES))
        .with_state(state.clone());

    let app = Router::new()
        .nest("/api/v1", api_routes)
        .route("/api", axum::routing::any(api_fallback))
        .route("/api/*path", axum::routing::any(api_fallback))
        .route("/health", get(|| async { "OK" }))
        .fallback_service(serve_dir)
        .layer(cors);

    let addr = format!("0.0.0.0:{}", port);
    let listener = match TcpListener::bind(&addr).await {
        Ok(listener) => listener,
        Err(error) => {
            tracing::error!("Failed to bind {}: {}", addr, error);
            std::process::exit(1);
        }
    };
    tracing::info!("Server listening on {}", addr);

    if let Err(error) = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    {
        tracing::error!("Server error: {}", error);
        std::process::exit(1);
    }
}

fn env_optional(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn env_flag(name: &str) -> bool {
    env_optional(name)
        .map(|value| {
            value.eq_ignore_ascii_case("true") || value == "1" || value.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
}

fn build_cors_layer() -> Result<CorsLayer, String> {
    let raw = std::env::var("CORS_ALLOWED_ORIGINS").unwrap_or_default();
    let mut origins = Vec::new();
    for entry in raw.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        if entry == "*" {
            return Err("CORS_ALLOWED_ORIGINS must not be '*'; list explicit origins".to_string());
        }
        if !(entry.starts_with("http://") || entry.starts_with("https://")) {
            return Err(format!(
                "CORS_ALLOWED_ORIGINS entry '{entry}' must start with http:// or https://"
            ));
        }
        let value: HeaderValue = entry
            .parse()
            .map_err(|_| format!("CORS_ALLOWED_ORIGINS entry '{entry}' is not a valid origin"))?;
        origins.push(value);
    }
    if origins.is_empty() {
        return Err(
            "CORS_ALLOWED_ORIGINS is required (comma-separated origins); refusing to start with an empty/open CORS policy"
                .to_string(),
        );
    }

    Ok(CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE, header::ACCEPT])
        .max_age(Duration::from_secs(600)))
}

async fn api_fallback(uri: axum::http::Uri) -> impl IntoResponse {
    let response_body = json!({
        "error": "Endpoint not found",
        "requested_path": uri.path(),
        "hint": "All API endpoints are under /api/v1. Available endpoints are listed below:",
        "available_endpoints": [
            {
                "method": "POST",
                "endpoint": "/api/v1/send",
                "description": "Send FCM push notification to topic, token, or condition (admin ID token or SERVICE_API_KEY)"
            },
            {
                "method": "GET",
                "endpoint": "/api/v1/apps",
                "description": "List Firebase apps where the caller is an admin (admin ID token or SERVICE_API_KEY)"
            },
            {
                "method": "GET",
                "endpoint": "/api/v1/history",
                "description": "Get notification history (admin ID token or SERVICE_API_KEY; supports ?app=, ?topic=, ?search=, ?limit=)"
            },
            {
                "method": "GET",
                "endpoint": "/api/v1/topics",
                "description": "Get saved topics (admin ID token or SERVICE_API_KEY; supports ?app=)"
            },
            {
                "method": "POST",
                "endpoint": "/api/v1/topics",
                "description": "Save a new topic (admin ID token or SERVICE_API_KEY; payload: {\"app\": \"...\", \"name\": \"...\"})"
            },
            {
                "method": "POST",
                "endpoint": "/api/v1/topics/subscribe",
                "description": "Subscribe device tokens to a topic (public topics anonymously; role/admin topics with a valid ID token)"
            },
            {
                "method": "POST",
                "endpoint": "/api/v1/topics/unsubscribe",
                "description": "Unsubscribe device tokens from a topic (same authorization as subscribe)"
            },
            {
                "method": "GET",
                "endpoint": "/health",
                "description": "Service health check"
            }
        ]
    });

    (StatusCode::NOT_FOUND, Json(response_body))
}

fn auth_error_with_limit(
    state: &AppState,
    headers: &HeaderMap,
    peer: SocketAddr,
    error: AuthError,
) -> (StatusCode, String) {
    if matches!(error, AuthError::Unauthorized(_) | AuthError::Forbidden(_)) {
        let key = format!(
            "ip:{}",
            auth::client_ip(headers, Some(peer), state.trust_proxy)
        );
        if !state.auth_fail_limiter.check(&key) {
            return topics::too_many_requests();
        }
    }
    error.into_response()
}

async fn authorize_admin(
    state: &AppState,
    headers: &HeaderMap,
    app: Option<&str>,
    peer: SocketAddr,
) -> Result<(AdminIdentity, Option<Vec<String>>), (StatusCode, String)> {
    let result = match app {
        Some(app) => state
            .auth
            .authenticate_admin(headers, app)
            .await
            .map(|identity| {
                let apps = if identity.is_service() {
                    None
                } else {
                    Some(vec![app.to_string()])
                };
                (identity, apps)
            }),
        None => state.auth.authenticate_admin_any(headers).await,
    };

    result.map_err(|error| auth_error_with_limit(state, headers, peer, error))
}

fn enforce_topics_rate_limit(
    state: &AppState,
    headers: &HeaderMap,
    peer: SocketAddr,
    identity: &AdminIdentity,
) -> Result<(), (StatusCode, String)> {
    topics::check_ip_limit(state, headers, peer)?;
    if !identity.is_service() {
        topics::check_uid_limit(state, &identity.uid)?;
    }
    Ok(())
}

async fn list_apps(
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let (_, admin_apps) = authorize_admin(&state, &headers, None, peer).await?;

    let map = state.auth_map.read().await;
    let mut apps: Vec<String> = match admin_apps {
        None => map.keys().cloned().collect(),
        Some(allowed) => map
            .keys()
            .filter(|app| allowed.contains(app))
            .cloned()
            .collect(),
    };
    apps.sort();
    Ok((StatusCode::OK, Json(apps)))
}

#[derive(Deserialize)]
struct HistoryQuery {
    app: Option<String>,
    topic: Option<String>,
    search: Option<String>,
    limit: Option<i64>,
}

async fn get_history(
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Query(query): Query<HistoryQuery>,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let app = query
        .app
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let (_, admin_apps) = authorize_admin(&state, &headers, app, peer).await?;

    let mut qb = sqlx::QueryBuilder::new(
        "SELECT id, app, target, title, status, created_at, payload FROM notifications_history WHERE 1=1",
    );

    if let Some(app) = app {
        qb.push(" AND app = ");
        qb.push_bind(app.to_string());
    } else if let Some(apps) = &admin_apps {
        if apps.is_empty() {
            return Ok((StatusCode::OK, Json(Vec::new())));
        }
        qb.push(" AND app IN (");
        {
            let mut separated = qb.separated(", ");
            for app in apps {
                separated.push_bind(app.clone());
            }
        }
        qb.push(")");
    }

    if let Some(topic) = &query.topic {
        if !topic.trim().is_empty() {
            qb.push(" AND target LIKE ");
            qb.push_bind(format!("%{}%", topic.trim()));
        }
    }

    if let Some(search) = &query.search {
        if !search.trim().is_empty() {
            qb.push(" AND (title LIKE ");
            qb.push_bind(format!("%{}%", search.trim()));
            qb.push(" OR payload LIKE ");
            qb.push_bind(format!("%{}%", search.trim()));
            qb.push(")");
        }
    }

    let limit = query.limit.unwrap_or(100).clamp(1, 1000);
    qb.push(" ORDER BY id DESC LIMIT ");
    qb.push_bind(limit);

    let query_exec = qb.build();
    let rows = query_exec
        .fetch_all(&state.db)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;

    let mut result = Vec::new();
    for row in rows {
        let id: i64 = row
            .try_get("id")
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
        let app: String = row
            .try_get("app")
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
        let target: String = row
            .try_get("target")
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
        let title: String = row
            .try_get("title")
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
        let status: String = row
            .try_get("status")
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
        let created_at: String = row
            .try_get("created_at")
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
        let payload: Option<String> = row
            .try_get("payload")
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
        result.push(json!({
            "id": id,
            "app": app,
            "target": target,
            "title": title,
            "status": status,
            "created_at": created_at,
            "payload": payload
        }));
    }
    Ok((StatusCode::OK, Json(result)))
}

#[derive(Deserialize)]
struct TopicQuery {
    app: Option<String>,
}

async fn get_topics(
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Query(query): Query<TopicQuery>,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let app = query
        .app
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let (identity, admin_apps) = authorize_admin(&state, &headers, app, peer).await?;
    enforce_topics_rate_limit(&state, &headers, peer, &identity)?;

    let mut qb = sqlx::QueryBuilder::new("SELECT name FROM topics WHERE 1=1");

    if let Some(app) = app {
        qb.push(" AND app = ");
        qb.push_bind(app.to_string());
    } else if let Some(apps) = &admin_apps {
        if apps.is_empty() {
            return Ok((StatusCode::OK, Json(Vec::new())));
        }
        qb.push(" AND app IN (");
        {
            let mut separated = qb.separated(", ");
            for app in apps {
                separated.push_bind(app.clone());
            }
        }
        qb.push(")");
    }

    qb.push(" ORDER BY name ASC");
    let rows = qb
        .build()
        .fetch_all(&state.db)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;

    let mut result = Vec::new();
    for row in rows {
        let name: String = row
            .try_get("name")
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
        result.push(name);
    }
    Ok((StatusCode::OK, Json(result)))
}

async fn add_topic(
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    State(state): State<Arc<AppState>>,
    Json(payload): Json<TopicRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if payload.app.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "app field is required".to_string()));
    }
    validation::validate_topic_name(&payload.name)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))?;

    let app_exists = {
        let map = state.project_map.read().await;
        map.contains_key(&payload.app)
    };
    if !app_exists {
        return Err((
            StatusCode::NOT_FOUND,
            format!("App '{}' not found in credentials folder.", payload.app),
        ));
    }

    let (identity, _) = authorize_admin(&state, &headers, Some(&payload.app), peer).await?;
    enforce_topics_rate_limit(&state, &headers, peer, &identity)?;

    db::save_topic(&state.db, &payload.app, &payload.name)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok((StatusCode::OK, Json(json!({"status": "success"}))))
}

async fn send_notification(
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SendNotificationRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let identity = state
        .auth
        .authenticate_send(&headers, &payload.app)
        .await
        .map_err(|error| auth_error_with_limit(&state, &headers, peer, error))?;

    validation::validate_send(
        &payload.app,
        payload.topic.as_deref(),
        payload.token.as_deref(),
        payload.condition.as_deref(),
        &payload.title,
        payload.body.as_deref(),
        payload.image.as_deref(),
        payload.data.as_ref(),
    )
    .map_err(|error| (StatusCode::BAD_REQUEST, error))?;

    // Never persist device registration tokens or authorization material.
    let payload_str = {
        let mut value = serde_json::to_value(&payload).map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not serialize payload".to_string(),
            )
        })?;
        if payload.token.is_some() {
            if let Some(object) = value.as_object_mut() {
                object.insert("token".to_string(), json!("[redacted]"));
            }
        }
        value.to_string()
    };

    let target = if let Some(topic) = &payload.topic {
        topic.clone()
    } else if payload.token.is_some() {
        "[device-token]".to_string()
    } else {
        payload.condition.clone().unwrap_or_default()
    };

    let project_id = {
        let map = state.project_map.read().await;
        map.get(&payload.app).cloned()
    };
    let Some(project_id) = project_id else {
        let _ = db::log_notification(
            &state.db,
            &payload.app,
            &target,
            &payload.title,
            "failed",
            "App not found",
            &payload_str,
            &identity.uid,
            identity.email.as_deref(),
        )
        .await;
        return Err((
            StatusCode::NOT_FOUND,
            format!("App '{}' not found in credentials folder.", payload.app),
        ));
    };

    let auth = {
        let map = state.auth_map.read().await;
        map.get(&payload.app).cloned()
    }
    .ok_or((
        StatusCode::INTERNAL_SERVER_ERROR,
        "Service account for this app is not loaded".to_string(),
    ))?;

    let token = auth
        .token(&["https://www.googleapis.com/auth/firebase.messaging"])
        .await
        .map_err(|_| {
            (
                StatusCode::BAD_GATEWAY,
                "Could not authenticate with Firebase".to_string(),
            )
        })?;
    let access_token = token.token().ok_or((
        StatusCode::BAD_GATEWAY,
        "Missing Firebase access token".to_string(),
    ))?;

    let mut message = json!({
        "notification": {
            "title": payload.title,
        }
    });

    if let Some(body) = &payload.body {
        message["notification"]["body"] = json!(body);
    }
    if let Some(image) = &payload.image {
        message["notification"]["image"] = json!(image);
    }

    if let Some(data) = &payload.data {
        if let Some(obj) = data.as_object() {
            let mut string_map = serde_json::Map::new();
            for (key, value) in obj {
                let value_str = match value {
                    serde_json::Value::String(string) => string.clone(),
                    other => other.to_string(),
                };
                string_map.insert(key.clone(), serde_json::Value::String(value_str));
            }
            message["data"] = serde_json::Value::Object(string_map);
        }
    }

    if let Some(label) = &payload.analytics_label {
        message["fcm_options"] = json!({"analytics_label": label});
    }

    if let Some(topic) = &payload.topic {
        message["topic"] = json!(topic);
        // Automatically save topic to DB
        let _ = db::save_topic(&state.db, &payload.app, topic).await;
    } else if let Some(token) = &payload.token {
        message["token"] = json!(token);
    } else if let Some(condition) = &payload.condition {
        message["condition"] = json!(condition);
    }

    let fcm_payload = json!({ "message": message });
    let url = format!(
        "https://fcm.googleapis.com/v1/projects/{}/messages:send",
        project_id
    );

    let res = state
        .client
        .post(&url)
        .bearer_auth(access_token)
        .json(&fcm_payload)
        .send()
        .await
        .map_err(|_| {
            (
                StatusCode::BAD_GATEWAY,
                "Network error while calling Firebase".to_string(),
            )
        })?;

    let status = res.status();
    let body_text = res
        .text()
        .await
        .unwrap_or_else(|_| "Failed to read response body".to_string());

    if status.is_success() {
        let _ = db::log_notification(
            &state.db,
            &payload.app,
            &target,
            &payload.title,
            "success",
            &body_text,
            &payload_str,
            &identity.uid,
            identity.email.as_deref(),
        )
        .await;
        Ok(raw_send_success_response(body_text))
    } else {
        let _ = db::log_notification(
            &state.db,
            &payload.app,
            &target,
            &payload.title,
            "failed",
            &body_text,
            &payload_str,
            &identity.uid,
            identity.email.as_deref(),
        )
        .await;
        Ok(raw_send_fcm_error_response(status.as_u16(), &body_text))
    }
}

/// Maps a successful FCM response to the raw `/send` response.
fn raw_send_success_response(body_text: String) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::OK,
        Json(json!({ "status": "success", "response": body_text })),
    )
}

/// Maps a failed FCM HTTP response to the raw `/send` response.
///
/// An FCM `4xx` status is passed through; every other status is reported as
/// `400`, which is the historical behavior of the route.
fn raw_send_fcm_error_response(
    http_status: u16,
    body_text: &str,
) -> (StatusCode, Json<serde_json::Value>) {
    let parsed_error: serde_json::Value =
        serde_json::from_str(body_text).unwrap_or_else(|_| json!(body_text));
    let client_status = if (400..500).contains(&http_status) {
        StatusCode::from_u16(http_status).unwrap_or(StatusCode::BAD_REQUEST)
    } else {
        StatusCode::BAD_REQUEST
    };
    (
        client_status,
        Json(json!({
            "status": "failed",
            "error": "fcm_error",
            "response": parsed_error
        })),
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegisterDeviceRequest {
    app: String,
    token: String,
    platform: Platform,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UnregisterDeviceRequest {
    app: String,
    token: String,
}

/// Maps a device error to a response. The message never contains a token.
fn device_error(error: DeviceError) -> (StatusCode, String) {
    match error {
        DeviceError::InvalidToken => (
            StatusCode::BAD_REQUEST,
            "the device token is invalid".to_string(),
        ),
        DeviceError::InvalidRoleValue => (
            StatusCode::BAD_REQUEST,
            "the role value is invalid".to_string(),
        ),
        DeviceError::NotConfigured => (
            StatusCode::BAD_REQUEST,
            "role members are not configured for this app".to_string(),
        ),
        DeviceError::Unavailable(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            "the data source is unavailable".to_string(),
        ),
        DeviceError::Database(error) => {
            tracing::error!(error = %error, "device storage failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "device storage failed".to_string(),
            )
        }
    }
}

/// An app is known when it has stored configuration or loaded credentials.
/// Keeping the historical `404` message for every other app (as `topics.rs`).
async fn ensure_known_app(state: &AppState, app: &str) -> Result<(), (StatusCode, String)> {
    if state.config.get(app).is_some() {
        return Ok(());
    }
    let known = {
        let projects = state.project_map.read().await;
        projects.contains_key(app)
    };
    if known {
        Ok(())
    } else {
        Err((
            StatusCode::NOT_FOUND,
            format!("App '{app}' not found in credentials folder."),
        ))
    }
}

/// Verifies a user token (never a service or legacy key) and applies the
/// per-uid device limit. Returns the uid.
async fn authorize_device(
    state: &AppState,
    headers: &HeaderMap,
    peer: SocketAddr,
    app: &str,
) -> Result<String, (StatusCode, String)> {
    let identity = state
        .auth
        .authenticate_identity(headers, app)
        .await
        .map_err(|error| auth_error_with_limit(state, headers, peer, error))?;
    if !state
        .devices_limiter
        .check(&format!("uid:{app}:{}", identity.uid))
    {
        return Err(topics::too_many_requests());
    }
    Ok(identity.uid)
}

async fn register_device(
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    State(state): State<Arc<AppState>>,
    payload: Result<Json<RegisterDeviceRequest>, JsonRejection>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let Json(payload) =
        payload.map_err(|_| (StatusCode::BAD_REQUEST, "invalid request body".to_string()))?;
    let app = payload.app.trim().to_string();
    if app.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "app field is required".to_string()));
    }

    ensure_known_app(&state, &app).await?;
    topics::check_ip_limit(&state, &headers, peer)?;
    let uid = authorize_device(&state, &headers, peer, &app).await?;
    devices::validate_token(&payload.token).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "the device token is invalid".to_string(),
        )
    })?;

    let max_per_uid = state
        .config
        .get(&app)
        .map(|snapshot| snapshot.config.devices.max_per_uid)
        .unwrap_or_else(|| config::Devices::default().max_per_uid);
    let evicted = state
        .devices
        .register(&app, &uid, &payload.token, payload.platform, max_per_uid)
        .await
        .map_err(device_error)?;

    // A count and a debug line only: the token is never logged.
    tracing::debug!(event = "devices.register", app = %app, evicted, "device registered");

    Ok((
        StatusCode::OK,
        Json(json!({ "registered": true, "evicted": evicted })),
    ))
}

async fn unregister_device(
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    State(state): State<Arc<AppState>>,
    payload: Result<Json<UnregisterDeviceRequest>, JsonRejection>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let Json(payload) =
        payload.map_err(|_| (StatusCode::BAD_REQUEST, "invalid request body".to_string()))?;
    let app = payload.app.trim().to_string();
    if app.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "app field is required".to_string()));
    }

    ensure_known_app(&state, &app).await?;
    topics::check_ip_limit(&state, &headers, peer)?;
    let uid = authorize_device(&state, &headers, peer, &app).await?;
    devices::validate_token(&payload.token).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "the device token is invalid".to_string(),
        )
    })?;

    // `false` covers both "unknown token" and "owned by someone else", so the
    // route is not an ownership oracle.
    let removed = state
        .devices
        .unregister(&app, &uid, &payload.token)
        .await
        .map_err(device_error)?;

    Ok((StatusCode::OK, Json(json!({ "removed": removed }))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin::{session::CookieConfig, ToolAuth};
    use crate::config::store::ConfigStore;
    use crate::datasource::registry::DataSourceRegistry;
    use crate::devices::DeviceStore;
    use crate::identity::jwt::EnvSource;
    use crate::identity::registry::IdentityRegistry;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use hyper::client::HttpConnector;
    use hyper_rustls::HttpsConnector;
    use serde_json::Value;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::RwLock;
    use tower::ServiceExt;
    use yup_oauth2::{
        authenticator::Authenticator, ServiceAccountAuthenticator, ServiceAccountKey,
    };

    const APP: &str = "testapp";
    const SERVICE_KEY: &str = "test-service-key";
    const PROJECT: &str = "test-project";

    fn test_client() -> Client {
        Client::builder()
            .build()
            .expect("the test client must build")
    }

    async fn memory_pool() -> sqlx::SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("the in-memory database must open");
        db::migrate(&pool).await.expect("the migrations must run");
        pool
    }

    struct FakeEnv;

    impl EnvSource for FakeEnv {
        fn get(&self, _name: &str) -> Option<String> {
            None
        }
    }

    async fn test_state(client: Client) -> Arc<AppState> {
        let pool = memory_pool().await;
        let config = Arc::new(
            ConfigStore::new(pool.clone())
                .await
                .expect("the config store must open"),
        );
        config
            .create_app(APP, "tester")
            .await
            .expect("the app must be created");

        let project_map: ProjectIdMap = Arc::new(RwLock::new(HashMap::new()));
        let auth_map: AuthMap = Arc::new(RwLock::new(HashMap::new()));
        let env: Arc<dyn EnvSource> = Arc::new(FakeEnv);
        let identity = Arc::new(IdentityRegistry::new(
            config.clone(),
            project_map.clone(),
            client.clone(),
            env.clone(),
        ));
        let datasource = Arc::new(DataSourceRegistry::new(
            config.clone(),
            project_map.clone(),
            auth_map.clone(),
            client.clone(),
            env,
        ));
        let auth = AuthService::new(
            project_map.clone(),
            identity.clone(),
            datasource.clone(),
            config.clone(),
            AuthConfig {
                service_api_key: Some(SERVICE_KEY.to_string()),
                legacy_api_key: None,
                legacy_enabled: false,
            },
        );
        let tool_auth = Arc::new(ToolAuth {
            pool: pool.clone(),
            cookie: CookieConfig::with_hours(true, 12),
            login_limiter: RateLimiter::new(5, Duration::from_secs(900)),
            dummy_hash: "unused in send tests".to_string(),
            allowed_origins: Vec::new(),
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
            config,
            tool_auth,
            devices: Arc::new(DeviceStore::new(pool.clone())),
            devices_limiter: RateLimiter::new(20, Duration::from_secs(60)),
            identity,
            datasource,
        })
    }

    fn send_router(state: Arc<AppState>) -> Router {
        Router::new()
            .route("/send", post(send_notification))
            .with_state(state)
    }

    fn send_request(body: Value) -> Request<Body> {
        let mut request = Request::builder()
            .method(Method::POST)
            .uri("/send")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {SERVICE_KEY}"))
            .body(Body::from(body.to_string()))
            .expect("the request must build");
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 40_000))));
        request
    }

    async fn call(router: &Router, request: Request<Body>) -> (StatusCode, String) {
        let response = router
            .clone()
            .oneshot(request)
            .await
            .expect("the router must answer");
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("the body must be readable")
            .to_bytes();
        (status, String::from_utf8_lossy(&bytes).to_string())
    }

    /// A service-account key with a throwaway RSA key, only to build a real
    /// `yup-oauth2` authenticator whose token endpoint is under test control.
    fn test_private_key_pem() -> String {
        use rsa::pkcs8::{EncodePrivateKey, LineEnding};
        let key = rsa::RsaPrivateKey::new(&mut rand::thread_rng(), 2048)
            .expect("the test key must generate");
        key.to_pkcs8_pem(LineEnding::LF)
            .expect("the test key must encode")
            .to_string()
    }

    async fn fake_authenticator(token_uri: &str) -> Authenticator<HttpsConnector<HttpConnector>> {
        let key = ServiceAccountKey {
            key_type: Some("service_account".to_string()),
            project_id: Some(PROJECT.to_string()),
            private_key_id: None,
            private_key: test_private_key_pem(),
            client_email: "tester@example.com".to_string(),
            client_id: None,
            auth_uri: None,
            token_uri: token_uri.to_string(),
            auth_provider_x509_cert_url: None,
            client_x509_cert_url: None,
        };
        ServiceAccountAuthenticator::builder(key)
            .build()
            .await
            .expect("the fake authenticator must build")
    }

    /// A minimal plain-HTTP token endpoint for the fake authenticator.
    async fn spawn_token_server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("the token server must bind");
        let addr = listener.local_addr().expect("the address must be known");
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buffer = [0u8; 4096];
                    let _ = socket.read(&mut buffer).await;
                    let body =
                        r#"{"access_token":"test-token","expires_in":3600,"token_type":"Bearer"}"#;
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        format!("http://{addr}/token")
    }

    #[test]
    fn raw_send_success_response_shape() {
        let body_text = r#"{"name":"projects/test/messages/1"}"#;
        let (status, Json(body)) = raw_send_success_response(body_text.to_string());

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, json!({ "status": "success", "response": body_text }));
    }

    #[test]
    fn raw_send_fcm_4xx_is_passed_through() {
        for code in [400u16, 403, 404, 429] {
            let (status, Json(body)) =
                raw_send_fcm_error_response(code, r#"{"error":{"status":"INVALID_ARGUMENT"}}"#);

            assert_eq!(status.as_u16(), code);
            assert_eq!(body["status"], "failed");
            assert_eq!(body["error"], "fcm_error");
            assert_eq!(body["response"]["error"]["status"], "INVALID_ARGUMENT");
        }

        // A non-JSON body is kept as a string, and a non-4xx status is 400.
        let (status, Json(body)) = raw_send_fcm_error_response(503, "upstream down");
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["response"], "upstream down");
    }

    #[tokio::test]
    async fn raw_send_token_failure_is_502() {
        let state = test_state(test_client()).await;
        state
            .project_map
            .write()
            .await
            .insert(APP.to_string(), PROJECT.to_string());
        // Nothing listens on port 1, so the token request fails.
        let auth = fake_authenticator("http://127.0.0.1:1/token").await;
        state.auth_map.write().await.insert(APP.to_string(), auth);

        let (status, body) = call(
            &send_router(state),
            send_request(json!({ "app": APP, "topic": "all", "title": "Hi" })),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(body, "Could not authenticate with Firebase");
    }

    #[tokio::test]
    async fn raw_send_network_failure_is_502() {
        // The token step succeeds against the local server; the FCM call itself
        // is pointed at a closed port.
        let client = Client::builder()
            .resolve("fcm.googleapis.com", SocketAddr::from(([127, 0, 0, 1], 1)))
            .build()
            .expect("the test client must build");
        let state = test_state(client).await;
        state
            .project_map
            .write()
            .await
            .insert(APP.to_string(), PROJECT.to_string());
        let token_uri = spawn_token_server().await;
        let auth = fake_authenticator(&token_uri).await;
        state.auth_map.write().await.insert(APP.to_string(), auth);

        let (status, body) = call(
            &send_router(state),
            send_request(json!({ "app": APP, "topic": "all", "title": "Hi" })),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(body, "Network error while calling Firebase");
    }
}

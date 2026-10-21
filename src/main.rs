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
mod send;
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
use config::store::{AppSnapshot, ConfigStore};
use credentials::{AuthMap, ProjectIdMap};
use datasource::registry::DataSourceRegistry;
use datasource::DataError;
use db::TemplateRateLimiter;
use devices::{DeviceError, DeviceStore, Platform, RoleDirectory};
use identity::jwt::SystemEnv;
use identity::registry::IdentityRegistry;
use ratelimit::RateLimiter;
use reqwest::Client;
use send::{FcmOutcome, FcmTransport, HttpFcm, SendError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::Row;
use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use template::{Rendered, ResolvedTarget};
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
    /// T1: the configuration store.
    config: Arc<ConfigStore>,
    /// T2: dashboard accounts and sessions.
    tool_auth: Arc<admin::ToolAuth>,
    /// T6: token ↔ uid registry.
    devices: Arc<DeviceStore>,
    /// T6: per-uid registration limit.
    devices_limiter: RateLimiter,
    /// T3: identity providers per app. The auth service owns its own handle;
    /// this field stays for the dashboard wiring.
    #[allow(dead_code)]
    identity: Arc<IdentityRegistry>,
    /// T4: data sources per app.
    datasource: Arc<DataSourceRegistry>,
    /// T8: the FCM transport, replaceable in tests.
    fcm: Arc<dyn FcmTransport>,
    /// T8: role member lookups for role-targeted templates.
    role_directory: Arc<RoleDirectory>,
    /// T8: per-uid template rate limit.
    template_rate: Arc<TemplateRateLimiter>,
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
#[serde(deny_unknown_fields)]
struct SendTemplateRequest {
    app: String,
    template: String,
    #[serde(default)]
    params: serde_json::Value,
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
    let fcm = Arc::new(HttpFcm::new(client.clone(), auth_map.clone()));

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
        fcm,
        role_directory: Arc::new(RoleDirectory::new()),
        template_rate: Arc::new(TemplateRateLimiter::new()),
    });

    // Serve static files from "dist", fallback to index.html for SPA routing
    let serve_dir = ServeDir::new("dist").not_found_service(ServeFile::new("dist/index.html"));

    let api_routes = Router::new()
        .route("/apps", get(list_apps))
        .route("/history", get(get_history))
        .route("/topics", get(get_topics).post(add_topic))
        .route("/send", post(send_notification))
        .route("/send/template", post(send_template))
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

    if !state.auth_map.read().await.contains_key(&payload.app) {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "Service account for this app is not loaded".to_string(),
        ));
    }

    let mut data = BTreeMap::new();
    if let Some(value) = &payload.data {
        if let Some(object) = value.as_object() {
            for (key, value) in object {
                let text = match value {
                    serde_json::Value::String(string) => string.clone(),
                    other => other.to_string(),
                };
                data.insert(key.clone(), text);
            }
        }
    }

    let destination = if let Some(topic) = &payload.topic {
        send::Destination::Topic(topic.clone())
    } else if let Some(token) = &payload.token {
        send::Destination::Token(token.clone())
    } else {
        send::Destination::Condition(payload.condition.clone().unwrap_or_default())
    };

    let fcm_payload = send::build_message(
        &payload.title,
        payload.body.as_deref(),
        payload.image.as_deref(),
        &data,
        payload.analytics_label.as_deref(),
        &destination,
    );

    match state.fcm.send(&payload.app, &project_id, fcm_payload).await {
        Ok(FcmOutcome::Accepted { body }) => {
            if let Some(topic) = &payload.topic {
                let _ = db::save_topic(&state.db, &payload.app, topic).await;
            }
            let _ = db::log_notification(
                &state.db,
                &payload.app,
                &target,
                &payload.title,
                "success",
                &body,
                &payload_str,
                &identity.uid,
                identity.email.as_deref(),
            )
            .await;
            Ok(raw_send_success_response(body))
        }
        Ok(FcmOutcome::Rejected {
            http_status, body, ..
        }) => {
            let _ = db::log_notification(
                &state.db,
                &payload.app,
                &target,
                &payload.title,
                "failed",
                &body,
                &payload_str,
                &identity.uid,
                identity.email.as_deref(),
            )
            .await;
            Ok(raw_send_fcm_error_response(http_status, &body))
        }
        Err(SendError::Auth) => Err((
            StatusCode::BAD_GATEWAY,
            "Could not authenticate with Firebase".to_string(),
        )),
        Err(SendError::Network) => Err((
            StatusCode::BAD_GATEWAY,
            "Network error while calling Firebase".to_string(),
        )),
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

/// A `uid`/`role` target still needs a syntactically valid destination for
/// `/send`'s content checks; the value is never sent.
const VALIDATION_PLACEHOLDER_TOKEN: &str = "00000000000000000000000000000000";

fn template_not_found() -> (StatusCode, String) {
    (StatusCode::NOT_FOUND, "Template not found".to_string())
}

fn template_error(error: template::RenderError) -> (StatusCode, String) {
    match error {
        template::RenderError::Forbidden => (StatusCode::FORBIDDEN, "Forbidden".to_string()),
        template::RenderError::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
    }
}

fn data_source_unavailable() -> (StatusCode, String) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        "the data source is unavailable".to_string(),
    )
}

fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

/// `POST /api/v1/send/template` (T8).
///
/// The numbered steps match the plan and are kept in this order on purpose:
/// authentication comes before template disclosure, and a disallowed sender
/// learns nothing about parameters or templates.
async fn send_template(
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SendTemplateRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, String)> {
    // 1. A known app, then the shared IP limit.
    ensure_known_app(&state, &payload.app).await?;
    topics::check_ip_limit(&state, &headers, peer)?;

    // 2. The service key, or a verified identity and its resolved role.
    let (sender, sender_email, is_service) = if state.auth.verify_service_key(&headers) {
        (
            template::Sender {
                uid: "service".to_string(),
                name: None,
                role: None,
            },
            None,
            true,
        )
    } else {
        let identity = state
            .auth
            .authenticate_identity(&headers, &payload.app)
            .await
            .map_err(|error| auth_error_with_limit(&state, &headers, peer, error))?;
        let role = state
            .auth
            .resolve_sender_role(&payload.app, &identity)
            .await
            .map_err(|error| auth_error_with_limit(&state, &headers, peer, error))?;
        (
            template::Sender {
                uid: identity.uid,
                name: identity.name,
                role,
            },
            identity.email,
            false,
        )
    };

    // 3. A missing or disabled template is a uniform 404.
    let snapshot = state
        .config
        .get(&payload.app)
        .ok_or_else(template_not_found)?;
    let definition = snapshot
        .templates
        .get(&payload.template)
        .filter(|definition| definition.enabled)
        .ok_or_else(template_not_found)?;

    // 4. A role must be listed literally, then the per-uid hourly limit.
    if !is_service {
        let listed = sender
            .role
            .as_deref()
            .is_some_and(|role| definition.senders.iter().any(|sender| sender == role));
        if !listed {
            return Err((StatusCode::FORBIDDEN, "Forbidden".to_string()));
        }
        let limit = definition
            .rate
            .as_ref()
            .map(|rate| rate.per_uid_per_hour)
            .unwrap_or(120);
        if !state
            .template_rate
            .allow((&payload.app, &payload.template, &sender.uid), limit)
        {
            return Err(topics::too_many_requests());
        }
    }

    // 5. Client parameters.
    let params = template::validate_params(definition, &payload.params).map_err(template_error)?;

    // 6. The context document, when the template asks for one.
    let doc = match template::context_path(definition, &params, &sender).map_err(template_error)? {
        Some(path) => {
            let source = state
                .datasource
                .source_for(&payload.app)
                .await
                .map_err(|_| data_source_unavailable())?;
            match source.get_doc(&path).await {
                Ok(doc) => doc,
                Err(DataError::Unavailable(_)) => return Err(data_source_unavailable()),
                Err(DataError::InvalidPath(message)) => {
                    return Err((StatusCode::BAD_REQUEST, message));
                }
            }
        }
        None => None,
    };

    // 7. Render the final notification.
    let rendered = template::render(definition, &params, &sender, doc.as_ref(), None)
        .map_err(template_error)?;

    // 8. The same content limits as `/send`, applied to the rendered output.
    let data_value = if rendered.data.is_empty() {
        None
    } else {
        Some(serde_json::Value::Object(
            rendered
                .data
                .iter()
                .map(|(key, value)| (key.clone(), serde_json::Value::String(value.clone())))
                .collect(),
        ))
    };
    let (topic, token, condition) = match &rendered.target {
        ResolvedTarget::Topic(topic) => (Some(topic.as_str()), None, None),
        ResolvedTarget::Condition(condition) => (None, None, Some(condition.as_str())),
        ResolvedTarget::Uid(_) | ResolvedTarget::Role(_) => {
            (None, Some(VALIDATION_PLACEHOLDER_TOKEN), None)
        }
    };
    validation::validate_send(
        &payload.app,
        topic,
        token,
        condition,
        &rendered.title,
        Some(&rendered.body),
        rendered.image.as_deref(),
        data_value.as_ref(),
    )
    .map_err(|error| (StatusCode::BAD_REQUEST, error))?;

    // 9. A claimed dedupe key never sends twice.
    if let Some(key) = &rendered.dedupe_key {
        let claimed = db::claim_dedupe(
            &state.db,
            &payload.app,
            &payload.template,
            key,
            rendered.dedupe_seconds.unwrap_or(0),
            unix_seconds(),
        )
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not record the dedupe key".to_string(),
            )
        })?;
        if !claimed {
            return Ok((
                StatusCode::OK,
                Json(json!({ "status": "duplicate", "delivered": 0, "failed": 0 })),
            ));
        }
    }

    // 10-11. Recipients and delivery. Every error exit releases the claimed
    // key, so a request that delivered nothing can be retried instead of being
    // swallowed as a duplicate.
    let delivery = match deliver(&state, &payload.app, &snapshot, &rendered).await {
        Ok(delivery) => delivery,
        Err(error) => {
            release_dedupe_key(&state, &payload.app, &payload.template, &rendered).await;
            return Err(error);
        }
    };

    for token in &delivery.dead_tokens {
        let _ = state.devices.remove_dead_token(&payload.app, token).await;
    }

    if delivery.delivered > 0 {
        let _ = db::touch_template_used(&state.db, &payload.app, &payload.template).await;
    } else {
        // Nothing was delivered (all failed, or no devices): free the key.
        release_dedupe_key(&state, &payload.app, &payload.template, &rendered).await;
    }

    if delivery.empty_audience {
        return Ok((
            StatusCode::OK,
            Json(json!({ "status": "no_devices", "delivered": 0, "failed": 0 })),
        ));
    }

    let delivered = delivery.delivered;
    let failed = delivery.failed;
    let target_label = delivery.target;

    // 12. One history row per request: rendered content only, never tokens or
    // raw parameters.
    let history_payload = json!({
        "title": rendered.title,
        "body": rendered.body,
        "data": rendered.data,
    })
    .to_string();
    let history_response = json!({ "delivered": delivered, "failed": failed }).to_string();
    let _ = db::log_notification_ext(
        &state.db,
        &payload.app,
        &target_label,
        &rendered.title,
        if delivered > 0 { "success" } else { "failed" },
        &history_response,
        &history_payload,
        &sender.uid,
        sender_email.as_deref(),
        "template",
        Some(&payload.template),
    )
    .await;

    // 13. The result.
    if delivered > 0 {
        Ok((
            StatusCode::OK,
            Json(json!({ "status": "success", "delivered": delivered, "failed": failed })),
        ))
    } else {
        Ok((
            StatusCode::BAD_GATEWAY,
            Json(json!({ "status": "failed", "delivered": 0, "failed": failed })),
        ))
    }
}

/// What steps 10-11 resolved and delivered.
struct Delivery {
    target: String,
    delivered: usize,
    failed: usize,
    dead_tokens: Vec<String>,
    /// A `uid`/`role` target with no registered devices is not an error.
    empty_audience: bool,
}

/// Frees a claimed dedupe key. A failure is ignored on purpose: it must not
/// hide the original error or change the response.
async fn release_dedupe_key(state: &AppState, app: &str, template: &str, rendered: &Rendered) {
    if let Some(key) = &rendered.dedupe_key {
        let _ = db::release_dedupe(&state.db, app, template, key).await;
    }
}

/// Steps 10-11 of the template route: resolve the recipients and send.
async fn deliver(
    state: &AppState,
    app: &str,
    snapshot: &AppSnapshot,
    rendered: &Rendered,
) -> Result<Delivery, (StatusCode, String)> {
    let mut tokens = Vec::new();
    let target = match &rendered.target {
        ResolvedTarget::Uid(uid) => {
            tokens = state
                .devices
                .tokens_for_uid(app, uid)
                .await
                .map_err(device_error)?;
            format!("uid:{uid}")
        }
        ResolvedTarget::Role(role) => {
            let role_members = snapshot
                .config
                .identity
                .role_members
                .as_ref()
                .ok_or_else(|| {
                    (
                        StatusCode::BAD_REQUEST,
                        "role targets require identity.role_members".to_string(),
                    )
                })?;
            let source = state
                .datasource
                .source_for(app)
                .await
                .map_err(|_| data_source_unavailable())?;
            tokens = state
                .devices
                .tokens_for_role(
                    app,
                    snapshot.version,
                    Some(role_members),
                    role,
                    &state.role_directory,
                    source.as_ref(),
                )
                .await
                .map_err(device_error)?;
            format!("role:{role}")
        }
        ResolvedTarget::Topic(topic) => format!("topic:{topic}"),
        ResolvedTarget::Condition(_) => "condition".to_string(),
    };

    let token_targeted = matches!(
        rendered.target,
        ResolvedTarget::Uid(_) | ResolvedTarget::Role(_)
    );
    if token_targeted && tokens.is_empty() {
        return Ok(Delivery {
            target,
            delivered: 0,
            failed: 0,
            dead_tokens: Vec::new(),
            empty_audience: true,
        });
    }

    let project_id = {
        let projects = state.project_map.read().await;
        projects.get(app).cloned()
    }
    .ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Service account for this app is not loaded".to_string(),
        )
    })?;

    let single_destination = match &rendered.target {
        ResolvedTarget::Topic(topic) => Some(send::Destination::Topic(topic.clone())),
        ResolvedTarget::Condition(condition) => {
            Some(send::Destination::Condition(condition.clone()))
        }
        ResolvedTarget::Uid(_) | ResolvedTarget::Role(_) => None,
    };

    let (delivered, failed, dead_tokens) = if let Some(destination) = single_destination {
        let message = send::build_message(
            &rendered.title,
            Some(&rendered.body),
            rendered.image.as_deref(),
            &rendered.data,
            None,
            &destination,
        );
        match state.fcm.send(app, &project_id, message).await {
            Ok(FcmOutcome::Accepted { .. }) => (1usize, 0usize, Vec::new()),
            Ok(FcmOutcome::Rejected { .. }) | Err(_) => (0usize, 1usize, Vec::new()),
        }
    } else {
        let delivery = send::send_to_tokens(
            &state.fcm,
            app,
            &project_id,
            &tokens,
            |token| {
                send::build_message(
                    &rendered.title,
                    Some(&rendered.body),
                    rendered.image.as_deref(),
                    &rendered.data,
                    None,
                    &send::Destination::Token(token.to_string()),
                )
            },
            send::DEFAULT_CONCURRENCY,
        )
        .await;
        (delivery.delivered, delivery.failed, delivery.dead_tokens)
    };

    Ok(Delivery {
        target,
        delivered,
        failed,
        dead_tokens,
        empty_audience: false,
    })
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
    use crate::send::test_support::FakeTransport;
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
    const LEGACY_KEY: &str = "test-legacy-key";
    const PROJECT: &str = "test-project";
    const JWT_SECRET_ENV: &str = "T8_JWT_SECRET";
    const JWT_SECRET: &str = "test-jwt-secret-test-jwt-secret-0123456789";
    const ISSUER: &str = "https://issuer.test";
    const AUDIENCE: &str = "test-audience";
    const RESOLVER_SECRET_ENV: &str = "T8_RESOLVER_SECRET";
    const RESOLVER_SECRET: &str = "test-resolver-secret-0123456789abcdef";

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
        fn get(&self, name: &str) -> Option<String> {
            match name {
                JWT_SECRET_ENV => Some(JWT_SECRET.to_string()),
                RESOLVER_SECRET_ENV => Some(RESOLVER_SECRET.to_string()),
                _ => None,
            }
        }
    }

    async fn test_state(client: Client) -> Arc<AppState> {
        test_state_with_fcm(client, |client, auth_map| {
            Arc::new(HttpFcm::new(client.clone(), auth_map.clone()))
        })
        .await
    }

    async fn test_state_with_fcm(
        client: Client,
        make_fcm: impl FnOnce(&Client, &AuthMap) -> Arc<dyn FcmTransport>,
    ) -> Arc<AppState> {
        build_state(client, make_fcm).await.0
    }

    struct Harness {
        state: Arc<AppState>,
        fcm: Arc<FakeTransport>,
        config: Arc<ConfigStore>,
        pool: sqlx::SqlitePool,
    }

    async fn harness(fcm: Arc<FakeTransport>) -> Harness {
        let state = test_state_with_fcm(test_client(), {
            let fcm = fcm.clone();
            move |_, _| fcm.clone() as Arc<dyn FcmTransport>
        })
        .await;
        state
            .project_map
            .write()
            .await
            .insert(APP.to_string(), PROJECT.to_string());
        let config = state.config.clone();
        let pool = state.db.clone();
        Harness {
            state,
            fcm,
            config,
            pool,
        }
    }

    async fn build_state(
        client: Client,
        make_fcm: impl FnOnce(&Client, &AuthMap) -> Arc<dyn FcmTransport>,
    ) -> (
        Arc<AppState>,
        Arc<ConfigStore>,
        sqlx::SqlitePool,
        ProjectIdMap,
        AuthMap,
    ) {
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
        let fcm = make_fcm(&client, &auth_map);
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
                legacy_api_key: Some(LEGACY_KEY.to_string()),
                legacy_enabled: true,
            },
        );
        let tool_auth = Arc::new(ToolAuth {
            pool: pool.clone(),
            cookie: CookieConfig::with_hours(true, 12),
            login_limiter: RateLimiter::new(5, Duration::from_secs(900)),
            dummy_hash: "unused in send tests".to_string(),
            allowed_origins: Vec::new(),
        });

        let state = Arc::new(AppState {
            db: pool.clone(),
            auth_map: auth_map.clone(),
            project_map: project_map.clone(),
            client,
            auth,
            trust_proxy: false,
            topics_limiter: RateLimiter::new(30, Duration::from_secs(60)),
            auth_fail_limiter: RateLimiter::new(10, Duration::from_secs(60)),
            config: config.clone(),
            tool_auth,
            devices: Arc::new(DeviceStore::new(pool.clone())),
            devices_limiter: RateLimiter::new(20, Duration::from_secs(60)),
            identity,
            datasource,
            fcm,
            role_directory: Arc::new(RoleDirectory::new()),
            template_rate: Arc::new(TemplateRateLimiter::new()),
        });

        (state, config, pool, project_map, auth_map)
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

    // ---------- template route ----------

    fn jwt(uid: &str, role: &str) -> String {
        let now = jsonwebtoken::get_current_timestamp();
        jsonwebtoken::encode(
            &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
            &json!({
                "sub": uid,
                "aud": AUDIENCE,
                "iss": ISSUER,
                "exp": now + 3600,
                "iat": now - 10,
                "role": role,
            }),
            &jsonwebtoken::EncodingKey::from_secret(JWT_SECRET.as_bytes()),
        )
        .expect("the test token must encode")
    }

    fn jwt_identity(role_members: bool) -> Value {
        let mut config = json!({
            "provider": {
                "type": "jwt",
                "algorithms": ["HS256"],
                "hs256_secret_env": JWT_SECRET_ENV,
                "issuer": ISSUER,
                "audience": AUDIENCE,
                "uid_claim": "sub",
                "max_age_seconds": 3600
            },
            "role": { "source": "claim", "claim": "role" },
            "admin_roles": ["admin"]
        });
        if role_members {
            config["role_members"] = json!({
                "collection": "users",
                "field": "role",
                "cache_seconds": 0,
                "max": 50
            });
        }
        config
    }

    async fn save_section(harness: &Harness, section: &str, value: Value) {
        let version = harness.config.get(APP).expect("the app must exist").version;
        harness
            .config
            .save_section(APP, section, value, "tester", version)
            .await
            .expect("the section must be saved");
    }

    async fn save_template(harness: &Harness, key: &str, definition: Value) {
        harness
            .config
            .save_template(APP, key, definition, "tester", None)
            .await
            .expect("the template must be saved");
    }

    async fn use_http_source(harness: &Harness, base_url: &str) {
        save_section(
            harness,
            "data_source",
            json!({
                "type": "http",
                "base_url": base_url,
                "secret_env": RESOLVER_SECRET_ENV,
                "timeout_ms": 2000
            }),
        )
        .await;
    }

    fn device_token(uid: &str) -> String {
        format!("device-{uid}-000000000000000000000000")
    }

    async fn register_device(harness: &Harness, uid: &str) {
        harness
            .state
            .devices
            .register(APP, uid, &device_token(uid), Platform::Android, 20)
            .await
            .expect("the device must register");
    }

    fn uid_template(senders: &[&str], rate: Option<u32>) -> Value {
        let mut template = json!({
            "enabled": true,
            "senders": senders,
            "to": { "uid": "{sender.uid}" },
            "title": "Hello",
            "body": "Body"
        });
        if let Some(limit) = rate {
            template["rate"] = json!({ "per_uid_per_hour": limit });
        }
        template
    }

    fn order_new_template() -> Value {
        json!({
            "enabled": true,
            "senders": ["user", "company", "technical"],
            "params": {
                "order_id": {
                    "type": "pattern",
                    "pattern": "^[A-Za-z0-9_-]{1,64}$",
                    "max": 64
                }
            },
            "context": {
                "doc": "store_orders/{p.order_id}",
                "require": { "buyer_id": "{sender.uid}" }
            },
            "to": { "uid": "{doc.seller_id}" },
            "title": "طلب جديد #{doc.order_number}",
            "body": "طلب جديد من {doc.buyer_name} بقيمة {doc.total|int} {doc.currency}",
            "data": { "type": "order", "target": "/orders/{p.order_id}" },
            "rate": { "per_uid_per_hour": 120 },
            "dedupe": {
                "key": "order-new:{p.order_id}:{sender.uid}",
                "seconds": 86400
            }
        })
    }

    fn order_status_template() -> Value {
        json!({
            "enabled": true,
            "senders": ["user", "company", "technical"],
            "params": {
                "order_id": {
                    "type": "pattern",
                    "pattern": "^[A-Za-z0-9_-]{1,64}$",
                    "max": 64
                }
            },
            "context": {
                "doc": "store_orders/{p.order_id}",
                "require": { "seller_id": "{sender.uid}" }
            },
            "to": { "uid": "{doc.buyer_id}" },
            "title": "حالة الطلب #{doc.order_number}",
            "body": "تم تحديث حالة طلبك.",
            "rate": { "per_uid_per_hour": 120 }
        })
    }

    fn order_doc(buyer: &str, seller: &str) -> Value {
        json!({
            "buyer_id": buyer,
            "seller_id": seller,
            "buyer_name": "Buyer",
            "order_number": "42",
            "total": 150_000.0,
            "currency": "IQD"
        })
    }

    fn template_router(state: Arc<AppState>) -> Router {
        Router::new()
            .route("/send/template", post(send_template))
            .with_state(state)
    }

    fn template_request(body: Value, token: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder()
            .method(Method::POST)
            .uri("/send/template")
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(token) = token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let mut request = builder
            .body(Body::from(body.to_string()))
            .expect("the request must build");
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 41_000))));
        request
    }

    async fn call_json(router: &Router, request: Request<Body>) -> (StatusCode, Value) {
        let (status, body) = call(router, request).await;
        (status, serde_json::from_str(&body).unwrap_or(Value::Null))
    }

    /// A tiny resolver backend: `GET /doc` and `GET /members`. The request
    /// signature is irrelevant for these tests.
    async fn spawn_source(doc: Option<Value>, members: Vec<String>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("the source server must bind");
        let addr = listener.local_addr().expect("the address must be known");
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let doc = doc.clone();
                let members = members.clone();
                tokio::spawn(async move {
                    let mut buffer = [0u8; 8192];
                    let read = socket.read(&mut buffer).await.unwrap_or(0);
                    let request = String::from_utf8_lossy(&buffer[..read]).to_string();
                    let path = request
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().nth(1))
                        .unwrap_or_default();
                    let (status, body) = if path.starts_with("/doc") {
                        match &doc {
                            Some(doc) => ("200 OK", doc.to_string()),
                            None => ("404 Not Found", "{}".to_string()),
                        }
                    } else if path.starts_with("/members") {
                        ("200 OK", json!({ "uids": members }).to_string())
                    } else {
                        ("404 Not Found", "{}".to_string())
                    };
                    let response = format!(
                        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn template_unknown_app_is_404() {
        let harness = harness(FakeTransport::accepting()).await;
        let body = json!({ "app": "ghost", "template": "order_new" });

        let (status, text) = call(
            &template_router(harness.state),
            template_request(body, Some(&jwt("uid-1", "user"))),
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(text, "App 'ghost' not found in credentials folder.");
    }

    #[tokio::test]
    async fn template_without_token_is_401() {
        let harness = harness(FakeTransport::accepting()).await;
        save_template(&harness, "hello", uid_template(&["user"], Some(120))).await;

        let (status, _) = call(
            &template_router(harness.state),
            template_request(json!({ "app": APP, "template": "hello" }), None),
        )
        .await;

        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn template_unknown_or_disabled_is_404_same_message() {
        let harness = harness(FakeTransport::accepting()).await;
        save_section(&harness, "identity", jwt_identity(false)).await;
        save_template(&harness, "known", uid_template(&["user"], Some(120))).await;
        let mut disabled = uid_template(&["user"], Some(120));
        disabled["enabled"] = json!(false);
        save_template(&harness, "disabled", disabled).await;

        let token = jwt("uid-1", "user");
        let router = template_router(harness.state);

        let (missing_status, missing) = call(
            &router,
            template_request(json!({ "app": APP, "template": "nope" }), Some(&token)),
        )
        .await;
        let (disabled_status, disabled_body) = call(
            &router,
            template_request(json!({ "app": APP, "template": "disabled" }), Some(&token)),
        )
        .await;

        assert_eq!(missing_status, StatusCode::NOT_FOUND);
        assert_eq!(disabled_status, StatusCode::NOT_FOUND);
        assert_eq!(missing, "Template not found");
        assert_eq!(disabled_body, missing);
    }

    #[tokio::test]
    async fn template_unauthenticated_cannot_probe_template_names() {
        let harness = harness(FakeTransport::accepting()).await;
        save_template(&harness, "known", uid_template(&["user"], Some(120))).await;
        let router = template_router(harness.state);

        let (known_status, known_body) = call(
            &router,
            template_request(json!({ "app": APP, "template": "known" }), None),
        )
        .await;
        let (missing_status, missing_body) = call(
            &router,
            template_request(json!({ "app": APP, "template": "missing" }), None),
        )
        .await;

        assert_eq!(known_status, StatusCode::UNAUTHORIZED);
        assert_eq!(missing_status, StatusCode::UNAUTHORIZED);
        assert_eq!(known_body, missing_body);
    }

    #[tokio::test]
    async fn template_role_not_in_senders_is_403() {
        let harness = harness(FakeTransport::accepting()).await;
        save_section(&harness, "identity", jwt_identity(false)).await;
        save_template(
            &harness,
            "admin_note",
            json!({
                "enabled": true,
                "senders": ["admin"],
                "params": { "note": { "type": "text", "max": 20 } },
                "to": { "uid": "{sender.uid}" },
                "title": "Admin {p.note}",
                "body": "Body"
            }),
        )
        .await;

        // The sender role check must win over the missing parameter.
        let (status, _) = call(
            &template_router(harness.state),
            template_request(
                json!({ "app": APP, "template": "admin_note" }),
                Some(&jwt("uid-1", "user")),
            ),
        )
        .await;

        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn template_admin_role_must_be_listed_explicitly() {
        let harness = harness(FakeTransport::accepting()).await;
        save_section(&harness, "identity", jwt_identity(false)).await;
        save_template(&harness, "members_only", uid_template(&["user"], Some(120))).await;

        let (status, _) = call(
            &template_router(harness.state),
            template_request(
                json!({ "app": APP, "template": "members_only" }),
                Some(&jwt("admin-1", "admin")),
            ),
        )
        .await;

        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn template_rate_limit_429() {
        let harness = harness(FakeTransport::accepting()).await;
        save_section(&harness, "identity", jwt_identity(false)).await;
        save_template(&harness, "slow", uid_template(&["user"], Some(1))).await;
        let router = template_router(harness.state);
        let token = jwt("uid-1", "user");
        let body = json!({ "app": APP, "template": "slow" });

        let (first, _) = call(&router, template_request(body.clone(), Some(&token))).await;
        let (second, _) = call(&router, template_request(body, Some(&token))).await;

        assert_eq!(first, StatusCode::OK);
        assert_eq!(second, StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn template_bad_param_is_400() {
        let harness = harness(FakeTransport::accepting()).await;
        save_section(&harness, "identity", jwt_identity(false)).await;
        use_http_source(&harness, "http://127.0.0.1:1").await;
        save_template(&harness, "order_new", order_new_template()).await;

        let (status, text) = call(
            &template_router(harness.state),
            template_request(
                json!({
                    "app": APP,
                    "template": "order_new",
                    "params": { "order_id": "bad id!" }
                }),
                Some(&jwt("buyer-1", "user")),
            ),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(text.contains("order_id"), "unexpected body: {text}");
    }

    #[tokio::test]
    async fn template_context_unavailable_is_503() {
        let harness = harness(FakeTransport::accepting()).await;
        save_section(&harness, "identity", jwt_identity(false)).await;
        use_http_source(&harness, "http://127.0.0.1:1").await;
        save_template(&harness, "order_new", order_new_template()).await;

        let (status, text) = call(
            &template_router(harness.state),
            template_request(
                json!({
                    "app": APP,
                    "template": "order_new",
                    "params": { "order_id": "o1" }
                }),
                Some(&jwt("buyer-1", "user")),
            ),
        )
        .await;

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(text, "the data source is unavailable");
    }

    #[tokio::test]
    async fn template_require_mismatch_is_403() {
        let harness = harness(FakeTransport::accepting()).await;
        let base = spawn_source(Some(order_doc("someone-else", "seller-1")), Vec::new()).await;
        save_section(&harness, "identity", jwt_identity(false)).await;
        use_http_source(&harness, &base).await;
        save_template(&harness, "order_new", order_new_template()).await;

        let (status, text) = call(
            &template_router(harness.state),
            template_request(
                json!({
                    "app": APP,
                    "template": "order_new",
                    "params": { "order_id": "o1" }
                }),
                Some(&jwt("buyer-1", "user")),
            ),
        )
        .await;

        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(text, "Forbidden");
    }

    #[tokio::test]
    async fn order_new_from_non_buyer_is_403() {
        let harness = harness(FakeTransport::accepting()).await;
        let base = spawn_source(Some(order_doc("buyer-2", "seller-1")), Vec::new()).await;
        save_section(&harness, "identity", jwt_identity(false)).await;
        use_http_source(&harness, &base).await;
        save_template(&harness, "order_new", order_new_template()).await;

        let (status, _) = call(
            &template_router(harness.state),
            template_request(
                json!({
                    "app": APP,
                    "template": "order_new",
                    "params": { "order_id": "o1" }
                }),
                Some(&jwt("buyer-1", "user")),
            ),
        )
        .await;

        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn order_status_from_buyer_is_403() {
        let harness = harness(FakeTransport::accepting()).await;
        let base = spawn_source(Some(order_doc("buyer-1", "seller-1")), Vec::new()).await;
        save_section(&harness, "identity", jwt_identity(false)).await;
        use_http_source(&harness, &base).await;
        save_template(&harness, "order_status", order_status_template()).await;

        let (status, _) = call(
            &template_router(harness.state),
            template_request(
                json!({
                    "app": APP,
                    "template": "order_status",
                    "params": { "order_id": "o1" }
                }),
                Some(&jwt("buyer-1", "user")),
            ),
        )
        .await;

        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn order_new_delivers_to_seller_devices() {
        let harness = harness(FakeTransport::accepting()).await;
        let base = spawn_source(Some(order_doc("buyer-1", "seller-1")), Vec::new()).await;
        save_section(&harness, "identity", jwt_identity(false)).await;
        use_http_source(&harness, &base).await;
        save_template(&harness, "order_new", order_new_template()).await;
        register_device(&harness, "seller-1").await;

        let (status, body) = call_json(
            &template_router(harness.state),
            template_request(
                json!({
                    "app": APP,
                    "template": "order_new",
                    "params": { "order_id": "o1" }
                }),
                Some(&jwt("buyer-1", "user")),
            ),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "success");
        assert_eq!(body["delivered"], 1);
        let messages = harness.fcm.messages();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["message"]["token"], device_token("seller-1"));
        assert!(messages[0]["message"]["notification"]["title"]
            .as_str()
            .is_some_and(|title| title.contains("42")));
    }

    #[tokio::test]
    async fn template_to_role_uses_role_members() {
        let harness = harness(FakeTransport::accepting()).await;
        let base = spawn_source(None, vec!["seller-1".to_string()]).await;
        save_section(&harness, "identity", jwt_identity(true)).await;
        use_http_source(&harness, &base).await;
        save_template(
            &harness,
            "role_target",
            json!({
                "enabled": true,
                "senders": ["admin"],
                "to": { "role": "admin" },
                "title": "Role",
                "body": "Body"
            }),
        )
        .await;
        register_device(&harness, "seller-1").await;

        let (status, body) = call_json(
            &template_router(harness.state),
            template_request(
                json!({ "app": APP, "template": "role_target" }),
                Some(&jwt("admin-1", "admin")),
            ),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["delivered"], 1);
        assert_eq!(harness.fcm.message_count(), 1);
    }

    #[tokio::test]
    async fn template_to_role_without_role_members_is_400() {
        let harness = harness(FakeTransport::accepting()).await;
        save_section(&harness, "identity", jwt_identity(false)).await;
        save_template(
            &harness,
            "role_target",
            json!({
                "enabled": true,
                "senders": ["admin"],
                "to": { "role": "admin" },
                "title": "Role",
                "body": "Body"
            }),
        )
        .await;

        let (status, text) = call(
            &template_router(harness.state),
            template_request(
                json!({ "app": APP, "template": "role_target" }),
                Some(&jwt("admin-1", "admin")),
            ),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(text, "role targets require identity.role_members");
    }

    #[tokio::test]
    async fn template_topic_target_sends_single_message() {
        let harness = harness(FakeTransport::accepting()).await;
        save_section(&harness, "identity", jwt_identity(false)).await;
        save_template(
            &harness,
            "news",
            json!({
                "enabled": true,
                "senders": ["admin"],
                "to": { "topic": "news" },
                "title": "News",
                "body": "Body"
            }),
        )
        .await;

        let (status, body) = call_json(
            &template_router(harness.state),
            template_request(
                json!({ "app": APP, "template": "news" }),
                Some(&jwt("admin-1", "admin")),
            ),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["delivered"], 1);
        let messages = harness.fcm.messages();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["message"]["topic"], "news");
        assert!(messages[0]["message"].get("token").is_none());

        // Unlike `/send`, the template route does not write the topics table.
        let topics: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM topics")
            .fetch_one(&harness.pool)
            .await
            .expect("the topics table must be readable");
        assert_eq!(topics, 0);
    }

    #[tokio::test]
    async fn template_duplicate_returns_duplicate_without_sending() {
        let harness = harness(FakeTransport::accepting()).await;
        let base = spawn_source(Some(order_doc("buyer-1", "seller-1")), Vec::new()).await;
        save_section(&harness, "identity", jwt_identity(false)).await;
        use_http_source(&harness, &base).await;
        save_template(&harness, "order_new", order_new_template()).await;
        register_device(&harness, "seller-1").await;
        let router = template_router(harness.state);
        let token = jwt("buyer-1", "user");
        let request = json!({
            "app": APP,
            "template": "order_new",
            "params": { "order_id": "o1" }
        });

        let (first_status, first) =
            call_json(&router, template_request(request.clone(), Some(&token))).await;
        let (second_status, second) =
            call_json(&router, template_request(request, Some(&token))).await;

        assert_eq!(first_status, StatusCode::OK);
        assert_eq!(first["status"], "success");
        assert_eq!(second_status, StatusCode::OK);
        assert_eq!(
            second,
            json!({ "status": "duplicate", "delivered": 0, "failed": 0 })
        );
        assert_eq!(harness.fcm.message_count(), 1);
    }

    #[tokio::test]
    async fn template_dedupe_released_when_nothing_delivered() {
        let harness = harness(FakeTransport::accepting()).await;
        let base = spawn_source(Some(order_doc("buyer-1", "seller-1")), Vec::new()).await;
        save_section(&harness, "identity", jwt_identity(false)).await;
        use_http_source(&harness, &base).await;
        save_template(&harness, "order_new", order_new_template()).await;
        let router = template_router(harness.state.clone());
        let token = jwt("buyer-1", "user");
        let request = json!({
            "app": APP,
            "template": "order_new",
            "params": { "order_id": "o1" }
        });

        let (first_status, first) =
            call_json(&router, template_request(request.clone(), Some(&token))).await;
        assert_eq!(first_status, StatusCode::OK);
        assert_eq!(first["status"], "no_devices");

        register_device(&harness, "seller-1").await;
        let (second_status, second) =
            call_json(&router, template_request(request, Some(&token))).await;

        assert_eq!(second_status, StatusCode::OK);
        assert_eq!(second["status"], "success");
        assert_eq!(harness.fcm.message_count(), 1);
    }

    #[tokio::test]
    async fn template_no_devices_returns_zero() {
        let harness = harness(FakeTransport::accepting()).await;
        save_section(&harness, "identity", jwt_identity(false)).await;
        save_template(&harness, "hello", uid_template(&["user"], Some(120))).await;

        let (status, body) = call_json(
            &template_router(harness.state),
            template_request(
                json!({ "app": APP, "template": "hello" }),
                Some(&jwt("uid-1", "user")),
            ),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body,
            json!({ "status": "no_devices", "delivered": 0, "failed": 0 })
        );
        assert_eq!(harness.fcm.message_count(), 0);
    }

    #[tokio::test]
    async fn template_dead_token_is_removed() {
        let harness = harness(FakeTransport::new(|_, _| {
            Ok(FcmOutcome::Rejected {
                http_status: 404,
                body: r#"{"error":{"details":[{"errorCode":"UNREGISTERED"}]}}"#.to_string(),
                dead_token: true,
            })
        }))
        .await;
        save_section(&harness, "identity", jwt_identity(false)).await;
        save_template(&harness, "hello", uid_template(&["user"], Some(120))).await;
        register_device(&harness, "uid-1").await;

        let (status, body) = call_json(
            &template_router(harness.state.clone()),
            template_request(
                json!({ "app": APP, "template": "hello" }),
                Some(&jwt("uid-1", "user")),
            ),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(body["status"], "failed");
        assert_eq!(body["failed"], 1);
        assert!(harness
            .state
            .devices
            .tokens_for_uid(APP, "uid-1")
            .await
            .expect("the tokens must be readable")
            .is_empty());
    }

    #[tokio::test]
    async fn template_all_failed_is_502() {
        let harness = harness(FakeTransport::new(|_, _| {
            Ok(FcmOutcome::Rejected {
                http_status: 400,
                body: "{}".to_string(),
                dead_token: false,
            })
        }))
        .await;
        save_section(&harness, "identity", jwt_identity(false)).await;
        save_template(&harness, "hello", uid_template(&["user"], Some(120))).await;
        register_device(&harness, "uid-1").await;

        let (status, body) = call_json(
            &template_router(harness.state.clone()),
            template_request(
                json!({ "app": APP, "template": "hello" }),
                Some(&jwt("uid-1", "user")),
            ),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(
            body,
            json!({ "status": "failed", "delivered": 0, "failed": 1 })
        );
        assert_eq!(
            harness
                .state
                .devices
                .tokens_for_uid(APP, "uid-1")
                .await
                .expect("the tokens must be readable")
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn template_history_row_has_kind_ref_and_no_tokens() {
        let harness = harness(FakeTransport::accepting()).await;
        save_section(&harness, "identity", jwt_identity(false)).await;
        save_template(&harness, "hello", uid_template(&["user"], Some(120))).await;
        register_device(&harness, "uid-1").await;

        let (status, _) = call_json(
            &template_router(harness.state.clone()),
            template_request(
                json!({ "app": APP, "template": "hello" }),
                Some(&jwt("uid-1", "user")),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let row: (String, String, String, String, String) = sqlx::query_as(
            "SELECT kind, \"ref\", target, payload, response FROM notifications_history",
        )
        .fetch_one(&harness.pool)
        .await
        .expect("the history row must exist");

        assert_eq!(row.0, "template");
        assert_eq!(row.1, "hello");
        assert_eq!(row.2, "uid:uid-1");
        assert!(!row.3.contains(&device_token("uid-1")));
        assert!(row.3.contains("title"));
        assert_eq!(row.4, r#"{"delivered":1,"failed":0}"#);
    }

    #[tokio::test]
    async fn template_updates_last_used_at() {
        let harness = harness(FakeTransport::accepting()).await;
        save_section(&harness, "identity", jwt_identity(false)).await;
        save_template(&harness, "hello", uid_template(&["user"], Some(120))).await;
        register_device(&harness, "uid-1").await;

        let before: (Option<String>, i64, String) = sqlx::query_as(
            "SELECT last_used_at, version, definition FROM templates WHERE app = ? AND key = ?",
        )
        .bind(APP)
        .bind("hello")
        .fetch_one(&harness.pool)
        .await
        .expect("the template row must exist");
        assert_eq!(before.0, None);

        let (status, _) = call_json(
            &template_router(harness.state.clone()),
            template_request(
                json!({ "app": APP, "template": "hello" }),
                Some(&jwt("uid-1", "user")),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let after: (Option<String>, i64, String) = sqlx::query_as(
            "SELECT last_used_at, version, definition FROM templates WHERE app = ? AND key = ?",
        )
        .bind(APP)
        .bind("hello")
        .fetch_one(&harness.pool)
        .await
        .expect("the template row must exist");

        assert!(after.0.is_some(), "last_used_at must be set");
        assert_eq!(after.1, before.1, "the version must not change");
        assert_eq!(after.2, before.2, "the definition must not change");
    }

    #[tokio::test]
    async fn service_key_can_send_template() {
        let harness = harness(FakeTransport::accepting()).await;
        save_section(&harness, "identity", jwt_identity(false)).await;
        save_template(
            &harness,
            "news",
            json!({
                "enabled": true,
                "senders": ["admin"],
                "to": { "topic": "news" },
                "title": "News",
                "body": "Body"
            }),
        )
        .await;

        let (status, body) = call_json(
            &template_router(harness.state),
            template_request(json!({ "app": APP, "template": "news" }), Some(SERVICE_KEY)),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "success");
        assert_eq!(harness.fcm.message_count(), 1);
    }

    #[tokio::test]
    async fn legacy_key_cannot_send_template() {
        let harness = harness(FakeTransport::accepting()).await;
        save_section(&harness, "identity", jwt_identity(false)).await;
        save_template(&harness, "hello", uid_template(&["user"], Some(120))).await;

        let (status, _) = call(
            &template_router(harness.state),
            template_request(json!({ "app": APP, "template": "hello" }), Some(LEGACY_KEY)),
        )
        .await;

        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(harness.fcm.message_count(), 0);
    }

    #[tokio::test]
    async fn template_works_with_jwt_identity_and_http_source() {
        let harness = harness(FakeTransport::accepting()).await;
        let base = spawn_source(Some(order_doc("buyer-1", "seller-1")), Vec::new()).await;
        save_section(&harness, "identity", jwt_identity(false)).await;
        use_http_source(&harness, &base).await;
        save_template(&harness, "order_new", order_new_template()).await;
        register_device(&harness, "seller-1").await;

        let (status, body) = call_json(
            &template_router(harness.state),
            template_request(
                json!({
                    "app": APP,
                    "template": "order_new",
                    "params": { "order_id": "o1" }
                }),
                Some(&jwt("buyer-1", "user")),
            ),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "success");
        assert_eq!(body["delivered"], 1);
        assert_eq!(body["failed"], 0);
    }

    #[tokio::test]
    async fn template_validation_rs_limits_apply() {
        let harness = harness(FakeTransport::accepting()).await;
        save_section(&harness, "identity", jwt_identity(false)).await;
        save_template(
            &harness,
            "bad_image",
            json!({
                "enabled": true,
                "senders": ["admin"],
                "to": { "uid": "{sender.uid}" },
                "title": "Title",
                "body": "Body",
                "image": "http://insecure.example/i.png"
            }),
        )
        .await;

        let (status, text) = call(
            &template_router(harness.state),
            template_request(
                json!({ "app": APP, "template": "bad_image" }),
                Some(&jwt("uid-1", "admin")),
            ),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(text.contains("https://"), "unexpected body: {text}");
    }

    fn dedupe_uid_template() -> Value {
        json!({
            "enabled": true,
            "senders": ["user"],
            "to": { "uid": "{sender.uid}" },
            "title": "Hello",
            "body": "Body",
            "rate": { "per_uid_per_hour": 120 },
            "dedupe": { "key": "hello-{sender.uid}", "seconds": 600 }
        })
    }

    fn dedupe_role_template() -> Value {
        json!({
            "enabled": true,
            "senders": ["admin"],
            "to": { "role": "admin" },
            "title": "Role",
            "body": "Body",
            "dedupe": { "key": "role-{sender.uid}", "seconds": 600 }
        })
    }

    #[tokio::test]
    async fn dedupe_released_when_device_lookup_fails() {
        let harness = harness(FakeTransport::accepting()).await;
        save_section(&harness, "identity", jwt_identity(false)).await;
        save_template(&harness, "hello", dedupe_uid_template()).await;
        let router = template_router(harness.state.clone());
        let token = jwt("uid-1", "user");
        let request = json!({ "app": APP, "template": "hello" });

        sqlx::query("DROP TABLE devices")
            .execute(&harness.pool)
            .await
            .expect("the devices table must drop");

        // A storage failure is a 500 (`device_error(Database)`), not a 503:
        // only data-source failures map to 503.
        let (status, text) = call(&router, template_request(request.clone(), Some(&token))).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(text, "device storage failed");

        // Restore the schema: the same request must not come back as a
        // duplicate after the failed attempt.
        crate::db::migrate(&harness.pool)
            .await
            .expect("the schema must be restored");

        let (status, body) = call_json(&router, template_request(request, Some(&token))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body,
            json!({ "status": "no_devices", "delivered": 0, "failed": 0 })
        );
    }

    #[tokio::test]
    async fn dedupe_released_when_role_source_unavailable() {
        let harness = harness(FakeTransport::accepting()).await;
        save_section(&harness, "identity", jwt_identity(true)).await;
        use_http_source(&harness, "http://127.0.0.1:1").await;
        save_template(&harness, "role_target", dedupe_role_template()).await;
        let router = template_router(harness.state.clone());
        let token = jwt("admin-1", "admin");
        let request = json!({ "app": APP, "template": "role_target" });

        let (status, text) = call(&router, template_request(request.clone(), Some(&token))).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(text, "the data source is unavailable");

        // A working source with one member: the retry must deliver.
        let base = spawn_source(None, vec!["seller-1".to_string()]).await;
        use_http_source(&harness, &base).await;
        register_device(&harness, "seller-1").await;

        let (status, body) = call_json(&router, template_request(request, Some(&token))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "success");
        assert_eq!(body["delivered"], 1);
        assert_eq!(harness.fcm.message_count(), 1);
    }

    #[tokio::test]
    async fn dedupe_released_when_role_members_missing() {
        let harness = harness(FakeTransport::accepting()).await;
        save_section(&harness, "identity", jwt_identity(false)).await;
        save_template(&harness, "role_target", dedupe_role_template()).await;
        let router = template_router(harness.state.clone());
        let token = jwt("admin-1", "admin");
        let request = json!({ "app": APP, "template": "role_target" });

        let (status, text) = call(&router, template_request(request.clone(), Some(&token))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(text, "role targets require identity.role_members");

        // Configure members and one working source, then retry.
        let base = spawn_source(None, vec!["seller-1".to_string()]).await;
        save_section(&harness, "identity", jwt_identity(true)).await;
        use_http_source(&harness, &base).await;
        register_device(&harness, "seller-1").await;

        let (status, body) = call_json(&router, template_request(request, Some(&token))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "success");
        assert_eq!(harness.fcm.message_count(), 1);
    }

    #[tokio::test]
    async fn dedupe_released_when_credentials_missing() {
        let harness = harness(FakeTransport::accepting()).await;
        save_section(&harness, "identity", jwt_identity(false)).await;
        save_template(&harness, "hello", dedupe_uid_template()).await;
        register_device(&harness, "uid-1").await;
        harness.state.project_map.write().await.remove(APP);
        let router = template_router(harness.state.clone());
        let token = jwt("uid-1", "user");
        let request = json!({ "app": APP, "template": "hello" });

        let (status, text) = call(&router, template_request(request.clone(), Some(&token))).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(text, "Service account for this app is not loaded");

        // Load the credentials, then the retry must deliver.
        harness
            .state
            .project_map
            .write()
            .await
            .insert(APP.to_string(), PROJECT.to_string());

        let (status, body) = call_json(&router, template_request(request, Some(&token))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "success");
        assert_eq!(body["delivered"], 1);
        assert_eq!(harness.fcm.message_count(), 1);
    }

    #[tokio::test]
    async fn dedupe_still_blocks_after_successful_delivery() {
        let harness = harness(FakeTransport::accepting()).await;
        save_section(&harness, "identity", jwt_identity(false)).await;
        save_template(&harness, "hello", dedupe_uid_template()).await;
        register_device(&harness, "uid-1").await;
        let router = template_router(harness.state.clone());
        let token = jwt("uid-1", "user");
        let request = json!({ "app": APP, "template": "hello" });

        let (first_status, first) =
            call_json(&router, template_request(request.clone(), Some(&token))).await;
        let (second_status, second) =
            call_json(&router, template_request(request, Some(&token))).await;

        assert_eq!(first_status, StatusCode::OK);
        assert_eq!(first["status"], "success");
        assert_eq!(second_status, StatusCode::OK);
        assert_eq!(
            second,
            json!({ "status": "duplicate", "delivered": 0, "failed": 0 })
        );
        assert_eq!(harness.fcm.message_count(), 1);
    }
}

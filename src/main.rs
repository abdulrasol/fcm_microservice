mod credentials;
mod db;
mod topics;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::cors::CorsLayer;

use credentials::{AuthMap, ProjectIdMap};

struct AppState {
    api_key: String,
    db: sqlx::SqlitePool,
    auth_map: AuthMap,
    project_map: ProjectIdMap,
    client: Client,
}

#[derive(Deserialize, Debug)]
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
struct LoginRequest {
    email: Option<String>,
    password: Option<String>,
}

#[derive(Deserialize)]
struct TopicRequest {
    name: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let _ = dotenvy::dotenv();

    let api_key = std::env::var("API_KEY").expect("API_KEY must be set");
    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());

    let db_pool = db::init_db().await.expect("Failed to initialize database");

    let auth_map: AuthMap = Arc::new(RwLock::new(HashMap::new()));
    let project_map: ProjectIdMap = Arc::new(RwLock::new(HashMap::new()));

    let creds_dir = PathBuf::from("credentials");
    credentials::scan_credentials_dir(&creds_dir, auth_map.clone(), project_map.clone()).await;
    credentials::watch_credentials_dir(creds_dir.clone(), auth_map.clone(), project_map.clone());

    let state = Arc::new(AppState {
        api_key,
        db: db_pool,
        auth_map,
        project_map,
        client: Client::new(),
    });

    let cors = CorsLayer::new()
        .allow_origin(tower_http::cors::Any)
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers(tower_http::cors::Any);

    // Serve static files from "dist", fallback to index.html for SPA routing
    let serve_dir = ServeDir::new("dist").not_found_service(ServeFile::new("dist/index.html"));

    let api_routes = Router::new()
        .route("/auth/login", post(login))
        .route("/apps", get(list_apps))
        .route("/history", get(get_history))
        .route("/topics", get(get_topics).post(add_topic))
        .route("/send", post(send_notification))
        .route("/topics/subscribe", post(topics::subscribe)) // Original functionality
        .route("/topics/unsubscribe", post(topics::unsubscribe)) // Original functionality
        .with_state(state.clone());

    let app = Router::new()
        .nest("/api/v1", api_routes)
        .route("/health", get(|| async { "OK" }))
        .fallback_service(serve_dir)
        .layer(cors);

    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr).await.unwrap();
    tracing::info!("Server listening on {}", addr);
    axum::serve(listener, app).await.unwrap();
}

// Authentication Middleware Logic (Helper)
fn verify_api_key(headers: &HeaderMap, expected_key: &str) -> bool {
    let auth_header = headers
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let expected = format!("Bearer {}", expected_key);
    auth_header == expected
}

async fn login(
    Json(payload): Json<LoginRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let expected_email = std::env::var("DASHBOARD_EMAIL").unwrap_or_default();
    let expected_password = std::env::var("DASHBOARD_PASSWORD").unwrap_or_default();

    if payload.email.as_deref() == Some(&expected_email)
        && payload.password.as_deref() == Some(&expected_password)
    {
        let api_key = std::env::var("API_KEY").unwrap_or_default();
        Ok((StatusCode::OK, Json(json!({ "token": api_key }))))
    } else {
        Err((StatusCode::UNAUTHORIZED, "Invalid credentials".to_string()))
    }
}

async fn list_apps(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if !verify_api_key(&headers, &state.api_key) {
        return Err((StatusCode::UNAUTHORIZED, "Invalid API Key".to_string()));
    }
    let map = state.auth_map.read().await;
    let apps: Vec<String> = map.keys().cloned().collect();
    Ok((StatusCode::OK, Json(apps)))
}

use sqlx::Row;

async fn get_history(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if !verify_api_key(&headers, &state.api_key) {
        return Err((StatusCode::UNAUTHORIZED, "Invalid API Key".to_string()));
    }
    // Fetch top 100 history items
    let rows = sqlx::query(
        "SELECT id, app, target, title, status, created_at FROM notifications_history ORDER BY id DESC LIMIT 100"
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut result = Vec::new();
    for row in rows {
        let id: i64 = row.get("id");
        let app: String = row.get("app");
        let target: String = row.get("target");
        let title: String = row.get("title");
        let status: String = row.get("status");
        let created_at: String = row.get("created_at");
        result.push(json!({
            "id": id,
            "app": app,
            "target": target,
            "title": title,
            "status": status,
            "created_at": created_at
        }));
    }
    Ok((StatusCode::OK, Json(result)))
}

async fn get_topics(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if !verify_api_key(&headers, &state.api_key) {
        return Err((StatusCode::UNAUTHORIZED, "Invalid API Key".to_string()));
    }
    let rows = sqlx::query("SELECT id, name FROM topics ORDER BY name ASC")
        .fetch_all(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut result = Vec::new();
    for row in rows {
        let name: String = row.get("name");
        result.push(name);
    }
    Ok((StatusCode::OK, Json(result)))
}

async fn add_topic(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(payload): Json<TopicRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if !verify_api_key(&headers, &state.api_key) {
        return Err((StatusCode::UNAUTHORIZED, "Invalid API Key".to_string()));
    }
    db::save_topic(&state.db, &payload.name)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok((StatusCode::OK, Json(json!({"status": "success"}))))
}

async fn send_notification(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SendNotificationRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if !verify_api_key(&headers, &state.api_key) {
        return Err((StatusCode::UNAUTHORIZED, "Invalid API Key".to_string()));
    }

    if payload.topic.is_none() && payload.token.is_none() && payload.condition.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Must provide topic, token, or condition".to_string(),
        ));
    }

    let target = payload.topic.clone().unwrap_or_else(|| {
        payload.token.clone().unwrap_or_else(|| {
            payload.condition.clone().unwrap_or_default()
        })
    });

    let project_id = {
        let pm = state.project_map.read().await;
        match pm.get(&payload.app) {
            Some(pid) => pid.clone(),
            None => {
                let _ = db::log_notification(&state.db, &payload.app, &target, &payload.title, "failed", "App not found").await;
                return Err((
                    StatusCode::NOT_FOUND,
                    format!("App '{}' not found in credentials folder.", payload.app),
                ));
            }
        }
    };

    let token = {
        let am = state.auth_map.read().await;
        let auth = am.get(&payload.app).unwrap();
        let scopes = &["https://www.googleapis.com/auth/firebase.messaging"];
        auth.token(scopes).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Auth Error: {}", e),
            )
        })?
    };

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
            for (k, v) in obj {
                let v_str = match v {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                string_map.insert(k.clone(), serde_json::Value::String(v_str));
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
        let _ = db::save_topic(&state.db, topic).await;
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
        .bearer_auth(token.token().unwrap())
        .json(&fcm_payload)
        .send()
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Network Error: {}", e),
            )
        })?;

    let status = res.status();
    let body_text = res
        .text()
        .await
        .unwrap_or_else(|_| "Failed to read response body".to_string());

    if status.is_success() {
        let _ = db::log_notification(&state.db, &payload.app, &target, &payload.title, "success", &body_text).await;
        Ok((
            StatusCode::OK,
            Json(json!({ "status": "success", "response": body_text })),
        ))
    } else {
        let _ = db::log_notification(&state.db, &payload.app, &target, &payload.title, "failed", &body_text).await;
        Err((StatusCode::BAD_GATEWAY, format!("FCM Error: {}", body_text)))
    }
}

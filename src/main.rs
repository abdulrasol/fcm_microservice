use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use hyper::client::HttpConnector;
use hyper_rustls::HttpsConnector;
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use tokio::net::TcpListener;
use yup_oauth2::{authenticator::Authenticator, ServiceAccountAuthenticator};

struct AppState {
    api_key: String,
    project_id: String,
    auth: Authenticator<HttpsConnector<HttpConnector>>,
    client: Client,
}

#[derive(Deserialize, Debug)]
struct SendNotificationRequest {
    topic: Option<String>,
    token: Option<String>,
    condition: Option<String>,
    title: String,
    body: Option<String>,
    image: Option<String>,
    data: Option<serde_json::Value>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let _ = dotenvy::dotenv();

    let api_key = std::env::var("API_KEY").expect("API_KEY must be set");
    let project_id = std::env::var("FIREBASE_PROJECT_ID").expect("FIREBASE_PROJECT_ID must be set");
    let credentials_path = std::env::var("GOOGLE_APPLICATION_CREDENTIALS")
        .unwrap_or_else(|_| "service-account.json".to_string());
    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());

    tracing::info!("Loading Service Account from: {}", credentials_path);
    let secret = yup_oauth2::read_service_account_key(&credentials_path)
        .await
        .expect("Failed to read service account key");

    let auth = ServiceAccountAuthenticator::builder(secret)
        .build()
        .await
        .expect("Failed to create authenticator");

    let state = Arc::new(AppState {
        api_key,
        project_id,
        auth,
        client: Client::new(),
    });

    let app = Router::new()
        .route("/health", get(|| async { "OK" }))
        .route("/api/v1/send", post(send_notification))
        .route("/api/v1/notification/send-topic", post(send_notification)) // backward compatibility
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr).await.unwrap();
    tracing::info!("Server listening on {}", addr);
    axum::serve(listener, app).await.unwrap();
}

async fn send_notification(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SendNotificationRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    // 1. Verify API Key
    let auth_header = headers
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    let expected_header = format!("Bearer {}", state.api_key);
    if auth_header != expected_header {
        return Err((StatusCode::UNAUTHORIZED, "Invalid API Key".to_string()));
    }

    // 2. Validate Target
    if payload.topic.is_none() && payload.token.is_none() && payload.condition.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Must provide topic, token, or condition".to_string(),
        ));
    }

    // 3. Get OAuth2 Token for FCM
    let scopes = &["https://www.googleapis.com/auth/firebase.messaging"];
    let token = state.auth.token(scopes).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Auth Error: {}", e),
        )
    })?;

    // 4. Construct FCM v1 Payload
    let mut message = json!({
        "notification": {
            "title": payload.title,
        }
    });

    if let Some(body) = payload.body {
        message["notification"]["body"] = json!(body);
    }
    if let Some(image) = payload.image {
        message["notification"]["image"] = json!(image);
    }

    // Ensure data values are strings
    if let Some(data) = payload.data {
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

    if let Some(topic) = payload.topic {
        message["topic"] = json!(topic);
    } else if let Some(token) = payload.token {
        message["token"] = json!(token);
    } else if let Some(condition) = payload.condition {
        message["condition"] = json!(condition);
    }

    let fcm_payload = json!({ "message": message });

    // 5. Send to FCM
    let url = format!(
        "https://fcm.googleapis.com/v1/projects/{}/messages:send",
        state.project_id
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
        Ok((
            StatusCode::OK,
            Json(json!({ "status": "success", "response": body_text })),
        ))
    } else {
        Err((StatusCode::BAD_GATEWAY, format!("FCM Error: {}", body_text)))
    }
}

use crate::auth::{self, AuthError};
use crate::validation;
use crate::AppState;
use axum::{
    extract::{ConnectInfo, State},
    http::{header, HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;

pub(crate) const FCM_SCOPE: &str = "https://www.googleapis.com/auth/firebase.messaging";

/// Returns an OAuth access token for one app without exposing credentials.
/// Admin diagnostics and topic mutation share this exact credential path.
pub(crate) async fn firebase_access_token(state: &AppState, app: &str) -> Result<String, ApiError> {
    let auth = {
        let map = state.auth_map.read().await;
        map.get(app).cloned()
    }
    .ok_or((
        StatusCode::INTERNAL_SERVER_ERROR,
        "Service account for this app is not loaded".to_string(),
    ))?;

    let token = auth.token(&[FCM_SCOPE]).await.map_err(|_| {
        (
            StatusCode::BAD_GATEWAY,
            "Could not authenticate with Firebase".into(),
        )
    })?;
    token.token().map(str::to_owned).ok_or((
        StatusCode::BAD_GATEWAY,
        "Missing Firebase access token".into(),
    ))
}

type ApiError = (StatusCode, String);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TopicRequest {
    app: String,
    topic: String,
    tokens: Vec<String>,
}

impl TopicRequest {
    fn validate(&self) -> Result<(), ApiError> {
        if self.app.trim().is_empty() {
            return Err((StatusCode::BAD_REQUEST, "app field is required".into()));
        }
        validation::validate_topic_name(&self.topic).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
        validation::validate_subscribe_tokens(&self.tokens)
            .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
        Ok(())
    }
}

pub(crate) async fn subscribe(
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    State(state): State<Arc<AppState>>,
    Json(body): Json<TopicRequest>,
) -> Result<Json<Value>, ApiError> {
    manage(headers, peer, state, body, "batchAdd").await
}

pub(crate) async fn unsubscribe(
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    State(state): State<Arc<AppState>>,
    Json(body): Json<TopicRequest>,
) -> Result<Json<Value>, ApiError> {
    manage(headers, peer, state, body, "batchRemove").await
}

async fn manage(
    headers: HeaderMap,
    peer: SocketAddr,
    state: Arc<AppState>,
    body: TopicRequest,
    operation: &str,
) -> Result<Json<Value>, ApiError> {
    body.validate()?;

    let app_exists = {
        let map = state.project_map.read().await;
        map.contains_key(&body.app)
    };
    if !app_exists {
        return Err((
            StatusCode::NOT_FOUND,
            format!("App '{}' not found in credentials folder.", body.app),
        ));
    }

    check_ip_limit(&state, &headers, peer)?;

    let resolved = state
        .auth
        .authorize_topic(&headers, &body.app, &body.topic)
        .await;
    let (_, user) = match resolved {
        Ok(value) => value,
        Err(error) => {
            if matches!(error, AuthError::Unauthorized(_) | AuthError::Forbidden(_))
                && headers.contains_key(header::AUTHORIZATION)
                && !state.auth_fail_limiter.check(&format!(
                    "ip:{}",
                    auth::client_ip(
                        &headers,
                        Some(peer),
                        state.trust_proxy,
                        state.client_ip_header
                    )
                ))
            {
                return Err(too_many_requests());
            }
            return Err(error.into_response());
        }
    };

    if let Some(user) = user {
        check_uid_limit(&state, &user.uid)?;
    }

    let access_token = firebase_access_token(&state, &body.app).await?;
    let response = state
        .client
        .post(format!("https://iid.googleapis.com/iid/v1:{operation}"))
        .timeout(std::time::Duration::from_secs(30))
        .bearer_auth(&access_token)
        .header("access_token_auth", "true")
        .json(&json!({"to": format!("/topics/{}", body.topic), "registration_tokens": body.tokens}))
        .send()
        .await
        .map_err(|_| {
            (
                StatusCode::BAD_GATEWAY,
                "Firebase topic request failed".into(),
            )
        })?;
    if !response.status().is_success() {
        let status = response.status();
        let client_status = if status.as_u16() >= 400 && status.as_u16() < 500 {
            StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_REQUEST)
        } else {
            StatusCode::BAD_REQUEST
        };
        let err_text = response.text().await.unwrap_or_default();
        return Err((
            client_status,
            format!(
                "Firebase topic request returned HTTP {}: {}",
                status, err_text
            ),
        ));
    }
    let payload: Value = response
        .json()
        .await
        .map_err(|_| (StatusCode::BAD_GATEWAY, "Invalid Firebase response".into()))?;
    summarize(&payload, body.tokens.len()).map(Json)
}

pub(crate) fn check_ip_limit(
    state: &AppState,
    headers: &HeaderMap,
    peer: SocketAddr,
) -> Result<(), ApiError> {
    let ip = auth::client_ip(
        headers,
        Some(peer),
        state.trust_proxy,
        state.client_ip_header,
    );
    if state.topics_limiter.check(&format!("ip:{ip}")) {
        Ok(())
    } else {
        Err(too_many_requests())
    }
}

pub(crate) fn check_uid_limit(state: &AppState, uid: &str) -> Result<(), ApiError> {
    if state.topics_limiter.check(&format!("uid:{uid}")) {
        Ok(())
    } else {
        Err(too_many_requests())
    }
}

pub(crate) fn too_many_requests() -> ApiError {
    (
        StatusCode::TOO_MANY_REQUESTS,
        "Too many requests; retry later".to_string(),
    )
}

fn summarize(payload: &Value, expected: usize) -> Result<Value, ApiError> {
    let results = payload
        .get("results")
        .and_then(Value::as_array)
        .filter(|r| r.len() == expected)
        .ok_or((
            StatusCode::BAD_GATEWAY,
            "Unexpected Firebase result count".into(),
        ))?;
    let mut errors = Vec::new();
    for (index, result) in results.iter().enumerate() {
        let object = result.as_object().ok_or((
            StatusCode::BAD_GATEWAY,
            "Invalid Firebase token result".into(),
        ))?;
        if let Some(error) = object.get("error") {
            errors.push(json!({"index": index, "error": error}));
        } else if !object.is_empty() {
            return Err((
                StatusCode::BAD_GATEWAY,
                "Unknown Firebase token result".into(),
            ));
        }
    }
    Ok(
        json!({"success_count": expected - errors.len(), "failure_count": errors.len(), "errors": errors}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_topic_and_batch_boundaries() {
        let valid = || TopicRequest {
            app: "app".into(),
            topic: "news-1_.~%".into(),
            tokens: vec!["token".into()],
        };
        assert!(valid().validate().is_ok());
        for name in ["", "/topics/all", "a b", "عربي", "a/b"] {
            assert!(TopicRequest {
                topic: name.into(),
                ..valid()
            }
            .validate()
            .is_err());
        }
        assert!(TopicRequest {
            topic: "a".repeat(validation::MAX_TOPIC_LENGTH + 1),
            ..valid()
        }
        .validate()
        .is_err());
        assert!(TopicRequest {
            tokens: vec![],
            ..valid()
        }
        .validate()
        .is_err());
        assert!(TopicRequest {
            tokens: vec!["token".into(); validation::MAX_SUBSCRIBE_TOKENS],
            ..valid()
        }
        .validate()
        .is_ok());
        assert!(TopicRequest {
            tokens: vec!["token".into(); validation::MAX_SUBSCRIBE_TOKENS + 1],
            ..valid()
        }
        .validate()
        .is_err());
        assert!(TopicRequest {
            tokens: vec![" ".into()],
            ..valid()
        }
        .validate()
        .is_err());
        assert!(TopicRequest {
            tokens: vec!["x".repeat(validation::MAX_DEVICE_TOKEN_LENGTH + 1)],
            ..valid()
        }
        .validate()
        .is_err());
    }
    #[test]
    fn partial_failure_keeps_original_indexes() {
        let result = summarize(&json!({"results":[{}, {"error":"NOT_FOUND"}, {}]}), 3).unwrap();
        assert_eq!(result["success_count"], 2);
        assert_eq!(result["failure_count"], 1);
        assert_eq!(result["errors"][0]["index"], 1);
    }
    #[test]
    fn rejects_malformed_upstream_results() {
        for payload in [
            json!({}),
            json!({"results":[]}),
            json!({"results":[null]}),
            json!({"results":[{"unknown":true}]}),
        ] {
            assert!(summarize(&payload, 1).is_err());
        }
    }
}

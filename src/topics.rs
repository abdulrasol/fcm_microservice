use crate::AppState;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

type ApiError = (StatusCode, String);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TopicRequest {
    topic: String,
    tokens: Vec<String>,
}

impl TopicRequest {
    fn validate(&self) -> Result<(), ApiError> {
        if self.topic.is_empty()
            || self.topic.len() > 900
            || !self
                .topic
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.~%".contains(&c))
        {
            return Err((StatusCode::BAD_REQUEST, "topic must be 1-900 ASCII characters: letters, digits, - _ . ~ % (no /topics/ prefix)".into()));
        }
        if self.tokens.is_empty()
            || self.tokens.len() > 1000
            || self
                .tokens
                .iter()
                .any(|t| t.trim().is_empty() || t.chars().any(char::is_whitespace))
        {
            return Err((
                StatusCode::BAD_REQUEST,
                "tokens must contain 1-1000 nonempty registration tokens without whitespace".into(),
            ));
        }
        Ok(())
    }
}

pub(crate) async fn subscribe(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(body): Json<TopicRequest>,
) -> Result<Json<Value>, ApiError> {
    manage(headers, state, body, "batchAdd").await
}
pub(crate) async fn unsubscribe(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(body): Json<TopicRequest>,
) -> Result<Json<Value>, ApiError> {
    manage(headers, state, body, "batchRemove").await
}

async fn manage(
    headers: HeaderMap,
    state: Arc<AppState>,
    body: TopicRequest,
    operation: &str,
) -> Result<Json<Value>, ApiError> {
    let expected = format!("Bearer {}", state.api_key);
    if headers.get("Authorization").and_then(|h| h.to_str().ok()) != Some(expected.as_str()) {
        return Err((StatusCode::UNAUTHORIZED, "Invalid API Key".into()));
    }
    body.validate()?;
    let token = state
        .auth
        .token(&["https://www.googleapis.com/auth/firebase.messaging"])
        .await
        .map_err(|_| {
            (
                StatusCode::BAD_GATEWAY,
                "Could not authenticate with Firebase".into(),
            )
        })?;
    let access_token = token.token().ok_or((
        StatusCode::BAD_GATEWAY,
        "Missing Firebase access token".into(),
    ))?;
    let response = state
        .client
        .post(format!("https://iid.googleapis.com/iid/v1:{operation}"))
        .timeout(std::time::Duration::from_secs(30))
        .bearer_auth(access_token)
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
        return Err((
            StatusCode::BAD_GATEWAY,
            format!("Firebase topic request returned HTTP {}", response.status()),
        ));
    }
    let payload: Value = response
        .json()
        .await
        .map_err(|_| (StatusCode::BAD_GATEWAY, "Invalid Firebase response".into()))?;
    summarize(&payload, body.tokens.len()).map(Json)
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
            topic: "news-1_.~%".into(),
            tokens: vec!["token".into()],
        };
        assert!(valid().validate().is_ok());
        for name in ["", "/topics/all", "a b", "عربي"] {
            assert!(TopicRequest {
                topic: name.into(),
                ..valid()
            }
            .validate()
            .is_err());
        }
        assert!(TopicRequest {
            tokens: vec![],
            ..valid()
        }
        .validate()
        .is_err());
        assert!(TopicRequest {
            tokens: vec!["token".into(); 1000],
            ..valid()
        }
        .validate()
        .is_ok());
        assert!(TopicRequest {
            tokens: vec!["token".into(); 1001],
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

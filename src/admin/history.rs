//! Dashboard history, the free send and the device statistics (T10-B).
//!
//! The history shows what was sent and never a device token: the target is a
//! label (`topic:news`, `uid:<uid>`) and the payload is the rendered content.
//! The free send is the dashboard's own `/send`: it accepts a topic, a
//! condition or a uid, never a raw token.

use super::auth::AuthedUser;
use super::{authorize, AdminError, AdminJson, Need};
use crate::db;
use crate::send::{self, FcmOutcome};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;

/// The kinds of history row the dashboard knows.
const HISTORY_KINDS: [&str; 4] = ["raw", "template", "campaign", "test"];

/// The largest page of the history list.
const MAX_HISTORY_LIMIT: u32 = 200;

/// The default page of the history list.
const DEFAULT_HISTORY_LIMIT: u32 = 50;

/// The `response` column is a diagnostic; the list shows a short prefix.
const MAX_RESPONSE_CHARS: usize = 300;

/// A `uid` target still needs a syntactically valid destination for the
/// content checks; the value is never sent.
const VALIDATION_PLACEHOLDER_TOKEN: &str = "00000000000000000000000000000000";

/// The filters of the history list.
#[derive(Deserialize)]
pub struct HistoryQuery {
    pub limit: Option<u32>,
    pub before: Option<i64>,
    pub kind: Option<String>,
    pub status: Option<String>,
    /// The reference of the row, matched exactly (T13-A).
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    /// The oldest row of the page, as an RFC 3339 stamp or a plain date.
    pub since: Option<String>,
    /// The newest row of the page, as an RFC 3339 stamp or a plain date.
    pub until: Option<String>,
}

/// The largest reference a filter accepts.
const MAX_REFERENCE_CHARS: usize = 100;

/// Checks a reference filter: an exact match, at most 100 characters.
fn check_reference(value: &str) -> Result<&str, AdminError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok("");
    }
    if trimmed.chars().count() > MAX_REFERENCE_CHARS {
        return Err(AdminError::invalid_request(
            "the reference filter is longer than 100 characters",
        ));
    }
    Ok(trimmed)
}

/// Checks a date filter and returns the value to bind.
///
/// The value is an RFC 3339 stamp (`2026-01-02T03:04:05Z`, with an offset
/// too) or a plain date (`2026-01-02`). SQLite reads both and compares them
/// in UTC.
fn check_date(value: &str, name: &str) -> Result<String, AdminError> {
    let text = value.trim();
    let invalid =
        || AdminError::invalid_request(&format!("the {} filter is not a valid date", name));

    let (date, rest) = match text.split_once(['T', 't']) {
        Some((date, rest)) => (date, Some(rest)),
        None => (text, None),
    };
    let mut parts = date.split('-');
    let (year, month, day) = match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(year), Some(month), Some(day), None) => (year, month, day),
        _ => return Err(invalid()),
    };
    if !is_digits(year, 4) || !is_digits(month, 2) || !is_digits(day, 2) {
        return Err(invalid());
    }
    let month: u32 = month.parse().unwrap_or(0);
    let day: u32 = day.parse().unwrap_or(0);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return Err(invalid());
    }

    match rest {
        None => Ok(text.to_string()),
        Some(rest) => {
            let rest = rest.trim_end_matches('Z').trim_end_matches('z');
            let (clock, offset) = match rest.split_once(['+', '-']) {
                Some((clock, offset)) => (clock, Some(offset)),
                None => (rest, None),
            };
            let mut pieces = clock.split(':');
            let (hour, minute, second) = match (pieces.next(), pieces.next(), pieces.next()) {
                (Some(hour), Some(minute), Some(second)) => (hour, minute, second),
                _ => return Err(invalid()),
            };
            if pieces.next().is_some() {
                return Err(invalid());
            }
            if !is_digits(hour, 2) || !is_digits(minute, 2) || !is_digits(second, 2) {
                return Err(invalid());
            }
            let hour: u32 = hour.parse().unwrap_or(99);
            let minute: u32 = minute.parse().unwrap_or(99);
            let second: u32 = second.parse().unwrap_or(99);
            if hour > 23 || minute > 59 || second > 60 {
                return Err(invalid());
            }
            if let Some(offset) = offset {
                let mut pieces = offset.split(':');
                let (hours, minutes) = match (pieces.next(), pieces.next(), pieces.next()) {
                    (Some(hours), Some(minutes), None) => (hours, minutes),
                    _ => return Err(invalid()),
                };
                if !is_digits(hours, 2) || !is_digits(minutes, 2) {
                    return Err(invalid());
                }
                if hours.parse::<u32>().unwrap_or(99) > 23
                    || minutes.parse::<u32>().unwrap_or(99) > 59
                {
                    return Err(invalid());
                }
            }
            Ok(text.to_string())
        }
    }
}

/// True when the text is exactly `length` digits.
fn is_digits(text: &str, length: usize) -> bool {
    text.len() == length && text.bytes().all(|byte| byte.is_ascii_digit())
}

/// `GET /apps/{app}/history`: the app's notification history.
pub async fn list_history(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path(app): Path<String>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<Value>, AdminError> {
    authorize(&state, &user, &app, Need::AnyMember).await?;

    let kind = match query.kind.as_deref() {
        None => None,
        Some(value) if HISTORY_KINDS.contains(&value) => Some(value),
        Some(_) => return Err(AdminError::invalid_request("unknown history kind")),
    };
    let limit = query
        .limit
        .unwrap_or(DEFAULT_HISTORY_LIMIT)
        .clamp(1, MAX_HISTORY_LIMIT);

    let reference = match query.reference.as_deref() {
        Some(value) => {
            let trimmed = check_reference(value)?;
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        }
        None => None,
    };
    let since = match query.since.as_deref() {
        Some(value) => Some(check_date(value, "since")?),
        None => None,
    };
    let until = match query.until.as_deref() {
        Some(value) => Some(check_date(value, "until")?),
        None => None,
    };

    let rows = db::read_history(
        &state.db,
        &app,
        db::HistoryFilter {
            kind,
            status: query.status.as_deref(),
            reference,
            since: since.as_deref(),
            until: until.as_deref(),
            before: query.before,
            limit,
        },
    )
    .await
    .map_err(|error| AdminError::from_db("read the history", error))?;

    let items: Vec<Value> = rows
        .iter()
        .map(|row| {
            json!({
                "id": row.id,
                "target": row.target,
                "title": row.title,
                "status": row.status,
                "kind": row.kind,
                "ref": row.reference,
                "sender_uid": row.sender_uid,
                "sender_email": row.sender_email,
                "created_at": row.created_at,
                "response": row
                    .response
                    .as_deref()
                    .map(|response| truncate(response, MAX_RESPONSE_CHARS)),
            })
        })
        .collect();

    Ok(Json(Value::Array(items)))
}

/// `GET /apps/{app}/history/{id}`: one history row with its payload.
pub async fn history_detail(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path((app, id)): Path<(String, i64)>,
) -> Result<Json<Value>, AdminError> {
    authorize(&state, &user, &app, Need::AnyMember).await?;

    let row = db::read_history_detail(&state.db, &app, id)
        .await
        .map_err(|error| AdminError::from_db("read the history row", error))?
        .ok_or_else(|| AdminError::not_found("the history row does not exist"))?;

    Ok(Json(json!({
        "id": row.id,
        "target": row.target,
        "title": row.title,
        "status": row.status,
        "kind": row.kind,
        "ref": row.reference,
        "sender_uid": row.sender_uid,
        "sender_email": row.sender_email,
        "created_at": row.created_at,
        "response": row
            .response
            .as_deref()
            .map(|response| truncate(response, MAX_RESPONSE_CHARS)),
        "payload": row.payload,
    })))
}

/// The body of a free send: exactly one target.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FreeSendRequest {
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub data: Option<Value>,
    #[serde(default)]
    pub topic: Option<String>,
    #[serde(default)]
    pub condition: Option<String>,
    #[serde(default)]
    pub uid: Option<String>,
}

/// `POST /apps/{app}/send`: the dashboard's own send.
///
/// A topic or a condition is one FCM message; a uid is the devices of that
/// uid. A raw token is never accepted: the body has no field for it.
pub async fn free_send(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path(app): Path<String>,
    AdminJson(body): AdminJson<FreeSendRequest>,
) -> Result<(StatusCode, Json<Value>), AdminError> {
    authorize(&state, &user, &app, Need::AnyMember).await?;

    let targets = [
        body.topic.is_some(),
        body.condition.is_some(),
        body.uid.is_some(),
    ]
    .into_iter()
    .filter(|present| *present)
    .count();
    if targets != 1 {
        return Err(AdminError::invalid_request(
            "provide exactly one of topic, condition or uid",
        ));
    }

    // The same content limits as `/send`. A uid target borrows a valid
    // placeholder token, exactly like the template route.
    let (topic, condition, token) = match (&body.topic, &body.condition, &body.uid) {
        (Some(topic), _, _) => (Some(topic.as_str()), None, None),
        (_, Some(condition), _) => (None, Some(condition.as_str()), None),
        (_, _, Some(_)) => (None, None, Some(VALIDATION_PLACEHOLDER_TOKEN)),
        _ => (None, None, None),
    };
    crate::validation::validate_send(
        &app,
        topic,
        token,
        condition,
        &body.title,
        body.body.as_deref(),
        body.image.as_deref(),
        body.data.as_ref(),
    )
    .map_err(|message| AdminError::invalid_request(&message))?;

    // T13-A: the free send has its own budget, per dashboard account. The
    // budget of the topics routes is never spent here.
    if !state
        .admin_limiters
        .free_send
        .check(&format!("admin:send:{}", user.user_id))
    {
        return Err(AdminError::too_many_requests());
    }

    let project_id = {
        let projects = state.project_map.read().await;
        projects.get(&app).cloned()
    }
    .ok_or_else(|| AdminError::internal("Service account for this app is not loaded"))?;

    let data: BTreeMap<String, String> = match &body.data {
        Some(Value::Object(entries)) => entries
            .iter()
            .map(|(key, value)| {
                let text = match value {
                    Value::String(text) => text.clone(),
                    other => other.to_string(),
                };
                (key.clone(), text)
            })
            .collect(),
        _ => BTreeMap::new(),
    };

    let (target, delivered, failed) = if let Some(topic) = &body.topic {
        let message = send::build_message(
            &body.title,
            body.body.as_deref(),
            body.image.as_deref(),
            &data,
            None,
            &send::Destination::Topic(topic.clone()),
        );
        let accepted = matches!(
            state.fcm.send(&app, &project_id, message).await,
            Ok(FcmOutcome::Accepted { .. })
        );
        if accepted {
            let _ = db::save_topic(&state.db, &app, topic).await;
        }
        (
            format!("topic:{}", topic),
            usize::from(accepted),
            usize::from(!accepted),
        )
    } else if let Some(condition) = &body.condition {
        let message = send::build_message(
            &body.title,
            body.body.as_deref(),
            body.image.as_deref(),
            &data,
            None,
            &send::Destination::Condition(condition.clone()),
        );
        let accepted = matches!(
            state.fcm.send(&app, &project_id, message).await,
            Ok(FcmOutcome::Accepted { .. })
        );
        (
            "condition".to_string(),
            usize::from(accepted),
            usize::from(!accepted),
        )
    } else {
        let uid = body.uid.clone().unwrap_or_default();
        let tokens = state
            .devices
            .tokens_for_uid(&app, &uid)
            .await
            .map_err(|_| AdminError::internal("device storage failed"))?;
        let delivery = send::send_to_tokens(
            &state.fcm,
            &app,
            &project_id,
            &tokens,
            |token| {
                send::build_message(
                    &body.title,
                    body.body.as_deref(),
                    body.image.as_deref(),
                    &data,
                    None,
                    &send::Destination::Token(token.to_string()),
                )
            },
            send::DEFAULT_CONCURRENCY,
        )
        .await;
        for token in &delivery.dead_tokens {
            let _ = state.devices.remove_dead_token(&app, token).await;
        }
        (format!("uid:{}", uid), delivery.delivered, delivery.failed)
    };

    let payload = json!({
        "title": body.title,
        "body": body.body,
        "data": data,
    })
    .to_string();
    let response = json!({ "delivered": delivered, "failed": failed }).to_string();
    let _ = db::log_notification_ext(
        &state.db,
        &app,
        &target,
        &body.title,
        if delivered > 0 { "success" } else { "failed" },
        &response,
        &payload,
        &user.email,
        None,
        "raw",
        Some("dashboard"),
    )
    .await;

    // The audit carries the target only, never the notification text.
    let detail = json!({ "target": target }).to_string();
    let _ = db::write_audit(&state.db, &user.email, Some(&app), "send.free", &detail).await;

    if delivered > 0 {
        Ok((
            StatusCode::OK,
            Json(json!({
                "status": "success",
                "delivered": delivered,
                "failed": failed,
            })),
        ))
    } else {
        Ok((
            StatusCode::BAD_GATEWAY,
            Json(json!({
                "status": "failed",
                "delivered": 0,
                "failed": failed,
            })),
        ))
    }
}

/// `GET /apps/{app}/devices/stats`: counts only, never a token or a uid.
pub async fn device_stats(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path(app): Path<String>,
) -> Result<Json<Value>, AdminError> {
    authorize(&state, &user, &app, Need::AnyMember).await?;

    let stats = db::device_stats(&state.db, &app)
        .await
        .map_err(|error| AdminError::from_db("read the device statistics", error))?;

    Ok(Json(json!({
        "total": stats.total,
        "distinct_uids": stats.distinct_uids,
        "by_platform": {
            "android": stats.android,
            "ios": stats.ios,
            "web": stats.web,
        },
        "active_last_7d": stats.active_last_7d,
        "newest_last_seen": stats.newest_last_seen,
    })))
}

/// Shortens a diagnostic to `max` characters.
fn truncate(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::super::tests::{
        app as admin_router, bind_app, call, insert_user, login_request, memory_pool,
        session_token, setup, test_state_with_env, Harness, APP, OTHER_APP, PASSWORD,
    };
    use super::*;
    use crate::send::{FcmOutcome, SendError};
    use axum::http::Method;
    use axum::Router;
    use serde_json::json;
    use sqlx::SqlitePool;
    use std::sync::Arc;

    /// Writes one history row.
    async fn insert_history(
        pool: &SqlitePool,
        app: &str,
        kind: &str,
        status: &str,
        response: &str,
        payload: &str,
    ) -> i64 {
        let result = sqlx::query(
            "INSERT INTO notifications_history \
             (app, target, title, status, response, payload, sender_uid, sender_email, kind, \"ref\") \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(app)
        .bind("topic:news")
        .bind("A title")
        .bind(status)
        .bind(response)
        .bind(payload)
        .bind("sender-uid")
        .bind("sender@example.com")
        .bind(kind)
        .bind("ref-1")
        .execute(pool)
        .await
        .expect("the history row must be inserted");
        result.last_insert_rowid()
    }

    /// A transport that always rejects, like a failing FCM.
    struct FailingFcm;

    #[async_trait::async_trait]
    impl crate::send::FcmTransport for FailingFcm {
        async fn send(
            &self,
            _app: &str,
            _project_id: &str,
            _message: Value,
        ) -> Result<FcmOutcome, SendError> {
            Ok(FcmOutcome::Rejected {
                http_status: 500,
                body: "fcm is down".to_string(),
                dead_token: false,
            })
        }
    }

    /// A router whose FCM always fails, with a logged-in owner.
    async fn setup_failing() -> (Router, SqlitePool, String) {
        let pool = memory_pool().await;
        let state = test_state_with_env(pool.clone(), None, Some(Arc::new(FailingFcm))).await;
        bind_app(&state, APP, "project-watt").await;
        insert_user(&pool, "user@example.com", "owner", false).await;
        let router = admin_router(state);
        let login = login_request(&router, "user@example.com", PASSWORD).await;
        let token = session_token(&login);
        (router, pool, token)
    }

    async fn free_send(harness: &Harness, body: Value) -> (StatusCode, Value) {
        harness
            .call(Method::POST, &format!("/apps/{}/send", APP), Some(body))
            .await
    }

    #[tokio::test]
    async fn history_scoped_to_app_and_paginated() {
        let harness = setup("owner", &[]).await;
        let mut ids = Vec::new();
        for _ in 0..5 {
            ids.push(insert_history(&harness.pool, APP, "raw", "success", "ok", "{}").await);
        }
        insert_history(&harness.pool, OTHER_APP, "raw", "success", "ok", "{}").await;

        let (status, body) = harness
            .call(Method::GET, &format!("/apps/{}/history", APP), None)
            .await;
        assert_eq!(status, StatusCode::OK);
        let rows = body.as_array().expect("an array");
        assert_eq!(rows.len(), 5, "another app is never listed");

        let (status, body) = harness
            .call(Method::GET, &format!("/apps/{}/history?limit=2", APP), None)
            .await;
        assert_eq!(status, StatusCode::OK);
        let rows = body.as_array().expect("an array");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["id"], ids[4].clone());

        let (status, body) = harness
            .call(
                Method::GET,
                &format!("/apps/{}/history?before={}", APP, ids[3]),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        let rows = body.as_array().expect("an array");
        assert_eq!(rows.len(), 3);
        assert!(rows
            .iter()
            .all(|row| row["id"].as_i64().unwrap_or(i64::MAX) < ids[3]));
    }

    #[tokio::test]
    async fn history_filters_by_kind_and_status() {
        let harness = setup("owner", &[]).await;
        insert_history(&harness.pool, APP, "raw", "success", "ok", "{}").await;
        insert_history(&harness.pool, APP, "template", "success", "ok", "{}").await;
        insert_history(&harness.pool, APP, "template", "failed", "error", "{}").await;
        insert_history(&harness.pool, APP, "campaign", "failed", "error", "{}").await;

        let (status, body) = harness
            .call(
                Method::GET,
                &format!("/apps/{}/history?kind=template", APP),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        let rows = body.as_array().expect("an array");
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| row["kind"] == "template"));

        let (status, body) = harness
            .call(
                Method::GET,
                &format!("/apps/{}/history?status=failed", APP),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        let rows = body.as_array().expect("an array");
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| row["status"] == "failed"));

        let (status, body) = harness
            .call(
                Method::GET,
                &format!("/apps/{}/history?kind=bogus", APP),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_request");
    }

    #[tokio::test]
    async fn history_response_truncated_and_payload_only_in_detail() {
        let harness = setup("owner", &[]).await;
        let long = "r".repeat(500);
        let id = insert_history(&harness.pool, APP, "raw", "success", &long, "{\"a\":1}").await;

        let (status, body) = harness
            .call(Method::GET, &format!("/apps/{}/history", APP), None)
            .await;
        assert_eq!(status, StatusCode::OK);
        let row = &body.as_array().expect("an array")[0];
        assert_eq!(
            row["response"].as_str().expect("a response").len(),
            300,
            "the response is shortened"
        );
        assert!(row.get("payload").is_none(), "the list has no payload");

        let (status, body) = harness
            .call(Method::GET, &format!("/apps/{}/history/{}", APP, id), None)
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["payload"], "{\"a\":1}");
    }

    #[tokio::test]
    async fn history_detail_never_contains_tokens() {
        let harness = setup("owner", &[]).await;
        harness
            .register_device("u1", "token-u1-1-0000000000000000000000000000")
            .await;

        let (status, _) = free_send(
            &harness,
            json!({ "title": "A title", "body": "A body", "uid": "u1" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, body) = harness
            .call(Method::GET, &format!("/apps/{}/history", APP), None)
            .await;
        assert_eq!(status, StatusCode::OK);
        let id = body.as_array().expect("an array")[0]["id"]
            .as_i64()
            .expect("an id");

        let (status, body) = harness
            .call(Method::GET, &format!("/apps/{}/history/{}", APP, id), None)
            .await;
        assert_eq!(status, StatusCode::OK);
        let text = body.to_string();
        assert!(
            !text.contains("token-u1"),
            "the history must not keep a device token"
        );
        assert!(!text.contains("token-u1-1-0000000000000000000000000000"));
    }

    #[tokio::test]
    async fn free_send_topic_calls_fcm_once() {
        let harness = setup("owner", &[]).await;
        let (status, body) = free_send(
            &harness,
            json!({ "title": "A title", "body": "A body", "topic": "news" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "success");
        assert_eq!(body["delivered"], 1);
        assert_eq!(harness.fcm.calls(), 1, "a topic is one message");

        let saved: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM topics WHERE app = ? AND name = ?")
                .bind(APP)
                .bind("news")
                .fetch_one(&harness.pool)
                .await
                .expect("the topics table must answer");
        assert_eq!(saved, 1, "the topic is saved like /send does");
    }

    #[tokio::test]
    async fn free_send_condition_ok() {
        let harness = setup("owner", &[]).await;
        let (status, body) = free_send(
            &harness,
            json!({
                "title": "A title",
                "body": "A body",
                "condition": "'promos' in topics"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "success");
        assert_eq!(body["delivered"], 1);
        assert_eq!(harness.fcm.calls(), 1);
    }

    #[tokio::test]
    async fn free_send_uid_reaches_only_that_uids_devices() {
        let harness = setup("owner", &[]).await;
        harness
            .register_device("u1", "token-u1-1-0000000000000000000000000000")
            .await;
        harness
            .register_device("u1", "token-u1-2-0000000000000000000000000000")
            .await;
        harness
            .register_device("u2", "token-u2-1-0000000000000000000000000000")
            .await;

        let (status, body) = free_send(
            &harness,
            json!({ "title": "A title", "body": "A body", "uid": "u1" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["delivered"], 2);
        assert_eq!(body["failed"], 0);

        let tokens = harness.fcm.tokens();
        assert_eq!(tokens.len(), 2);
        assert!(tokens.iter().all(|token| token.contains("token-u1")));
        assert!(!tokens.iter().any(|token| token.contains("token-u2")));
    }

    #[tokio::test]
    async fn free_send_requires_exactly_one_target() {
        let harness = setup("owner", &[]).await;

        let (status, _) = free_send(&harness, json!({ "title": "A title" })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let (status, _) = free_send(
            &harness,
            json!({ "title": "A title", "topic": "news", "uid": "u1" }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let (status, _) = free_send(
            &harness,
            json!({ "title": "A title", "topic": "news", "condition": "'promos' in topics" }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(harness.fcm.calls(), 0);
    }

    #[tokio::test]
    async fn free_send_rejects_raw_token_field() {
        let harness = setup("owner", &[]).await;
        let (status, body) = free_send(
            &harness,
            json!({
                "title": "A title",
                "token": "token-u1-1-0000000000000000000000000000"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_request");
        assert_eq!(harness.fcm.calls(), 0);
    }

    #[tokio::test]
    async fn free_send_applies_validation_limits() {
        let harness = setup("owner", &[]).await;

        let (status, _) = free_send(
            &harness,
            json!({ "title": "t".repeat(201), "topic": "news" }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let (status, _) = free_send(
            &harness,
            json!({ "title": "A title", "body": "b".repeat(4001), "topic": "news" }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let (status, _) = free_send(
            &harness,
            json!({ "title": "A title", "topic": "not a topic" }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let (status, _) = free_send(&harness, json!({ "title": "  ", "topic": "news" })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        assert_eq!(harness.fcm.calls(), 0);
    }

    #[tokio::test]
    async fn free_send_rate_limited_per_user() {
        let harness = setup("owner", &[]).await;
        // T13-A: the free send has its own budget of 10 per minute.
        for attempt in 0..10 {
            let (status, _) =
                free_send(&harness, json!({ "title": "A title", "topic": "news" })).await;
            assert_eq!(status, StatusCode::OK, "attempt {}", attempt);
        }

        let (status, body) =
            free_send(&harness, json!({ "title": "A title", "topic": "news" })).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(body["error"], "rate_limited");
    }

    #[tokio::test]
    async fn free_send_has_own_limiter_and_does_not_consume_topics() {
        let harness = setup("owner", &[]).await;

        // The budget of the topics routes is untouched before the free send
        // spends its own.
        assert!(!harness.state.topics_limiter.is_blocked("ip:127.0.0.1"));

        for _ in 0..10 {
            let (status, _) =
                free_send(&harness, json!({ "title": "A title", "topic": "news" })).await;
            assert_eq!(status, StatusCode::OK);
        }
        let (status, _) = free_send(&harness, json!({ "title": "A title", "topic": "news" })).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);

        // The free send spent its own budget, never the budget of the topics
        // routes. The old wiring borrowed the topics limiter with this exact
        // key, so the check below fails if the wiring comes back.
        assert!(!harness
            .state
            .topics_limiter
            .is_blocked(&format!("admin:send:{}", harness.user_id)));
        assert!(!harness.state.topics_limiter.is_blocked("ip:127.0.0.1"));
    }

    #[tokio::test]
    async fn free_send_history_row_kind_raw_ref_dashboard() {
        let harness = setup("owner", &[]).await;
        let (status, _) = free_send(
            &harness,
            json!({ "title": "A title", "body": "A body", "topic": "news" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let row: (String, Option<String>, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT kind, \"ref\", target, sender_uid FROM notifications_history \
             ORDER BY id DESC LIMIT 1",
        )
        .fetch_one(&harness.pool)
        .await
        .expect("the history row must exist");
        assert_eq!(row.0, "raw");
        assert_eq!(row.1.as_deref(), Some("dashboard"));
        assert_eq!(row.2.as_deref(), Some("topic:news"));
        assert_eq!(row.3.as_deref(), Some("user@example.com"));
    }

    #[tokio::test]
    async fn free_send_failure_is_502() {
        let (router, pool, token) = setup_failing().await;

        let (status, body) = call(
            &router,
            Method::POST,
            &format!("/apps/{}/send", APP),
            Some(json!({ "title": "A title", "topic": "news" })),
            Some(&token),
            true,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(body["status"], "failed");
        assert_eq!(body["delivered"], 0);

        let row: String =
            sqlx::query_scalar("SELECT status FROM notifications_history ORDER BY id DESC LIMIT 1")
                .fetch_one(&pool)
                .await
                .expect("the history row must exist");
        assert_eq!(row, "failed");
    }

    /// Writes one history row with its own timestamp and reference.
    async fn insert_history_at(
        pool: &SqlitePool,
        app: &str,
        kind: &str,
        status: &str,
        reference: &str,
        created_at: &str,
    ) -> i64 {
        let result = sqlx::query(
            "INSERT INTO notifications_history \
             (app, target, title, status, response, payload, sender_uid, sender_email, kind, \"ref\", created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(app)
        .bind("topic:news")
        .bind("A title")
        .bind(status)
        .bind("ok")
        .bind("{}")
        .bind("sender-uid")
        .bind("sender@example.com")
        .bind(kind)
        .bind(reference)
        .bind(created_at)
        .execute(pool)
        .await
        .expect("the history row must be inserted");
        result.last_insert_rowid()
    }

    #[tokio::test]
    async fn history_filters_by_ref_exact() {
        let harness = setup("owner", &[]).await;
        insert_history_at(
            &harness.pool,
            APP,
            "raw",
            "success",
            "ref-1",
            "2026-01-01 10:00:00",
        )
        .await;
        insert_history_at(
            &harness.pool,
            APP,
            "raw",
            "success",
            "ref-2",
            "2026-01-01 11:00:00",
        )
        .await;

        let (status, body) = harness
            .call(
                Method::GET,
                &format!("/apps/{}/history?ref=ref-2", APP),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        let rows = body.as_array().expect("an array");
        assert_eq!(rows.len(), 1, "the reference is matched exactly");
        assert_eq!(rows[0]["ref"], "ref-2");

        // A reference that is a prefix of another one matches nothing.
        let (status, body) = harness
            .call(Method::GET, &format!("/apps/{}/history?ref=ref", APP), None)
            .await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.as_array().expect("an array").is_empty());

        // A reference that is too long is refused.
        let long = "x".repeat(101);
        let (status, _) = harness
            .call(
                Method::GET,
                &format!("/apps/{}/history?ref={}", APP, long),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn history_filters_by_date_range() {
        let harness = setup("owner", &[]).await;
        insert_history_at(
            &harness.pool,
            APP,
            "raw",
            "success",
            "ref-1",
            "2026-01-01 10:00:00",
        )
        .await;
        insert_history_at(
            &harness.pool,
            APP,
            "raw",
            "success",
            "ref-2",
            "2026-02-01 10:00:00",
        )
        .await;
        insert_history_at(
            &harness.pool,
            APP,
            "raw",
            "success",
            "ref-3",
            "2026-03-01 10:00:00",
        )
        .await;

        let (status, body) = harness
            .call(
                Method::GET,
                &format!("/apps/{}/history?since=2026-02-01&until=2026-02-28", APP),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        let rows = body.as_array().expect("an array");
        assert_eq!(rows.len(), 1, "only the row of February answers");
        assert_eq!(rows[0]["ref"], "ref-2");

        // An RFC 3339 stamp works too.
        let (status, body) = harness
            .call(
                Method::GET,
                &format!("/apps/{}/history?since=2026-02-01T00:00:00Z", APP),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.as_array().expect("an array").len(), 2);
    }

    #[tokio::test]
    async fn history_rejects_bad_date_400() {
        let harness = setup("owner", &[]).await;
        insert_history(&harness.pool, APP, "raw", "success", "ok", "{}").await;

        for value in [
            "not-a-date",
            "2026-13-01",
            "2026-01-32",
            "2026-01-01T99:00:00Z",
        ] {
            let (status, _) = harness
                .call(
                    Method::GET,
                    &format!("/apps/{}/history?since={}", APP, value),
                    None,
                )
                .await;
            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "the value {} must be refused",
                value
            );
        }
    }

    #[tokio::test]
    async fn history_filters_compose_with_kind_and_status() {
        let harness = setup("owner", &[]).await;
        insert_history_at(
            &harness.pool,
            APP,
            "raw",
            "success",
            "ref-1",
            "2026-02-01 10:00:00",
        )
        .await;
        insert_history_at(
            &harness.pool,
            APP,
            "template",
            "failed",
            "ref-1",
            "2026-02-01 11:00:00",
        )
        .await;
        insert_history_at(
            &harness.pool,
            APP,
            "raw",
            "failed",
            "ref-1",
            "2026-03-01 10:00:00",
        )
        .await;

        let (status, body) = harness
            .call(
                Method::GET,
                &format!(
                    "/apps/{}/history?ref=ref-1&kind=raw&status=failed&since=2026-02-01",
                    APP
                ),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        let rows = body.as_array().expect("an array");
        assert_eq!(rows.len(), 1, "the filters answer together");
        assert_eq!(rows[0]["kind"], "raw");
        assert_eq!(rows[0]["status"], "failed");
    }

    #[tokio::test]
    async fn device_stats_counts_without_identifiers() {
        let harness = setup("owner", &[]).await;
        harness
            .register_device("u1", "token-u1-1-0000000000000000000000000000")
            .await;
        harness
            .state
            .devices
            .register(
                APP,
                "u1",
                "token-u1-2-0000000000000000000000000000",
                crate::devices::Platform::Ios,
                20,
            )
            .await
            .expect("the device must register");
        harness
            .state
            .devices
            .register(
                APP,
                "u2",
                "token-u2-1-0000000000000000000000000000",
                crate::devices::Platform::Web,
                20,
            )
            .await
            .expect("the device must register");

        let (status, body) = harness
            .call(Method::GET, &format!("/apps/{}/devices/stats", APP), None)
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["total"], 3);
        assert_eq!(body["distinct_uids"], 2);
        assert_eq!(body["by_platform"]["android"], 1);
        assert_eq!(body["by_platform"]["ios"], 1);
        assert_eq!(body["by_platform"]["web"], 1);
        assert_eq!(body["active_last_7d"], 3);
        assert!(!body["newest_last_seen"].is_null());

        let text = body.to_string();
        assert!(!text.contains("token-"), "the statistics carry no token");
        assert!(!text.contains("\"u1\""), "the statistics carry no uid");
        assert!(!text.contains("\"u2\""));
    }

    #[tokio::test]
    async fn device_stats_scoped_to_app() {
        let harness = setup("owner", &[]).await;
        harness
            .register_device("u1", "token-u1-1-0000000000000000000000000000")
            .await;
        harness
            .state
            .devices
            .register(
                OTHER_APP,
                "u2",
                "token-u2-1-0000000000000000000000000000",
                crate::devices::Platform::Android,
                20,
            )
            .await
            .expect("the device must register");

        let (status, body) = harness
            .call(Method::GET, &format!("/apps/{}/devices/stats", APP), None)
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["total"], 1, "another app is never counted");
        assert_eq!(body["distinct_uids"], 1);
    }
}

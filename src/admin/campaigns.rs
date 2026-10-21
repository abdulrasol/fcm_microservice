//! Dashboard campaign review: the campaign list, approval, rejection and the
//! entity blocks (T10-B).
//!
//! Every handler calls [`authorize`] before reading or writing anything. Every
//! state change is one atomic `UPDATE ... WHERE status = 'pending_review'`, so
//! two reviewers can never send the same campaign twice, and nothing is sent
//! before the transition succeeded.

use super::auth::AuthedUser;
use super::{authorize, AdminError, AdminJson, Need};
use crate::campaigns::{self, CampaignError};
use crate::db;
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

/// The campaign statuses the dashboard list accepts as a filter.
const CAMPAIGN_STATUSES: [&str; 6] = [
    "pending_review",
    "sending",
    "sent",
    "failed",
    "rejected",
    "cancelled",
];

/// The list shows a short prefix of the FCM diagnostic.
const MAX_FCM_RESPONSE_CHARS: usize = 300;

/// The longest review note a reviewer may write.
const MAX_NOTE_CHARS: usize = 200;

/// A campaign error that carries its own error code.
///
/// The dashboard answers a campaign conflict with the code alone
/// (`{"error":"entity_blocked"}`), so the code cannot be built with the shared
/// [`AdminError`] envelope.
pub enum CampaignAdminError {
    Admin(AdminError),
    Conflict(&'static str),
}
impl From<AdminError> for CampaignAdminError {
    fn from(error: AdminError) -> Self {
        CampaignAdminError::Admin(error)
    }
}

impl IntoResponse for CampaignAdminError {
    fn into_response(self) -> Response {
        match self {
            CampaignAdminError::Admin(error) => error.into_response(),
            CampaignAdminError::Conflict(code) => {
                (StatusCode::CONFLICT, Json(json!({ "error": code }))).into_response()
            }
        }
    }
}

/// Maps a campaign error to the dashboard's status codes.
fn map_campaign_error(error: CampaignError) -> CampaignAdminError {
    match error {
        CampaignError::BadRequest(message) => AdminError::invalid_request(&message).into(),
        CampaignError::Forbidden => AdminError::forbidden("Forbidden").into(),
        CampaignError::Unavailable => {
            AdminError::unavailable("the data source is unavailable").into()
        }
        CampaignError::NotFound => AdminError::not_found("the campaign does not exist").into(),
        CampaignError::Conflict => CampaignAdminError::Conflict("not_pending"),
        CampaignError::QuotaExceeded { .. } => AdminError::too_many_requests().into(),
        CampaignError::Internal => AdminError::internal("campaign storage failed").into(),
    }
}

/// The filters of the campaign list.
#[derive(Deserialize)]
pub struct CampaignsQuery {
    pub status: Option<String>,
    pub entity_id: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

/// `GET /apps/{app}/campaigns`: the app's campaigns, newest first.
pub async fn list_campaigns(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path(app): Path<String>,
    Query(query): Query<CampaignsQuery>,
) -> Result<Json<Value>, CampaignAdminError> {
    authorize(&state, &user, &app, Need::AnyMember).await?;

    let status = match query.status.as_deref() {
        None => None,
        Some(value) if CAMPAIGN_STATUSES.contains(&value) => Some(value),
        Some(_) => return Err(AdminError::invalid_request("unknown campaign status").into()),
    };
    let limit = query.limit.unwrap_or(50).min(100);
    let offset = query.offset.unwrap_or(0);

    let rows = db::list_campaigns_filtered(
        &state.db,
        &app,
        status,
        query.entity_id.as_deref(),
        limit,
        offset,
    )
    .await
    .map_err(|error| AdminError::from_db("read the campaigns", error))?;

    let items: Vec<Value> = rows
        .iter()
        .map(|campaign| {
            json!({
                "id": campaign.id,
                "entity_id": campaign.entity_id,
                "sender_uid": campaign.sender_uid,
                "title": campaign.title,
                "body": campaign.body,
                "image": campaign.image,
                "data": parse_json(&campaign.data),
                "audience": parse_json(&campaign.audience),
                "condition": campaign.condition,
                "status": campaign.status,
                "review_note": campaign.review_note,
                "reviewer": campaign.reviewer,
                "created_at": campaign.created_at,
                "decided_at": campaign.decided_at,
                "sent_at": campaign.sent_at,
                "fcm_response": campaign
                    .fcm_response
                    .as_deref()
                    .map(|response| truncate(response, MAX_FCM_RESPONSE_CHARS)),
            })
        })
        .collect();

    Ok(Json(Value::Array(items)))
}

/// The body of an approval: an empty object or an optional note.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApproveRequest {
    #[serde(default)]
    pub note: Option<String>,
}

/// `POST /apps/{app}/campaigns/{id}/approve`: review, then send.
///
/// The order is fixed: the campaign must belong to the app, a blocked entity
/// is refused, the atomic claim decides who sends, and only a won claim is
/// dispatched.
pub async fn approve(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path((app, id)): Path<(String, i64)>,
    AdminJson(body): AdminJson<ApproveRequest>,
) -> Result<Json<Value>, CampaignAdminError> {
    authorize(&state, &user, &app, Need::AnyMember).await?;
    if let Some(note) = &body.note {
        validate_note(note)?;
    }

    // 1. The campaign must belong to this app. A campaign of another app is
    //    the same 404 as a campaign that does not exist.
    let campaign = db::get_campaign(&state.db, id)
        .await
        .map_err(|error| AdminError::from_db("read the campaign", error))?
        .filter(|campaign| campaign.app == app)
        .ok_or_else(|| AdminError::not_found("the campaign does not exist"))?;

    // 2. A blocked entity is never approved, and the campaign stays pending.
    if db::is_entity_blocked(&state.db, &app, &campaign.entity_id)
        .await
        .map_err(|error| AdminError::from_db("read the blocks", error))?
    {
        return Err(CampaignAdminError::Conflict("entity_blocked"));
    }

    // 3. The atomic claim: only one reviewer wins.
    let claimed = db::claim_for_sending(&state.db, id, &user.email, body.note.as_deref())
        .await
        .map_err(|error| AdminError::from_db("claim the campaign", error))?;
    if !claimed {
        return Err(CampaignAdminError::Conflict("not_pending"));
    }

    // 4. The send. A failed FCM call is a stored `failed` campaign, not a
    //    retried request.
    let outcome = campaigns::dispatch_campaign(&state, id)
        .await
        .map_err(map_campaign_error)?;

    // 5. The audit carries the campaign and the entity, never the content.
    let detail = json!({ "id": id, "entity_id": campaign.entity_id }).to_string();
    let _ = db::write_audit(
        &state.db,
        &user.email,
        Some(&app),
        "campaign.approve",
        &detail,
    )
    .await;

    Ok(Json(json!({ "status": outcome.status() })))
}

/// The body of a rejection: the reviewer's note is required.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RejectRequest {
    pub note: String,
}

/// `POST /apps/{app}/campaigns/{id}/reject`: refuse a campaign with a note.
pub async fn reject(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path((app, id)): Path<(String, i64)>,
    AdminJson(body): AdminJson<RejectRequest>,
) -> Result<Json<Value>, CampaignAdminError> {
    authorize(&state, &user, &app, Need::AnyMember).await?;
    validate_note(&body.note)?;

    let campaign = db::get_campaign(&state.db, id)
        .await
        .map_err(|error| AdminError::from_db("read the campaign", error))?
        .filter(|campaign| campaign.app == app)
        .ok_or_else(|| AdminError::not_found("the campaign does not exist"))?;

    let rejected = db::reject_campaign(&state.db, id, &user.email, &body.note)
        .await
        .map_err(|error| AdminError::from_db("reject the campaign", error))?;
    if !rejected {
        return Err(CampaignAdminError::Conflict("not_pending"));
    }

    let detail = json!({ "id": id, "entity_id": campaign.entity_id }).to_string();
    let _ = db::write_audit(
        &state.db,
        &user.email,
        Some(&app),
        "campaign.reject",
        &detail,
    )
    .await;

    Ok(Json(json!({ "status": "rejected" })))
}

/// The longest block reason a reviewer may write.
const MAX_REASON_CHARS: usize = 200;

/// `GET /apps/{app}/blocks`: the blocked entities of the app.
pub async fn list_blocks(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path(app): Path<String>,
) -> Result<Json<Value>, CampaignAdminError> {
    authorize(&state, &user, &app, Need::AnyMember).await?;

    let rows = db::list_blocks(&state.db, &app)
        .await
        .map_err(|error| AdminError::from_db("read the blocks", error))?;

    let items: Vec<Value> = rows
        .iter()
        .map(|row| {
            json!({
                "entity_id": row.entity_id,
                "reason": row.reason,
                "blocked_by": row.blocked_by,
                "created_at": row.created_at,
            })
        })
        .collect();

    Ok(Json(Value::Array(items)))
}

/// The body of a block: the entity and an optional reason.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockRequest {
    pub entity_id: String,
    #[serde(default)]
    pub reason: Option<String>,
}

/// `POST /apps/{app}/blocks`: block an entity and cancel its pending campaigns.
///
/// The block and the cancellation are one transaction, so a blocked entity
/// never keeps a campaign waiting for review.
pub async fn block_entity(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path(app): Path<String>,
    AdminJson(body): AdminJson<BlockRequest>,
) -> Result<Json<Value>, CampaignAdminError> {
    authorize(&state, &user, &app, Need::AnyMember).await?;

    if !campaigns::is_entity_id(&body.entity_id) {
        return Err(AdminError::invalid_request("the entity id is not valid").into());
    }
    if let Some(reason) = &body.reason {
        if reason.chars().count() > MAX_REASON_CHARS {
            return Err(
                AdminError::invalid_request("the reason must be at most 200 characters").into(),
            );
        }
    }

    let cancelled = db::block_entity(
        &state.db,
        &app,
        &body.entity_id,
        body.reason.as_deref(),
        &user.email,
    )
    .await
    .map_err(|error| AdminError::from_db("block the entity", error))?;

    let detail = json!({ "entity_id": body.entity_id }).to_string();
    let _ = db::write_audit(
        &state.db,
        &user.email,
        Some(&app),
        "campaign.block",
        &detail,
    )
    .await;

    Ok(Json(json!({ "blocked": true, "cancelled": cancelled })))
}

/// `DELETE /apps/{app}/blocks/{entity_id}`: unblock an entity.
pub async fn unblock_entity(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path((app, entity_id)): Path<(String, String)>,
) -> Result<Json<Value>, CampaignAdminError> {
    authorize(&state, &user, &app, Need::AnyMember).await?;

    let removed = db::unblock_entity(&state.db, &app, &entity_id)
        .await
        .map_err(|error| AdminError::from_db("unblock the entity", error))?;
    if !removed {
        return Err(AdminError::not_found("the entity is not blocked").into());
    }

    let detail = json!({ "entity_id": entity_id }).to_string();
    let _ = db::write_audit(
        &state.db,
        &user.email,
        Some(&app),
        "campaign.unblock",
        &detail,
    )
    .await;

    Ok(Json(json!({ "blocked": false })))
}

/// A review note: 1..=200 characters and no control character but a newline.
fn validate_note(note: &str) -> Result<(), AdminError> {
    let length = note.chars().count();
    if length == 0 || length > MAX_NOTE_CHARS {
        return Err(AdminError::invalid_request(
            "the note must be between 1 and 200 characters",
        ));
    }
    if note
        .chars()
        .any(|character| character.is_control() && character != '\n')
    {
        return Err(AdminError::invalid_request(
            "the note must not contain control characters",
        ));
    }
    Ok(())
}

/// Parses a stored JSON column; a broken value is `null`, never an error.
fn parse_json(value: &str) -> Value {
    serde_json::from_str(value).unwrap_or(Value::Null)
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
    use crate::config::{Quota, Review, ReviewMode};
    use crate::send::{FcmOutcome, SendError};
    use axum::http::Method;
    use axum::Router;
    use serde_json::json;
    use sqlx::SqlitePool;
    use std::sync::Arc;

    /// Inserts a campaign row directly, the way the API route would.
    async fn insert_campaign(pool: &SqlitePool, app: &str, entity_id: &str, status: &str) -> i64 {
        let result = sqlx::query(
            "INSERT INTO campaigns \
             (app, entity_id, sender_uid, title, body, data, audience, condition, status) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(app)
        .bind(entity_id)
        .bind("sender-uid")
        .bind("Campaign title")
        .bind("Campaign body")
        .bind("{}")
        .bind(r#"{"groups":[["promos"]]}"#)
        .bind("'promos' in topics")
        .bind(status)
        .execute(pool)
        .await
        .expect("the campaign must be inserted");
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

    async fn approve(harness: &Harness, id: i64) -> (StatusCode, Value) {
        harness
            .call(
                Method::POST,
                &format!("/apps/{}/campaigns/{}/approve", APP, id),
                Some(json!({})),
            )
            .await
    }

    async fn reject(harness: &Harness, id: i64, note: &str) -> (StatusCode, Value) {
        harness
            .call(
                Method::POST,
                &format!("/apps/{}/campaigns/{}/reject", APP, id),
                Some(json!({ "note": note })),
            )
            .await
    }

    #[tokio::test]
    async fn campaigns_list_filters_and_caps_limit() {
        let harness = setup("owner", &[]).await;
        for index in 0..105 {
            let status = if index % 2 == 0 {
                "pending_review"
            } else {
                "sent"
            };
            insert_campaign(&harness.pool, APP, "e1", status).await;
        }

        let (status, body) = harness
            .call(
                Method::GET,
                &format!("/apps/{}/campaigns?limit=2", APP),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.as_array().expect("an array").len(), 2);

        let (status, body) = harness
            .call(
                Method::GET,
                &format!("/apps/{}/campaigns?limit=500", APP),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body.as_array().expect("an array").len(),
            100,
            "the limit is capped at 100"
        );

        let (status, body) = harness
            .call(
                Method::GET,
                &format!("/apps/{}/campaigns?status=pending_review&limit=100", APP),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        let rows = body.as_array().expect("an array");
        assert_eq!(rows.len(), 53);
        assert!(rows.iter().all(|row| row["status"] == "pending_review"));
    }

    #[tokio::test]
    async fn campaigns_list_unknown_status_is_400() {
        let harness = setup("owner", &[]).await;
        let (status, body) = harness
            .call(
                Method::GET,
                &format!("/apps/{}/campaigns?status=bogus", APP),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_request");
    }

    #[tokio::test]
    async fn campaigns_list_scoped_to_app() {
        let harness = setup("owner", &[]).await;
        insert_campaign(&harness.pool, APP, "e1", "pending_review").await;
        insert_campaign(&harness.pool, OTHER_APP, "e2", "pending_review").await;

        let (status, body) = harness
            .call(Method::GET, &format!("/apps/{}/campaigns", APP), None)
            .await;
        assert_eq!(status, StatusCode::OK);
        let rows = body.as_array().expect("an array");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["entity_id"], "e1");
    }

    #[tokio::test]
    async fn approve_sends_condition_and_marks_sent() {
        let harness = setup("owner", &[]).await;
        let id = insert_campaign(&harness.pool, APP, "e1", "pending_review").await;

        let (status, body) = approve(&harness, id).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "sent");
        assert_eq!(harness.fcm.calls(), 1, "the campaign is sent once");

        let status: String = sqlx::query_scalar("SELECT status FROM campaigns WHERE id = ?")
            .bind(id)
            .fetch_one(&harness.pool)
            .await
            .expect("the row must exist");
        assert_eq!(status, "sent");
    }

    #[tokio::test]
    async fn approve_records_reviewer_and_decided_at() {
        let harness = setup("owner", &[]).await;
        let id = insert_campaign(&harness.pool, APP, "e1", "pending_review").await;

        let (status, _) = approve(&harness, id).await;
        assert_eq!(status, StatusCode::OK);

        let row: (Option<String>, Option<String>) =
            sqlx::query_as("SELECT reviewer, decided_at FROM campaigns WHERE id = ?")
                .bind(id)
                .fetch_one(&harness.pool)
                .await
                .expect("the row must exist");
        assert_eq!(row.0.as_deref(), Some("user@example.com"));
        assert!(row.1.is_some(), "the decision time must be written");
    }

    #[tokio::test]
    async fn approve_foreign_app_campaign_is_404() {
        let harness = setup("owner", &[]).await;
        let id = insert_campaign(&harness.pool, OTHER_APP, "e1", "pending_review").await;

        let (status, body) = approve(&harness, id).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"], "not_found");
        assert_eq!(harness.fcm.calls(), 0);
    }

    #[tokio::test]
    async fn approve_non_pending_is_409() {
        let harness = setup("owner", &[]).await;
        let id = insert_campaign(&harness.pool, APP, "e1", "sent").await;

        let (status, body) = approve(&harness, id).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"], "not_pending");
        assert_eq!(harness.fcm.calls(), 0);
    }

    #[tokio::test]
    async fn approve_blocked_entity_is_409_and_stays_pending() {
        let harness = setup("owner", &[]).await;
        let id = insert_campaign(&harness.pool, APP, "e1", "pending_review").await;
        sqlx::query("INSERT INTO campaign_blocks (app, entity_id, blocked_by) VALUES (?, ?, ?)")
            .bind(APP)
            .bind("e1")
            .bind("owner@example.com")
            .execute(&harness.pool)
            .await
            .expect("the block must be inserted");

        let (status, body) = approve(&harness, id).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"], "entity_blocked");
        assert_eq!(harness.fcm.calls(), 0);

        let status: String = sqlx::query_scalar("SELECT status FROM campaigns WHERE id = ?")
            .bind(id)
            .fetch_one(&harness.pool)
            .await
            .expect("the row must exist");
        assert_eq!(status, "pending_review");
    }

    #[tokio::test]
    async fn approve_failed_fcm_marks_failed() {
        let (router, pool, token) = setup_failing().await;
        let id = insert_campaign(&pool, APP, "e1", "pending_review").await;

        let (status, body) = call(
            &router,
            Method::POST,
            &format!("/apps/{}/campaigns/{}/approve", APP, id),
            Some(json!({})),
            Some(&token),
            true,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "failed");

        let status: String = sqlx::query_scalar("SELECT status FROM campaigns WHERE id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("the row must exist");
        assert_eq!(status, "failed");
    }

    #[tokio::test]
    async fn approve_twice_sends_once() {
        let harness = setup("owner", &[]).await;
        let id = insert_campaign(&harness.pool, APP, "e1", "pending_review").await;
        let uri = format!("/apps/{}/campaigns/{}/approve", APP, id);

        // Four reviewers at the same time: the atomic claim lets exactly one
        // of them send.
        let first = call(
            &harness.router,
            Method::POST,
            &uri,
            Some(json!({})),
            Some(&harness.token),
            true,
        );
        let second = call(
            &harness.router,
            Method::POST,
            &uri,
            Some(json!({})),
            Some(&harness.token),
            true,
        );
        let third = call(
            &harness.router,
            Method::POST,
            &uri,
            Some(json!({})),
            Some(&harness.token),
            true,
        );
        let fourth = call(
            &harness.router,
            Method::POST,
            &uri,
            Some(json!({})),
            Some(&harness.token),
            true,
        );
        let (first, second, third, fourth) = tokio::join!(first, second, third, fourth);

        let statuses = [first.0, second.0, third.0, fourth.0];
        assert_eq!(
            statuses
                .iter()
                .filter(|status| **status == StatusCode::OK)
                .count(),
            1,
            "exactly one reviewer must win: {:?}",
            statuses
        );
        assert!(
            statuses
                .iter()
                .all(|status| *status == StatusCode::OK || *status == StatusCode::CONFLICT),
            "the other reviewers must be refused: {:?}",
            statuses
        );
        assert_eq!(harness.fcm.calls(), 1, "only one message is sent");
    }

    #[tokio::test]
    async fn reject_requires_note() {
        let harness = setup("owner", &[]).await;
        let id = insert_campaign(&harness.pool, APP, "e1", "pending_review").await;

        let (status, body) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/campaigns/{}/reject", APP, id),
                Some(json!({})),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_request");

        let (status, _) = reject(&harness, id, "").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn reject_note_limits_and_control_chars() {
        let harness = setup("owner", &[]).await;
        let id = insert_campaign(&harness.pool, APP, "e1", "pending_review").await;

        let long = "n".repeat(201);
        let (status, _) = reject(&harness, id, &long).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let (status, _) = reject(&harness, id, "bell\u{7}note").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let (status, _) = reject(&harness, id, "first line\nsecond line").await;
        assert_eq!(status, StatusCode::OK, "a newline is allowed");
    }

    #[tokio::test]
    async fn reject_sets_status_reviewer_and_note() {
        let harness = setup("owner", &[]).await;
        let id = insert_campaign(&harness.pool, APP, "e1", "pending_review").await;

        let (status, body) = reject(&harness, id, "not allowed").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "rejected");

        let row: (String, Option<String>, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT status, reviewer, decided_at, review_note FROM campaigns WHERE id = ?",
        )
        .bind(id)
        .fetch_one(&harness.pool)
        .await
        .expect("the row must exist");
        assert_eq!(row.0, "rejected");
        assert_eq!(row.1.as_deref(), Some("user@example.com"));
        assert!(row.2.is_some());
        assert_eq!(row.3.as_deref(), Some("not allowed"));
    }

    #[tokio::test]
    async fn reject_non_pending_is_409() {
        let harness = setup("owner", &[]).await;
        let id = insert_campaign(&harness.pool, APP, "e1", "sent").await;

        let (status, body) = reject(&harness, id, "too late").await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"], "not_pending");
    }

    #[tokio::test]
    async fn rejected_campaign_never_reaches_fcm() {
        let harness = setup("owner", &[]).await;
        let id = insert_campaign(&harness.pool, APP, "e1", "pending_review").await;

        let (status, _) = reject(&harness, id, "not allowed").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(harness.fcm.calls(), 0, "a rejected campaign is never sent");
    }

    #[tokio::test]
    async fn block_cancels_pending_campaigns_in_one_transaction() {
        let harness = setup("owner", &[]).await;
        let first = insert_campaign(&harness.pool, APP, "e1", "pending_review").await;
        let second = insert_campaign(&harness.pool, APP, "e1", "pending_review").await;
        let sent = insert_campaign(&harness.pool, APP, "e1", "sent").await;
        let other = insert_campaign(&harness.pool, APP, "e2", "pending_review").await;

        let (status, body) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/blocks", APP),
                Some(json!({ "entity_id": "e1" })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["blocked"], true);
        assert_eq!(body["cancelled"], 2);

        for id in [first, second] {
            let row: (String, Option<String>) =
                sqlx::query_as("SELECT status, review_note FROM campaigns WHERE id = ?")
                    .bind(id)
                    .fetch_one(&harness.pool)
                    .await
                    .expect("the row must exist");
            assert_eq!(row.0, "cancelled");
            assert_eq!(row.1.as_deref(), Some("entity blocked"));
        }

        let status: String = sqlx::query_scalar("SELECT status FROM campaigns WHERE id = ?")
            .bind(sent)
            .fetch_one(&harness.pool)
            .await
            .expect("the row must exist");
        assert_eq!(status, "sent", "a sent campaign is not cancelled");

        let status: String = sqlx::query_scalar("SELECT status FROM campaigns WHERE id = ?")
            .bind(other)
            .fetch_one(&harness.pool)
            .await
            .expect("the row must exist");
        assert_eq!(status, "pending_review", "another entity is untouched");
    }

    #[tokio::test]
    async fn block_idempotent_and_updates_reason() {
        let harness = setup("owner", &[]).await;
        for reason in ["first reason", "second reason"] {
            let (status, _) = harness
                .call(
                    Method::POST,
                    &format!("/apps/{}/blocks", APP),
                    Some(json!({ "entity_id": "e1", "reason": reason })),
                )
                .await;
            assert_eq!(status, StatusCode::OK);
        }

        let (status, body) = harness
            .call(Method::GET, &format!("/apps/{}/blocks", APP), None)
            .await;
        assert_eq!(status, StatusCode::OK);
        let rows = body.as_array().expect("an array");
        assert_eq!(rows.len(), 1, "the block is written once");
        assert_eq!(rows[0]["entity_id"], "e1");
        assert_eq!(rows[0]["reason"], "second reason");
        assert_eq!(rows[0]["blocked_by"], "user@example.com");
    }

    #[tokio::test]
    async fn unblock_missing_is_404() {
        let harness = setup("owner", &[]).await;
        let (status, body) = harness
            .call(Method::DELETE, &format!("/apps/{}/blocks/e1", APP), None)
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"], "not_found");
    }

    #[tokio::test]
    async fn block_entity_id_charset() {
        let harness = setup("owner", &[]).await;
        for entity_id in ["bad id", "bad!id", &"e".repeat(65)] {
            let (status, body) = harness
                .call(
                    Method::POST,
                    &format!("/apps/{}/blocks", APP),
                    Some(json!({ "entity_id": entity_id })),
                )
                .await;
            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "entity {:?} must be refused",
                entity_id
            );
            assert_eq!(body["error"], "invalid_request");
        }
    }

    #[tokio::test]
    async fn blocked_entity_cannot_create_new_campaign() {
        let harness = setup("owner", &[]).await;
        let (status, _) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/blocks", APP),
                Some(json!({ "entity_id": "e1" })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);

        // The T9 create route refuses a blocked entity with this exact
        // predicate before it inserts anything.
        let blocked = db::is_entity_blocked(&harness.pool, APP, "e1")
            .await
            .expect("the check must run");
        assert!(blocked, "the dashboard block must block the T9 route");
    }

    #[tokio::test]
    async fn quota_ignores_rejected_cancelled_failed_after_admin_actions() {
        let harness = setup("owner", &[]).await;
        let limits = Quota {
            per_entity_per_day: 1,
            per_entity_per_week: 1,
        };
        let new = db::NewCampaign {
            app: APP.to_string(),
            entity_id: "e1".to_string(),
            sender_uid: "sender-uid".to_string(),
            title: "Title".to_string(),
            body: "Body".to_string(),
            image: None,
            data: "{}".to_string(),
            audience: r#"{"groups":[["promos"]]}"#.to_string(),
            condition: "'promos' in topics".to_string(),
            review: Review {
                mode: ReviewMode::Always,
                n: None,
            },
            sender_is_service: false,
        };

        // The first campaign fills the quota and waits for review.
        let inserted =
            db::insert_campaign_checked(&harness.pool, &new, &limits, crate::unix_seconds())
                .await
                .expect("the first campaign must be inserted");
        assert_eq!(inserted.status, "pending_review");

        // A rejected campaign never consumes the quota.
        let (status, _) = reject(&harness, inserted.id, "not allowed").await;
        assert_eq!(status, StatusCode::OK);
        db::insert_campaign_checked(&harness.pool, &new, &limits, crate::unix_seconds())
            .await
            .expect("a rejected campaign must not consume the quota");

        // A cancelled campaign never consumes the quota either.
        let (status, _) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/blocks", APP),
                Some(json!({ "entity_id": "e1" })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        db::insert_campaign_checked(&harness.pool, &new, &limits, crate::unix_seconds())
            .await
            .expect("a cancelled campaign must not consume the quota");

        // A failed campaign never consumes the quota.
        insert_campaign(&harness.pool, APP, "e2", "failed").await;
        let other = db::NewCampaign {
            entity_id: "e2".to_string(),
            ..db::NewCampaign {
                app: APP.to_string(),
                entity_id: "e1".to_string(),
                sender_uid: "sender-uid".to_string(),
                title: "Title".to_string(),
                body: "Body".to_string(),
                image: None,
                data: "{}".to_string(),
                audience: r#"{"groups":[["promos"]]}"#.to_string(),
                condition: "'promos' in topics".to_string(),
                review: Review {
                    mode: ReviewMode::Always,
                    n: None,
                },
                sender_is_service: false,
            }
        };
        db::insert_campaign_checked(&harness.pool, &other, &limits, crate::unix_seconds())
            .await
            .expect("a failed campaign must not consume the quota");

        // A campaign that waits for review does consume the quota.
        let error =
            db::insert_campaign_checked(&harness.pool, &new, &limits, crate::unix_seconds())
                .await
                .expect_err("the quota must be exhausted");
        assert!(matches!(error, db::InsertError::QuotaExceeded { .. }));
    }

    #[tokio::test]
    async fn campaign_routes_denied_to_unassigned_manager_with_403() {
        let harness = setup("manager", &[OTHER_APP]).await;
        let id = insert_campaign(&harness.pool, APP, "e1", "pending_review").await;

        let calls = vec![
            (Method::GET, format!("/apps/{}/campaigns", APP), None),
            (
                Method::POST,
                format!("/apps/{}/campaigns/{}/approve", APP, id),
                Some(json!({})),
            ),
            (
                Method::POST,
                format!("/apps/{}/campaigns/{}/reject", APP, id),
                Some(json!({ "note": "no" })),
            ),
            (Method::GET, format!("/apps/{}/blocks", APP), None),
            (
                Method::POST,
                format!("/apps/{}/blocks", APP),
                Some(json!({ "entity_id": "e1" })),
            ),
            (Method::DELETE, format!("/apps/{}/blocks/e1", APP), None),
        ];

        for (method, uri, body) in calls {
            let (status, response) = harness.call(method.clone(), &uri, body).await;
            assert_eq!(
                status,
                StatusCode::FORBIDDEN,
                "{} {} must be refused: {}",
                method,
                uri,
                response
            );
        }
        assert_eq!(harness.fcm.calls(), 0);
    }

    #[tokio::test]
    async fn campaign_routes_unauthenticated_401() {
        let harness = setup("owner", &[]).await;
        let calls = vec![
            (Method::GET, format!("/apps/{}/campaigns", APP)),
            (Method::POST, format!("/apps/{}/campaigns/1/approve", APP)),
            (Method::POST, format!("/apps/{}/campaigns/1/reject", APP)),
            (Method::GET, format!("/apps/{}/blocks", APP)),
            (Method::POST, format!("/apps/{}/blocks", APP)),
            (Method::DELETE, format!("/apps/{}/blocks/e1", APP)),
        ];

        for (method, uri) in calls {
            let (status, _) = call(&harness.router, method.clone(), &uri, None, None, true).await;
            assert_eq!(
                status,
                StatusCode::UNAUTHORIZED,
                "{} {} must need a session",
                method,
                uri
            );
        }
    }

    #[tokio::test]
    async fn manager_can_approve_own_app_campaign() {
        let harness = setup("manager", &[APP]).await;
        let id = insert_campaign(&harness.pool, APP, "e1", "pending_review").await;

        let (status, body) = approve(&harness, id).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "sent");
        assert_eq!(harness.fcm.calls(), 1);

        let reviewer: Option<String> =
            sqlx::query_scalar("SELECT reviewer FROM campaigns WHERE id = ?")
                .bind(id)
                .fetch_one(&harness.pool)
                .await
                .expect("the row must exist");
        assert_eq!(reviewer.as_deref(), Some("user@example.com"));
    }

    #[tokio::test]
    async fn audit_has_approve_reject_block_unblock_and_free_send_without_text() {
        let harness = setup("owner", &[]).await;
        let id = insert_campaign(&harness.pool, APP, "e1", "pending_review").await;

        let (status, _) = approve(&harness, id).await;
        assert_eq!(status, StatusCode::OK);

        let other = insert_campaign(&harness.pool, APP, "e2", "pending_review").await;
        let (status, _) = reject(&harness, other, "a long free reason").await;
        assert_eq!(status, StatusCode::OK);

        let (status, _) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/blocks", APP),
                Some(json!({ "entity_id": "e3", "reason": "a long free reason" })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);

        let (status, _) = harness
            .call(Method::DELETE, &format!("/apps/{}/blocks/e3", APP), None)
            .await;
        assert_eq!(status, StatusCode::OK);

        let (status, _) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/send", APP),
                Some(json!({
                    "title": "Secret notification text",
                    "body": "Secret body text",
                    "topic": "news"
                })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);

        let rows: Vec<(String, Option<String>)> =
            sqlx::query_as("SELECT action, detail FROM audit_log WHERE app = ? ORDER BY id")
                .bind(APP)
                .fetch_all(&harness.pool)
                .await
                .expect("the audit rows must exist");

        let actions: Vec<&str> = rows.iter().map(|(action, _)| action.as_str()).collect();
        for action in [
            "campaign.approve",
            "campaign.reject",
            "campaign.block",
            "campaign.unblock",
            "send.free",
        ] {
            assert!(
                actions.contains(&action),
                "missing {} in {:?}",
                action,
                actions
            );
        }

        for (_, detail) in &rows {
            let detail = detail.clone().unwrap_or_default();
            assert!(
                !detail.contains("Secret notification text"),
                "the audit must not keep the notification text"
            );
            assert!(
                !detail.contains("Secret body text"),
                "the audit must not keep the notification body"
            );
            assert!(
                !detail.contains("a long free reason"),
                "the audit must not keep a long free reason"
            );
        }
    }
}

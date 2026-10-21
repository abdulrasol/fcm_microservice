//! Dashboard template CRUD, validation, preview and test-send (T10-A).
//!
//! Preview and test-send use the exact send engine (`validate_params`,
//! `context_path`, `render`). Preview never calls FCM; test-send only reaches
//! the devices of the caller's `test_uid`.

use super::auth::AuthedUser;
use super::{authorize, map_config_error, AdminError, AdminJson, Need};
use crate::config::store::AppSnapshot;
use crate::config::validate::{validate_template, Issue};
use crate::config::Template;
use crate::datasource::DataError;
use crate::db;
use crate::devices::DeviceError;
use crate::send;
use crate::template::{self, RenderError, Rendered, ResolvedTarget, Sender};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

const MAX_TEMPLATE_KEY_LEN: usize = 40;
/// Test-send is limited to 10 messages per dashboard user per minute.
const TEST_SEND_KEY_PREFIX: &str = "admin:test_send:";

fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_TEMPLATE_KEY_LEN
        && key
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn snapshot_of(state: &AppState, app: &str) -> Result<Arc<AppSnapshot>, AdminError> {
    state
        .config
        .get(app)
        .ok_or_else(|| AdminError::not_found("the app has no configuration"))
}

async fn metadata(
    state: &AppState,
    app: &str,
) -> Result<HashMap<String, db::TemplateMeta>, AdminError> {
    let rows = db::template_metadata(&state.db, app)
        .await
        .map_err(|error| AdminError::from_db("read the template metadata", error))?;
    Ok(rows.into_iter().map(|row| (row.key.clone(), row)).collect())
}

fn key_error() -> AdminError {
    AdminError::invalid_request("a template key must match [a-z0-9_]{1,40}")
}

/// `GET /apps/{app}/templates`.
pub async fn list_templates(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path(app): Path<String>,
) -> Result<Json<Value>, AdminError> {
    authorize(&state, &user, &app, Need::AnyMember).await?;
    let snapshot = snapshot_of(&state, &app)?;
    let metadata = metadata(&state, &app).await?;

    let items: Vec<Value> = snapshot
        .templates
        .iter()
        .map(|(key, template)| {
            let meta = metadata.get(key);
            json!({
                "key": key,
                "description": template.description,
                "enabled": template.enabled,
                "version": meta.map(|meta| meta.version).unwrap_or(0),
                "senders": template.senders,
                "target_kind": template.to.kind(),
                "last_used_at": meta.and_then(|meta| meta.last_used_at.clone()),
            })
        })
        .collect();
    Ok(Json(Value::Array(items)))
}

/// `GET /apps/{app}/templates/{key}`.
pub async fn get_template(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path((app, key)): Path<(String, String)>,
) -> Result<Json<Value>, AdminError> {
    authorize(&state, &user, &app, Need::AnyMember).await?;
    let snapshot = snapshot_of(&state, &app)?;
    let template = snapshot
        .templates
        .get(&key)
        .ok_or_else(|| AdminError::not_found("the template does not exist"))?;
    let metadata = metadata(&state, &app).await?;
    let meta = metadata.get(&key);
    let definition = serde_json::to_value(template)
        .map_err(|_| AdminError::internal("the template could not be rendered"))?;

    Ok(Json(json!({
        "key": key,
        "version": meta.map(|meta| meta.version).unwrap_or(0),
        "enabled": template.enabled,
        "last_used_at": meta.and_then(|meta| meta.last_used_at.clone()),
        "definition": definition,
    })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateTemplateRequest {
    pub key: String,
    pub definition: Value,
    pub enabled: Option<bool>,
}

/// `POST /apps/{app}/templates`.
pub async fn create_template(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path(app): Path<String>,
    AdminJson(request): AdminJson<CreateTemplateRequest>,
) -> Result<(StatusCode, Json<Value>), AdminError> {
    authorize(&state, &user, &app, Need::AnyMember).await?;
    let snapshot = snapshot_of(&state, &app)?;
    if !valid_key(&request.key) {
        return Err(key_error());
    }
    if snapshot.templates.contains_key(&request.key) {
        let metadata = metadata(&state, &app).await?;
        let current = metadata
            .get(&request.key)
            .map(|meta| meta.version)
            .unwrap_or(0);
        return Err(AdminError::conflict(current));
    }

    let mut definition = request.definition;
    if let Some(enabled) = request.enabled {
        let object = definition
            .as_object_mut()
            .ok_or_else(|| AdminError::invalid_request("definition must be an object"))?;
        object.insert("enabled".to_string(), Value::Bool(enabled));
    }

    let version = state
        .config
        .save_template(&app, &request.key, definition, &user.email, None)
        .await
        .map_err(map_config_error)?;
    Ok((StatusCode::CREATED, Json(json!({ "version": version }))))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateTemplateRequest {
    pub definition: Value,
    pub expected_version: i64,
}

/// `PUT /apps/{app}/templates/{key}`.
pub async fn update_template(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path((app, key)): Path<(String, String)>,
    AdminJson(request): AdminJson<UpdateTemplateRequest>,
) -> Result<Json<Value>, AdminError> {
    authorize(&state, &user, &app, Need::AnyMember).await?;
    let snapshot = snapshot_of(&state, &app)?;
    if !snapshot.templates.contains_key(&key) {
        return Err(AdminError::not_found("the template does not exist"));
    }

    let version = state
        .config
        .save_template(
            &app,
            &key,
            request.definition,
            &user.email,
            Some(request.expected_version),
        )
        .await
        .map_err(map_config_error)?;
    Ok(Json(json!({ "version": version })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnabledRequest {
    pub enabled: bool,
}

/// `PUT /apps/{app}/templates/{key}/enabled`.
pub async fn set_enabled(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path((app, key)): Path<(String, String)>,
    AdminJson(request): AdminJson<EnabledRequest>,
) -> Result<StatusCode, AdminError> {
    authorize(&state, &user, &app, Need::AnyMember).await?;
    let snapshot = snapshot_of(&state, &app)?;
    if !snapshot.templates.contains_key(&key) {
        return Err(AdminError::not_found("the template does not exist"));
    }

    state
        .config
        .set_template_enabled(&app, &key, request.enabled, &user.email)
        .await
        .map_err(map_config_error)?;
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /apps/{app}/templates/{key}`.
pub async fn delete_template(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path((app, key)): Path<(String, String)>,
) -> Result<StatusCode, AdminError> {
    authorize(&state, &user, &app, Need::AnyMember).await?;
    let snapshot = snapshot_of(&state, &app)?;
    if !snapshot.templates.contains_key(&key) {
        return Err(AdminError::not_found("the template does not exist"));
    }
    let metadata = metadata(&state, &app).await?;
    if metadata.get(&key).is_some_and(|meta| meta.recently_used) {
        return Err(AdminError::recently_used());
    }

    state
        .config
        .delete_template(&app, &key, &user.email)
        .await
        .map_err(map_config_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidateRequest {
    pub key: Option<String>,
    pub definition: Value,
}

fn issue_json(issues: Vec<Issue>) -> Value {
    Value::Array(
        issues
            .into_iter()
            .map(|issue| json!({ "path": issue.path, "message": issue.message }))
            .collect(),
    )
}

/// `POST /apps/{app}/templates/validate`: a dry run, nothing is saved.
pub async fn validate_definition(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path(app): Path<String>,
    AdminJson(request): AdminJson<ValidateRequest>,
) -> Result<Json<Value>, AdminError> {
    authorize(&state, &user, &app, Need::AnyMember).await?;
    let snapshot = snapshot_of(&state, &app)?;
    let key = request.key.unwrap_or_else(|| "draft".to_string());

    let template: Template = match serde_json::from_value(request.definition) {
        Ok(template) => template,
        Err(error) => {
            let issues = vec![Issue::new(
                format!("templates.{}", key),
                format!("[S1] the template could not be read: {}", error),
            )];
            return Ok(Json(json!({ "ok": false, "issues": issue_json(issues) })));
        }
    };

    let issues = validate_template(&key, &template, &snapshot.config);
    let ok = issues.is_empty();
    Ok(Json(json!({ "ok": ok, "issues": issue_json(issues) })))
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct SenderBody {
    pub uid: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewRequest {
    #[serde(default)]
    pub params: Value,
    pub sender: SenderBody,
    #[serde(default)]
    pub doc: Option<Value>,
    #[serde(default)]
    pub definition: Option<Value>,
    #[serde(default)]
    pub use_real_doc: bool,
}

enum Prepared {
    Rendered(Rendered),
    Forbidden,
}

/// Shared resolution of preview and test-send.
async fn prepare(
    state: &Arc<AppState>,
    app: &str,
    key: &str,
    request: &PreviewRequest,
) -> Result<Prepared, AdminError> {
    let snapshot = snapshot_of(state, app)?;

    let template = match &request.definition {
        Some(definition) => {
            let template: Template =
                serde_json::from_value(definition.clone()).map_err(|error| {
                    AdminError::invalid(vec![Issue::new(
                        format!("templates.{}", key),
                        format!("[S1] the template could not be read: {}", error),
                    )])
                })?;
            let issues = validate_template(key, &template, &snapshot.config);
            if !issues.is_empty() {
                return Err(AdminError::invalid(issues));
            }
            template
        }
        None => snapshot
            .templates
            .get(key)
            .cloned()
            .ok_or_else(|| AdminError::not_found("the template does not exist"))?,
    };

    let sender = Sender {
        uid: request.sender.uid.clone(),
        name: request.sender.name.clone(),
        role: request.sender.role.clone(),
    };
    let params = template::validate_params(&template, &request.params).map_err(render_error)?;

    let doc = match (&request.doc, request.use_real_doc) {
        (Some(doc), _) => Some(doc.clone()),
        (None, true) => {
            let path = template::context_path(&template, &params, &sender).map_err(render_error)?;
            match path {
                Some(path) => {
                    let source =
                        state.datasource.source_for(app).await.map_err(|_| {
                            AdminError::unavailable("the data source is unavailable")
                        })?;
                    match source.get_doc(&path).await {
                        Ok(doc) => doc,
                        Err(DataError::Unavailable(_)) => {
                            return Err(AdminError::unavailable("the data source is unavailable"))
                        }
                        Err(DataError::InvalidPath(message)) => {
                            return Err(AdminError::invalid_request(&message))
                        }
                    }
                }
                None => None,
            }
        }
        (None, false) => None,
    };

    match template::render(&template, &params, &sender, doc.as_ref(), None) {
        Ok(rendered) => Ok(Prepared::Rendered(rendered)),
        Err(RenderError::Forbidden) => Ok(Prepared::Forbidden),
        Err(RenderError::BadRequest(message)) => Err(AdminError::invalid_request(&message)),
    }
}

fn render_error(error: RenderError) -> AdminError {
    match error {
        RenderError::BadRequest(message) => AdminError::invalid_request(&message),
        RenderError::Forbidden => AdminError::forbidden("Forbidden"),
    }
}

fn device_error(error: DeviceError) -> AdminError {
    match error {
        DeviceError::Database(error) => AdminError::from_db("read the devices", error),
        DeviceError::InvalidToken => AdminError::internal("a stored device token is invalid"),
        DeviceError::InvalidRoleValue => AdminError::invalid_request("the role value is invalid"),
        DeviceError::NotConfigured => {
            AdminError::invalid_request("role targets require identity.role_members")
        }
        DeviceError::Unavailable(_) => AdminError::unavailable("the data source is unavailable"),
    }
}

async fn recipient_count(
    state: &Arc<AppState>,
    app: &str,
    snapshot: &AppSnapshot,
    target: &ResolvedTarget,
) -> Result<Option<usize>, AdminError> {
    match target {
        ResolvedTarget::Uid(uid) => {
            let tokens = state
                .devices
                .tokens_for_uid(app, uid)
                .await
                .map_err(device_error)?;
            Ok(Some(tokens.len()))
        }
        ResolvedTarget::Role(role) => {
            let Some(role_members) = snapshot.config.identity.role_members.as_ref() else {
                // The app cannot enumerate the role; the count is unknown.
                return Ok(None);
            };
            let source = state
                .datasource
                .source_for(app)
                .await
                .map_err(|_| AdminError::unavailable("the data source is unavailable"))?;
            let uids = state
                .role_directory
                .uids_for_role(app, snapshot.version, role_members, role, source.as_ref())
                .await
                .map_err(device_error)?;
            Ok(Some(uids.len()))
        }
        ResolvedTarget::Topic(_) | ResolvedTarget::Condition(_) => Ok(None),
    }
}

/// `POST /apps/{app}/templates/{key}/preview`.
pub async fn preview(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path((app, key)): Path<(String, String)>,
    AdminJson(request): AdminJson<PreviewRequest>,
) -> Result<Json<Value>, AdminError> {
    authorize(&state, &user, &app, Need::AnyMember).await?;
    let snapshot = snapshot_of(&state, &app)?;

    let rendered = match prepare(&state, &app, &key, &request).await? {
        Prepared::Forbidden => {
            return Ok(Json(json!({ "rendered": false, "reason": "forbidden" })));
        }
        Prepared::Rendered(rendered) => rendered,
    };

    let kind = match &rendered.target {
        ResolvedTarget::Uid(_) => "uid",
        ResolvedTarget::Role(_) => "role",
        ResolvedTarget::Topic(_) => "topic",
        ResolvedTarget::Condition(_) => "condition",
    };
    let value = match &rendered.target {
        ResolvedTarget::Uid(value)
        | ResolvedTarget::Role(value)
        | ResolvedTarget::Topic(value)
        | ResolvedTarget::Condition(value) => value.as_str(),
    };
    let count = recipient_count(&state, &app, &snapshot, &rendered.target).await?;

    Ok(Json(json!({
        "title": rendered.title,
        "body": rendered.body,
        "image": rendered.image,
        "data": rendered.data,
        "target": { "kind": kind, "value": value },
        "dedupe_key": rendered.dedupe_key,
        "recipients": { "kind": kind, "count": count },
    })))
}

/// `POST /apps/{app}/templates/{key}/test-send`.
pub async fn test_send(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path((app, key)): Path<(String, String)>,
    AdminJson(request): AdminJson<PreviewRequest>,
) -> Result<Json<Value>, AdminError> {
    authorize(&state, &user, &app, Need::AnyMember).await?;
    snapshot_of(&state, &app)?;

    let test_uid = super::auth::load_test_uid(&state.tool_auth.pool, user.user_id)
        .await
        .map_err(|error| AdminError::from_db("read the test uid", error))?
        .ok_or_else(|| AdminError::invalid_request("Set a test_uid on your account first"))?;

    if !state
        .auth_fail_limiter
        .check(&format!("{}{}", TEST_SEND_KEY_PREFIX, user.user_id))
    {
        return Err(AdminError::too_many_requests());
    }

    let rendered = match prepare(&state, &app, &key, &request).await? {
        Prepared::Forbidden => return Err(AdminError::forbidden("Forbidden")),
        Prepared::Rendered(rendered) => rendered,
    };

    let tokens = state
        .devices
        .tokens_for_uid(&app, &test_uid)
        .await
        .map_err(device_error)?;
    if tokens.is_empty() {
        return Ok(Json(json!({ "delivered": 0, "reason": "no_devices" })));
    }

    let project_id = {
        let projects = state.project_map.read().await;
        projects.get(&app).cloned()
    }
    .ok_or_else(|| AdminError::internal("Service account for this app is not loaded"))?;

    let delivery = send::send_to_tokens(
        &state.fcm,
        &app,
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

    for token in &delivery.dead_tokens {
        let _ = state.devices.remove_dead_token(&app, token).await;
    }

    let payload = json!({
        "title": rendered.title,
        "body": rendered.body,
        "data": rendered.data,
    })
    .to_string();
    let response = json!({
        "delivered": delivery.delivered,
        "failed": delivery.failed,
    })
    .to_string();
    let _ = db::log_notification_ext(
        &state.db,
        &app,
        &format!("uid:{}", test_uid),
        &rendered.title,
        if delivery.delivered > 0 {
            "success"
        } else {
            "failed"
        },
        &response,
        &payload,
        &test_uid,
        Some(&user.email),
        "test",
        Some(&key),
    )
    .await;

    Ok(Json(json!({
        "delivered": delivery.delivered,
        "failed": delivery.failed,
    })))
}

#[cfg(test)]
mod tests {
    use super::super::tests::{
        sample_template, setup, setup_with_env, FakeEnv, Harness, APP, DATASOURCE_SECRET_ENV,
    };
    use super::*;
    use axum::http::Method;
    use serde_json::json;

    fn doc_template() -> Value {
        json!({
            "description": "Document greeting",
            "enabled": true,
            "senders": ["admin"],
            "to": { "role": "admin" },
            "title": "Hello",
            "body": "Hello {doc.name}",
            "context": {
                "doc": "users/{sender.uid}",
                "require": { "uid": "{sender.uid}" }
            }
        })
    }

    fn uid_template() -> Value {
        json!({
            "description": "UID greeting",
            "enabled": true,
            "senders": ["admin"],
            "to": { "uid": "{sender.uid}" },
            "title": "Hello",
            "body": "Body"
        })
    }

    fn preview_body() -> Value {
        json!({ "params": {}, "sender": { "uid": "u1" } })
    }

    async fn create(harness: &Harness, key: &str, definition: Value) -> (StatusCode, Value) {
        harness
            .call(
                Method::POST,
                &format!("/apps/{}/templates", APP),
                Some(json!({ "key": key, "definition": definition })),
            )
            .await
    }

    async fn update(
        harness: &Harness,
        key: &str,
        definition: Value,
        expected_version: i64,
    ) -> (StatusCode, Value) {
        harness
            .call(
                Method::PUT,
                &format!("/apps/{}/templates/{}", APP, key),
                Some(json!({
                    "definition": definition,
                    "expected_version": expected_version,
                })),
            )
            .await
    }

    #[tokio::test]
    async fn template_create_then_get_roundtrip() {
        let harness = setup("owner", &[]).await;
        let (status, body) = create(&harness, "sample", sample_template()).await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body["version"], 1);

        let (status, body) = harness
            .call(
                Method::GET,
                &format!("/apps/{}/templates/sample", APP),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["key"], "sample");
        assert_eq!(body["version"], 1);
        assert_eq!(body["enabled"], true);
        assert_eq!(body["definition"]["description"], "Sample admin alert");
        assert_eq!(body["definition"]["to"]["role"], "admin");
        assert_eq!(body["definition"]["title"], "Hello");
    }

    #[tokio::test]
    async fn template_create_duplicate_is_409() {
        let harness = setup("owner", &[]).await;
        let _ = create(&harness, "sample", sample_template()).await;
        let (status, body) = create(&harness, "sample", sample_template()).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["current_version"], 1);
    }

    #[tokio::test]
    async fn template_update_requires_expected_version() {
        let harness = setup("owner", &[]).await;
        let _ = create(&harness, "sample", sample_template()).await;
        let (status, body) = harness
            .call(
                Method::PUT,
                &format!("/apps/{}/templates/sample", APP),
                Some(json!({ "definition": sample_template() })),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_request");
    }

    #[tokio::test]
    async fn template_update_stale_version_is_409() {
        let harness = setup("owner", &[]).await;
        let _ = create(&harness, "sample", sample_template()).await;
        let (status, body) = update(&harness, "sample", sample_template(), 99).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["current_version"], 1);
    }

    #[tokio::test]
    async fn template_invalid_definition_returns_guard_issues() {
        let harness = setup("owner", &[]).await;
        let definition = json!({
            "description": "Broadcast with free text",
            "enabled": true,
            "senders": ["user"],
            "params": { "message": { "type": "text", "max": 100 } },
            "to": { "topic": "all" },
            "title": "Hello",
            "body": "{p.message}",
            "rate": { "per_uid_per_hour": 10 }
        });
        let (status, body) = create(&harness, "broadcast", definition).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid");
        let text = body["issues"].to_string();
        assert!(text.contains("[G1]"), "issues: {}", text);
    }

    #[tokio::test]
    async fn template_enable_disable() {
        let harness = setup("owner", &[]).await;
        let _ = create(&harness, "sample", sample_template()).await;

        let (status, _) = harness
            .call(
                Method::PUT,
                &format!("/apps/{}/templates/sample/enabled", APP),
                Some(json!({ "enabled": false })),
            )
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (_, body) = harness
            .call(
                Method::GET,
                &format!("/apps/{}/templates/sample", APP),
                None,
            )
            .await;
        assert_eq!(body["enabled"], false);

        let (status, _) = harness
            .call(
                Method::PUT,
                &format!("/apps/{}/templates/sample/enabled", APP),
                Some(json!({ "enabled": true })),
            )
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn template_delete_unused_ok() {
        let harness = setup("owner", &[]).await;
        let _ = create(&harness, "sample", sample_template()).await;
        let (status, _) = harness
            .call(
                Method::DELETE,
                &format!("/apps/{}/templates/sample", APP),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, _) = harness
            .call(
                Method::GET,
                &format!("/apps/{}/templates/sample", APP),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn template_delete_recently_used_is_409() {
        let harness = setup("owner", &[]).await;
        let _ = create(&harness, "sample", sample_template()).await;
        sqlx::query(
            "UPDATE templates SET last_used_at = CURRENT_TIMESTAMP WHERE app = ? AND key = ?",
        )
        .bind(APP)
        .bind("sample")
        .execute(&harness.pool)
        .await
        .expect("the template must be marked used");

        let (status, body) = harness
            .call(
                Method::DELETE,
                &format!("/apps/{}/templates/sample", APP),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"], "recently_used");
        assert_eq!(body["hint"], "disable the template instead");
    }

    #[tokio::test]
    async fn template_delete_old_usage_ok() {
        let harness = setup("owner", &[]).await;
        let _ = create(&harness, "sample", sample_template()).await;
        sqlx::query(
            "UPDATE templates SET last_used_at = datetime('now', '-40 days') WHERE app = ? AND key = ?",
        )
        .bind(APP)
        .bind("sample")
        .execute(&harness.pool)
        .await
        .expect("the template must be marked used");

        let (status, _) = harness
            .call(
                Method::DELETE,
                &format!("/apps/{}/templates/sample", APP),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn template_key_charset_enforced() {
        let harness = setup("owner", &[]).await;
        for key in [
            "Bad".to_string(),
            "with space".to_string(),
            "x".repeat(41),
            String::new(),
        ] {
            let (status, body) = create(&harness, &key, sample_template()).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "key {:?}", key);
            assert_eq!(body["error"], "invalid_request");
        }
    }

    #[tokio::test]
    async fn validate_endpoint_reports_issues_without_saving() {
        let harness = setup("owner", &[]).await;
        let definition = json!({
            "enabled": true,
            "senders": ["user"],
            "params": { "message": { "type": "text", "max": 100 } },
            "to": { "topic": "all" },
            "title": "Hello",
            "body": "{p.message}",
            "rate": { "per_uid_per_hour": 10 }
        });
        let (status, body) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/templates/validate", APP),
                Some(json!({ "key": "broadcast", "definition": definition })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], false);
        assert!(!body["issues"].as_array().expect("issues").is_empty());

        let (_, list) = harness
            .call(Method::GET, &format!("/apps/{}/templates", APP), None)
            .await;
        assert!(list.as_array().expect("an array").is_empty());
    }

    #[tokio::test]
    async fn validate_endpoint_unknown_field_is_issue_not_500() {
        let harness = setup("owner", &[]).await;
        let definition = json!({ "title": "Hi", "bogus": true });
        let (status, body) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/templates/validate", APP),
                Some(json!({ "definition": definition })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], false);
        let issues = body["issues"].as_array().expect("issues");
        assert!(issues[0]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("[S1]"));
    }

    #[tokio::test]
    async fn preview_renders_with_inline_doc() {
        let harness = setup("owner", &[]).await;
        let (status, body) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/templates/draft/preview", APP),
                Some(json!({
                    "params": {},
                    "sender": { "uid": "u1" },
                    "definition": doc_template(),
                    "doc": { "name": "Sara", "uid": "u1" }
                })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "body: {}", body);
        assert_eq!(body["body"], "Hello Sara");
        assert_eq!(body["title"], "Hello");
        assert_eq!(body["target"]["kind"], "role");
    }

    #[tokio::test]
    async fn preview_never_calls_fcm() {
        let harness = setup("owner", &[]).await;
        let (status, _) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/templates/draft/preview", APP),
                Some(json!({
                    "params": {},
                    "sender": { "uid": "u1" },
                    "definition": doc_template(),
                    "doc": { "name": "Sara", "uid": "u1" }
                })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(harness.fcm.calls(), 0, "preview must never reach FCM");
    }

    #[tokio::test]
    async fn preview_unsaved_definition_must_validate() {
        let harness = setup("owner", &[]).await;
        let mut definition = doc_template();
        definition["bogus"] = json!(true);
        let (status, body) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/templates/draft/preview", APP),
                Some(json!({
                    "params": {},
                    "sender": { "uid": "u1" },
                    "definition": definition,
                    "doc": { "name": "Sara" }
                })),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid");
        assert!(!body["issues"].as_array().expect("issues").is_empty());
    }

    #[tokio::test]
    async fn preview_require_failure_reports_reason_without_doc_data() {
        let harness = setup("owner", &[]).await;
        let (status, body) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/templates/draft/preview", APP),
                Some(json!({
                    "params": {},
                    "sender": { "uid": "u1" },
                    "definition": doc_template(),
                    "doc": { "name": "Sara", "uid": "someone-else", "secret": "TOP-SECRET" }
                })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["rendered"], false);
        assert_eq!(body["reason"], "forbidden");
        assert_eq!(body.as_object().expect("an object").len(), 2);
        assert!(!body.to_string().contains("TOP-SECRET"));
    }

    async fn configure_http_source(harness: &Harness, base_url: &str) {
        let value = json!({
            "type": "http",
            "base_url": base_url,
            "secret_env": DATASOURCE_SECRET_ENV
        });
        let (status, _) = harness
            .call(
                Method::PUT,
                &format!("/apps/{}/data_source", APP),
                Some(json!({ "value": value, "expected_version": 1 })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn preview_real_doc_reads_data_source() {
        let server = crate::datasource::test_support::TestServer::start(vec![
            crate::datasource::test_support::json(200, "OK", r#"{"name":"Sara","uid":"u1"}"#),
        ])
        .await;
        let harness = setup_with_env("owner", &[], Some(Arc::new(FakeEnv))).await;
        configure_http_source(&harness, &server.base_url).await;

        let (status, body) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/templates/draft/preview", APP),
                Some(json!({
                    "params": {},
                    "sender": { "uid": "u1" },
                    "definition": doc_template(),
                    "use_real_doc": true
                })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["body"], "Hello Sara");
    }

    #[tokio::test]
    async fn preview_real_doc_source_down_is_503() {
        let harness = setup_with_env("owner", &[], Some(Arc::new(FakeEnv))).await;
        configure_http_source(&harness, "http://127.0.0.1:1").await;

        let (status, body) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/templates/draft/preview", APP),
                Some(json!({
                    "params": {},
                    "sender": { "uid": "u1" },
                    "definition": doc_template(),
                    "use_real_doc": true
                })),
            )
            .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"], "unavailable");
    }

    #[tokio::test]
    async fn preview_recipient_counts_only_no_uids_or_tokens() {
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
        let _ = create(&harness, "sample", uid_template()).await;

        let (status, body) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/templates/sample/preview", APP),
                Some(preview_body()),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["recipients"]["kind"], "uid");
        assert_eq!(body["recipients"]["count"], 2);
        assert!(body.get("uids").is_none());
        let text = body.to_string();
        assert!(!text.contains("token-u1"));
        assert!(!text.contains("token-u2"));
        assert!(!text.contains("u2"));
    }

    #[tokio::test]
    async fn test_send_without_test_uid_is_400() {
        let harness = setup("owner", &[]).await;
        let _ = create(&harness, "sample", sample_template()).await;
        let (status, body) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/templates/sample/test-send", APP),
                Some(preview_body()),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["message"], "Set a test_uid on your account first");
    }

    #[tokio::test]
    async fn test_send_goes_only_to_test_uid_devices() {
        let harness = setup("owner", &[]).await;
        harness.set_test_uid("tester").await;
        harness
            .register_device("tester", "token-tester-1-0000000000000000000000000000")
            .await;
        harness
            .register_device("tester", "token-tester-2-0000000000000000000000000000")
            .await;
        harness
            .register_device("other", "token-other-1-0000000000000000000000000000")
            .await;
        let _ = create(&harness, "sample", uid_template()).await;

        let (status, body) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/templates/sample/test-send", APP),
                Some(preview_body()),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["delivered"], 2);
        let tokens = harness.fcm.tokens();
        assert_eq!(tokens.len(), 2);
        assert!(tokens.iter().all(|token| token.starts_with("token-tester")));
    }

    #[tokio::test]
    async fn test_send_ignores_template_target() {
        let harness = setup("owner", &[]).await;
        harness.set_test_uid("tester").await;
        harness
            .register_device("tester", "token-tester-1-0000000000000000000000000000")
            .await;
        harness
            .register_device("other", "token-other-1-0000000000000000000000000000")
            .await;
        let topic = json!({
            "description": "Broadcast",
            "enabled": true,
            "senders": ["admin"],
            "to": { "topic": "all" },
            "title": "Hello",
            "body": "Body"
        });
        let _ = create(&harness, "sample", topic).await;

        let (status, body) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/templates/sample/test-send", APP),
                Some(preview_body()),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["delivered"], 1);
        assert_eq!(
            harness.fcm.tokens(),
            vec!["token-tester-1-0000000000000000000000000000".to_string()]
        );
    }

    #[tokio::test]
    async fn test_send_body_cannot_override_test_uid() {
        let harness = setup("owner", &[]).await;
        harness.set_test_uid("tester").await;
        harness
            .register_device("tester", "token-tester-1-0000000000000000000000000000")
            .await;
        harness
            .register_device("other", "token-other-1-0000000000000000000000000000")
            .await;
        let _ = create(&harness, "sample", uid_template()).await;

        let (status, _) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/templates/sample/test-send", APP),
                Some(json!({
                    "params": {},
                    "sender": { "uid": "tester" },
                    "test_uid": "other"
                })),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(harness.fcm.calls(), 0);
    }

    #[tokio::test]
    async fn test_send_rate_limited_per_user() {
        let harness = setup("owner", &[]).await;
        harness.set_test_uid("tester").await;
        let _ = create(&harness, "sample", sample_template()).await;

        for attempt in 0..10 {
            let (status, _) = harness
                .call(
                    Method::POST,
                    &format!("/apps/{}/templates/sample/test-send", APP),
                    Some(preview_body()),
                )
                .await;
            assert_eq!(status, StatusCode::OK, "attempt {}", attempt);
        }
        let (status, body) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/templates/sample/test-send", APP),
                Some(preview_body()),
            )
            .await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(body["error"], "rate_limited");
    }

    #[tokio::test]
    async fn test_send_history_kind_test() {
        let harness = setup("owner", &[]).await;
        harness.set_test_uid("tester").await;
        harness
            .register_device("tester", "token-tester-1-0000000000000000000000000000")
            .await;
        let _ = create(&harness, "sample", uid_template()).await;

        let (status, _) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/templates/sample/test-send", APP),
                Some(preview_body()),
            )
            .await;
        assert_eq!(status, StatusCode::OK);

        let row: (String, Option<String>, String, Option<String>) = sqlx::query_as(
            "SELECT kind, \"ref\", target, sender_uid FROM notifications_history ORDER BY id DESC LIMIT 1",
        )
        .fetch_one(&harness.pool)
        .await
        .expect("the history row must exist");
        assert_eq!(row.0, "test");
        assert_eq!(row.1.as_deref(), Some("sample"));
        assert_eq!(row.2, "uid:tester");
        assert_eq!(row.3.as_deref(), Some("tester"));
    }
}

//! Dashboard app settings, section saves, versions, import/export and audit
//! (T10-A).
//!
//! Every handler calls [`authorize`] before reading or writing anything. Writes
//! go through `ConfigStore` only: this module never runs SQL against
//! `app_configs`, `templates` or `config_versions`.

use super::auth::{AuthedUser, Role};
use super::{authorize, map_config_error, AdminError, AdminJson, Need};
use crate::config::Entity;
use crate::db;
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

/// `GET /apps`: owners see the loaded apps, managers only their bindings.
pub async fn list_apps(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
) -> Result<Json<Value>, AdminError> {
    let project_map = state.project_map.read().await;
    let mut names: Vec<String> = match user.role {
        // The union of the configured apps and the apps that only have
        // credentials, so an app is never hidden from its owner.
        Role::Owner => {
            let mut names = state.config.app_names();
            names.extend(project_map.keys().cloned());
            names
        }
        Role::Manager => user.apps.clone(),
    };
    names.sort();
    names.dedup();

    let items: Vec<Value> = names
        .iter()
        .map(|app| {
            let snapshot = state.config.get(app);
            json!({
                "app": app,
                "configured": snapshot.is_some(),
                "has_credentials": project_map.contains_key(app),
                "project_id": project_map.get(app),
                "version": snapshot.as_ref().map(|snapshot| snapshot.version).unwrap_or(0),
                "template_count": snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.templates.len())
                    .unwrap_or(0),
                "enabled_templates": snapshot
                    .as_ref()
                    .map(|snapshot| {
                        snapshot
                            .templates
                            .values()
                            .filter(|template| template.enabled)
                            .count()
                    })
                    .unwrap_or(0),
            })
        })
        .collect();

    Ok(Json(Value::Array(items)))
}

/// `GET /apps/{app}`: the six sections plus a summary per template.
pub async fn get_app(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path(app): Path<String>,
) -> Result<Json<Value>, AdminError> {
    authorize(&state, &user, &app, Need::AnyMember).await?;
    let snapshot = state
        .config
        .get(&app)
        .ok_or_else(|| AdminError::not_found("the app has no configuration"))?;
    let metadata = template_metadata(&state, &app).await?;

    let templates: Vec<Value> = snapshot
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

    let config = serde_json::to_value(&snapshot.config)
        .map_err(|_| AdminError::internal("the configuration could not be rendered"))?;
    Ok(Json(json!({
        "app": app,
        "version": snapshot.version,
        "config": config,
        "templates": templates,
    })))
}

/// `POST /apps/{app}/init`: the default configuration for an app that already
/// has credentials.
pub async fn init_app(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path(app): Path<String>,
) -> Result<(StatusCode, Json<Value>), AdminError> {
    user.require_owner()?;

    let has_credentials = state.project_map.read().await.contains_key(&app);
    if !has_credentials {
        return Err(AdminError::invalid_request("the app has no credentials"));
    }
    if let Some(snapshot) = state.config.get(&app) {
        return Err(AdminError::conflict(snapshot.version));
    }

    state
        .config
        .create_app(&app, &user.email)
        .await
        .map_err(map_config_error)?;
    Ok((StatusCode::CREATED, Json(json!({ "version": 1 }))))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SectionRequest {
    pub value: Value,
    pub expected_version: i64,
}

/// `PUT /apps/{app}/campaigns`: the campaign list path shadows the section
/// path, so the section save is registered on the static path as well.
pub async fn put_campaigns_section(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path(app): Path<String>,
    AdminJson(request): AdminJson<SectionRequest>,
) -> Result<Json<Value>, AdminError> {
    put_section(
        State(state),
        user,
        Path((app, "campaigns".to_string())),
        AdminJson(request),
    )
    .await
}

/// `PUT /apps/{app}/{section}`.
pub async fn put_section(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path((app, section)): Path<(String, String)>,
    AdminJson(request): AdminJson<SectionRequest>,
) -> Result<Json<Value>, AdminError> {
    authorize(&state, &user, &app, Need::AnyMember).await?;

    match section.as_str() {
        "identity" | "data_source" | "devices" | "app_check" => user.require_owner()?,
        "web_subscribe" | "campaigns" => {}
        _ => return Err(AdminError::not_found("unknown section")),
    }

    // A manager may tune campaigns but never who sends them or which entity
    // document they read (plan 3.2: managers do not change identity).
    if user.role == Role::Manager && section == "campaigns" {
        let snapshot = state
            .config
            .get(&app)
            .ok_or_else(|| AdminError::not_found("the app has no configuration"))?;
        let current = serde_json::to_value(&snapshot.config.campaigns)
            .map_err(|_| AdminError::internal("the configuration could not be rendered"))?;
        let default_entity = serde_json::to_value(Entity::default())
            .map_err(|_| AdminError::internal("the default entity could not be rendered"))?;

        let senders = request.value.get("senders").cloned().unwrap_or(json!([]));
        let entity = request
            .value
            .get("entity")
            .cloned()
            .unwrap_or(default_entity);
        if senders != current.get("senders").cloned().unwrap_or(json!([]))
            || entity != current.get("entity").cloned().unwrap_or(json!({}))
        {
            return Err(AdminError::forbidden(
                "a manager cannot change campaigns.senders or campaigns.entity",
            ));
        }
    }

    let version = state
        .config
        .save_section(
            &app,
            &section,
            request.value,
            &user.email,
            request.expected_version,
        )
        .await
        .map_err(map_config_error)?;
    Ok(Json(json!({ "version": version })))
}

#[derive(Deserialize)]
pub struct VersionsQuery {
    pub section: Option<String>,
    pub limit: Option<i64>,
}

/// `GET /apps/{app}/versions`: the list never carries the stored content.
pub async fn list_versions(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path(app): Path<String>,
    Query(query): Query<VersionsQuery>,
) -> Result<Json<Value>, AdminError> {
    authorize(&state, &user, &app, Need::AnyMember).await?;
    let limit = query.limit.unwrap_or(50).clamp(1, 500);
    let rows = state
        .config
        .versions(&app, query.section.as_deref(), limit)
        .await
        .map_err(map_config_error)?;

    let items: Vec<Value> = rows
        .into_iter()
        .map(|row| {
            json!({
                "id": row.id,
                "section": row.section,
                "changed_by": row.changed_by,
                "created_at": row.created_at,
                "size": row.content.len(),
            })
        })
        .collect();
    Ok(Json(Value::Array(items)))
}

/// Managers may read/restore only the sections they can write (plan 3.11).
fn manager_section_allowed(section: &str) -> bool {
    matches!(section, "web_subscribe" | "campaigns") || section.starts_with("template:")
}

fn require_section_access(user: &AuthedUser, section: &str) -> Result<(), AdminError> {
    if user.role == Role::Manager && !manager_section_allowed(section) {
        return Err(AdminError::forbidden(
            "this section is not available to the manager role",
        ));
    }
    Ok(())
}

/// `GET /apps/{app}/versions/{id}`: the content, for the section's readers.
pub async fn get_version(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path((app, id)): Path<(String, i64)>,
) -> Result<Json<Value>, AdminError> {
    authorize(&state, &user, &app, Need::AnyMember).await?;
    let row = db::read_version(&state.db, &app, id)
        .await
        .map_err(|error| AdminError::from_db("read the version", error))?
        .ok_or_else(|| AdminError::not_found("the version does not exist"))?;
    require_section_access(&user, &row.section)?;

    let content: Value = serde_json::from_str(&row.content)
        .map_err(|_| AdminError::internal("the stored version is not valid JSON"))?;
    Ok(Json(json!({
        "id": row.id,
        "section": row.section,
        "changed_by": row.changed_by,
        "created_at": row.created_at,
        "content": content,
    })))
}

/// `POST /apps/{app}/versions/{id}/restore`.
pub async fn restore_version(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path((app, id)): Path<(String, i64)>,
) -> Result<Json<Value>, AdminError> {
    authorize(&state, &user, &app, Need::AnyMember).await?;
    let row = db::read_version(&state.db, &app, id)
        .await
        .map_err(|error| AdminError::from_db("read the version", error))?
        .ok_or_else(|| AdminError::not_found("the version does not exist"))?;
    require_section_access(&user, &row.section)?;

    let version = state
        .config
        .restore(&app, id, &user.email)
        .await
        .map_err(map_config_error)?;
    Ok(Json(json!({ "version": version })))
}

/// `GET /apps/{app}/export` (owner): the import document, without secrets.
pub async fn export(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path(app): Path<String>,
) -> Result<Json<Value>, AdminError> {
    authorize(&state, &user, &app, Need::Owner).await?;
    let document = state.config.export(&app).await.map_err(map_config_error)?;
    Ok(Json(document))
}

/// `POST /apps/{app}/import` (owner, recent password confirmation).
pub async fn import(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path(app): Path<String>,
    AdminJson(document): AdminJson<Value>,
) -> Result<Json<Value>, AdminError> {
    authorize(&state, &user, &app, Need::Owner).await?;
    user.require_reauth(300)?;

    state
        .config
        .import(&app, document, &user.email)
        .await
        .map_err(map_config_error)?;
    let version = state
        .config
        .get(&app)
        .map(|snapshot| snapshot.version)
        .unwrap_or(0);
    Ok(Json(json!({ "version": version })))
}

#[derive(Deserialize)]
pub struct AuditQuery {
    pub app: Option<String>,
    pub limit: Option<i64>,
    pub before: Option<i64>,
}

/// `GET /audit`: owners see every app, managers only their own and must filter.
pub async fn audit(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Query(query): Query<AuditQuery>,
) -> Result<Json<Value>, AdminError> {
    if user.role == Role::Manager {
        let app = query
            .app
            .as_deref()
            .ok_or_else(|| AdminError::forbidden("the app parameter is required"))?;
        user.require_app(app)?;
    }

    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let rows = db::read_audit(&state.db, query.app.as_deref(), limit, query.before)
        .await
        .map_err(|error| AdminError::from_db("read the audit log", error))?;

    let items: Vec<Value> = rows
        .into_iter()
        .map(|row| {
            json!({
                "id": row.id,
                "actor": row.actor,
                "app": row.app,
                "action": row.action,
                "created_at": row.created_at,
                "detail": row.detail.map(|detail| {
                    detail.chars().take(500).collect::<String>()
                }),
            })
        })
        .collect();
    Ok(Json(Value::Array(items)))
}

/// Template metadata keyed by template key.
async fn template_metadata(
    state: &AppState,
    app: &str,
) -> Result<HashMap<String, db::TemplateMeta>, AdminError> {
    let rows = db::template_metadata(&state.db, app)
        .await
        .map_err(|error| AdminError::from_db("read the template metadata", error))?;
    Ok(rows.into_iter().map(|row| (row.key.clone(), row)).collect())
}

#[cfg(test)]
mod tests {
    use super::super::tests::{
        add_credentials, sample_template, setup, Harness, APP, OTHER_APP, PASSWORD,
    };
    use super::*;
    use axum::http::Method;
    use serde_json::json;

    async fn put_section(
        harness: &Harness,
        section: &str,
        value: Value,
        expected_version: i64,
    ) -> (StatusCode, Value) {
        harness
            .call(
                Method::PUT,
                &format!("/apps/{}/{}", APP, section),
                Some(json!({ "value": value, "expected_version": expected_version })),
            )
            .await
    }

    fn current_section(harness: &Harness, section: &str) -> Value {
        let snapshot = harness.state.config.get(APP).expect("the app exists");
        match section {
            "identity" => serde_json::to_value(&snapshot.config.identity),
            "data_source" => serde_json::to_value(&snapshot.config.data_source),
            "devices" => serde_json::to_value(&snapshot.config.devices),
            "web_subscribe" => serde_json::to_value(&snapshot.config.web_subscribe),
            "campaigns" => serde_json::to_value(&snapshot.config.campaigns),
            "app_check" => serde_json::to_value(&snapshot.config.app_check),
            other => panic!("unknown section {}", other),
        }
        .expect("the section must serialize")
    }

    #[tokio::test]
    async fn list_apps_owner_includes_configured_without_credentials() {
        let harness = setup("owner", &[]).await;
        harness
            .state
            .config
            .create_app("configured_only", "tester")
            .await
            .expect("the app configuration must be created");

        let (status, body) = harness.call(Method::GET, "/apps", None).await;
        assert_eq!(status, StatusCode::OK);
        let items = body.as_array().expect("an array");
        let configured_only = items
            .iter()
            .find(|item| item["app"] == "configured_only")
            .expect("the configured app must be listed");
        assert_eq!(configured_only["configured"], true);
        assert_eq!(configured_only["has_credentials"], false);
        assert!(configured_only["project_id"].is_null());

        // The apps with credentials are still listed.
        assert!(items.iter().any(|item| item["app"] == APP));
    }

    #[tokio::test]
    async fn list_apps_scoped_for_manager() {
        let harness = setup("manager", &[APP]).await;
        let (status, body) = harness.call(Method::GET, "/apps", None).await;
        assert_eq!(status, StatusCode::OK);
        let items = body.as_array().expect("an array");
        assert_eq!(items.len(), 1, "only the bound app is listed");
        assert_eq!(items[0]["app"], APP);
        assert_eq!(items[0]["configured"], true);
        assert_eq!(items[0]["has_credentials"], true);
        assert_eq!(items[0]["version"], 1);
        assert_eq!(items[0]["template_count"], 0);
    }

    #[tokio::test]
    async fn list_apps_never_exposes_credentials_content() {
        let harness = setup("owner", &[]).await;
        let (status, body) = harness.call(Method::GET, "/apps", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.as_array().expect("an array").len(), 2);
        let text = body.to_string();
        for marker in [
            "private_key",
            "BEGIN PRIVATE KEY",
            "client_email",
            "auth_map",
        ] {
            assert!(!text.contains(marker), "list leaked {}", marker);
        }
    }

    #[tokio::test]
    async fn get_app_returns_sections_and_template_summaries() {
        let harness = setup("owner", &[]).await;
        harness.save_template("sample", sample_template()).await;

        let (status, body) = harness
            .call(Method::GET, &format!("/apps/{}", APP), None)
            .await;
        assert_eq!(status, StatusCode::OK);
        for section in [
            "identity",
            "data_source",
            "devices",
            "web_subscribe",
            "campaigns",
            "app_check",
        ] {
            assert!(body["config"].get(section).is_some(), "missing {}", section);
        }
        let templates = body["templates"].as_array().expect("an array");
        let sample = templates
            .iter()
            .find(|item| item["key"] == "sample")
            .expect("the template summary");
        assert_eq!(sample["enabled"], true);
        assert_eq!(sample["target_kind"], "role");
        assert!(sample["version"].as_i64().unwrap_or(0) >= 1);
        assert!(sample["last_used_at"].is_null());
    }

    #[tokio::test]
    async fn init_creates_default_config_once() {
        let harness = setup("owner", &[]).await;
        add_credentials(&harness.state, "fresh", "project-fresh").await;

        let (status, body) = harness
            .call(Method::POST, "/apps/fresh/init", Some(json!({})))
            .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body["version"], 1);

        let (status, _) = harness.call(Method::GET, "/apps/fresh", None).await;
        assert_eq!(status, StatusCode::OK);

        let (status, body) = harness
            .call(Method::POST, "/apps/fresh/init", Some(json!({})))
            .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["current_version"], 1);
    }

    #[tokio::test]
    async fn init_without_credentials_is_400() {
        let harness = setup("owner", &[]).await;
        let (status, body) = harness
            .call(Method::POST, "/apps/ghost/init", Some(json!({})))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_request");
    }

    #[tokio::test]
    async fn init_unassigned_manager_is_403() {
        // A manager that is not bound to the app learns nothing: the role is
        // checked before the credentials and before any disclosure.
        let harness = setup("manager", &[OTHER_APP]).await;
        add_credentials(&harness.state, "fresh", "project-fresh").await;

        let (status, body) = harness
            .call(Method::POST, "/apps/fresh/init", Some(json!({})))
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"], "forbidden");

        let (status, body) = harness.call(Method::GET, "/apps/fresh", None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"], "forbidden");
    }

    #[tokio::test]
    async fn put_section_saves_and_bumps_version() {
        let harness = setup("owner", &[]).await;
        let value = json!({ "anonymous": ["all"], "signed_in": [] });
        let (status, body) = put_section(&harness, "web_subscribe", value, 1).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["version"], 2);

        let snapshot = harness.state.config.get(APP).expect("the app exists");
        assert_eq!(snapshot.version, 2);
        assert_eq!(snapshot.config.web_subscribe.anonymous, vec!["all"]);
    }

    #[tokio::test]
    async fn put_section_invalid_returns_issues_400() {
        let harness = setup("owner", &[]).await;
        let (status, body) = put_section(&harness, "identity", json!({}), 1).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid");
        assert!(!body["issues"].as_array().expect("issues").is_empty());
    }

    #[tokio::test]
    async fn put_section_stale_version_is_409_with_current() {
        let harness = setup("owner", &[]).await;
        let (status, body) = put_section(
            &harness,
            "web_subscribe",
            current_section(&harness, "web_subscribe"),
            99,
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["current_version"], 1);
    }

    #[tokio::test]
    async fn manager_cannot_change_campaign_senders_or_entity() {
        let harness = setup("manager", &[APP]).await;
        let mut campaigns = current_section(&harness, "campaigns");
        campaigns["senders"] = json!(["company"]);
        let (status, body) = put_section(&harness, "campaigns", campaigns, 1).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"], "forbidden");

        let mut allowed = current_section(&harness, "campaigns");
        allowed["enabled"] = json!(false);
        let (status, _) = put_section(&harness, "campaigns", allowed, 1).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn manager_can_change_web_subscribe() {
        let harness = setup("manager", &[APP]).await;
        let value = json!({ "anonymous": ["promos"], "signed_in": ["users"] });
        let (status, body) = put_section(&harness, "web_subscribe", value, 1).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["version"], 2);
    }

    #[tokio::test]
    async fn unknown_section_is_404() {
        let harness = setup("owner", &[]).await;
        let (status, _) = put_section(&harness, "bogus", json!({}), 1).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn versions_list_hides_content() {
        let harness = setup("owner", &[]).await;
        harness.save_template("sample", sample_template()).await;

        let (status, body) = harness
            .call(
                Method::GET,
                &format!("/apps/{}/versions?limit=500", APP),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        let rows = body.as_array().expect("an array");
        assert!(!rows.is_empty(), "create_app writes a version");
        for row in rows {
            assert!(row.get("content").is_none(), "the list must hide content");
            assert!(row.get("size").is_some());
            assert!(row.get("section").is_some());
        }
    }

    #[tokio::test]
    async fn restore_by_owner_any_section() {
        let harness = setup("owner", &[]).await;
        let first = json!({ "anonymous": ["first"], "signed_in": [] });
        let second = json!({ "anonymous": ["second"], "signed_in": [] });
        let (_, _) = put_section(&harness, "web_subscribe", first.clone(), 1).await;
        let (_, _) = put_section(&harness, "web_subscribe", second, 2).await;

        let rows = harness
            .state
            .config
            .versions(APP, Some("web_subscribe"), 50)
            .await
            .expect("versions");
        let target = rows.get(1).expect("the older web_subscribe version");

        let (status, body) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/versions/{}/restore", APP, target.id),
                Some(json!({})),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["version"].as_i64().unwrap_or(0) > 3);
        let snapshot = harness.state.config.get(APP).expect("the app exists");
        assert_eq!(
            serde_json::to_value(&snapshot.config.web_subscribe).expect("json"),
            first
        );
    }

    #[tokio::test]
    async fn restore_by_manager_allowed_sections_only() {
        let harness = setup("manager", &[APP]).await;
        harness.save_template("sample", sample_template()).await;

        let snapshot = harness.state.config.get(APP).expect("the app exists");
        harness
            .state
            .config
            .save_section(
                APP,
                "identity",
                serde_json::to_value(&snapshot.config.identity).expect("json"),
                "owner@example.com",
                snapshot.version,
            )
            .await
            .expect("the identity version must be written");

        let template_versions = harness
            .state
            .config
            .versions(APP, Some("template:sample"), 10)
            .await
            .expect("versions");
        let template_version = template_versions.first().expect("a template version");
        let (status, _) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/versions/{}/restore", APP, template_version.id),
                Some(json!({})),
            )
            .await;
        assert_eq!(status, StatusCode::OK);

        let identity_versions = harness
            .state
            .config
            .versions(APP, Some("identity"), 10)
            .await
            .expect("versions");
        let identity_version = identity_versions.first().expect("an identity version");
        let (status, _) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/versions/{}/restore", APP, identity_version.id),
                Some(json!({})),
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn restore_foreign_app_version_is_404() {
        let harness = setup("owner", &[]).await;
        let foreign_id: i64 =
            sqlx::query_scalar("SELECT id FROM config_versions WHERE app = ? ORDER BY id LIMIT 1")
                .bind(OTHER_APP)
                .fetch_one(&harness.pool)
                .await
                .expect("the other app has a version");

        let (status, _) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/versions/{}/restore", APP, foreign_id),
                Some(json!({})),
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, _) = harness
            .call(
                Method::GET,
                &format!("/apps/{}/versions/{}", APP, foreign_id),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn export_has_no_secrets() {
        let harness = setup("owner", &[]).await;
        let (status, body) = harness
            .call(Method::GET, &format!("/apps/{}/export", APP), None)
            .await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.get("templates").is_some(), "the document format");
        let text = body.to_string();
        for marker in ["private_key", "BEGIN PRIVATE KEY", "client_email", PASSWORD] {
            assert!(!text.contains(marker), "export leaked {}", marker);
        }
    }

    #[tokio::test]
    async fn import_requires_recent_reauth() {
        let harness = setup("owner", &[]).await;
        let (_, document) = harness
            .call(Method::GET, &format!("/apps/{}/export", APP), None)
            .await;

        let (status, body) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/import", APP),
                Some(document.clone()),
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["message"], "reauth_required");

        harness.reauth().await;
        let (status, _) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/import", APP),
                Some(document),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn import_roundtrip_via_http() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let (_, document) = harness
            .call(Method::GET, &format!("/apps/{}/export", APP), None)
            .await;

        let (status, _) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/import", APP),
                Some(document.clone()),
            )
            .await;
        assert_eq!(status, StatusCode::OK);

        let (status, again) = harness
            .call(Method::GET, &format!("/apps/{}/export", APP), None)
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(again, document);
    }

    #[tokio::test]
    async fn audit_shows_every_write_above() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let (_, _) = put_section(
            &harness,
            "web_subscribe",
            json!({ "anonymous": ["all"], "signed_in": [] }),
            1,
        )
        .await;
        let (_, _) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/templates", APP),
                Some(json!({ "key": "sample", "definition": sample_template() })),
            )
            .await;
        let (_, document) = harness
            .call(Method::GET, &format!("/apps/{}/export", APP), None)
            .await;
        let _ = harness
            .call(
                Method::POST,
                &format!("/apps/{}/import", APP),
                Some(document),
            )
            .await;

        let (status, body) = harness
            .call(Method::GET, &format!("/audit?app={}", APP), None)
            .await;
        assert_eq!(status, StatusCode::OK);
        let actions: Vec<&str> = body
            .as_array()
            .expect("an array")
            .iter()
            .filter_map(|row| row["action"].as_str())
            .collect();
        for action in [
            "config.save_section.web_subscribe",
            "config.save_template",
            "config.import",
        ] {
            assert!(
                actions.contains(&action),
                "missing {} in {:?}",
                action,
                actions
            );
        }
    }

    #[tokio::test]
    async fn audit_manager_scoped_and_requires_app() {
        let harness = setup("manager", &[APP]).await;
        let (status, _) = harness.call(Method::GET, "/audit", None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let (status, _) = harness.call(Method::GET, "/audit?app=other", None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let (status, body) = harness
            .call(Method::GET, &format!("/audit?app={}", APP), None)
            .await;
        assert_eq!(status, StatusCode::OK);
        for row in body.as_array().expect("an array") {
            assert_eq!(row["app"], APP);
        }
    }

    #[tokio::test]
    async fn audit_pagination_before_and_limit_cap() {
        let harness = setup("owner", &[]).await;
        for value in 0..4 {
            let (_, _) = put_section(
                &harness,
                "web_subscribe",
                json!({ "anonymous": [format!("list-{}", value)], "signed_in": [] }),
                harness
                    .state
                    .config
                    .get(APP)
                    .expect("the app exists")
                    .version,
            )
            .await;
        }

        let (status, body) = harness.call(Method::GET, "/audit?limit=2", None).await;
        assert_eq!(status, StatusCode::OK);
        let rows = body.as_array().expect("an array");
        assert_eq!(rows.len(), 2);
        let second = rows[1]["id"].as_i64().expect("an id");

        let (status, body) = harness
            .call(
                Method::GET,
                &format!("/audit?before={}&limit=500", second),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        let rows = body.as_array().expect("an array");
        assert!(!rows.is_empty());
        assert!(rows
            .iter()
            .all(|row| row["id"].as_i64().unwrap_or(i64::MAX) < second));

        let (_, body) = harness.call(Method::GET, "/audit?limit=1000", None).await;
        assert!(body.as_array().expect("an array").len() <= 200);
    }

    #[tokio::test]
    async fn audit_never_contains_secret_values() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let _ = put_section(
            &harness,
            "web_subscribe",
            json!({ "anonymous": ["all"], "signed_in": [] }),
            1,
        )
        .await;

        let (status, body) = harness.call(Method::GET, "/audit", None).await;
        assert_eq!(status, StatusCode::OK);
        let text = body.to_string();
        for marker in [
            "private_key",
            "BEGIN PRIVATE KEY",
            PASSWORD,
            "correct horse battery",
            &harness.token,
        ] {
            assert!(!text.contains(marker), "audit leaked {}", marker);
        }
    }
}

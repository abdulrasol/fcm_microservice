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
        Role::Owner => project_map.keys().cloned().collect(),
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






















}

//! Dashboard connection checks (T10-C).
//!
//! The checks run with the settings of the application itself: the owner
//! pastes an identity token or a document path, and the tool answers with
//! what it could read. The token is never echoed, never logged and never
//! audited, and a document answer carries the field names and their types
//! only, never their values.

use super::auth::AuthedUser;
use super::{AdminError, AdminJson};
use crate::datasource::DataError;
use crate::db;
use crate::identity::IdentityError;
use crate::roles;
use crate::AppState;
use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

/// The largest identity token the check accepts.
const MAX_TOKEN_BYTES: usize = 8192;

/// The largest number of members the check returns.
const MAX_MEMBERS_LIMIT: usize = 200;

/// The default number of members the check returns.
const DEFAULT_MEMBERS_LIMIT: usize = 50;

/// The check limit: 20 per minute and dashboard user.
///
/// The existing 20-per-minute limiter is reused with its own key, so the
/// account routes and the device limit are untouched.
fn check_limit(state: &AppState, user: &AuthedUser) -> Result<(), AdminError> {
    if !state
        .devices_limiter
        .check(&format!("admin:diagnostics:{}", user.user_id))
    {
        return Err(AdminError::too_many_requests());
    }
    Ok(())
}

/// The app must have a configuration or credentials.
async fn ensure_app_exists(state: &AppState, app: &str) -> Result<(), AdminError> {
    if state.config.get(app).is_some() {
        return Ok(());
    }
    if state.project_map.read().await.contains_key(app) {
        return Ok(());
    }
    Err(AdminError::not_found("the app does not exist"))
}

/// The body of an identity check.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityTestRequest {
    pub token: String,
}

/// `POST /apps/{app}/identity/test`: checks a token against the saved
/// provider of the app.
pub async fn identity_test(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path(app): Path<String>,
    AdminJson(body): AdminJson<IdentityTestRequest>,
) -> Result<Json<Value>, AdminError> {
    user.require_owner()?;
    check_limit(&state, &user)?;
    ensure_app_exists(&state, &app).await?;

    if body.token.len() > MAX_TOKEN_BYTES {
        return Err(AdminError::invalid_request("the token is too long"));
    }

    let provider = state
        .identity
        .provider_for(&app)
        .await
        .map_err(|error| match error {
            IdentityError::Invalid(_) => AdminError::not_found("the app has no identity provider"),
            IdentityError::Unavailable(_) => {
                AdminError::unavailable("the identity provider is unavailable")
            }
        })?;

    let outcome = match provider.verify(&body.token).await {
        Ok(identity) => {
            // A role lookup that fails leaves the role empty: the identity
            // itself is already verified.
            let role = state
                .auth
                .resolve_sender_role(&app, &identity)
                .await
                .ok()
                .flatten();
            let admin_roles = match state.config.get(&app) {
                Some(snapshot) => snapshot.config.identity.admin_roles.clone(),
                None => roles::legacy_policy().admin_roles,
            };
            let mut claim_names: Vec<String> = identity
                .claims
                .as_object()
                .map(|claims| claims.keys().cloned().collect())
                .unwrap_or_default();
            claim_names.sort();
            json!({
                "ok": true,
                "uid": identity.uid,
                "name": identity.name,
                "role": role,
                "is_admin": roles::is_admin_role(role.as_deref(), &admin_roles),
                "claim_names": claim_names,
            })
        }
        Err(IdentityError::Invalid(reason)) => json!({ "ok": false, "reason": reason }),
        Err(IdentityError::Unavailable(_)) => {
            return Err(AdminError::unavailable(
                "the identity provider is unavailable",
            ))
        }
    };

    // The audit holds no detail: the token is never written anywhere.
    let _ = db::write_audit(
        &state.db,
        &user.email,
        Some(&app),
        "identity.test",
        "identity check",
    )
    .await;

    Ok(Json(outcome))
}

/// The body of a data source check: a document path or a members query.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataSourceTestRequest {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub members: Option<MembersRequest>,
}

/// A members query.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MembersRequest {
    pub collection: String,
    pub field: String,
    pub value: String,
    #[serde(default)]
    pub limit: Option<usize>,
}

/// `POST /apps/{app}/data_source/test`: reads one document or counts the
/// members of a collection.
pub async fn data_source_test(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path(app): Path<String>,
    AdminJson(body): AdminJson<DataSourceTestRequest>,
) -> Result<Json<Value>, AdminError> {
    user.require_owner()?;
    check_limit(&state, &user)?;
    ensure_app_exists(&state, &app).await?;

    let modes = [body.path.is_some(), body.members.is_some()]
        .into_iter()
        .filter(|present| *present)
        .count();
    if modes != 1 {
        return Err(AdminError::invalid_request(
            "provide exactly one of path or members",
        ));
    }

    let source = state
        .datasource
        .source_for(&app)
        .await
        .map_err(|error| match error {
            DataError::InvalidPath(message) => AdminError::invalid_request(&message),
            DataError::Unavailable(_) => AdminError::unavailable("the data source is unavailable"),
        })?;

    let outcome = if let Some(path) = &body.path {
        match source.get_doc(path).await {
            Ok(Some(document)) => json!({
                "ok": true,
                "found": true,
                "fields": field_types(&document),
            }),
            Ok(None) => json!({ "ok": true, "found": false, "fields": [] }),
            Err(DataError::InvalidPath(message)) => {
                return Err(AdminError::invalid_request(&message))
            }
            Err(DataError::Unavailable(_)) => {
                json!({ "ok": false, "reason": "data source unavailable" })
            }
        }
    } else {
        let members = body.members.as_ref().expect("one mode");
        let limit = members
            .limit
            .unwrap_or(DEFAULT_MEMBERS_LIMIT)
            .clamp(1, MAX_MEMBERS_LIMIT);
        match source
            .members(&members.collection, &members.field, &members.value, limit)
            .await
        {
            Ok(uids) => json!({ "ok": true, "count": uids.len() }),
            Err(DataError::InvalidPath(message)) => {
                return Err(AdminError::invalid_request(&message))
            }
            Err(DataError::Unavailable(_)) => {
                json!({ "ok": false, "reason": "data source unavailable" })
            }
        }
    };

    let _ = db::write_audit(
        &state.db,
        &user.email,
        Some(&app),
        "data_source.test",
        "data source check",
    )
    .await;

    Ok(Json(outcome))
}

/// The top level field names of a document and their types. The values are
/// never returned.
fn field_types(document: &Value) -> Vec<Value> {
    match document.as_object() {
        Some(fields) => fields
            .iter()
            .map(|(name, value)| json!({ "name": name, "type": value_type(value) }))
            .collect(),
        None => Vec::new(),
    }
}

/// The JSON type of a value.
fn value_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{
        setup, setup_with_env, FakeEnv, Harness, APP, DATASOURCE_SECRET_ENV, JWT_SECRET,
        JWT_SECRET_ENV,
    };
    use super::*;
    use axum::http::{Method, StatusCode};
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
    use serde_json::json;
    use std::sync::Arc;

    const ISSUER: &str = "https://auth.example.com/";
    const AUDIENCE: &str = "client-mobile-app";

    /// The identity configuration of a JWT application whose role is a claim.
    fn jwt_identity(secret_env: &str) -> Value {
        json!({
            "provider": {
                "type": "jwt",
                "algorithms": ["HS256"],
                "hs256_secret_env": secret_env,
                "issuer": ISSUER,
                "audience": AUDIENCE,
                "uid_claim": "sub",
                "name_claim": "name",
                "max_age_seconds": 3600
            },
            "role": { "source": "claim", "claim": "role" },
            "admin_roles": ["admin"]
        })
    }

    /// Saves one configuration section of the app.
    async fn save_section(harness: &Harness, section: &str, value: Value) {
        let version = harness
            .state
            .config
            .get(APP)
            .expect("the app exists")
            .version;
        harness
            .state
            .config
            .save_section(APP, section, value, "tester", version)
            .await
            .expect("the section must save");
    }

    /// Builds a signed token.
    fn token(secret: &str) -> String {
        let now = jsonwebtoken::get_current_timestamp();
        let claims = json!({
            "sub": "uid-1",
            "iss": ISSUER,
            "aud": AUDIENCE,
            "exp": now + 3600,
            "iat": now - 10,
            "name": "Test User",
            "email": "user@example.com",
            "role": "admin"
        });
        encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .expect("the token must sign")
    }

    /// The owner harness with the test environment.
    async fn owner() -> Harness {
        setup_with_env("owner", &[], Some(Arc::new(FakeEnv))).await
    }

    #[tokio::test]
    async fn identity_test_ok_returns_role_and_claim_names_only() {
        let harness = owner().await;
        save_section(&harness, "identity", jwt_identity(JWT_SECRET_ENV)).await;

        let (status, body) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/identity/test", APP),
                Some(json!({ "token": token(JWT_SECRET) })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], true);
        assert_eq!(body["uid"], "uid-1");
        assert_eq!(body["name"], "Test User");
        assert_eq!(body["role"], "admin");
        assert_eq!(body["is_admin"], true);

        let names: Vec<String> = body["claim_names"]
            .as_array()
            .expect("the claim names")
            .iter()
            .map(|name| name.as_str().unwrap_or_default().to_string())
            .collect();
        assert!(names.contains(&"sub".to_string()));
        assert!(names.contains(&"role".to_string()));
        assert!(names.contains(&"email".to_string()));

        // The claim values are never returned.
        assert!(body.get("claims").is_none());
        assert!(!body.to_string().contains("user@example.com"));
    }

    #[tokio::test]
    async fn identity_test_invalid_token_is_ok_false_with_reason() {
        let harness = owner().await;
        save_section(&harness, "identity", jwt_identity(JWT_SECRET_ENV)).await;

        let (status, body) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/identity/test", APP),
                Some(json!({ "token": token("another-secret-value-1234567890abcd") })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], false);
        let reason = body["reason"].as_str().expect("a reason");
        assert!(!reason.is_empty());
        assert!(!reason.contains("another-secret-value"));
    }

    #[tokio::test]
    async fn identity_test_never_echoes_token() {
        let harness = owner().await;
        save_section(&harness, "identity", jwt_identity(JWT_SECRET_ENV)).await;
        let token = token(JWT_SECRET);

        let (status, body) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/identity/test", APP),
                Some(json!({ "token": token })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            !body.to_string().contains(&token),
            "the token is never echoed"
        );

        let rows: Vec<(String, Option<String>)> =
            sqlx::query_as("SELECT action, detail FROM audit_log")
                .fetch_all(&harness.pool)
                .await
                .expect("the audit rows must exist");
        assert!(rows.iter().any(|(action, _)| action == "identity.test"));
        for (action, detail) in &rows {
            let detail = detail.clone().unwrap_or_default();
            assert!(
                !detail.contains(&token),
                "the audit {} must not keep the token",
                action
            );
        }
    }

    #[tokio::test]
    async fn identity_test_oversized_token_is_400() {
        let harness = owner().await;
        save_section(&harness, "identity", jwt_identity(JWT_SECRET_ENV)).await;

        let (status, body) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/identity/test", APP),
                Some(json!({ "token": "t".repeat(9000) })),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_request");
    }

    #[tokio::test]
    async fn identity_test_source_down_is_503() {
        // The environment has no secret for this provider.
        let harness = setup("owner", &[]).await;
        save_section(
            &harness,
            "identity",
            jwt_identity("ADMIN_TEST_MISSING_SECRET"),
        )
        .await;

        let (status, body) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/identity/test", APP),
                Some(json!({ "token": token(JWT_SECRET) })),
            )
            .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"], "unavailable");
    }

    #[tokio::test]
    async fn data_source_test_path_returns_field_names_and_types_only() {
        let server = crate::datasource::test_support::TestServer::start(vec![
            crate::datasource::test_support::json(
                200,
                "OK",
                r#"{"name":"Sara","age":30,"tags":["a"]}"#,
            ),
        ])
        .await;
        let harness = owner().await;
        save_section(
            &harness,
            "data_source",
            json!({
                "type": "http",
                "base_url": server.base_url,
                "secret_env": DATASOURCE_SECRET_ENV
            }),
        )
        .await;

        let (status, body) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/data_source/test", APP),
                Some(json!({ "path": "users/u1" })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], true);
        assert_eq!(body["found"], true);

        let fields = body["fields"].as_array().expect("the fields");
        assert_eq!(fields.len(), 3);
        assert!(fields
            .iter()
            .any(|field| field["name"] == "name" && field["type"] == "string"));
        assert!(fields
            .iter()
            .any(|field| field["name"] == "age" && field["type"] == "number"));
        assert!(fields
            .iter()
            .any(|field| field["name"] == "tags" && field["type"] == "array"));
        assert!(!body.to_string().contains("Sara"), "no value is returned");
    }

    #[tokio::test]
    async fn data_source_test_members_returns_count_only() {
        let server = crate::datasource::test_support::TestServer::start(vec![
            crate::datasource::test_support::json(200, "OK", r#"{"uids":["u1","u2"]}"#),
        ])
        .await;
        let harness = owner().await;
        save_section(
            &harness,
            "data_source",
            json!({
                "type": "http",
                "base_url": server.base_url,
                "secret_env": DATASOURCE_SECRET_ENV
            }),
        )
        .await;

        let (status, body) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/data_source/test", APP),
                Some(json!({
                    "members": {
                        "collection": "users",
                        "field": "role",
                        "value": "admin"
                    }
                })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], true);
        assert_eq!(body["count"], 2);
        assert!(body.get("uids").is_none(), "the members are never returned");
        assert!(!body.to_string().contains("u1"));
    }

    #[tokio::test]
    async fn data_source_test_needs_exactly_one_mode() {
        let harness = owner().await;

        let (status, body) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/data_source/test", APP),
                Some(json!({})),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_request");

        let (status, _) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/data_source/test", APP),
                Some(json!({
                    "path": "users/u1",
                    "members": {
                        "collection": "users",
                        "field": "role",
                        "value": "admin"
                    }
                })),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn data_source_test_unavailable_is_generic() {
        let harness = owner().await;
        // The role comes from a claim, so the app may have no data source.
        save_section(&harness, "identity", jwt_identity(JWT_SECRET_ENV)).await;
        save_section(&harness, "data_source", json!({ "type": "none" })).await;

        let (status, body) = harness
            .call(
                Method::POST,
                &format!("/apps/{}/data_source/test", APP),
                Some(json!({ "path": "users/u1" })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], false);
        assert_eq!(body["reason"], "data source unavailable");
        assert!(
            !body.to_string().contains("no data source"),
            "the reason is generic"
        );
    }

    #[tokio::test]
    async fn diagnostics_owner_only() {
        let harness = setup("manager", &[APP]).await;
        let calls = vec![
            (
                format!("/apps/{}/identity/test", APP),
                json!({ "token": "t" }),
            ),
            (
                format!("/apps/{}/data_source/test", APP),
                json!({ "path": "users/u1" }),
            ),
        ];
        for (uri, body) in calls {
            let (status, response) = harness.call(Method::POST, &uri, Some(body)).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{}: {}", uri, response);
            assert_eq!(response["error"], "forbidden");
        }
    }

    #[tokio::test]
    async fn diagnostics_unknown_app_is_404() {
        let harness = owner().await;
        let calls = vec![
            (
                "/apps/ghost/identity/test".to_string(),
                json!({ "token": "t" }),
            ),
            (
                "/apps/ghost/data_source/test".to_string(),
                json!({ "path": "users/u1" }),
            ),
        ];
        for (uri, body) in calls {
            let (status, response) = harness.call(Method::POST, &uri, Some(body)).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{}: {}", uri, response);
            assert_eq!(response["error"], "not_found");
        }
    }
}

//! Dashboard management of the apps and their credential files (T16).
//!
//! Only the owner may install, replace, disable or remove a service account
//! file, and only after a fresh password confirmation. No route ever answers
//! with the file, the private key or any part of it: the dashboard shows the
//! project id, the client email, the private key id and the upload time.

use super::auth::AuthedUser;
use super::{AdminError, AdminJson};
use crate::credentials::{CredentialError, MAX_UPLOAD_BODY_BYTES};
use crate::db;
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

/// A dashboard error that carries its own status and code.
pub enum CredentialAdminError {
    Admin(AdminError),
    /// The app already has a credential file.
    Conflict(&'static str),
    /// The request is larger than the upload limit.
    PayloadTooLarge,
}

impl From<AdminError> for CredentialAdminError {
    fn from(error: AdminError) -> Self {
        CredentialAdminError::Admin(error)
    }
}

impl IntoResponse for CredentialAdminError {
    fn into_response(self) -> Response {
        match self {
            CredentialAdminError::Admin(error) => error.into_response(),
            CredentialAdminError::Conflict(code) => {
                (StatusCode::CONFLICT, Json(json!({ "error": code }))).into_response()
            }
            CredentialAdminError::PayloadTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(json!({
                    "error": "payload_too_large",
                    "message": "the request is larger than 16KB",
                })),
            )
                .into_response(),
        }
    }
}

/// The body of an app creation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateAppRequest {
    pub app: String,
    pub service_account: Value,
}

/// The body of a credential replacement.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplaceCredentialsRequest {
    pub service_account: Value,
}

/// The body of an app deletion: the name of the app, written out.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteAppRequest {
    pub confirm: String,
}

/// The write limit of the credential routes: 10 per minute and dashboard user.
///
/// T13-A: the budget of the credential files is its own; the budget of the
/// failed authentications is never spent here.
fn check_rate_limit(state: &AppState, user: &AuthedUser) -> Result<(), AdminError> {
    if !state
        .admin_limiters
        .credentials
        .check(&format!("admin:credentials:{}", user.user_id))
    {
        return Err(AdminError::too_many_requests());
    }
    Ok(())
}

/// The upload is only allowed when the tool was started for it.
fn require_enabled(state: &AppState) -> Result<(), AdminError> {
    if state.credentials.enabled() {
        Ok(())
    } else {
        // The route is not part of this tool, so it does not exist.
        Err(AdminError::not_found("the route does not exist"))
    }
}

/// The service account of the request, as the bytes of a file.
fn service_account_bytes(value: &Value) -> Result<Vec<u8>, CredentialAdminError> {
    let bytes = serde_json::to_vec(value)
        .map_err(|_| AdminError::invalid_request("the service account is not valid"))?;
    if bytes.len() > MAX_UPLOAD_BODY_BYTES {
        return Err(CredentialAdminError::PayloadTooLarge);
    }
    Ok(bytes)
}

/// The error of a credential operation.
fn map_credential_error(error: CredentialError) -> CredentialAdminError {
    match error {
        // The reason of a live check is logged, never answered.
        CredentialError::Invalid(message) => AdminError::invalid_request(message).into(),
        CredentialError::ProjectMismatch => {
            AdminError::invalid_request("the replacement belongs to another project").into()
        }
        CredentialError::LiveCheckFailed => {
            AdminError::invalid_request("the service account could not be verified").into()
        }
        CredentialError::Exists => CredentialAdminError::Conflict("credentials_exist"),
        CredentialError::NotFound => AdminError::not_found("the app has no credential file").into(),
        CredentialError::Disabled => AdminError::not_found("the route does not exist").into(),
        CredentialError::Io => {
            AdminError::internal("the credential file could not be written").into()
        }
    }
}

/// `POST /apps`: installs the credential file of a new app and creates its
/// default configuration.
pub async fn create_app(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    AdminJson(body): AdminJson<CreateAppRequest>,
) -> Result<(StatusCode, Json<Value>), CredentialAdminError> {
    user.require_owner()?;
    require_enabled(&state)?;
    check_rate_limit(&state, &user)?;
    user.require_reauth(300)?;

    let app = body.app.trim().to_string();
    let bytes = service_account_bytes(&body.service_account)?;

    let outcome = state
        .credentials
        .install(&app, &bytes, false)
        .await
        .map_err(map_credential_error)?;

    // The configuration is created after the file: a failure here must not
    // leave half an app behind.
    if let Err(error) = state.config.create_app(&app, &user.email).await {
        let _ = state.credentials.remove(&app).await;
        let _ = db::forget_app_credentials(&state.db, &app).await;
        return Err(super::map_config_error(error).into());
    }
    let _ = db::record_app_credentials(&state.db, &app, &user.email).await;

    let detail = json!({
        "app": app,
        "project_id": outcome.project_id,
        "client_email": outcome.client_email,
        "private_key_id": outcome.private_key_id,
    })
    .to_string();
    let _ = db::write_audit(&state.db, &user.email, Some(&app), "app.create", &detail).await;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "app": app,
            "project_id": outcome.project_id,
            "client_email": outcome.client_email,
            "private_key_id": outcome.private_key_id,
        })),
    ))
}

/// `PUT /apps/{app}/credentials`: replaces the credential file of an app.
pub async fn replace_credentials(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path(app): Path<String>,
    AdminJson(body): AdminJson<ReplaceCredentialsRequest>,
) -> Result<Json<Value>, CredentialAdminError> {
    user.require_owner()?;
    require_enabled(&state)?;
    check_rate_limit(&state, &user)?;
    user.require_reauth(300)?;

    let bytes = service_account_bytes(&body.service_account)?;
    let outcome = state
        .credentials
        .install(&app, &bytes, true)
        .await
        .map_err(map_credential_error)?;
    let _ = db::record_app_credentials(&state.db, &app, &user.email).await;

    let previous = match &outcome.replaced {
        Some((client_email, private_key_id)) => json!({
            "client_email": client_email,
            "private_key_id": private_key_id,
        }),
        None => Value::Null,
    };
    let detail = json!({
        "app": app,
        "project_id": outcome.project_id,
        "client_email": outcome.client_email,
        "private_key_id": outcome.private_key_id,
    })
    .to_string();
    let _ = db::write_audit(
        &state.db,
        &user.email,
        Some(&app),
        "app.credentials.replace",
        &detail,
    )
    .await;

    Ok(Json(json!({
        "project_id": outcome.project_id,
        "previous": previous,
        "current": {
            "client_email": outcome.client_email,
            "private_key_id": outcome.private_key_id,
        },
        "reminder": "Revoke the old key in Google Cloud Console",
    })))
}

/// `GET /apps/{app}/credentials`: the public information about the file.
pub async fn get_credentials(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path(app): Path<String>,
) -> Result<Json<Value>, CredentialAdminError> {
    user.require_owner()?;
    require_enabled(&state)?;

    let meta = state
        .credentials
        .metadata(&app)
        .await
        .ok_or_else(|| AdminError::not_found("the app has no credential file"))?;
    let upload = db::app_credentials_info(&state.db, &app)
        .await
        .map_err(|error| AdminError::from_db("read the upload information", error))?;
    let disabled = state.credentials.is_disabled(&app).await;

    Ok(Json(json!({
        "app": app,
        "project_id": meta.project_id,
        "client_email": meta.client_email,
        "private_key_id": meta.private_key_id,
        "uploaded_by": upload.as_ref().map(|(by, _)| by.clone()),
        "uploaded_at": upload.map(|(_, at)| at),
        "disabled": disabled,
    })))
}

/// `POST /apps/{app}/disable`: stops the app at once, the file stays.
pub async fn disable_app(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path(app): Path<String>,
) -> Result<Json<Value>, CredentialAdminError> {
    user.require_owner()?;
    require_enabled(&state)?;
    check_rate_limit(&state, &user)?;
    user.require_reauth(300)?;

    state
        .credentials
        .disable(&app)
        .await
        .map_err(map_credential_error)?;
    let _ = db::set_app_disabled(&state.db, &app, &user.email).await;

    let detail = json!({ "app": app }).to_string();
    let _ = db::write_audit(&state.db, &user.email, Some(&app), "app.disable", &detail).await;

    Ok(Json(json!({ "app": app, "disabled": true })))
}

/// `POST /apps/{app}/enable`: starts a disabled app again.
pub async fn enable_app(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path(app): Path<String>,
) -> Result<Json<Value>, CredentialAdminError> {
    user.require_owner()?;
    require_enabled(&state)?;
    check_rate_limit(&state, &user)?;

    state
        .credentials
        .enable(&app)
        .await
        .map_err(map_credential_error)?;
    let _ = db::clear_app_disabled(&state.db, &app).await;

    let detail = json!({ "app": app }).to_string();
    let _ = db::write_audit(&state.db, &user.email, Some(&app), "app.enable", &detail).await;

    Ok(Json(json!({ "app": app, "disabled": false })))
}

/// `DELETE /apps/{app}`: removes the app, its file and its configuration.
///
/// The order is fixed: the exported configuration is written to the audit
/// before anything is deleted, so the history of the app survives the
/// deletion.
pub async fn delete_app(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path(app): Path<String>,
    AdminJson(body): AdminJson<DeleteAppRequest>,
) -> Result<(StatusCode, Json<Value>), CredentialAdminError> {
    user.require_owner()?;
    require_enabled(&state)?;
    check_rate_limit(&state, &user)?;
    user.require_reauth(300)?;

    if body.confirm != app {
        return Err(
            AdminError::invalid_request("the confirmation must be the name of the app").into(),
        );
    }

    // 1. The configuration is written to the audit before any deletion.
    let document = state
        .config
        .export(&app)
        .await
        .map_err(|_| AdminError::not_found("the app does not exist"))?;
    let detail = serde_json::to_string(&document)
        .map_err(|_| AdminError::internal("the configuration could not be written"))?;
    db::write_audit(&state.db, &user.email, Some(&app), "app.delete", &detail)
        .await
        .map_err(|error| AdminError::from_db("write the delete audit", error))?;

    // 2. The credential file goes first: a file without a configuration is
    //    harmless, a configuration without a file is not.
    state
        .credentials
        .remove(&app)
        .await
        .map_err(map_credential_error)?;

    // 3. The configuration and the templates.
    state
        .config
        .delete_app(&app, &user.email)
        .await
        .map_err(|error| -> CredentialAdminError { super::map_config_error(error).into() })?;

    // 4. The devices, the keys and the dedupe keys.
    let _ = db::delete_app_devices(&state.db, &app).await;
    let _ = db::revoke_app_keys(&state.db, &app).await;
    let _ = db::delete_app_dedupe(&state.db, &app).await;
    let _ = db::clear_app_disabled(&state.db, &app).await;
    let _ = db::forget_app_credentials(&state.db, &app).await;

    // 5. The history of the app stays: notifications, campaigns and the
    //    audit log are kept.
    Ok((StatusCode::OK, Json(json!({ "app": app, "deleted": true }))))
}

#[cfg(test)]
mod tests {
    use super::super::tests::{setup, setup_with_upload, Harness, APP};
    use super::*;
    use axum::http::Method;
    use serde_json::json;

    /// A service account file for the tests.
    fn service_account(project_id: &str, key_id: &str) -> Value {
        crate::credentials::tests::test_service_account(project_id, key_id)
    }

    /// The body of an app creation.
    fn create_body(app: &str) -> Value {
        json!({ "app": app, "service_account": service_account("demo-project", "key-1") })
    }

    /// Creates an app through the route.
    async fn create_app(harness: &Harness, app: &str) -> (StatusCode, Value) {
        harness
            .call(Method::POST, "/apps", Some(create_body(app)))
            .await
    }

    #[tokio::test]
    async fn credentials_routes_owner_only() {
        let harness = setup("manager", &[APP]).await;
        let calls = vec![
            (
                Method::POST,
                "/apps".to_string(),
                Some(create_body("newapp")),
            ),
            (Method::GET, format!("/apps/{}/credentials", APP), None),
            (
                Method::PUT,
                format!("/apps/{}/credentials", APP),
                Some(json!({ "service_account": service_account("demo-project", "key-2") })),
            ),
            (Method::POST, format!("/apps/{}/disable", APP), None),
            (Method::POST, format!("/apps/{}/enable", APP), None),
            (
                Method::DELETE,
                format!("/apps/{}", APP),
                Some(json!({ "confirm": APP })),
            ),
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
    }

    #[tokio::test]
    async fn credentials_routes_need_recent_reauth() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let (status, body) = create_app(&harness, "newapp").await;
        assert_eq!(status, StatusCode::CREATED, "body: {}", body);

        // A new session has no password confirmation.
        let fresh = setup("owner", &[]).await;
        let (status, body) = create_app(&fresh, "another").await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["message"], "reauth_required");

        let (status, body) = fresh
            .call(
                Method::PUT,
                &format!("/apps/{}/credentials", "newapp"),
                Some(json!({ "service_account": service_account("demo-project", "key-2") })),
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["message"], "reauth_required");

        let (status, body) = fresh
            .call(Method::POST, &format!("/apps/{}/disable", "newapp"), None)
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["message"], "reauth_required");

        let (status, body) = fresh
            .call(
                Method::DELETE,
                "/apps/newapp",
                Some(json!({ "confirm": "newapp" })),
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["message"], "reauth_required");
    }

    #[tokio::test]
    async fn credentials_upload_disabled_flag_returns_404() {
        let harness = setup_with_upload("owner", &[], false).await;
        harness.reauth().await;
        let (status, body) = create_app(&harness, "newapp").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"], "not_found");
    }

    #[tokio::test]
    async fn create_app_creates_default_config_and_credential() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;

        let (status, body) = create_app(&harness, "newapp").await;
        assert_eq!(status, StatusCode::CREATED, "body: {}", body);
        assert_eq!(body["app"], "newapp");
        assert_eq!(body["project_id"], "demo-project");
        assert!(body["client_email"]
            .as_str()
            .expect("a client email")
            .ends_with(".gserviceaccount.com"));
        assert!(body["private_key_id"].is_string());

        // The configuration exists and the file is on the disk.
        assert!(harness.state.config.get("newapp").is_some());
        let dir = harness.state.credentials.directory();
        assert!(dir.join("newapp.json").exists());
        // The tool knows the app at once.
        assert!(harness
            .state
            .project_map
            .read()
            .await
            .contains_key("newapp"));
    }

    #[tokio::test]
    async fn create_app_rolls_back_file_when_config_fails() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        // The configuration of the app exists, so the creation of it fails.
        harness
            .state
            .config
            .create_app("second", "tester")
            .await
            .expect("the app must be created");

        let (status, body) = create_app(&harness, "second").await;
        assert_eq!(status, StatusCode::CONFLICT, "body: {}", body);

        // The file of the failed app is gone.
        let dir = harness.state.credentials.directory();
        assert!(
            !dir.join("second.json").exists(),
            "the file must be removed"
        );
    }

    #[tokio::test]
    async fn create_app_duplicate_is_409() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let (status, _) = create_app(&harness, "newapp").await;
        assert_eq!(status, StatusCode::CREATED);

        let (status, body) = create_app(&harness, "newapp").await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"], "credentials_exist");
    }

    #[tokio::test]
    async fn create_app_invalid_json_is_400() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let body = json!({
            "app": "newapp",
            "service_account": { "type": "authorized_user", "project_id": "demo" }
        });
        let (status, response) = harness.call(Method::POST, "/apps", Some(body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(response["error"], "invalid_request");
    }

    #[tokio::test]
    async fn create_app_payload_over_16kb_is_413() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let body = json!({
            "app": "newapp",
            "service_account": {
                "type": "service_account",
                "project_id": "demo-project",
                "private_key_id": "key-1",
                "private_key": "x".repeat(17 * 1024),
                "client_email": "service@demo.iam.gserviceaccount.com"
            }
        });
        let (status, response) = harness.call(Method::POST, "/apps", Some(body)).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(response["error"], "payload_too_large");
    }

    #[tokio::test]
    async fn replace_other_project_is_400() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let (status, _) = create_app(&harness, "newapp").await;
        assert_eq!(status, StatusCode::CREATED);

        let (status, body) = harness
            .call(
                Method::PUT,
                "/apps/newapp/credentials",
                Some(json!({ "service_account": service_account("other-project", "key-2") })),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_request");
    }

    #[tokio::test]
    async fn get_credentials_returns_metadata_only() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let (status, _) = create_app(&harness, "newapp").await;
        assert_eq!(status, StatusCode::CREATED);

        let (status, body) = harness
            .call(Method::GET, "/apps/newapp/credentials", None)
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["project_id"], "demo-project");
        assert_eq!(body["client_email"], "service@demo.iam.gserviceaccount.com");
        assert_eq!(body["private_key_id"], "key-1");
        assert_eq!(body["uploaded_by"], "user@example.com");
        assert_eq!(body["disabled"], false);

        let text = body.to_string();
        assert!(!text.contains("BEGIN PRIVATE KEY"), "no key is answered");
        assert!(!text.contains("private_key\""));
    }

    #[tokio::test]
    async fn no_route_returns_file_content() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let (status, _) = create_app(&harness, "newapp").await;
        assert_eq!(status, StatusCode::CREATED);

        let calls = vec![
            (Method::GET, "/apps".to_string(), None),
            (Method::GET, "/apps/newapp".to_string(), None),
            (Method::GET, "/apps/newapp/credentials".to_string(), None),
            (Method::GET, "/apps/newapp/templates".to_string(), None),
            (Method::GET, "/apps/newapp/versions".to_string(), None),
            (Method::GET, "/apps/newapp/keys".to_string(), None),
            (Method::GET, "/apps/newapp/blocks".to_string(), None),
            (Method::GET, "/apps/newapp/history".to_string(), None),
            (Method::GET, "/apps/newapp/devices/stats".to_string(), None),
            (Method::GET, "/audit?app=newapp".to_string(), None),
        ];
        for (method, uri, body) in calls {
            let (status, response) = harness.call(method.clone(), &uri, body).await;
            assert!(
                status != StatusCode::OK || !response.to_string().contains("BEGIN PRIVATE KEY"),
                "{} {} answered the file",
                method,
                uri
            );
        }
    }

    #[tokio::test]
    async fn delete_app_requires_matching_confirm() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let (status, _) = create_app(&harness, "newapp").await;
        assert_eq!(status, StatusCode::CREATED);

        let (status, body) = harness
            .call(
                Method::DELETE,
                "/apps/newapp",
                Some(json!({ "confirm": "other" })),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_request");
        // The app is still there.
        assert!(harness.state.config.get("newapp").is_some());
    }

    #[tokio::test]
    async fn delete_app_exports_config_to_audit_before_deleting() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let (status, _) = create_app(&harness, "newapp").await;
        assert_eq!(status, StatusCode::CREATED);

        let (status, _) = harness
            .call(
                Method::DELETE,
                "/apps/newapp",
                Some(json!({ "confirm": "newapp" })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);

        let detail: Option<String> = sqlx::query_scalar(
            "SELECT detail FROM audit_log WHERE app = ? AND action = 'app.delete'",
        )
        .bind("newapp")
        .fetch_one(&harness.pool)
        .await
        .expect("the audit row must exist");
        let detail = detail.expect("the detail must be there");
        assert!(
            detail.contains("\"identity\""),
            "the export must be in the audit: {}",
            detail
        );
    }

    #[tokio::test]
    async fn delete_app_revokes_keys_and_devices() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let (status, _) = create_app(&harness, "newapp").await;
        assert_eq!(status, StatusCode::CREATED);

        // A device and a key of the app.
        sqlx::query("INSERT INTO devices (app, token, uid, platform) VALUES (?, ?, ?, ?)")
            .bind("newapp")
            .bind("token-0123456789abcdef0123456789abcdef")
            .bind("u1")
            .bind("android")
            .execute(&harness.pool)
            .await
            .expect("the device must be inserted");
        harness
            .state
            .auth
            .app_keys()
            .expect("the key store must be known")
            .create("newapp", "backend", "tester")
            .await
            .expect("the key must be created");

        let (status, _) = harness
            .call(
                Method::DELETE,
                "/apps/newapp",
                Some(json!({ "confirm": "newapp" })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);

        let devices: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM devices WHERE app = ?")
            .bind("newapp")
            .fetch_one(&harness.pool)
            .await
            .expect("the count must exist");
        assert_eq!(devices, 0, "the devices must be gone");

        let revoked: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM app_keys WHERE app = ? AND revoked_at IS NOT NULL",
        )
        .bind("newapp")
        .fetch_one(&harness.pool)
        .await
        .expect("the count must exist");
        assert_eq!(revoked, 1, "the key must be revoked");
    }

    #[tokio::test]
    async fn delete_app_keeps_history() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let (status, _) = create_app(&harness, "newapp").await;
        assert_eq!(status, StatusCode::CREATED);

        sqlx::query(
            "INSERT INTO notifications_history (app, target, title, status) VALUES (?, ?, ?, ?)",
        )
        .bind("newapp")
        .bind("topic:news")
        .bind("A title")
        .bind("success")
        .execute(&harness.pool)
        .await
        .expect("the history row must be inserted");

        let (status, _) = harness
            .call(
                Method::DELETE,
                "/apps/newapp",
                Some(json!({ "confirm": "newapp" })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);

        let history: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notifications_history WHERE app = ?")
                .bind("newapp")
                .fetch_one(&harness.pool)
                .await
                .expect("the count must exist");
        assert_eq!(history, 1, "the history of the app stays");
    }

    #[tokio::test]
    async fn audit_has_no_private_key() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let (status, _) = create_app(&harness, "newapp").await;
        assert_eq!(status, StatusCode::CREATED);

        let rows: Vec<(String, Option<String>)> =
            sqlx::query_as("SELECT action, detail FROM audit_log")
                .fetch_all(&harness.pool)
                .await
                .expect("the audit rows must exist");
        assert!(!rows.is_empty());
        for (action, detail) in &rows {
            let detail = detail.clone().unwrap_or_default();
            assert!(
                !detail.contains("BEGIN PRIVATE KEY"),
                "the audit {} holds the key",
                action
            );
            // The private key id is an identifier and may be there; the key
            // itself may not.
            assert!(
                !detail.contains("\"private_key\""),
                "the audit {} holds the key",
                action
            );
            assert!(
                !detail.contains("PRIVATE KEY"),
                "the audit {} holds the key",
                action
            );
        }
    }

    #[tokio::test]
    async fn credentials_rate_limited() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        for index in 0..10 {
            let (status, body) = create_app(&harness, &format!("app{}", index)).await;
            assert_eq!(status, StatusCode::CREATED, "write {}: {}", index, body);
        }

        let (status, body) = create_app(&harness, "one-too-many").await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(body["error"], "rate_limited");
    }

    #[tokio::test]
    async fn new_app_can_send_without_restart() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let (status, _) = create_app(&harness, "newapp").await;
        assert_eq!(status, StatusCode::CREATED);

        // The app is known by the tool at once: no restart is needed.
        let project = harness
            .state
            .project_map
            .read()
            .await
            .get("newapp")
            .cloned()
            .unwrap_or_default();
        assert_eq!(project, "demo-project");
    }
}

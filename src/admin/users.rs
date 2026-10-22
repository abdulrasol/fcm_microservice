//! Dashboard tool accounts (T10-C).
//!
//! Only the owner manages accounts, and only managers are created here: an
//! owner is created with the CLI, never over HTTP. A temporary password is
//! shown once, in the response of the creation or of the reset, and is never
//! stored in clear text, never audited and never logged.

use super::auth::{AuthedUser, Role};
use super::{AdminError, AdminJson};
use crate::db;
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

/// The largest number of apps one manager can be bound to.
const MAX_APPS_PER_USER: usize = 50;

/// A dashboard error that carries its own error code.
pub enum UserAdminError {
    Admin(AdminError),
    Conflict(&'static str),
}

impl From<AdminError> for UserAdminError {
    fn from(error: AdminError) -> Self {
        UserAdminError::Admin(error)
    }
}

impl IntoResponse for UserAdminError {
    fn into_response(self) -> Response {
        match self {
            UserAdminError::Admin(error) => error.into_response(),
            UserAdminError::Conflict(code) => {
                (StatusCode::CONFLICT, Json(json!({ "error": code }))).into_response()
            }
        }
    }
}

/// The write limit of the account routes: 30 per minute and dashboard user.
///
/// T13-A: the budget of the administrative writes is its own; the limit of
/// the device registrations is never spent here.
pub fn check_write_limit(state: &AppState, user: &AuthedUser) -> Result<(), AdminError> {
    if !state
        .admin_limiters
        .admin_writes
        .check(&format!("admin:users:{}", user.user_id))
    {
        return Err(AdminError::too_many_requests());
    }
    Ok(())
}

/// `GET /users`: the dashboard accounts, without any secret.
pub async fn list_users(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
) -> Result<Json<Value>, AdminError> {
    user.require_owner()?;

    let accounts = db::list_accounts(&state.db)
        .await
        .map_err(|error| AdminError::from_db("read the accounts", error))?;

    let items: Vec<Value> = accounts
        .iter()
        .map(|account| {
            json!({
                "id": account.id,
                "email": account.email,
                "role": account.role,
                "apps": account.apps,
                "disabled": account.disabled,
                "must_change_password": account.must_change_password,
                "test_uid": account.test_uid,
                "created_at": account.created_at,
            })
        })
        .collect();

    Ok(Json(Value::Array(items)))
}

/// The body of an account creation. The role is not a field: only a manager
/// is created here.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateUserRequest {
    pub email: String,
    pub apps: Vec<String>,
}

/// `POST /users`: creates a manager and shows its temporary password once.
pub async fn create_user(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    AdminJson(body): AdminJson<CreateUserRequest>,
) -> Result<Response, UserAdminError> {
    user.require_owner()?;
    check_write_limit(&state, &user)?;
    user.require_reauth(300)?;

    let email = crate::cli::normalize_email(&body.email)
        .map_err(|message| AdminError::invalid_request(&message))?;
    let apps = validate_apps(&state, &body.apps).await?;

    if db::account_exists(&state.db, &email)
        .await
        .map_err(|error| AdminError::from_db("read the accounts", error))?
    {
        return Err(UserAdminError::Conflict("duplicate_email"));
    }

    let temporary = crate::cli::generate_temp_password();
    let hash = super::password::hash_password(&temporary)
        .await
        .map_err(|error| AdminError::internal(&error))?;
    let id = db::insert_account(&state.db, &email, &hash, &apps)
        .await
        .map_err(|error| AdminError::from_db("create the account", error))?;

    let detail = json!({ "id": id, "email": email, "apps": apps }).to_string();
    let _ = db::write_audit(&state.db, &user.email, None, "user.create", &detail).await;

    Ok(temporary_password_response(
        StatusCode::CREATED,
        id,
        &email,
        &temporary,
    ))
}

/// The body of an account update.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateUserRequest {
    #[serde(default)]
    pub apps: Option<Vec<String>>,
    #[serde(default)]
    pub disabled: Option<bool>,
}

/// `PUT /users/{id}`: the app bindings and the enabled flag of a manager.
pub async fn update_user(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path(id): Path<i64>,
    AdminJson(body): AdminJson<UpdateUserRequest>,
) -> Result<Json<Value>, UserAdminError> {
    user.require_owner()?;
    check_write_limit(&state, &user)?;

    let target = db::get_account(&state.db, id)
        .await
        .map_err(|error| AdminError::from_db("read the account", error))?
        .ok_or_else(|| AdminError::not_found("the account does not exist"))?;
    if target.role == Role::Owner.as_str() || target.id == user.user_id {
        return Err(AdminError::forbidden("owners are managed with the CLI").into());
    }

    let mut sessions_dropped = 0u64;
    if let Some(apps) = &body.apps {
        let apps = validate_apps(&state, apps).await?;
        db::replace_account_apps(&state.db, id, &apps)
            .await
            .map_err(|error| AdminError::from_db("replace the app bindings", error))?;
        // A new set of apps is a new authorization: the old sessions go.
        sessions_dropped = db::delete_account_sessions(&state.db, id)
            .await
            .map_err(|error| AdminError::from_db("delete the sessions", error))?;
    }
    if let Some(disabled) = body.disabled {
        db::set_account_disabled(&state.db, id, disabled)
            .await
            .map_err(|error| AdminError::from_db("disable the account", error))?;
        if disabled {
            sessions_dropped = db::delete_account_sessions(&state.db, id)
                .await
                .map_err(|error| AdminError::from_db("delete the sessions", error))?;
        }
    }

    let detail = json!({
        "id": id,
        "apps": body.apps,
        "disabled": body.disabled,
        "sessions_dropped": sessions_dropped,
    })
    .to_string();
    let _ = db::write_audit(&state.db, &user.email, None, "user.update", &detail).await;

    Ok(Json(json!({ "id": id, "updated": true })))
}

/// `POST /users/{id}/reset-password`: a new temporary password, shown once.
pub async fn reset_password(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path(id): Path<i64>,
) -> Result<Response, UserAdminError> {
    user.require_owner()?;
    check_write_limit(&state, &user)?;
    user.require_reauth(300)?;

    let target = db::get_account(&state.db, id)
        .await
        .map_err(|error| AdminError::from_db("read the account", error))?
        .ok_or_else(|| AdminError::not_found("the account does not exist"))?;
    if target.role == Role::Owner.as_str() || target.id == user.user_id {
        return Err(AdminError::forbidden("owners are managed with the CLI").into());
    }

    let temporary = crate::cli::generate_temp_password();
    let hash = super::password::hash_password(&temporary)
        .await
        .map_err(|error| AdminError::internal(&error))?;
    db::set_account_password(&state.db, id, &hash)
        .await
        .map_err(|error| AdminError::from_db("reset the password", error))?;
    db::delete_account_sessions(&state.db, id)
        .await
        .map_err(|error| AdminError::from_db("delete the sessions", error))?;

    let detail = json!({ "id": id, "email": target.email }).to_string();
    let _ = db::write_audit(&state.db, &user.email, None, "user.reset_password", &detail).await;

    Ok(temporary_password_response(
        StatusCode::CREATED,
        id,
        &target.email,
        &temporary,
    ))
}

/// `DELETE /users/{id}`: removes a manager, its sessions and its bindings.
pub async fn delete_user(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path(id): Path<i64>,
) -> Result<(StatusCode, Json<Value>), UserAdminError> {
    user.require_owner()?;
    check_write_limit(&state, &user)?;

    let target = db::get_account(&state.db, id)
        .await
        .map_err(|error| AdminError::from_db("read the account", error))?
        .ok_or_else(|| AdminError::not_found("the account does not exist"))?;
    if target.role == Role::Owner.as_str() || target.id == user.user_id {
        return Err(AdminError::forbidden("owners are managed with the CLI").into());
    }

    db::delete_account(&state.db, id)
        .await
        .map_err(|error| AdminError::from_db("delete the account", error))?;

    let detail = json!({ "id": id, "email": target.email }).to_string();
    let _ = db::write_audit(&state.db, &user.email, None, "user.delete", &detail).await;

    Ok((
        StatusCode::NO_CONTENT,
        Json(json!({ "id": id, "deleted": true })),
    ))
}

/// The response that shows a temporary password once.
///
/// The password must never be cached, so the response carries `no-store`.
fn temporary_password_response(
    status: StatusCode,
    id: i64,
    email: &str,
    temporary: &str,
) -> Response {
    (
        status,
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({
            "id": id,
            "email": email,
            "temporary_password": temporary,
        })),
    )
        .into_response()
}

/// Checks the app bindings of an account.
///
/// The list must not be empty, must not hold more than
/// [`MAX_APPS_PER_USER`] apps, and every app must exist.
async fn validate_apps(state: &AppState, apps: &[String]) -> Result<Vec<String>, AdminError> {
    if apps.is_empty() {
        return Err(AdminError::invalid_request("at least one app is required"));
    }
    if apps.len() > MAX_APPS_PER_USER {
        return Err(AdminError::invalid_request(
            "at most 50 apps are allowed for one account",
        ));
    }

    let configured = state.config.app_names();
    let credentials = state.project_map.read().await;
    let mut unique: Vec<String> = Vec::new();
    for app in apps {
        let app = app.trim();
        if app.is_empty() {
            return Err(AdminError::invalid_request("the app name is not valid"));
        }
        if !configured.iter().any(|name| name == app) && !credentials.contains_key(app) {
            return Err(AdminError::invalid_request(&format!(
                "the app '{}' does not exist",
                app
            )));
        }
        if !unique.iter().any(|name| name == app) {
            unique.push(app.to_string());
        }
    }
    unique.sort();
    Ok(unique)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{
        call, call_raw, insert_user, login_request, session_token, setup, Harness, APP, OTHER_APP,
    };
    use super::*;
    use crate::admin::session;
    use axum::http::Method;
    use serde_json::json;

    /// Creates a manager through the route and returns its id and password.
    async fn create_manager(harness: &Harness, email: &str, apps: &[&str]) -> (i64, String) {
        let apps: Vec<String> = apps.iter().map(|app| app.to_string()).collect();
        let (status, body) = harness
            .call(
                Method::POST,
                "/users",
                Some(json!({ "email": email, "apps": apps })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "body: {}", body);
        let id = body["id"].as_i64().expect("an id");
        let password = body["temporary_password"]
            .as_str()
            .expect("a password")
            .to_string();
        (id, password)
    }

    /// Creates a session for one account.
    async fn create_session(harness: &Harness, user_id: i64) {
        let cookie = session::CookieConfig::with_hours(true, 12);
        session::create_session(&harness.pool, &cookie, user_id, None, None)
            .await
            .expect("the session must be created");
    }

    /// The number of sessions of one account.
    async fn session_count(harness: &Harness, user_id: i64) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM tool_sessions WHERE user_id = ?")
            .bind(user_id)
            .fetch_one(&harness.pool)
            .await
            .expect("the count must exist")
    }

    /// A second owner, created the way the CLI creates one.
    async fn insert_owner(harness: &Harness, email: &str) -> i64 {
        insert_user(&harness.pool, email, "owner", false).await
    }

    #[tokio::test]
    async fn users_list_hides_hash_and_is_owner_only() {
        let harness = setup("owner", &[]).await;
        let (status, body) = harness.call(Method::GET, "/users", None).await;
        assert_eq!(status, StatusCode::OK);
        let rows = body.as_array().expect("an array");
        assert!(!rows.is_empty(), "the owner is listed");
        let text = body.to_string();
        assert!(!text.contains("password_hash"), "the list has no hash");
        assert!(!text.contains("$argon2"), "the list has no hash");

        let manager = setup("manager", &[APP]).await;
        let (status, body) = manager.call(Method::GET, "/users", None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"], "forbidden");
    }

    #[tokio::test]
    async fn create_user_returns_temp_password_once() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;

        let (status, body) = harness
            .call(
                Method::POST,
                "/users",
                Some(json!({ "email": "manager@example.com", "apps": [APP] })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
        let password = body["temporary_password"]
            .as_str()
            .expect("a password")
            .to_string();
        assert!(password.len() >= 16, "the password is long enough");
        assert_eq!(body["email"], "manager@example.com");

        // The password is shown once: the list never holds it.
        let (_, list) = harness.call(Method::GET, "/users", None).await;
        assert!(!list.to_string().contains(&password));

        // The account works with the temporary password.
        let login = login_request(&harness.router, "manager@example.com", &password).await;
        assert_eq!(login.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn create_user_rejects_role_field() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let (status, body) = harness
            .call(
                Method::POST,
                "/users",
                Some(json!({
                    "email": "manager@example.com",
                    "apps": [APP],
                    "role": "owner"
                })),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_request");
    }

    #[tokio::test]
    async fn create_user_needs_recent_reauth() {
        let harness = setup("owner", &[]).await;
        let (status, body) = harness
            .call(
                Method::POST,
                "/users",
                Some(json!({ "email": "manager@example.com", "apps": [APP] })),
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["message"], "reauth_required");
    }

    #[tokio::test]
    async fn create_user_duplicate_email_is_409_case_insensitive() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        create_manager(&harness, "manager@example.com", &[APP]).await;

        let (status, body) = harness
            .call(
                Method::POST,
                "/users",
                Some(json!({ "email": "MANAGER@example.com", "apps": [APP] })),
            )
            .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"], "duplicate_email");
    }

    #[tokio::test]
    async fn create_user_requires_existing_apps() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let (status, body) = harness
            .call(
                Method::POST,
                "/users",
                Some(json!({ "email": "manager@example.com", "apps": ["ghost"] })),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_request");
    }

    #[tokio::test]
    async fn create_user_empty_or_too_many_apps_is_400() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;

        let (status, _) = harness
            .call(
                Method::POST,
                "/users",
                Some(json!({ "email": "manager@example.com", "apps": [] })),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let many: Vec<String> = (0..51).map(|_| APP.to_string()).collect();
        let (status, body) = harness
            .call(
                Method::POST,
                "/users",
                Some(json!({ "email": "manager@example.com", "apps": many })),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_request");
    }

    #[tokio::test]
    async fn created_user_must_change_password_before_other_routes() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let (_, password) = create_manager(&harness, "manager@example.com", &[APP]).await;

        let login = login_request(&harness.router, "manager@example.com", &password).await;
        let token = session_token(&login);
        let (status, body) = call(
            &harness.router,
            Method::GET,
            "/apps",
            None,
            Some(&token),
            true,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"], "password_change_required");
    }

    #[tokio::test]
    async fn temp_password_never_in_audit_or_logs_or_list() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let (_, password) = create_manager(&harness, "manager@example.com", &[APP]).await;

        let rows: Vec<(String, Option<String>)> =
            sqlx::query_as("SELECT action, detail FROM audit_log")
                .fetch_all(&harness.pool)
                .await
                .expect("the audit rows must exist");
        assert!(!rows.is_empty());
        for (action, detail) in &rows {
            let detail = detail.clone().unwrap_or_default();
            assert!(
                !detail.contains(&password),
                "the audit {} must not keep the password",
                action
            );
        }

        let (_, list) = harness.call(Method::GET, "/users", None).await;
        assert!(!list.to_string().contains(&password));

        let hash: String =
            sqlx::query_scalar("SELECT password_hash FROM tool_users WHERE email = ?")
                .bind("manager@example.com")
                .fetch_one(&harness.pool)
                .await
                .expect("the row must exist");
        assert_ne!(hash, password, "the password is never stored in clear");
        assert!(hash.starts_with("$argon2"), "the password is hashed");
    }

    #[tokio::test]
    async fn admin_writes_limited_per_user_not_global() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let (id, _) = create_manager(&harness, "manager@example.com", &[APP]).await;

        // A second owner, with its own session.
        let second_id = insert_owner(&harness, "second@example.com").await;
        create_session(&harness, second_id).await;
        let second_token = session_token(
            &login_request(
                &harness.router,
                "second@example.com",
                super::super::tests::PASSWORD,
            )
            .await,
        );

        // The first owner spends its whole budget of administrative writes.
        // The creation of the manager above already spent one of them.
        for attempt in 0..29 {
            let (status, _) = harness
                .call(
                    Method::PUT,
                    &format!("/users/{}", id),
                    Some(json!({ "apps": [APP] })),
                )
                .await;
            assert_eq!(status, StatusCode::OK, "attempt {}", attempt);
        }
        let (status, body) = harness
            .call(
                Method::PUT,
                &format!("/users/{}", id),
                Some(json!({ "apps": [APP] })),
            )
            .await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(body["error"], "rate_limited");

        // The second account has its own budget.
        let (status, _) = call(
            &harness.router,
            Method::PUT,
            &format!("/users/{}", id),
            Some(json!({ "apps": [APP] })),
            Some(&second_token),
            true,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn update_user_apps_replaces_set_and_kills_sessions() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let (id, _) = create_manager(&harness, "manager@example.com", &[APP]).await;
        create_session(&harness, id).await;
        assert_eq!(session_count(&harness, id).await, 1);

        let (status, _) = harness
            .call(
                Method::PUT,
                &format!("/users/{}", id),
                Some(json!({ "apps": [OTHER_APP] })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);

        let apps: Vec<String> =
            sqlx::query_scalar("SELECT app FROM tool_user_apps WHERE user_id = ? ORDER BY app")
                .bind(id)
                .fetch_all(&harness.pool)
                .await
                .expect("the bindings must exist");
        assert_eq!(apps, vec![OTHER_APP.to_string()], "the set is replaced");
        assert_eq!(
            session_count(&harness, id).await,
            0,
            "the old sessions are gone"
        );
    }

    #[tokio::test]
    async fn update_user_disable_kills_sessions_and_blocks_login() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let (id, password) = create_manager(&harness, "manager@example.com", &[APP]).await;
        create_session(&harness, id).await;

        let (status, _) = harness
            .call(
                Method::PUT,
                &format!("/users/{}", id),
                Some(json!({ "disabled": true })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(session_count(&harness, id).await, 0);

        let login = login_request(&harness.router, "manager@example.com", &password).await;
        assert_eq!(login.status(), StatusCode::UNAUTHORIZED);

        let (_, list) = harness.call(Method::GET, "/users", None).await;
        let manager = list
            .as_array()
            .expect("an array")
            .iter()
            .find(|row| row["email"] == "manager@example.com")
            .expect("the manager must be listed");
        assert_eq!(manager["disabled"], true);
    }

    #[tokio::test]
    async fn update_owner_target_is_403() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let other_owner = insert_owner(&harness, "second@example.com").await;

        let (status, body) = harness
            .call(
                Method::PUT,
                &format!("/users/{}", other_owner),
                Some(json!({ "disabled": true })),
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["message"], "owners are managed with the CLI");
    }

    #[tokio::test]
    async fn owner_cannot_modify_self_via_users_route() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let (status, body) = harness
            .call(
                Method::PUT,
                &format!("/users/{}", harness.user_id),
                Some(json!({ "disabled": true })),
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["message"], "owners are managed with the CLI");
    }

    #[tokio::test]
    async fn reset_password_returns_new_temp_and_kills_sessions() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let (id, _) = create_manager(&harness, "manager@example.com", &[APP]).await;
        create_session(&harness, id).await;

        let (status, body) = harness
            .call(Method::POST, &format!("/users/{}/reset-password", id), None)
            .await;
        assert_eq!(status, StatusCode::CREATED);
        let password = body["temporary_password"]
            .as_str()
            .expect("a password")
            .to_string();
        assert_eq!(session_count(&harness, id).await, 0);

        let login = login_request(&harness.router, "manager@example.com", &password).await;
        assert_eq!(login.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn reset_password_needs_reauth() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let (id, _) = create_manager(&harness, "manager@example.com", &[APP]).await;

        // A fresh confirmation is needed again for the reset.
        let (status, _body) = harness
            .call(Method::POST, &format!("/users/{}/reset-password", id), None)
            .await;
        assert_eq!(status, StatusCode::CREATED, "the reauth is still fresh");

        let other = setup("owner", &[]).await;
        let (status, body) = other
            .call(Method::POST, &format!("/users/{}/reset-password", id), None)
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["message"], "reauth_required");
    }

    #[tokio::test]
    async fn reset_password_owner_target_is_403() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let other_owner = insert_owner(&harness, "second@example.com").await;

        let (status, body) = harness
            .call(
                Method::POST,
                &format!("/users/{}/reset-password", other_owner),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["message"], "owners are managed with the CLI");
    }

    #[tokio::test]
    async fn delete_user_cascades_apps_and_sessions() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let (id, _) = create_manager(&harness, "manager@example.com", &[APP]).await;
        create_session(&harness, id).await;

        let (status, _) = harness
            .call(Method::DELETE, &format!("/users/{}", id), None)
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        let accounts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tool_users WHERE id = ?")
            .bind(id)
            .fetch_one(&harness.pool)
            .await
            .expect("the count must exist");
        assert_eq!(accounts, 0);
        assert_eq!(session_count(&harness, id).await, 0);
        let bindings: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM tool_user_apps WHERE user_id = ?")
                .bind(id)
                .fetch_one(&harness.pool)
                .await
                .expect("the count must exist");
        assert_eq!(bindings, 0, "the app bindings are deleted too");
    }

    #[tokio::test]
    async fn delete_self_or_owner_is_403() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let other_owner = insert_owner(&harness, "second@example.com").await;

        let (status, body) = harness
            .call(Method::DELETE, &format!("/users/{}", harness.user_id), None)
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["message"], "owners are managed with the CLI");

        let (status, _) = harness
            .call(Method::DELETE, &format!("/users/{}", other_owner), None)
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn unknown_user_is_404() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let (status, body) = harness
            .call(
                Method::PUT,
                "/users/9999",
                Some(json!({ "disabled": true })),
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"], "not_found");

        let (status, _) = harness
            .call(Method::POST, "/users/9999/reset-password", None)
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, _) = harness.call(Method::DELETE, "/users/9999", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn manager_gets_403_on_every_users_route() {
        let harness = setup("manager", &[APP]).await;
        // A fresh confirmation, so the refusal comes from the role and not
        // from the missing re-authentication.
        harness.reauth().await;
        let calls = vec![
            (Method::GET, "/users".to_string(), None),
            (
                Method::POST,
                "/users".to_string(),
                Some(json!({ "email": "x@example.com", "apps": [APP] })),
            ),
            (
                Method::PUT,
                "/users/1".to_string(),
                Some(json!({ "disabled": true })),
            ),
            (Method::POST, "/users/1/reset-password".to_string(), None),
            (Method::DELETE, "/users/1".to_string(), None),
        ];
        for (method, uri, body) in calls {
            let (status, _response) = harness.call(method.clone(), &uri, body).await;
            // The body is not shown: a refusal must never print a password.
            assert_eq!(
                status,
                StatusCode::FORBIDDEN,
                "{} {} must be refused",
                method,
                uri
            );
        }
    }

    #[tokio::test]
    async fn users_write_rate_limited() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        // T13-A: the administrative writes have their own budget of 30 per
        // minute and account. The update of a manager is used here because it
        // is cheap: the creation hashes a password.
        let (id, _) = create_manager(&harness, "manager@example.com", &[APP]).await;
        for index in 0..29 {
            let (status, body) = harness
                .call(
                    Method::PUT,
                    &format!("/users/{}", id),
                    Some(json!({ "apps": [APP] })),
                )
                .await;
            assert_eq!(status, StatusCode::OK, "write {}: {}", index, body);
        }

        let (status, body) = harness
            .call(
                Method::PUT,
                &format!("/users/{}", id),
                Some(json!({ "apps": [APP] })),
            )
            .await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(body["error"], "rate_limited");
    }

    #[tokio::test]
    async fn users_responses_have_no_store_header_where_secret() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;

        let response = call_raw(
            &harness.router,
            Method::POST,
            "/users",
            Some(json!({ "email": "manager@example.com", "apps": [APP] })),
            Some(&harness.token),
            true,
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let cache_control = response
            .headers()
            .get(header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert_eq!(cache_control, "no-store");

        let (_, body) = harness.call(Method::GET, "/users", None).await;
        let id = body
            .as_array()
            .expect("an array")
            .iter()
            .find(|row| row["email"] == "manager@example.com")
            .and_then(|row| row["id"].as_i64())
            .expect("an id");

        let response = call_raw(
            &harness.router,
            Method::POST,
            &format!("/users/{}/reset-password", id),
            None,
            Some(&harness.token),
            true,
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let cache_control = response
            .headers()
            .get(header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert_eq!(cache_control, "no-store");
    }

    // ---------- the test uid ----------

    #[tokio::test]
    async fn test_uid_set_by_self_only() {
        let harness = setup("owner", &[]).await;
        let (status, body) = harness
            .call(
                Method::PUT,
                "/auth/test-uid",
                Some(json!({ "test_uid": "user-42" })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["test_uid"], "user-42");

        let value: Option<String> =
            sqlx::query_scalar("SELECT test_uid FROM tool_users WHERE id = ?")
                .bind(harness.user_id)
                .fetch_one(&harness.pool)
                .await
                .expect("the row must exist");
        assert_eq!(value.as_deref(), Some("user-42"));

        // Another account keeps its own value.
        let other_id = insert_user(&harness.pool, "other@example.com", "manager", false).await;
        let other: Option<String> =
            sqlx::query_scalar("SELECT test_uid FROM tool_users WHERE id = ?")
                .bind(other_id)
                .fetch_one(&harness.pool)
                .await
                .expect("the row must exist");
        assert!(other.is_none());
    }

    #[tokio::test]
    async fn test_uid_validation_rejects_bad_values() {
        let harness = setup("owner", &[]).await;
        for value in [String::new(), "x".repeat(200)] {
            let (status, body) = harness
                .call(
                    Method::PUT,
                    "/auth/test-uid",
                    Some(json!({ "test_uid": value })),
                )
                .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "uid {}", value.len());
            assert_eq!(body["error"], "invalid_request");
        }
    }

    #[tokio::test]
    async fn test_uid_null_clears() {
        let harness = setup("owner", &[]).await;
        let (status, _) = harness
            .call(
                Method::PUT,
                "/auth/test-uid",
                Some(json!({ "test_uid": "user-42" })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);

        let (status, body) = harness
            .call(
                Method::PUT,
                "/auth/test-uid",
                Some(json!({ "test_uid": null })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["test_uid"].is_null());

        let value: Option<String> =
            sqlx::query_scalar("SELECT test_uid FROM tool_users WHERE id = ?")
                .bind(harness.user_id)
                .fetch_one(&harness.pool)
                .await
                .expect("the row must exist");
        assert!(value.is_none(), "the test uid is cleared");
    }

    #[tokio::test]
    async fn test_uid_body_cannot_target_other_user() {
        let harness = setup("owner", &[]).await;
        let (status, body) = harness
            .call(
                Method::PUT,
                "/auth/test-uid",
                Some(json!({ "test_uid": "user-42", "user_id": 999 })),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid_request");
    }

    #[tokio::test]
    async fn me_still_hides_test_uid() {
        let harness = setup("owner", &[]).await;
        let (status, _) = harness
            .call(
                Method::PUT,
                "/auth/test-uid",
                Some(json!({ "test_uid": "user-42" })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);

        let (status, body) = harness.call(Method::GET, "/auth/me", None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            body.get("test_uid").is_none(),
            "the profile never shows the test uid"
        );
    }
}

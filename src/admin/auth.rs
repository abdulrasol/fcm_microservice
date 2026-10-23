//! Dashboard accounts: login, logout, password change, reauth and the session
//! extractor used by every protected route.

use super::session;
use super::{AdminError, AdminJson, ToolAuth};
use crate::auth::client_ip;
use crate::db;
use crate::AppState;
use axum::extract::{ConnectInfo, FromRequestParts, State};
use axum::http::header::{self, HeaderMap, HeaderValue};
use axum::http::request::Parts;
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{Row, SqlitePool};
use std::net::SocketAddr;
use std::sync::Arc;

/// The dashboard role. Anything unrecognised is treated as `Manager`, the
/// least privileged role.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Owner,
    Manager,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Manager => "manager",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "owner" => Some(Role::Owner),
            "manager" => Some(Role::Manager),
            _ => None,
        }
    }
}

/// A dashboard account as stored. `password_hash` never reaches a response.
#[derive(Clone, Debug)]
pub struct ToolUser {
    pub id: i64,
    pub email: String,
    pub password_hash: String,
    pub role: Role,
    pub must_change_password: bool,
    pub disabled: bool,
    pub apps: Vec<String>,
}

/// The response shape of a user: no hash, no `test_uid`, nothing extra.
#[derive(Serialize)]
pub struct UserView<'a> {
    pub id: i64,
    pub email: &'a str,
    pub role: &'a str,
    pub apps: &'a [String],
    pub must_change_password: bool,
}

impl ToolUser {
    pub fn view(&self) -> UserView<'_> {
        UserView {
            id: self.id,
            email: &self.email,
            role: self.role.as_str(),
            apps: &self.apps,
            must_change_password: self.must_change_password,
        }
    }
}

async fn load_apps(pool: &SqlitePool, user_id: i64) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT app FROM tool_user_apps WHERE user_id = ? ORDER BY app")
        .bind(user_id)
        .fetch_all(pool)
        .await
}

fn user_from_row(row: &sqlx::sqlite::SqliteRow, apps: Vec<String>) -> ToolUser {
    let role: String = row.get("role");
    ToolUser {
        id: row.get("id"),
        email: row.get("email"),
        password_hash: row.get("password_hash"),
        // The table has a CHECK constraint, so this is only a safety net.
        role: Role::parse(&role).unwrap_or(Role::Manager),
        must_change_password: row.get::<i64, _>("must_change_password") != 0,
        disabled: row.get::<i64, _>("disabled") != 0,
        apps,
    }
}

pub async fn load_user_by_email(
    pool: &SqlitePool,
    email: &str,
) -> Result<Option<ToolUser>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, email, password_hash, role, must_change_password, disabled \
         FROM tool_users WHERE email = ?",
    )
    .bind(email)
    .fetch_optional(pool)
    .await?;

    match row {
        Some(row) => {
            let id: i64 = row.get("id");
            let apps = load_apps(pool, id).await?;
            Ok(Some(user_from_row(&row, apps)))
        }
        None => Ok(None),
    }
}

pub async fn load_user_by_id(
    pool: &SqlitePool,
    user_id: i64,
) -> Result<Option<ToolUser>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, email, password_hash, role, must_change_password, disabled \
         FROM tool_users WHERE id = ?",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    match row {
        Some(row) => {
            let apps = load_apps(pool, user_id).await?;
            Ok(Some(user_from_row(&row, apps)))
        }
        None => Ok(None),
    }
}

/// The `test_uid` bound to a dashboard account (test-send destination).
pub async fn load_test_uid(pool: &SqlitePool, user_id: i64) -> Result<Option<String>, sqlx::Error> {
    let value: Option<Option<String>> =
        sqlx::query_scalar("SELECT test_uid FROM tool_users WHERE id = ?")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    Ok(value.flatten())
}

/// The authenticated caller, built from the session cookie on every request.
#[derive(Clone, Debug)]
pub struct AuthedUser {
    pub user_id: i64,
    pub email: String,
    pub role: Role,
    pub apps: Vec<String>,
    pub must_change_password: bool,
    pub session_id_hash: String,
    /// The last password confirmation, for `require_reauth`.
    pub reauth_at: Option<i64>,
}

impl AuthedUser {
    /// Owner-only actions (app keys, account management).
    pub fn require_owner(&self) -> Result<(), AdminError> {
        if self.role == Role::Owner {
            Ok(())
        } else {
            Err(AdminError::forbidden("this action requires the owner role"))
        }
    }

    /// Owners reach every app; managers only the apps bound to their account.
    pub fn require_app(&self, app: &str) -> Result<(), AdminError> {
        match self.role {
            Role::Owner => Ok(()),
            Role::Manager => {
                if self.apps.iter().any(|bound| bound == app) {
                    Ok(())
                } else {
                    Err(AdminError::forbidden(
                        "the app is not assigned to this account",
                    ))
                }
            }
        }
    }

    /// Requires a password confirmation newer than `max_age_secs` seconds.
    pub fn require_reauth(&self, max_age_secs: i64) -> Result<(), AdminError> {
        let fresh = self
            .reauth_at
            .map(|at| session::now_epoch() - at <= max_age_secs)
            .unwrap_or(false);
        if fresh {
            Ok(())
        } else {
            Err(AdminError::forbidden("reauth_required"))
        }
    }
}

/// `must_change_password` blocks everything except the routes needed to get out
/// of that state.
fn requires_password_change(parts: &Parts) -> bool {
    let path = parts.uri.path();
    let allowed = (parts.method == Method::GET && path.ends_with("/auth/me"))
        || (parts.method == Method::POST
            && (path.ends_with("/auth/password") || path.ends_with("/auth/logout")));
    !allowed
}

#[axum::async_trait]
impl FromRequestParts<Arc<AppState>> for AuthedUser {
    type Rejection = AdminError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let cookie_name = state.tool_auth.cookie.name();
        let token = session::read_cookie(
            parts
                .headers
                .get_all(header::COOKIE)
                .iter()
                .filter_map(|value| value.to_str().ok()),
            cookie_name,
        )
        .ok_or_else(AdminError::unauthorized_missing)?;

        let stored = session::load_session(&state.tool_auth.pool, &token)
            .await
            .map_err(|error| AdminError::from_db("load the session", error))?
            .ok_or_else(AdminError::unauthorized_missing)?;

        let user = load_user_by_id(&state.tool_auth.pool, stored.user_id)
            .await
            .map_err(|error| AdminError::from_db("load the account", error))?
            .ok_or_else(AdminError::unauthorized_missing)?;

        if user.disabled {
            return Err(AdminError::unauthorized_missing());
        }
        if user.must_change_password && requires_password_change(parts) {
            return Err(AdminError::password_change_required());
        }

        Ok(AuthedUser {
            user_id: user.id,
            email: user.email,
            role: user.role,
            apps: user.apps,
            must_change_password: user.must_change_password,
            session_id_hash: stored.id_hash,
            reauth_at: stored.reauth_at,
        })
    }
}

fn short_hash(value: &str) -> String {
    session::hash_token(value).chars().take(12).collect()
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Deserialize)]
pub struct ReauthRequest {
    pub password: String,
}

fn with_set_cookie(mut response: Response, cookie: String) -> Result<Response, AdminError> {
    let value = HeaderValue::from_str(&cookie)
        .map_err(|_| AdminError::internal("the session cookie could not be built"))?;
    response.headers_mut().insert(header::SET_COOKIE, value);
    Ok(response)
}

async fn record_login_failure(
    tool_auth: &ToolAuth,
    email_key: &str,
    ip_key: &str,
    email: Option<&str>,
) -> Result<(), AdminError> {
    // Only failures are recorded, so a successful login does not reset the
    // budget and an attacker cannot widen it by guessing correctly once.
    tool_auth.login_limiter.check(email_key);
    tool_auth.login_limiter.check(ip_key);

    // The detail never holds the raw address of an account that may not exist.
    let detail = match email {
        Some(email) => format!("email_sha256:{}", short_hash(email)),
        None => "unknown_email".to_string(),
    };
    db::write_audit(
        &tool_auth.pool,
        "anonymous",
        None,
        "admin.login_failed",
        &detail,
    )
    .await
    .map_err(|error| AdminError::from_db("write the login audit", error))
}

/// `POST /auth/login`: one message for every failure, so accounts cannot be
/// enumerated by the response.
pub async fn login(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    AdminJson(request): AdminJson<LoginRequest>,
) -> Result<Response, AdminError> {
    let tool_auth = &state.tool_auth;
    let email = request.email.trim().to_lowercase();
    let ip = client_ip(
        &headers,
        Some(peer),
        state.trust_proxy,
        state.client_ip_header,
    );
    let email_key = format!("login:email:{}", email);
    let ip_key = format!("login:ip:{}", ip);

    // Checked before any hashing: the sixth attempt must not cost argon2 work.
    if tool_auth.login_limiter.is_blocked(&email_key) || tool_auth.login_limiter.is_blocked(&ip_key)
    {
        return Err(AdminError::rate_limited());
    }

    let found = load_user_by_email(&tool_auth.pool, &email)
        .await
        .map_err(|error| AdminError::from_db("load the account", error))?;
    let hash = found.as_ref().map(|user| user.password_hash.as_str());
    let verified =
        super::password::verify_or_dummy(hash, &request.password, &tool_auth.dummy_hash).await;

    let user = match found {
        Some(user) if verified && !user.disabled => user,
        other => {
            let fingerprint = other.as_ref().map(|user| user.email.as_str());
            record_login_failure(tool_auth, &email_key, &ip_key, fingerprint).await?;
            return Err(AdminError::unauthorized_credentials());
        }
    };

    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.chars().take(200).collect::<String>());
    let created = session::create_session(
        &tool_auth.pool,
        &tool_auth.cookie,
        user.id,
        Some(&ip),
        user_agent.as_deref(),
    )
    .await
    .map_err(|error| AdminError::from_db("create the session", error))?;

    db::write_audit(
        &tool_auth.pool,
        &user.email,
        None,
        "admin.login",
        &format!("session created from {}", ip),
    )
    .await
    .map_err(|error| AdminError::from_db("write the login audit", error))?;

    let body = Json(json!({ "user": user.view() })).into_response();
    with_set_cookie(body, tool_auth.cookie.set_cookie(&created.token))
}

/// `POST /auth/logout`: always succeeds and always clears the cookie.
pub async fn logout(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    let tool_auth = &state.tool_auth;
    let token = session::read_cookie(
        headers
            .get_all(header::COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok()),
        tool_auth.cookie.name(),
    );

    if let Some(token) = token {
        // A lookup that ignores expiry, so an expired session is still removed.
        if let Ok(Some(user_id)) = session::find_session_user(&tool_auth.pool, &token).await {
            let _ = session::delete_session(&tool_auth.pool, &session::hash_token(&token)).await;
            if let Ok(Some(user)) = load_user_by_id(&tool_auth.pool, user_id).await {
                let detail = format!("email_sha256:{}", short_hash(&user.email));
                let _ =
                    db::write_audit(&tool_auth.pool, &user.email, None, "admin.logout", &detail)
                        .await;
            }
        }
    }

    let response = StatusCode::NO_CONTENT.into_response();
    with_set_cookie(response, tool_auth.cookie.clear_cookie())
}

/// `GET /auth/me`.
pub async fn me(user: AuthedUser) -> Json<Value> {
    Json(json!({
        "id": user.user_id,
        "email": user.email,
        "role": user.role.as_str(),
        "apps": user.apps,
        "must_change_password": user.must_change_password,
    }))
}

/// `POST /auth/password`: clears the flag and signs every other session out.
pub async fn change_password(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    AdminJson(request): AdminJson<ChangePasswordRequest>,
) -> Result<Response, AdminError> {
    let tool_auth = &state.tool_auth;
    let stored: Option<String> =
        sqlx::query_scalar("SELECT password_hash FROM tool_users WHERE id = ?")
            .bind(user.user_id)
            .fetch_optional(&tool_auth.pool)
            .await
            .map_err(|error| AdminError::from_db("load the password hash", error))?;
    let stored = stored.ok_or_else(AdminError::unauthorized_missing)?;

    if !super::password::verify_password(&stored, &request.current_password).await {
        return Err(AdminError::unauthorized("the current password is wrong"));
    }
    super::password::validate_password(&request.new_password, &user.email)
        .map_err(|message| AdminError::invalid_request(&message))?;
    let hash = super::password::hash_password(&request.new_password)
        .await
        .map_err(|error| AdminError::internal(&error))?;

    let mut transaction = tool_auth
        .pool
        .begin()
        .await
        .map_err(|error| AdminError::from_db("start the password transaction", error))?;
    sqlx::query("UPDATE tool_users SET password_hash = ?, must_change_password = 0 WHERE id = ?")
        .bind(&hash)
        .bind(user.user_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| AdminError::from_db("update the password", error))?;
    sqlx::query("DELETE FROM tool_sessions WHERE user_id = ? AND id_hash <> ?")
        .bind(user.user_id)
        .bind(&user.session_id_hash)
        .execute(&mut *transaction)
        .await
        .map_err(|error| AdminError::from_db("delete the other sessions", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| AdminError::from_db("commit the password change", error))?;

    db::write_audit(
        &tool_auth.pool,
        &user.email,
        None,
        "admin.password_changed",
        "all other sessions were deleted",
    )
    .await
    .map_err(|error| AdminError::from_db("write the password audit", error))?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// The body of the test uid update. It never names a user.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestUidRequest {
    pub test_uid: Option<String>,
}

/// `PUT /auth/test-uid`: the test uid of the calling account.
///
/// The account of the session is the only one that can change, so the body
/// carries no user id. The value is validated like a uid and never audited.
pub async fn set_test_uid(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    AdminJson(request): AdminJson<TestUidRequest>,
) -> Result<Json<Value>, AdminError> {
    let value = match &request.test_uid {
        Some(uid) => {
            if !crate::identity::is_valid_uid(uid) {
                return Err(AdminError::invalid_request("the test uid is not valid"));
            }
            Some(uid.clone())
        }
        None => None,
    };
    super::users::check_write_limit(&state, &user)?;

    db::set_account_test_uid(&state.db, user.user_id, value.as_deref())
        .await
        .map_err(|error| AdminError::from_db("update the test uid", error))?;

    let detail = json!({ "set": value.is_some() }).to_string();
    let _ = db::write_audit(&state.db, &user.email, None, "user.test_uid", &detail).await;

    Ok(Json(json!({ "test_uid": value })))
}

/// `POST /auth/reauth`: records a fresh password confirmation for the session.
pub async fn reauth(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    AdminJson(request): AdminJson<ReauthRequest>,
) -> Result<Response, AdminError> {
    let tool_auth = &state.tool_auth;
    let stored: Option<String> =
        sqlx::query_scalar("SELECT password_hash FROM tool_users WHERE id = ?")
            .bind(user.user_id)
            .fetch_optional(&tool_auth.pool)
            .await
            .map_err(|error| AdminError::from_db("load the password hash", error))?;
    let stored = stored.ok_or_else(AdminError::unauthorized_missing)?;

    if !super::password::verify_password(&stored, &request.password).await {
        return Err(AdminError::unauthorized("the password is wrong"));
    }

    sqlx::query("UPDATE tool_sessions SET reauth_at = ? WHERE id_hash = ?")
        .bind(session::now_epoch())
        .bind(&user.session_id_hash)
        .execute(&tool_auth.pool)
        .await
        .map_err(|error| AdminError::from_db("update the reauth timestamp", error))?;

    db::write_audit(
        &tool_auth.pool,
        &user.email,
        None,
        "admin.reauth",
        "password confirmed again",
    )
    .await
    .map_err(|error| AdminError::from_db("write the reauth audit", error))?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

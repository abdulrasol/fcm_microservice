//! Dashboard management of the application keys (T11).
//!
//! Only the owner manages the keys. A key is shown once, in the response of
//! the creation, and never again: the list holds a hint of its last four
//! characters, never the key and never its hash.

use super::auth::AuthedUser;
use super::{AdminError, AdminJson};
use crate::appkeys::{AppKeyStore, KeyError};
use crate::db;
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

/// A dashboard error that carries its own error code.
pub enum KeyAdminError {
    Admin(AdminError),
    Conflict(&'static str),
}

impl From<AdminError> for KeyAdminError {
    fn from(error: AdminError) -> Self {
        KeyAdminError::Admin(error)
    }
}

impl IntoResponse for KeyAdminError {
    fn into_response(self) -> Response {
        match self {
            KeyAdminError::Admin(error) => error.into_response(),
            KeyAdminError::Conflict(code) => {
                (StatusCode::CONFLICT, Json(json!({ "error": code }))).into_response()
            }
        }
    }
}

/// The body of a key creation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateKeyRequest {
    pub name: String,
}

/// `GET /apps/{app}/keys`: the keys of the app, the revoked ones included.
pub async fn list_keys(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path(app): Path<String>,
) -> Result<Json<Value>, AdminError> {
    user.require_owner()?;

    let keys = key_store(&state)?
        .list(&app)
        .await
        .map_err(|error| AdminError::from_db("read the keys", error))?;

    let items: Vec<Value> = keys
        .iter()
        .map(|key| {
            json!({
                "id": key.id,
                "name": key.name,
                "hint": key.hint,
                "created_by": key.created_by,
                "created_at": key.created_at,
                "last_used_at": key.last_used_at,
                "revoked_at": key.revoked_at,
            })
        })
        .collect();

    Ok(Json(Value::Array(items)))
}

/// `POST /apps/{app}/keys`: creates a key and shows it once.
pub async fn create_key(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path(app): Path<String>,
    AdminJson(body): AdminJson<CreateKeyRequest>,
) -> Result<Response, KeyAdminError> {
    user.require_owner()?;
    user.require_reauth(300)?;

    // The app must exist: a key without an app is useless.
    let known =
        state.config.get(&app).is_some() || state.project_map.read().await.contains_key(&app);
    if !known {
        return Err(AdminError::not_found("the app does not exist").into());
    }

    let created = key_store(&state)?
        .create(&app, &body.name, &user.email)
        .await
        .map_err(map_key_error)?;

    let detail = json!({ "id": created.id, "name": body.name, "app": app }).to_string();
    let _ = db::write_audit(&state.db, &user.email, Some(&app), "key.create", &detail).await;

    // The key is shown here and nowhere else.
    Ok((
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({
            "id": created.id,
            "name": body.name,
            "key": created.key,
        })),
    )
        .into_response())
}

/// `DELETE /apps/{app}/keys/{id}`: revokes a key.
pub async fn revoke_key(
    State(state): State<Arc<AppState>>,
    user: AuthedUser,
    Path((app, id)): Path<(String, i64)>,
) -> Result<Json<Value>, KeyAdminError> {
    user.require_owner()?;

    let revoked = key_store(&state)?
        .revoke(&app, id)
        .await
        .map_err(|error| AdminError::from_db("revoke the key", error))?;
    if !revoked {
        return Err(AdminError::not_found("the key does not exist").into());
    }

    let detail = json!({ "id": id }).to_string();
    let _ = db::write_audit(&state.db, &user.email, Some(&app), "key.revoke", &detail).await;

    Ok(Json(json!({ "revoked": true })))
}

/// The store of the application keys.
fn key_store(state: &AppState) -> Result<Arc<AppKeyStore>, AdminError> {
    state
        .auth
        .app_keys()
        .cloned()
        .ok_or_else(|| AdminError::internal("the key store is not available"))
}

/// The error of a key creation.
fn map_key_error(error: KeyError) -> KeyAdminError {
    match error {
        KeyError::Invalid(message) => AdminError::invalid_request(message).into(),
        KeyError::TooMany => KeyAdminError::Conflict("too_many_keys"),
        KeyError::Database => AdminError::internal("the key storage failed").into(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{call_raw, setup, Harness, APP};
    use super::*;
    use axum::http::{HeaderMap, Method};

    /// Creates a key through the route.
    async fn create_key(harness: &Harness, name: &str) -> (StatusCode, Value) {
        harness
            .call(
                Method::POST,
                &format!("/apps/{}/keys", APP),
                Some(json!({ "name": name })),
            )
            .await
    }

    #[tokio::test]
    async fn keys_routes_owner_only() {
        let harness = setup("manager", &[APP]).await;
        let calls = vec![
            (Method::GET, format!("/apps/{}/keys", APP), None),
            (
                Method::POST,
                format!("/apps/{}/keys", APP),
                Some(json!({ "name": "backend" })),
            ),
            (Method::DELETE, format!("/apps/{}/keys/1", APP), None),
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
    async fn keys_create_needs_reauth() {
        let harness = setup("owner", &[]).await;
        let (status, body) = create_key(&harness, "backend").await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["message"], "reauth_required");
    }

    #[tokio::test]
    async fn keys_create_response_is_no_store_and_has_key_once() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;

        let response = call_raw(
            &harness.router,
            Method::POST,
            &format!("/apps/{}/keys", APP),
            Some(json!({ "name": "backend" })),
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

        let body = crate::admin::tests::read_json(response).await;
        let key = body["key"].as_str().expect("a key").to_string();
        assert!(key.starts_with("fcm_"));

        // The list never holds the key again.
        let (_, list) = harness
            .call(Method::GET, &format!("/apps/{}/keys", APP), None)
            .await;
        assert!(!list.to_string().contains(&key), "the list holds no key");
    }

    #[tokio::test]
    async fn keys_create_unknown_app_404() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let (status, body) = harness
            .call(
                Method::POST,
                "/apps/ghost/keys",
                Some(json!({ "name": "backend" })),
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"], "not_found");
    }

    #[tokio::test]
    async fn keys_create_name_validation() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        for name in ["", "   ", "bell\u{7}name", &"n".repeat(61)] {
            let (status, body) = create_key(&harness, name).await;
            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "name {:?} must be refused",
                name
            );
            assert_eq!(body["error"], "invalid_request");
        }
    }

    #[tokio::test]
    async fn keys_create_21st_is_409() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        for index in 0..20 {
            let (status, body) = create_key(&harness, &format!("key-{}", index)).await;
            assert_eq!(status, StatusCode::CREATED, "key {}: {}", index, body);
        }

        let (status, body) = create_key(&harness, "one-too-many").await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"], "too_many_keys");
    }

    #[tokio::test]
    async fn keys_revoke_then_use_fails() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let (status, body) = create_key(&harness, "backend").await;
        assert_eq!(status, StatusCode::CREATED);
        let key = body["key"].as_str().expect("a key").to_string();
        let id = body["id"].as_i64().expect("an id");

        // The key works before the revocation.
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {}", key)
                .parse()
                .expect("the header must build"),
        );
        assert_eq!(
            harness
                .state
                .auth
                .trusted_sender(&headers, APP)
                .await
                .expect("the check must run"),
            Some(crate::appkeys::TrustedSender::AppKey { id })
        );

        let (status, body) = harness
            .call(Method::DELETE, &format!("/apps/{}/keys/{}", APP, id), None)
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["revoked"], true);

        // The revoked key is refused.
        let error = harness
            .state
            .auth
            .trusted_sender(&headers, APP)
            .await
            .expect_err("the revoked key must be refused");
        assert_eq!(error.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn keys_list_has_hint_and_last_used_no_hash() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let (status, body) = create_key(&harness, "backend").await;
        assert_eq!(status, StatusCode::CREATED);
        let key = body["key"].as_str().expect("a key").to_string();
        let hint: String = key.chars().skip(key.chars().count() - 4).collect();

        let (status, body) = harness
            .call(Method::GET, &format!("/apps/{}/keys", APP), None)
            .await;
        assert_eq!(status, StatusCode::OK);
        let rows = body.as_array().expect("an array");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["name"], "backend");
        assert_eq!(rows[0]["hint"], hint);
        assert_eq!(rows[0]["created_by"], "user@example.com");
        assert!(rows[0]["last_used_at"].is_null());
        assert!(rows[0]["revoked_at"].is_null());

        let text = body.to_string();
        assert!(!text.contains("key_hash"), "the list has no hash");
        assert!(!text.contains(&crate::admin::session::hash_token(&key)));
    }

    #[tokio::test]
    async fn keys_audit_has_no_key_material() {
        let harness = setup("owner", &[]).await;
        harness.reauth().await;
        let (status, body) = create_key(&harness, "backend").await;
        assert_eq!(status, StatusCode::CREATED);
        let key = body["key"].as_str().expect("a key").to_string();

        let (status, _) = harness
            .call(
                Method::DELETE,
                &format!("/apps/{}/keys/{}", APP, body["id"].as_i64().expect("an id")),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK);

        let rows: Vec<(String, Option<String>)> =
            sqlx::query_as("SELECT action, detail FROM audit_log")
                .fetch_all(&harness.pool)
                .await
                .expect("the audit rows must exist");
        let actions: Vec<&str> = rows.iter().map(|(action, _)| action.as_str()).collect();
        assert!(actions.contains(&"key.create"), "actions: {:?}", actions);
        assert!(actions.contains(&"key.revoke"), "actions: {:?}", actions);
        for (action, detail) in &rows {
            let detail = detail.clone().unwrap_or_default();
            assert!(
                !detail.contains(&key),
                "the audit {} must not keep the key",
                action
            );
            assert!(
                !detail.contains(&crate::admin::session::hash_token(&key)),
                "the audit {} must not keep the hash",
                action
            );
        }
    }
}

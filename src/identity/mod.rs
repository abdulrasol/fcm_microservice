//! Identity providers: one interface that answers "who is the caller".
//!
//! Each provider verifies a bearer token and returns an [`Identity`]. The
//! registry chooses the provider for an app from its stored configuration; the
//! send routes are wired to it in T5/T8, so until then most of the module is
//! reachable only from tests.
#![allow(dead_code)]

pub mod firebase;
pub mod jwks;
pub mod jwt;
pub mod registry;

use async_trait::async_trait;
use serde_json::Value;

/// Longest accepted bearer token, in bytes.
pub(crate) const MAX_TOKEN_LENGTH: usize = 8192;

/// Longest accepted uid, in bytes.
pub(crate) const MAX_UID_LENGTH: usize = 128;

/// The verified caller.
#[derive(Debug, Clone, PartialEq)]
pub struct Identity {
    pub uid: String,
    pub email: Option<String>,
    pub name: Option<String>,
    /// The full claim set, used by role rules that read claims (T5).
    pub claims: Value,
}

/// Why a token could not be turned into an [`Identity`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityError {
    /// The token is not acceptable for this app; maps to `401`.
    Invalid(&'static str),
    /// The provider cannot decide right now; maps to `503`.
    Unavailable(String),
}

#[async_trait]
pub trait IdentityProvider: Send + Sync {
    async fn verify(&self, token: &str) -> Result<Identity, IdentityError>;
}

/// Reads a dotted claim path (`app_metadata.role`, `realm_access.roles`).
///
/// Empty segments are rejected and only objects are traversed, so a path never
/// indexes into an array; the returned value may be any JSON type.
pub fn claim_at<'a>(claims: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = claims;
    for segment in path.split('.') {
        if segment.is_empty() {
            return None;
        }
        current = current.as_object()?.get(segment)?;
    }
    Some(current)
}

/// Converts a uid claim into a uid: a non-empty string of at most
/// [`MAX_UID_LENGTH`] characters without control characters, or an integer.
pub(crate) fn uid_from_claim(value: &Value) -> Result<String, IdentityError> {
    match value {
        Value::String(uid) => {
            if uid.trim().is_empty()
                || uid.chars().count() > MAX_UID_LENGTH
                || uid.chars().any(char::is_control)
            {
                return Err(IdentityError::Invalid("token has no usable uid"));
            }
            Ok(uid.clone())
        }
        Value::Number(number) => number
            .as_i64()
            .map(|value| value.to_string())
            .or_else(|| number.as_u64().map(|value| value.to_string()))
            .ok_or(IdentityError::Invalid("token has no usable uid")),
        _ => Err(IdentityError::Invalid("token has no usable uid")),
    }
}

/// A uid is a non-empty string of at most [`MAX_UID_LENGTH`] bytes.
pub(crate) fn is_valid_uid(uid: &str) -> bool {
    !uid.is_empty() && uid.len() <= MAX_UID_LENGTH
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn claim_path_reads_nested_and_arrays() {
        let claims = json!({
            "app_metadata": { "role": "admin" },
            "realm_access": { "roles": ["admin", "user"] },
        });

        assert_eq!(
            claim_at(&claims, "app_metadata.role"),
            Some(&json!("admin"))
        );
        assert_eq!(
            claim_at(&claims, "realm_access.roles"),
            Some(&json!(["admin", "user"]))
        );
        assert_eq!(
            claim_at(&claims, "realm_access"),
            Some(&json!({ "roles": ["admin", "user"] }))
        );
        assert_eq!(claim_at(&claims, "app_metadata.missing"), None);
        assert_eq!(
            claim_at(&claims, "realm_access.roles.0"),
            None,
            "a path must not index into an array"
        );
    }

    #[test]
    fn claim_path_rejects_empty_segments() {
        let claims = json!({ "app_metadata": { "role": "admin" } });

        assert_eq!(claim_at(&claims, ""), None);
        assert_eq!(claim_at(&claims, ".role"), None);
        assert_eq!(claim_at(&claims, "app_metadata."), None);
        assert_eq!(claim_at(&claims, "app_metadata..role"), None);
        assert_eq!(claim_at(&claims, "app_metadata.role."), None);
    }

    #[test]
    fn uid_conversion_rules() {
        assert_eq!(uid_from_claim(&json!("uid-1")), Ok("uid-1".to_string()));
        assert_eq!(uid_from_claim(&json!(42)), Ok("42".to_string()));
        assert_eq!(uid_from_claim(&json!(-7)), Ok("-7".to_string()));
        assert!(uid_from_claim(&json!("a".repeat(MAX_UID_LENGTH))).is_ok());

        for invalid in [
            json!(""),
            json!("   "),
            json!("bad\u{0}uid"),
            json!("a".repeat(MAX_UID_LENGTH + 1)),
            json!(1.5),
            json!(null),
            json!(true),
            json!({ "id": "uid" }),
            json!(["uid"]),
        ] {
            assert!(
                matches!(
                    uid_from_claim(&invalid),
                    Err(IdentityError::Invalid("token has no usable uid"))
                ),
                "{invalid} must not be a usable uid"
            );
        }
    }
}

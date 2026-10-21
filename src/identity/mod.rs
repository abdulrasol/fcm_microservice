//! Identity providers: one interface that answers "who is the caller".
//!
//! Each provider verifies a bearer token and returns an [`Identity`]. The
//! registry chooses the provider for an app from its stored configuration; the
//! send routes are wired to it in T5/T8.

pub mod firebase;
pub mod jwks;

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

/// A uid is a non-empty string of at most [`MAX_UID_LENGTH`] bytes.
pub(crate) fn is_valid_uid(uid: &str) -> bool {
    !uid.is_empty() && uid.len() <= MAX_UID_LENGTH
}

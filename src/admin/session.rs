//! Dashboard session tokens and the session cookie.
//!
//! The raw token never leaves the client: only its SHA-256 hex digest is
//! stored, so a database leak cannot be replayed as a live session.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use std::fmt::Write as _;
use std::time::{SystemTime, UNIX_EPOCH};

/// The `__Host-` prefix forces `Secure`, `Path=/` and no `Domain`.
pub const SECURE_COOKIE_NAME: &str = "__Host-fcm_session";
pub const PLAIN_COOKIE_NAME: &str = "fcm_session";

/// Sessions kept per account; creating one more evicts the oldest.
pub const MAX_SESSIONS_PER_USER: i64 = 10;

pub const DEFAULT_SESSION_HOURS: i64 = 12;
pub const MAX_SESSION_HOURS: i64 = 72;

const TOKEN_BYTES: usize = 32;

/// Cookie name, lifetime and the attributes used to set and clear it.
#[derive(Clone, Debug)]
pub struct CookieConfig {
    pub secure: bool,
    pub session_hours: i64,
}

impl CookieConfig {
    pub fn from_env() -> Self {
        let secure = std::env::var("COOKIE_SECURE")
            .ok()
            .map(|value| {
                let value = value.trim().to_ascii_lowercase();
                !(value == "false" || value == "0" || value == "no")
            })
            .unwrap_or(true);
        let hours = std::env::var("ADMIN_SESSION_HOURS")
            .ok()
            .and_then(|value| value.trim().parse::<i64>().ok())
            .unwrap_or(DEFAULT_SESSION_HOURS)
            .clamp(1, MAX_SESSION_HOURS);
        Self::with_hours(secure, hours)
    }

    pub fn with_hours(secure: bool, session_hours: i64) -> Self {
        Self {
            secure,
            session_hours: session_hours.clamp(1, MAX_SESSION_HOURS),
        }
    }

    pub fn name(&self) -> &'static str {
        if self.secure {
            SECURE_COOKIE_NAME
        } else {
            PLAIN_COOKIE_NAME
        }
    }

    pub fn max_age_seconds(&self) -> i64 {
        self.session_hours * 3600
    }

    pub fn set_cookie(&self, token: &str) -> String {
        let mut cookie = format!(
            "{}={}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}",
            self.name(),
            token,
            self.max_age_seconds()
        );
        if self.secure {
            cookie.push_str("; Secure");
        }
        cookie
    }

    pub fn clear_cookie(&self) -> String {
        let mut cookie = format!(
            "{}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0",
            self.name()
        );
        if self.secure {
            cookie.push_str("; Secure");
        }
        cookie
    }
}

/// Current UTC time in seconds; sessions are compared as integers, not text.
pub fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

/// A fresh session token: 32 random bytes, base64url without padding.
pub fn generate_token() -> String {
    let mut bytes = [0_u8; TOKEN_BYTES];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// The only form of the token that is ever stored.
pub fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in digest {
        // Writing into a String cannot fail.
        let _ = write!(hex, "{:02x}", byte);
    }
    hex
}

/// Reads `name` from one or more `Cookie` header values.
///
/// Values that are empty or contain a space are ignored: a browser never sends
/// them and accepting them would only widen what a malformed header can match.
pub fn read_cookie<'a, I>(headers: I, name: &str) -> Option<String>
where
    I: IntoIterator<Item = &'a str>,
{
    for header in headers {
        for pair in header.split(';') {
            let pair = pair.trim();
            let (key, value) = match pair.split_once('=') {
                Some(parts) => parts,
                None => continue,
            };
            if key.trim() != name {
                continue;
            }
            let value = value.trim();
            if !value.is_empty() && !value.contains(' ') {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// A session as stored.
#[derive(Clone, Debug)]
pub struct SessionRow {
    pub id_hash: String,
    pub user_id: i64,
    pub reauth_at: Option<i64>,
}

/// A newly created session; `token` is returned to the client once.
#[derive(Clone, Debug)]
pub struct NewSession {
    pub token: String,
}

/// Creates a session, evicting expired rows and then the oldest one when the
/// account already holds `MAX_SESSIONS_PER_USER` sessions.
pub async fn create_session(
    pool: &SqlitePool,
    cookie: &CookieConfig,
    user_id: i64,
    ip: Option<&str>,
    user_agent: Option<&str>,
) -> Result<NewSession, sqlx::Error> {
    let now = now_epoch();
    sqlx::query("DELETE FROM tool_sessions WHERE user_id = ? AND expires_at <= ?")
        .bind(user_id)
        .bind(now)
        .execute(pool)
        .await?;

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tool_sessions WHERE user_id = ?")
        .bind(user_id)
        .fetch_one(pool)
        .await?;
    let excess = count - (MAX_SESSIONS_PER_USER - 1);
    if excess > 0 {
        sqlx::query(
            "DELETE FROM tool_sessions WHERE id_hash IN (\
             SELECT id_hash FROM tool_sessions WHERE user_id = ? \
             ORDER BY created_at ASC, id_hash ASC LIMIT ?)",
        )
        .bind(user_id)
        .bind(excess)
        .execute(pool)
        .await?;
    }

    let token = generate_token();
    let id_hash = hash_token(&token);
    let expires_at = now + cookie.max_age_seconds();
    sqlx::query(
        "INSERT INTO tool_sessions (id_hash, user_id, expires_at, created_at, ip, user_agent) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&id_hash)
    .bind(user_id)
    .bind(expires_at)
    .bind(now)
    .bind(ip)
    .bind(user_agent)
    .execute(pool)
    .await?;

    Ok(NewSession { token })
}

/// Loads a live session; an expired row is deleted on sight and treated as absent.
pub async fn load_session(
    pool: &SqlitePool,
    token: &str,
) -> Result<Option<SessionRow>, sqlx::Error> {
    let id_hash = hash_token(token);
    let now = now_epoch();
    sqlx::query("DELETE FROM tool_sessions WHERE id_hash = ? AND expires_at <= ?")
        .bind(&id_hash)
        .bind(now)
        .execute(pool)
        .await?;

    let row =
        sqlx::query("SELECT id_hash, user_id, reauth_at FROM tool_sessions WHERE id_hash = ?")
            .bind(&id_hash)
            .fetch_optional(pool)
            .await?;

    Ok(row.map(|row| SessionRow {
        id_hash: row.get("id_hash"),
        user_id: row.get("user_id"),
        reauth_at: row.try_get("reauth_at").unwrap_or(None),
    }))
}

/// Finds a session row even when it has expired, so logout can still clean up.
pub async fn find_session_user(pool: &SqlitePool, token: &str) -> Result<Option<i64>, sqlx::Error> {
    let id_hash = hash_token(token);
    sqlx::query_scalar("SELECT user_id FROM tool_sessions WHERE id_hash = ?")
        .bind(&id_hash)
        .fetch_optional(pool)
        .await
}

pub async fn delete_session(pool: &SqlitePool, id_hash: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM tool_sessions WHERE id_hash = ?")
        .bind(id_hash)
        .execute(pool)
        .await?;
    Ok(())
}

/// Removes every expired session; used by the hourly cleanup task.
pub async fn delete_expired(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let deleted = sqlx::query("DELETE FROM tool_sessions WHERE expires_at <= ?")
        .bind(now_epoch())
        .execute(pool)
        .await?;
    Ok(deleted.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_id_is_32_random_bytes_and_stored_hashed() {
        let token = generate_token();
        let decoded = URL_SAFE_NO_PAD
            .decode(&token)
            .expect("the token must be valid base64url");
        assert_eq!(decoded.len(), 32);
        assert!(!token.contains('='), "no padding is used");

        let first = generate_token();
        let second = generate_token();
        assert_ne!(first, second, "tokens must not repeat");

        let hash = hash_token(&first);
        assert_eq!(hash.len(), 64, "SHA-256 hex is 64 characters");
        assert_ne!(hash, first, "the stored value is not the token");
        assert!(hash
            .chars()
            .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase()));
    }

    #[test]
    fn cookie_parse_handles_multiple_and_spaces() {
        let header = "other=1;  fcm_session = spaced value ; third=3";
        assert_eq!(
            read_cookie([header], "fcm_session"),
            None,
            "a value with a space is not accepted"
        );

        let header = "other=1;  fcm_session=abc123 ; third=3";
        assert_eq!(
            read_cookie([header], "fcm_session"),
            Some("abc123".to_string())
        );

        let first = "first=1";
        let second = "fcm_session=from_second_header";
        assert_eq!(
            read_cookie([first, second], "fcm_session"),
            Some("from_second_header".to_string())
        );

        assert_eq!(read_cookie(["nothing_here"], "fcm_session"), None);
        assert_eq!(read_cookie(["fcm_session="], "fcm_session"), None);
        assert_eq!(
            read_cookie(["other=fcm_session=1"], "fcm_session"),
            None,
            "the name must be the key of the pair"
        );
    }

    #[test]
    fn set_cookie_has_required_attributes_secure_mode() {
        let config = CookieConfig::with_hours(true, 12);
        let cookie = config.set_cookie("token-value");
        assert!(cookie.starts_with("__Host-fcm_session=token-value;"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Strict"));
        assert!(cookie.contains("Path=/"));
        assert!(cookie.contains("Secure"));
        assert!(cookie.contains("Max-Age=43200"));
        assert!(!cookie.contains("Domain"), "no Domain attribute is allowed");

        let cleared = config.clear_cookie();
        assert!(cleared.starts_with("__Host-fcm_session=;"));
        assert!(cleared.contains("Max-Age=0"));
        assert!(cleared.contains("Secure"));
    }

    #[test]
    fn set_cookie_insecure_mode_uses_plain_name() {
        let config = CookieConfig::with_hours(false, 12);
        let cookie = config.set_cookie("token-value");
        assert!(cookie.starts_with("fcm_session=token-value;"));
        assert!(!cookie.contains("Secure"), "Secure must be omitted locally");
        assert!(!cookie.contains("__Host-"));

        let cleared = config.clear_cookie();
        assert!(cleared.starts_with("fcm_session=;"));
    }

    #[test]
    fn hours_are_clamped_to_the_allowed_range() {
        assert_eq!(CookieConfig::with_hours(true, 0).session_hours, 1);
        assert_eq!(CookieConfig::with_hours(true, -5).session_hours, 1);
        assert_eq!(CookieConfig::with_hours(true, 500).session_hours, 72);
        assert_eq!(CookieConfig::with_hours(true, 12).session_hours, 12);
    }
}

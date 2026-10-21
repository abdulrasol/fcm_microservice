//! Application keys (T11).
//!
//! A key is a long-lived secret with a wide scope over its own app. It is
//! shown once, stored as a SHA-256 hash only, bound to its app, and its
//! revocation is immediate because nothing is cached: every check reads the
//! table.

use crate::admin::session::{generate_token, hash_token};
use sqlx::{Row, SqlitePool};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The prefix of every application key.
pub const APP_KEY_PREFIX: &str = "fcm_";

/// The number of characters of the random part (base64url without padding).
const KEY_RANDOM_CHARS: usize = 43;

/// The number of characters of the random part with its separator.
const KEY_TAIL_CHARS: usize = 44;

/// The largest number of live keys one app may hold.
pub const MAX_KEYS_PER_APP: usize = 20;

/// The longest key name.
const MAX_NAME_CHARS: usize = 60;

/// The shortest interval between two `last_used_at` writes of the same key.
const LAST_USED_INTERVAL: Duration = Duration::from_secs(60);

/// The largest number of throttling entries kept in memory.
const MAX_TOUCH_ENTRIES: usize = 10_000;

/// Why a key could not be created.
#[derive(Debug)]
pub enum KeyError {
    /// The app name or the key name is not valid.
    Invalid(&'static str),
    /// The app already holds the largest number of live keys.
    TooMany,
    /// The storage failed. The detail is in the log, never in the answer.
    Database,
}

impl std::fmt::Display for KeyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyError::Invalid(message) => write!(formatter, "{}", message),
            KeyError::TooMany => {
                write!(
                    formatter,
                    "the app already holds the maximum number of keys"
                )
            }
            // The database error never holds a key.
            KeyError::Database => write!(formatter, "the key storage failed"),
        }
    }
}

impl From<sqlx::Error> for KeyError {
    fn from(error: sqlx::Error) -> Self {
        // The SQL error never holds a key.
        tracing::error!(error = %error, "the key storage failed");
        KeyError::Database
    }
}

/// A new key. The secret is shown once and never stored in clear.
///
/// The `Debug` implementation prints the id only, so a failing assertion
/// never prints the key.
pub struct NewKey {
    pub id: i64,
    pub key: String,
}

impl std::fmt::Debug for NewKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "NewKey {{ id: {} }}", self.id)
    }
}

/// The public information about one key. The hash is never part of it.
#[derive(Debug)]
pub struct KeyInfo {
    pub id: i64,
    pub name: String,
    pub hint: String,
    pub created_by: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub revoked_at: Option<String>,
}

/// The result of checking a bearer token against the keys of an app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyCheck {
    /// The token is not an application key.
    NotAKey,
    /// The key exists and belongs to the requested app.
    Valid { id: i64 },
    /// The key exists but belongs to another app.
    WrongApp,
    /// The token has the shape of a key but is unknown, revoked or malformed.
    Invalid,
}

/// A sender the tool trusts without a user identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrustedSender {
    /// The global service key.
    Service,
    /// An application key of one app.
    AppKey { id: i64 },
}

impl TrustedSender {
    /// The sender uid of an application key.
    ///
    /// The prefix keeps the key apart from a user uid, so a template that
    /// compares `{sender.uid}` never matches an application key.
    pub fn app_key_uid(id: i64) -> String {
        format!("key:{}", id)
    }
}

/// The store of the application keys.
pub struct AppKeyStore {
    pool: SqlitePool,
    /// The last write of every key, so the usage time is not written twice.
    last_touch: Mutex<HashMap<i64, Instant>>,
}

impl AppKeyStore {
    /// Opens the store on a database.
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            last_touch: Mutex::new(HashMap::new()),
        }
    }

    /// Creates a key for an app and returns it once.
    pub async fn create(
        &self,
        app: &str,
        name: &str,
        created_by: &str,
    ) -> Result<NewKey, KeyError> {
        if !is_valid_app_name(app) {
            return Err(KeyError::Invalid("the app name is not valid"));
        }
        let name = name.trim();
        if name.is_empty()
            || name.chars().count() > MAX_NAME_CHARS
            || name.chars().any(char::is_control)
        {
            return Err(KeyError::Invalid("the key name is not valid"));
        }

        let live: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM app_keys WHERE app = ? AND revoked_at IS NULL",
        )
        .bind(app)
        .fetch_one(&self.pool)
        .await?;
        if live as usize >= MAX_KEYS_PER_APP {
            return Err(KeyError::TooMany);
        }

        // The random part is the same generator the session tokens use.
        let key = format!("{}{}_{}", APP_KEY_PREFIX, app, generate_token());
        let hash = hash_token(&key);
        let hint = hint_of(&key);
        let result = sqlx::query(
            "INSERT INTO app_keys (app, name, key_hash, hint, created_by) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(app)
        .bind(name)
        .bind(&hash)
        .bind(&hint)
        .bind(created_by)
        .execute(&self.pool)
        .await?;

        Ok(NewKey {
            id: result.last_insert_rowid(),
            key,
        })
    }

    /// Lists the keys of an app, the revoked ones included.
    pub async fn list(&self, app: &str) -> Result<Vec<KeyInfo>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, name, hint, created_by, created_at, last_used_at, revoked_at \
             FROM app_keys WHERE app = ? ORDER BY id",
        )
        .bind(app)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| KeyInfo {
                id: row.get("id"),
                name: row.get("name"),
                hint: row.get("hint"),
                created_by: row.get("created_by"),
                created_at: row.get("created_at"),
                last_used_at: row.get("last_used_at"),
                revoked_at: row.get("revoked_at"),
            })
            .collect())
    }

    /// Revokes a key.
    ///
    /// `false` means the key does not exist, belongs to another app or is
    /// already revoked.
    pub async fn revoke(&self, app: &str, id: i64) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE app_keys SET revoked_at = CURRENT_TIMESTAMP \
             WHERE id = ? AND app = ? AND revoked_at IS NULL",
        )
        .bind(id)
        .bind(app)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Checks a bearer token against the keys of an app.
    ///
    /// The table is read on every call, so a revoked key stops working at once.
    pub async fn check(&self, token: &str, request_app: &str) -> KeyCheck {
        if !token.starts_with(APP_KEY_PREFIX) {
            return KeyCheck::NotAKey;
        }
        let Some(random) = token.strip_prefix(APP_KEY_PREFIX) else {
            return KeyCheck::NotAKey;
        };
        if random.len() < KEY_TAIL_CHARS {
            return KeyCheck::Invalid;
        }

        // The app name may hold underscores, so the random part is cut from
        // the end and the app name is what is left.
        let tail = &random[random.len() - KEY_TAIL_CHARS..];
        let Some(random_part) = tail.strip_prefix('_') else {
            return KeyCheck::Invalid;
        };
        if random_part.len() != KEY_RANDOM_CHARS
            || !random_part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            return KeyCheck::Invalid;
        }
        let embedded = &random[..random.len() - KEY_TAIL_CHARS];
        if embedded.is_empty() {
            return KeyCheck::Invalid;
        }

        let hash = hash_token(token);
        let row = sqlx::query("SELECT id, app, revoked_at FROM app_keys WHERE key_hash = ?")
            .bind(&hash)
            .fetch_optional(&self.pool)
            .await;
        let Ok(Some(row)) = row else {
            return KeyCheck::Invalid;
        };
        if row.get::<Option<String>, _>("revoked_at").is_some() {
            // A revoked key answers like an unknown one.
            return KeyCheck::Invalid;
        }

        let id: i64 = row.get("id");
        let stored_app: String = row.get("app");
        if stored_app != request_app || embedded != stored_app {
            return KeyCheck::WrongApp;
        }

        self.touch(id).await;
        KeyCheck::Valid { id }
    }

    /// Records the use of a key, at most once a minute.
    ///
    /// A failed write never fails the request that used the key.
    async fn touch(&self, id: i64) {
        {
            let mut touch = lock(&self.last_touch);
            if let Some(last) = touch.get(&id) {
                if last.elapsed() < LAST_USED_INTERVAL {
                    return;
                }
            }
            if touch.len() >= MAX_TOUCH_ENTRIES {
                touch.clear();
            }
            touch.insert(id, Instant::now());
        }
        let _ = sqlx::query("UPDATE app_keys SET last_used_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await;
    }
}

/// `^[a-z0-9_-]{1,40}$`, the app name inside a key.
pub fn is_valid_app_name(app: &str) -> bool {
    !app.is_empty()
        && app.len() <= 40
        && app.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
        })
}

/// The last four characters of a key, shown in the list.
fn hint_of(key: &str) -> String {
    let characters: Vec<char> = key.chars().collect();
    let start = characters.len().saturating_sub(4);
    characters[start..].iter().collect()
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrate;
    use sqlx::sqlite::SqlitePoolOptions;
    use sqlx::Column;

    async fn memory_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("the in-memory database must open");
        migrate(&pool).await.expect("the migration must run");
        pool
    }

    async fn store() -> (AppKeyStore, SqlitePool) {
        let pool = memory_pool().await;
        let store = AppKeyStore::new(pool.clone());
        (store, pool)
    }

    /// Creates a key and returns it with the store.
    async fn create(store: &AppKeyStore, app: &str, name: &str) -> NewKey {
        store
            .create(app, name, "owner@example.com")
            .await
            .expect("the key must be created")
    }

    #[tokio::test]
    async fn key_format_and_length() {
        let (store, _pool) = store().await;
        let created = create(&store, "watt", "backend").await;

        assert!(created.key.starts_with("fcm_watt_"), "{}", created.key);
        let random = &created.key["fcm_watt_".len()..];
        assert_eq!(random.len(), 43, "32 random bytes in base64url");
        assert!(
            random
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'),
            "the random part is base64url"
        );
    }

    #[tokio::test]
    async fn key_hash_only_stored() {
        let (store, pool) = store().await;
        let created = create(&store, "watt", "backend").await;

        let hash: String = sqlx::query_scalar("SELECT key_hash FROM app_keys WHERE id = ?")
            .bind(created.id)
            .fetch_one(&pool)
            .await
            .expect("the row must exist");
        assert_eq!(hash, hash_token(&created.key));
        assert!(!hash.contains(&created.key));

        let row = sqlx::query("SELECT * FROM app_keys WHERE id = ?")
            .bind(created.id)
            .fetch_one(&pool)
            .await
            .expect("the row must exist");
        for column in row.columns() {
            let name = column.name();
            if let Some(value) = row.try_get::<Option<String>, _>(name).ok().flatten() {
                assert!(
                    !value.contains(&created.key),
                    "the column {} holds the raw key",
                    name
                );
            }
        }
    }

    #[tokio::test]
    async fn key_shown_once_not_in_list() {
        let (store, _pool) = store().await;
        let created = create(&store, "watt", "backend").await;

        let keys = store.list("watt").await.expect("the list must run");
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].name, "backend");
        assert_eq!(keys[0].created_by, "owner@example.com");
        let text = format!("{:?}", keys[0]);
        assert!(!text.contains(&created.key), "the list never holds the key");
        assert!(
            !text.contains(&hash_token(&created.key)),
            "the list has no hash"
        );
    }

    #[tokio::test]
    async fn check_valid_key_returns_id() {
        let (store, _pool) = store().await;
        let created = create(&store, "watt", "backend").await;

        assert_eq!(
            store.check(&created.key, "watt").await,
            KeyCheck::Valid { id: created.id }
        );
    }

    #[tokio::test]
    async fn check_wrong_app_is_wrong_app() {
        let (store, _pool) = store().await;
        let created = create(&store, "watt", "backend").await;

        assert_eq!(store.check(&created.key, "other").await, KeyCheck::WrongApp);
    }

    #[tokio::test]
    async fn check_revoked_is_invalid_same_as_unknown() {
        let (store, _pool) = store().await;
        let created = create(&store, "watt", "backend").await;
        let unknown = "fcm_watt_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

        store
            .revoke("watt", created.id)
            .await
            .expect("the revoke must run");

        assert_eq!(store.check(&created.key, "watt").await, KeyCheck::Invalid);
        assert_eq!(store.check(unknown, "watt").await, KeyCheck::Invalid);
    }

    #[tokio::test]
    async fn check_malformed_shapes_are_invalid() {
        let (store, _pool) = store().await;

        // Too short.
        assert_eq!(store.check("fcm_watt_abc", "watt").await, KeyCheck::Invalid);
        // A character outside base64url.
        assert_eq!(
            store
                .check("fcm_watt_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa+aaaa", "watt")
                .await,
            KeyCheck::Invalid
        );
        // The right prefix without a random part.
        assert_eq!(store.check("fcm_watt_", "watt").await, KeyCheck::Invalid);
        // The prefix alone.
        assert_eq!(store.check("fcm_", "watt").await, KeyCheck::Invalid);
    }

    #[tokio::test]
    async fn check_non_fcm_token_is_not_a_key() {
        let (store, _pool) = store().await;

        assert_eq!(
            store.check("a-user-identity-token", "watt").await,
            KeyCheck::NotAKey
        );
        assert_eq!(store.check("", "watt").await, KeyCheck::NotAKey);
    }

    #[tokio::test]
    async fn app_name_with_underscores_parses_correctly() {
        let (store, _pool) = store().await;
        let created = create(&store, "my_app_1", "backend").await;

        assert!(created.key.starts_with("fcm_my_app_1_"));
        assert_eq!(
            store.check(&created.key, "my_app_1").await,
            KeyCheck::Valid { id: created.id }
        );
        assert_eq!(
            store.check(&created.key, "my_app").await,
            KeyCheck::WrongApp
        );
    }

    #[tokio::test]
    async fn revoke_is_immediate_no_cache() {
        let (store, _pool) = store().await;
        let created = create(&store, "watt", "backend").await;

        assert_eq!(
            store.check(&created.key, "watt").await,
            KeyCheck::Valid { id: created.id }
        );
        assert!(store
            .revoke("watt", created.id)
            .await
            .expect("the revoke must run"));
        assert_eq!(
            store.check(&created.key, "watt").await,
            KeyCheck::Invalid,
            "a revoked key stops working at once"
        );
    }

    #[tokio::test]
    async fn revoke_other_apps_key_is_false() {
        let (store, _pool) = store().await;
        let created = create(&store, "watt", "backend").await;

        assert!(!store
            .revoke("other", created.id)
            .await
            .expect("the revoke must run"));
        assert!(store
            .revoke("watt", created.id)
            .await
            .expect("the revoke must run"));
        assert!(!store
            .revoke("watt", created.id)
            .await
            .expect("the second revoke must run"));
    }

    #[tokio::test]
    async fn max_20_active_keys_per_app_revoked_do_not_count() {
        let (store, _pool) = store().await;
        for index in 0..MAX_KEYS_PER_APP {
            let _ = create(&store, "watt", &format!("key-{}", index)).await;
        }

        let error = store
            .create("watt", "one-too-many", "owner@example.com")
            .await
            .expect_err("the limit must be reached");
        assert!(matches!(error, KeyError::TooMany));

        // A revoked key frees a slot.
        let keys = store.list("watt").await.expect("the list must run");
        store
            .revoke("watt", keys[0].id)
            .await
            .expect("the revoke must run");
        let _ = create(&store, "watt", "after-revoke").await;
    }

    #[tokio::test]
    async fn last_used_updates_on_first_use() {
        let (store, pool) = store().await;
        let created = create(&store, "watt", "backend").await;

        let before: Option<String> =
            sqlx::query_scalar("SELECT last_used_at FROM app_keys WHERE id = ?")
                .bind(created.id)
                .fetch_one(&pool)
                .await
                .expect("the row must exist");
        assert!(before.is_none(), "a new key was never used");

        assert_eq!(
            store.check(&created.key, "watt").await,
            KeyCheck::Valid { id: created.id }
        );

        let after: Option<String> =
            sqlx::query_scalar("SELECT last_used_at FROM app_keys WHERE id = ?")
                .bind(created.id)
                .fetch_one(&pool)
                .await
                .expect("the row must exist");
        assert!(after.is_some(), "the use is recorded");
    }

    #[tokio::test]
    async fn last_used_writes_are_throttled() {
        let (store, pool) = store().await;
        let created = create(&store, "watt", "backend").await;

        // A trigger counts the writes of the usage time.
        sqlx::query("CREATE TABLE touch_log (id INTEGER)")
            .execute(&pool)
            .await
            .expect("the table must be created");
        sqlx::query(
            "CREATE TRIGGER count_touch AFTER UPDATE OF last_used_at ON app_keys \
             BEGIN INSERT INTO touch_log (id) VALUES (NEW.id); END",
        )
        .execute(&pool)
        .await
        .expect("the trigger must be created");

        let _ = store.check(&created.key, "watt").await;
        let _ = store.check(&created.key, "watt").await;
        let _ = store.check(&created.key, "watt").await;

        let writes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM touch_log")
            .fetch_one(&pool)
            .await
            .expect("the count must exist");
        assert_eq!(writes, 1, "the usage time is written once a minute");
    }

    #[tokio::test]
    async fn key_never_in_error_text() {
        let (store, _pool) = store().await;
        let created = create(&store, "watt", "backend").await;

        let error = store
            .create("Bad App", "backend", "owner@example.com")
            .await
            .expect_err("the app name must be refused");
        let text = error.to_string();
        assert!(!text.contains(&created.key), "the error holds no key");
        assert!(
            !text.contains("fcm_watt"),
            "the error holds no key material"
        );
    }
}

//! Device registry (T6): which FCM token belongs to which user.
//!
//! A token identifies one `(app, token)` pair and is owned by one uid; signing
//! in with the same token on another account moves the row to that uid. The
//! registry stores no message content and never logs a token.
//!
//! Role-targeted sending (T8/T9) reads the member uids of a role from the app's
//! data source through [`RoleDirectory`] and then looks their tokens up here.

use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Sqlite, SqlitePool};
use std::collections::HashSet;
use tokio::sync::Mutex as AsyncMutex;

/// Eviction limit bounds, from the plan (3.8): values outside are clamped.
const MIN_PER_UID: u32 = 1;
const MAX_PER_UID: u32 = 20;

/// The platform that registered the token. Unknown values are rejected by
/// serde, which the routes turn into a `400`.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Android,
    Ios,
    Web,
}

impl Platform {
    fn as_str(self) -> &'static str {
        match self {
            Self::Android => "android",
            Self::Ios => "ios",
            Self::Web => "web",
        }
    }
}

/// Why a device operation failed.
#[derive(Debug)]
#[allow(dead_code)] // The role directory uses the remaining variants from T6-2.
pub enum DeviceError {
    /// The database failed. The message is generic: no token is ever echoed.
    Database(sqlx::Error),
    /// The token does not match [`validate_token`].
    InvalidToken,
    /// The role value does not match `^[A-Za-z0-9_\-]{1,64}$`.
    InvalidRoleValue,
    /// The app has no `identity.role_members` configuration.
    NotConfigured,
    /// The data source failed; callers fail closed with `503`.
    Unavailable(String),
}

impl std::fmt::Display for DeviceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(error) => write!(formatter, "device storage failed: {error}"),
            Self::InvalidToken => write!(formatter, "the device token is invalid"),
            Self::InvalidRoleValue => write!(formatter, "the role value is invalid"),
            Self::NotConfigured => write!(formatter, "role members are not configured"),
            Self::Unavailable(reason) => {
                write!(formatter, "the data source is unavailable: {reason}")
            }
        }
    }
}

impl std::error::Error for DeviceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for DeviceError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

/// A device token: 32..=4096 bytes, only `[A-Za-z0-9:_\-.]`.
pub fn validate_token(token: &str) -> Result<(), String> {
    if !(32..=4096).contains(&token.len()) {
        return Err("a device token must be 32..=4096 bytes".to_string());
    }
    if !token
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'_' | b'-' | b'.'))
    {
        return Err(
            "a device token may contain only A-Z, a-z, 0-9, ':', '_', '-' and '.'".to_string(),
        );
    }
    Ok(())
}

#[allow(dead_code)] // Used by the role directory in T6-2.
fn validate_role_value(value: &str) -> Result<(), DeviceError> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(DeviceError::InvalidRoleValue);
    }
    Ok(())
}

pub struct DeviceStore {
    pool: SqlitePool,
    write_lock: AsyncMutex<()>,
}

impl DeviceStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            write_lock: AsyncMutex::new(()),
        }
    }

    /// Upserts `(app, token)` for `uid`. A token already owned by another uid
    /// moves to this uid. Enforces `max_per_uid` by deleting the least recently
    /// seen tokens of that uid (never the one just registered) and returns the
    /// number of evicted rows.
    pub async fn register(
        &self,
        app: &str,
        uid: &str,
        token: &str,
        platform: Platform,
        max_per_uid: u32,
    ) -> Result<usize, DeviceError> {
        validate_token(token).map_err(|_| DeviceError::InvalidToken)?;
        let max_per_uid = i64::from(max_per_uid.clamp(MIN_PER_UID, MAX_PER_UID));

        // One transaction under the write lock: two concurrent registrations
        // must not both see a count below the limit.
        let _guard = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;

        sqlx::query(
            "INSERT INTO devices (app, token, uid, platform, created_at, last_seen) \
             VALUES (?, ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP) \
             ON CONFLICT(app, token) DO UPDATE SET \
             uid = excluded.uid, platform = excluded.platform, \
             last_seen = CURRENT_TIMESTAMP",
        )
        .bind(app)
        .bind(token)
        .bind(uid)
        .bind(platform.as_str())
        .execute(&mut *transaction)
        .await?;

        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM devices WHERE app = ? AND uid = ?")
                .bind(app)
                .bind(uid)
                .fetch_one(&mut *transaction)
                .await?;

        let mut evicted = 0;
        if count > max_per_uid {
            let excess = count - max_per_uid;
            let result = sqlx::query(
                "DELETE FROM devices WHERE rowid IN (\
                 SELECT rowid FROM devices WHERE app = ? AND uid = ? AND token <> ? \
                 ORDER BY last_seen ASC, rowid ASC LIMIT ?)",
            )
            .bind(app)
            .bind(uid)
            .bind(token)
            .bind(excess)
            .execute(&mut *transaction)
            .await?;
            evicted = result.rows_affected() as usize;
        }

        transaction.commit().await?;
        Ok(evicted)
    }

    /// Removes the token only if it belongs to `uid`.
    pub async fn unregister(&self, app: &str, uid: &str, token: &str) -> Result<bool, DeviceError> {
        let result = sqlx::query("DELETE FROM devices WHERE app = ? AND token = ? AND uid = ?")
            .bind(app)
            .bind(token)
            .bind(uid)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Every token of one user, most recently seen first.
    #[allow(dead_code)] // Wired to user-targeted sending in T8/T9.
    pub async fn tokens_for_uid(&self, app: &str, uid: &str) -> Result<Vec<String>, DeviceError> {
        let rows: Vec<String> = sqlx::query_scalar(
            "SELECT token FROM devices WHERE app = ? AND uid = ? \
             ORDER BY last_seen DESC, rowid ASC",
        )
        .bind(app)
        .bind(uid)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Bulk lookup for a list of uids: at most 50 bound parameters per query,
    /// de-duplicated, at most `cap` tokens.
    #[allow(dead_code)] // Wired to user/role-targeted sending in T8/T9.
    pub async fn tokens_for_uids(
        &self,
        app: &str,
        uids: &[String],
        cap: usize,
    ) -> Result<Vec<String>, DeviceError> {
        if uids.is_empty() || cap == 0 {
            return Ok(Vec::new());
        }

        let mut seen = HashSet::new();
        let mut tokens = Vec::new();
        for chunk in uids.chunks(50) {
            let mut builder = QueryBuilder::<Sqlite>::new("SELECT token FROM devices WHERE app = ");
            builder.push_bind(app.to_string());
            builder.push(" AND uid IN (");
            {
                let mut separated = builder.separated(", ");
                for uid in chunk {
                    separated.push_bind(uid.clone());
                }
            }
            builder.push(") ORDER BY last_seen DESC, rowid ASC");

            let rows: Vec<String> = builder.build_query_scalar().fetch_all(&self.pool).await?;
            for token in rows {
                if seen.insert(token.clone()) {
                    tokens.push(token);
                    if tokens.len() >= cap {
                        return Ok(tokens);
                    }
                }
            }
        }
        Ok(tokens)
    }

    /// Removes a token FCM reported as dead, whatever its owner.
    #[allow(dead_code)] // Wired to FCM error handling in T8.
    pub async fn remove_dead_token(&self, app: &str, token: &str) -> Result<bool, DeviceError> {
        let result = sqlx::query("DELETE FROM devices WHERE app = ? AND token = ?")
            .bind(app)
            .bind(token)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;
    use std::sync::Arc;

    const APP: &str = "test-app";
    const OTHER_APP: &str = "other-app";

    async fn memory_pool(connections: u32) -> SqlitePool {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .expect("the options must parse")
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(connections)
            .connect_with(options)
            .await
            .expect("the in-memory database must open");
        crate::db::migrate(&pool)
            .await
            .expect("the migration must run");
        pool
    }

    async fn store() -> DeviceStore {
        DeviceStore::new(memory_pool(1).await)
    }

    fn token(number: usize) -> String {
        format!("device-token-{number:0>24}")
    }

    // ---------- store ----------

    #[tokio::test]
    async fn register_inserts_and_updates_last_seen() {
        let store = store().await;
        let token = token(1);

        assert_eq!(
            store
                .register(APP, "uid-1", &token, Platform::Android, 5)
                .await
                .expect("the first registration"),
            0
        );
        sqlx::query(
            "UPDATE devices SET last_seen = '2000-01-01 00:00:00' WHERE app = ? AND token = ?",
        )
        .bind(APP)
        .bind(&token)
        .execute(&store.pool)
        .await
        .expect("the last_seen update");

        store
            .register(APP, "uid-1", &token, Platform::Ios, 5)
            .await
            .expect("the second registration");

        let (platform, last_seen): (String, String) =
            sqlx::query_as("SELECT platform, last_seen FROM devices WHERE app = ? AND token = ?")
                .bind(APP)
                .bind(&token)
                .fetch_one(&store.pool)
                .await
                .expect("the row must exist");
        assert_eq!(platform, "ios");
        assert_ne!(last_seen, "2000-01-01 00:00:00");
        assert_eq!(
            store
                .tokens_for_uid(APP, "uid-1")
                .await
                .expect("the tokens"),
            vec![token]
        );
    }

    #[tokio::test]
    async fn register_same_token_transfers_ownership() {
        let store = store().await;
        let token = token(1);

        store
            .register(APP, "uid-1", &token, Platform::Android, 5)
            .await
            .expect("the first owner");
        store
            .register(APP, "uid-2", &token, Platform::Web, 5)
            .await
            .expect("the transfer");

        assert!(store
            .tokens_for_uid(APP, "uid-1")
            .await
            .expect("the old owner")
            .is_empty());
        assert_eq!(
            store
                .tokens_for_uid(APP, "uid-2")
                .await
                .expect("the new owner"),
            vec![token]
        );
    }

    #[tokio::test]
    async fn register_enforces_max_per_uid_evicting_oldest() {
        let store = store().await;
        for (index, number) in [1usize, 2, 3].into_iter().enumerate() {
            let token = token(number);
            store
                .register(APP, "uid-1", &token, Platform::Android, 5)
                .await
                .expect("the seed registration");
            sqlx::query("UPDATE devices SET last_seen = ? WHERE app = ? AND token = ?")
                .bind(format!("2020-01-0{} 00:00:00", index + 1))
                .bind(APP)
                .bind(&token)
                .execute(&store.pool)
                .await
                .expect("the last_seen update");
        }

        let newest = token(4);
        let evicted = store
            .register(APP, "uid-1", &newest, Platform::Android, 3)
            .await
            .expect("the eviction");

        assert_eq!(evicted, 1);
        let mut tokens = store
            .tokens_for_uid(APP, "uid-1")
            .await
            .expect("the remaining tokens");
        tokens.sort();
        let mut expected = vec![token(2), token(3), newest];
        expected.sort();
        assert_eq!(tokens, expected);
    }

    #[tokio::test]
    async fn register_never_evicts_the_new_device() {
        let store = store().await;
        for number in 1..=3 {
            store
                .register(APP, "uid-1", &token(number), Platform::Android, 5)
                .await
                .expect("the seed registration");
        }

        let newest = token(4);
        let evicted = store
            .register(APP, "uid-1", &newest, Platform::Ios, 1)
            .await
            .expect("the eviction");

        assert_eq!(evicted, 3);
        assert_eq!(
            store
                .tokens_for_uid(APP, "uid-1")
                .await
                .expect("the remaining tokens"),
            vec![newest]
        );
    }

    #[tokio::test]
    async fn max_per_uid_is_clamped() {
        let store = store().await;

        store
            .register(APP, "zero", &token(1), Platform::Android, 0)
            .await
            .expect("the first registration");
        store
            .register(APP, "zero", &token(2), Platform::Android, 0)
            .await
            .expect("the second registration");
        assert_eq!(
            store
                .tokens_for_uid(APP, "zero")
                .await
                .expect("the tokens")
                .len(),
            1,
            "0 must behave like 1"
        );

        for number in 100..121 {
            store
                .register(APP, "many", &token(number), Platform::Android, 1000)
                .await
                .expect("the registration");
        }
        assert_eq!(
            store
                .tokens_for_uid(APP, "many")
                .await
                .expect("the tokens")
                .len(),
            20,
            "1000 must behave like 20"
        );
    }

    #[tokio::test]
    async fn concurrent_registers_respect_limit() {
        let store = Arc::new(DeviceStore::new(memory_pool(5).await));

        let mut tasks = Vec::new();
        for number in 0..50 {
            let store = store.clone();
            tasks.push(tokio::spawn(async move {
                store
                    .register(APP, "uid-1", &token(number), Platform::Android, 3)
                    .await
            }));
        }
        let mut evicted = 0;
        for task in tasks {
            evicted += task
                .await
                .expect("the task must not panic")
                .expect("the concurrent registration");
        }

        assert_eq!(
            evicted, 47,
            "every registration beyond the limit evicts one"
        );
        assert_eq!(
            store
                .tokens_for_uid(APP, "uid-1")
                .await
                .expect("the tokens")
                .len(),
            3
        );
    }

    #[tokio::test]
    async fn unregister_removes_only_own_token() {
        let store = store().await;
        let own = token(1);
        let foreign = token(2);
        store
            .register(APP, "uid-1", &own, Platform::Android, 5)
            .await
            .expect("the own registration");
        store
            .register(APP, "uid-2", &foreign, Platform::Android, 5)
            .await
            .expect("the foreign registration");

        assert!(
            !store
                .unregister(APP, "uid-1", &foreign)
                .await
                .expect("the foreign unregister"),
            "another user's token must not be removed"
        );
        assert!(store
            .unregister(APP, "uid-1", &own)
            .await
            .expect("the own unregister"));
        assert!(store
            .tokens_for_uid(APP, "uid-1")
            .await
            .expect("the tokens")
            .is_empty());
        assert_eq!(
            store
                .tokens_for_uid(APP, "uid-2")
                .await
                .expect("the foreign tokens"),
            vec![foreign]
        );
    }

    #[tokio::test]
    async fn unregister_foreign_or_missing_returns_false() {
        let store = store().await;
        let own = token(1);
        store
            .register(APP, "uid-1", &own, Platform::Android, 5)
            .await
            .expect("the registration");

        assert!(!store
            .unregister(APP, "uid-2", &own)
            .await
            .expect("the foreign unregister"));
        assert!(!store
            .unregister(APP, "uid-1", &token(9))
            .await
            .expect("the missing unregister"));
        assert_eq!(
            store
                .tokens_for_uid(APP, "uid-1")
                .await
                .expect("the tokens"),
            vec![own]
        );
    }

    #[tokio::test]
    async fn tokens_for_uid_returns_only_that_app_and_uid() {
        let store = store().await;
        store
            .register(APP, "uid-1", &token(1), Platform::Android, 5)
            .await
            .expect("the registration");
        store
            .register(APP, "uid-2", &token(2), Platform::Android, 5)
            .await
            .expect("the registration");
        store
            .register(OTHER_APP, "uid-1", &token(3), Platform::Android, 5)
            .await
            .expect("the registration");

        assert_eq!(
            store
                .tokens_for_uid(APP, "uid-1")
                .await
                .expect("the tokens"),
            vec![token(1)]
        );
    }

    #[tokio::test]
    async fn tokens_for_uids_dedups_chunks_and_caps() {
        let store = store().await;
        let mut uids = Vec::new();
        for number in 0..60 {
            let uid = format!("uid-{number:02}");
            store
                .register(APP, &uid, &token(number), Platform::Android, 5)
                .await
                .expect("the registration");
            uids.push(uid);
        }
        let mut requested = uids.clone();
        requested.extend(uids.iter().take(10).cloned());

        let tokens = store
            .tokens_for_uids(APP, &requested, 200)
            .await
            .expect("the bulk lookup");
        assert_eq!(tokens.len(), 60);
        assert_eq!(tokens.iter().collect::<HashSet<_>>().len(), 60);

        let capped = store
            .tokens_for_uids(APP, &requested, 10)
            .await
            .expect("the capped lookup");
        assert_eq!(capped.len(), 10);
    }

    #[tokio::test]
    async fn remove_dead_token_removes_any_owner() {
        let store = store().await;
        let token = token(1);
        store
            .register(APP, "uid-1", &token, Platform::Android, 5)
            .await
            .expect("the registration");

        assert!(store
            .remove_dead_token(APP, &token)
            .await
            .expect("the removal"));
        assert!(!store
            .remove_dead_token(APP, &token)
            .await
            .expect("the second removal"));
    }

    #[tokio::test]
    async fn token_validation_rejects_bad_charset_and_length() {
        assert!(validate_token(&"a".repeat(32)).is_ok());
        assert!(validate_token(&"a".repeat(4096)).is_ok());
        assert!(validate_token(&"a".repeat(31)).is_err());
        assert!(validate_token(&"a".repeat(4097)).is_err());
        assert!(validate_token("a b c d e f g h i j k l m n o p q").is_err());
        assert!(validate_token(&format!("{}!", "a".repeat(40))).is_err());
        assert!(validate_token("tökén-tökén-tökén-tökén-tökén-tökén").is_err());
    }

    #[test]
    fn platform_rejects_unknown_value() {
        assert_eq!(
            serde_json::from_value::<Platform>(json!("android")).expect("android"),
            Platform::Android
        );
        assert_eq!(
            serde_json::from_value::<Platform>(json!("ios")).expect("ios"),
            Platform::Ios
        );
        assert_eq!(
            serde_json::from_value::<Platform>(json!("web")).expect("web"),
            Platform::Web
        );
        for invalid in ["windows", "Android", "IOS", "", "web "] {
            assert!(
                serde_json::from_value::<Platform>(json!(invalid)).is_err(),
                "{invalid:?} must be rejected"
            );
        }
    }

    #[tokio::test]
    async fn migration_is_idempotent() {
        let pool = memory_pool(1).await;
        crate::db::migrate(&pool)
            .await
            .expect("the second migration must succeed");
        sqlx::query("INSERT INTO devices (app, token, uid, platform) VALUES (?, ?, ?, ?)")
            .bind(APP)
            .bind(token(1))
            .bind("uid-1")
            .bind("android")
            .execute(&pool)
            .await
            .expect("the insert");
        crate::db::migrate(&pool)
            .await
            .expect("the third migration must succeed");

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM devices")
            .fetch_one(&pool)
            .await
            .expect("the count");
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn token_never_appears_in_error_text() {
        let bad = "bad token with spaces and more text";
        let error = validate_token(bad).expect_err("the token must be rejected");
        assert!(!error.contains(bad));

        let store = store().await;
        let token = token(1);
        store.pool.close().await;
        let error = store
            .register(APP, "uid-1", &token, Platform::Android, 5)
            .await
            .expect_err("a closed pool must fail");
        assert!(!error.to_string().contains(&token));
    }
}

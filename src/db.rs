use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::collections::{HashMap, VecDeque};
use std::env;
use std::path::Path;
use std::str::FromStr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub async fn init_db() -> Result<SqlitePool, sqlx::Error> {
    let db_url = env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://data.db".to_string());
    let db_path = db_url.replace("sqlite://", "");

    // Create the database file if it doesn't exist
    if !Path::new(&db_path).exists() {
        std::fs::File::create(&db_path).map_err(sqlx::Error::Io)?;
    }

    // SQLite enforces foreign keys per connection and has them off by default.
    let options = SqliteConnectOptions::from_str(&db_url)?.foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    migrate(&pool).await?;

    Ok(pool)
}

/// Creates the tables and runs the safe migrations.
///
/// Kept separate from `init_db()` so the tests can run it on `sqlite::memory:`.
pub async fn migrate(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS notifications_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            app TEXT NOT NULL,
            target TEXT NOT NULL,
            title TEXT NOT NULL,
            status TEXT NOT NULL,
            response TEXT,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            payload TEXT,
            sender_uid TEXT,
            sender_email TEXT
        );
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS topics (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            app TEXT NOT NULL,
            name TEXT NOT NULL,
            UNIQUE(app, name)
        );
        "#,
    )
    .execute(pool)
    .await?;

    // App settings and templates (T1). The DDL follows the plan (3.14); the
    // `version` column on `app_configs` is the approved addition.
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS app_configs (
            app TEXT PRIMARY KEY, config TEXT NOT NULL,           -- JSON without templates
            version INTEGER NOT NULL DEFAULT 1,
            updated_by TEXT NOT NULL, updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS templates (
            app TEXT NOT NULL, key TEXT NOT NULL, definition TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1, version INTEGER NOT NULL,
            updated_by TEXT NOT NULL, updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            last_used_at DATETIME,
            PRIMARY KEY (app, key)
        );
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS config_versions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            app TEXT NOT NULL, section TEXT NOT NULL,             -- identity | data_source | ... | template:<key>
            content TEXT NOT NULL, changed_by TEXT NOT NULL, created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS audit_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT, actor TEXT NOT NULL, app TEXT, action TEXT NOT NULL,
            detail TEXT, created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        "#,
    )
    .execute(pool)
    .await?;

    // Dashboard accounts, their app bindings and their sessions (T2). Times in
    // `tool_sessions` are epoch seconds so comparisons never depend on text.
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS tool_users (
            id INTEGER PRIMARY KEY AUTOINCREMENT, email TEXT NOT NULL UNIQUE COLLATE NOCASE,
            password_hash TEXT NOT NULL, role TEXT NOT NULL CHECK (role IN ('owner','manager')),
            must_change_password INTEGER NOT NULL DEFAULT 1, test_uid TEXT,
            disabled INTEGER NOT NULL DEFAULT 0, created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS tool_user_apps (
            user_id INTEGER NOT NULL REFERENCES tool_users(id) ON DELETE CASCADE, app TEXT NOT NULL,
            PRIMARY KEY (user_id, app)
        );
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS tool_sessions (
            id_hash TEXT PRIMARY KEY, user_id INTEGER NOT NULL REFERENCES tool_users(id) ON DELETE CASCADE,
            expires_at INTEGER NOT NULL, reauth_at INTEGER, created_at INTEGER NOT NULL,
            ip TEXT, user_agent TEXT
        );
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_tool_sessions_user ON tool_sessions(user_id)")
        .execute(pool)
        .await?;

    // Device registrations (T6): one token per `(app, token)`, owned by one uid.
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS devices (
            app TEXT NOT NULL, token TEXT NOT NULL, uid TEXT NOT NULL,
            platform TEXT NOT NULL, created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            last_seen DATETIME DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (app, token)
        );
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_devices_uid ON devices(app, uid)")
        .execute(pool)
        .await?;

    // Safe migrations for existing databases. Errors (e.g. duplicate column) are
    // expected and ignored; no data is dropped.
    let _ = sqlx::query("ALTER TABLE notifications_history ADD COLUMN payload TEXT")
        .execute(pool)
        .await;
    let _ = sqlx::query("ALTER TABLE notifications_history ADD COLUMN sender_uid TEXT")
        .execute(pool)
        .await;
    let _ = sqlx::query("ALTER TABLE notifications_history ADD COLUMN sender_email TEXT")
        .execute(pool)
        .await;
    let _ = sqlx::query("ALTER TABLE topics ADD COLUMN app TEXT DEFAULT ''")
        .execute(pool)
        .await;
    // The `version` column was added to `app_configs` after the first release.
    let _ = sqlx::query("ALTER TABLE app_configs ADD COLUMN version INTEGER NOT NULL DEFAULT 1")
        .execute(pool)
        .await;

    // T8: history rows are typed (`raw` or `template`) and a template row
    // points at the template that produced it.
    if !has_column(pool, "notifications_history", "kind").await? {
        let _ = sqlx::query("ALTER TABLE notifications_history ADD COLUMN kind TEXT")
            .execute(pool)
            .await;
    }
    if !has_column(pool, "notifications_history", "ref").await? {
        let _ = sqlx::query("ALTER TABLE notifications_history ADD COLUMN \"ref\" TEXT")
            .execute(pool)
            .await;
    }

    // T8: one row per claimed dedupe key, with its expiry in UNIX seconds.
    // Only a SHA-256 hash of the key is stored.
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS template_dedupe (
            app TEXT NOT NULL,
            template TEXT NOT NULL,
            key_hash TEXT NOT NULL,
            expires_at INTEGER NOT NULL,
            PRIMARY KEY (app, template, key_hash)
        );
        "#,
    )
    .execute(pool)
    .await?;

    Ok(())
}

/// Does `table` already have `column`? Used to make the `ALTER`s idempotent.
async fn has_column(pool: &SqlitePool, table: &str, column: &str) -> Result<bool, sqlx::Error> {
    let names: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info(?)")
        .bind(table)
        .fetch_all(pool)
        .await?;
    Ok(names.iter().any(|name| name == column))
}

/// Appends one row to the audit log. Dashboard actions pass `None` for the app.
pub async fn write_audit(
    pool: &SqlitePool,
    actor: &str,
    app: Option<&str>,
    action: &str,
    detail: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO audit_log (actor, app, action, detail) VALUES (?, ?, ?, ?)")
        .bind(actor)
        .bind(app)
        .bind(action)
        .bind(detail)
        .execute(pool)
        .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn log_notification(
    pool: &SqlitePool,
    app: &str,
    target: &str,
    title: &str,
    status: &str,
    response: &str,
    payload: &str,
    sender_uid: &str,
    sender_email: Option<&str>,
) -> Result<(), sqlx::Error> {
    log_notification_ext(
        pool,
        app,
        target,
        title,
        status,
        response,
        payload,
        sender_uid,
        sender_email,
        "raw",
        None,
    )
    .await
}

/// Appends one history row, typed by `kind` and optionally linked to a template.
#[allow(clippy::too_many_arguments)]
pub async fn log_notification_ext(
    pool: &SqlitePool,
    app: &str,
    target: &str,
    title: &str,
    status: &str,
    response: &str,
    payload: &str,
    sender_uid: &str,
    sender_email: Option<&str>,
    kind: &str,
    reference: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO notifications_history (app, target, title, status, response, payload, sender_uid, sender_email, kind, \"ref\") VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(app)
    .bind(target)
    .bind(title)
    .bind(status)
    .bind(response)
    .bind(payload)
    .bind(sender_uid)
    .bind(sender_email)
    .bind(kind)
    .bind(reference)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn save_topic(pool: &SqlitePool, app: &str, name: &str) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT OR IGNORE INTO topics (app, name) VALUES (?, ?)")
        .bind(app)
        .bind(name)
        .execute(pool)
        .await?;
    Ok(())
}

// ---------- T8: template dedupe ----------

/// SHA-256 of a dedupe key, lowercase hex. The key itself is never stored.
fn sha256_hex(value: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(value.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// Claims a dedupe key until `now + seconds`.
///
/// Returns `true` when this call owns the key and `false` for a duplicate that
/// is still valid. The insert is a single atomic statement, so two concurrent
/// callers cannot both win. Expired keys for the same template are dropped
/// first, which keeps the table bounded without a background task.
pub async fn claim_dedupe(
    pool: &SqlitePool,
    app: &str,
    template: &str,
    key: &str,
    seconds: u32,
    now: i64,
) -> Result<bool, sqlx::Error> {
    let key_hash = sha256_hex(key);
    let expires_at = now.saturating_add(i64::from(seconds));

    sqlx::query("DELETE FROM template_dedupe WHERE app = ? AND template = ? AND expires_at <= ?")
        .bind(app)
        .bind(template)
        .bind(now)
        .execute(pool)
        .await?;

    let result = sqlx::query(
        "INSERT INTO template_dedupe (app, template, key_hash, expires_at) VALUES (?, ?, ?, ?) \
         ON CONFLICT(app, template, key_hash) DO UPDATE SET expires_at = excluded.expires_at \
         WHERE template_dedupe.expires_at <= ?",
    )
    .bind(app)
    .bind(template)
    .bind(&key_hash)
    .bind(expires_at)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Drops a claim so the same key can be used again (nothing was delivered).
pub async fn release_dedupe(
    pool: &SqlitePool,
    app: &str,
    template: &str,
    key: &str,
) -> Result<(), sqlx::Error> {
    let key_hash = sha256_hex(key);
    sqlx::query("DELETE FROM template_dedupe WHERE app = ? AND template = ? AND key_hash = ?")
        .bind(app)
        .bind(template)
        .bind(&key_hash)
        .execute(pool)
        .await?;
    Ok(())
}

/// Records that a template produced at least one delivery. The version, the
/// definition and `updated_at` are untouched.
pub async fn touch_template_used(
    pool: &SqlitePool,
    app: &str,
    key: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE templates SET last_used_at = CURRENT_TIMESTAMP WHERE app = ? AND key = ?")
        .bind(app)
        .bind(key)
        .execute(pool)
        .await?;
    Ok(())
}

// ---------- T8: per-uid template rate limit ----------

/// Sliding window of the per-uid template limit (plan 3.5/3.15).
const TEMPLATE_RATE_WINDOW: Duration = Duration::from_secs(3600);

/// Largest number of distinct `(app, template, uid)` keys kept in memory.
const MAX_TEMPLATE_RATE_KEYS: usize = 10_000;

/// In-memory sliding-window limiter keyed by `(app, template, uid)`.
///
/// The lock is a `std::sync::Mutex` and is never held across an `await`.
#[derive(Default)]
pub struct TemplateRateLimiter {
    entries: Mutex<HashMap<(String, String, String), VecDeque<Instant>>>,
}

impl TemplateRateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one attempt and reports whether it is within `limit` per hour.
    pub fn allow(&self, key: (&str, &str, &str), limit: u32) -> bool {
        self.allow_at(key, limit, Instant::now())
    }

    fn allow_at(&self, key: (&str, &str, &str), limit: u32, now: Instant) -> bool {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if entries.len() >= MAX_TEMPLATE_RATE_KEYS {
            entries.retain(|_, hits| {
                hits.back()
                    .is_some_and(|last| now.duration_since(*last) < TEMPLATE_RATE_WINDOW)
            });
            if entries.len() >= MAX_TEMPLATE_RATE_KEYS {
                entries.clear();
            }
        }

        let hits = entries
            .entry((key.0.to_string(), key.1.to_string(), key.2.to_string()))
            .or_default();
        while hits
            .front()
            .is_some_and(|first| now.duration_since(*first) >= TEMPLATE_RATE_WINDOW)
        {
            hits.pop_front();
        }

        if hits.len() as u64 >= u64::from(limit) {
            return false;
        }
        hits.push_back(now);
        true
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries
            .lock()
            .map(|entries| entries.len())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::time::{Duration, Instant};

    async fn memory_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("the in-memory database must open");
        migrate(&pool).await.expect("the migrations must run");
        pool
    }

    async fn column_names(pool: &SqlitePool, table: &str) -> Vec<String> {
        sqlx::query_scalar("SELECT name FROM pragma_table_info(?)")
            .bind(table)
            .fetch_all(pool)
            .await
            .expect("the columns must be readable")
    }

    #[tokio::test]
    async fn migration_adds_kind_and_ref_once() {
        let pool = memory_pool().await;
        // Running the migrations again must be a no-op, not an error.
        migrate(&pool).await.expect("the second migration must run");

        let columns = column_names(&pool, "notifications_history").await;
        assert_eq!(columns.iter().filter(|name| *name == "kind").count(), 1);
        assert_eq!(columns.iter().filter(|name| *name == "ref").count(), 1);
        assert!(columns.iter().any(|name| name == "payload"));

        let tables: Vec<String> =
            sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'table'")
                .fetch_all(&pool)
                .await
                .expect("the tables must be readable");
        assert!(tables.iter().any(|name| name == "template_dedupe"));
    }

    #[tokio::test]
    async fn log_notification_defaults_kind_raw() {
        let pool = memory_pool().await;

        log_notification(
            &pool,
            "app",
            "topic:all",
            "Title",
            "success",
            "{}",
            "{}",
            "uid-1",
            None,
        )
        .await
        .expect("the history row must be written");

        let row: (String, Option<String>) =
            sqlx::query_as("SELECT kind, \"ref\" FROM notifications_history")
                .fetch_one(&pool)
                .await
                .expect("the history row must exist");
        assert_eq!(row.0, "raw");
        assert_eq!(row.1, None);
    }

    #[tokio::test]
    async fn dedupe_key_is_stored_hashed() {
        let pool = memory_pool().await;
        let key = "order-42-seller-7";

        assert!(claim_dedupe(&pool, "app", "order_new", key, 600, 1_000)
            .await
            .expect("the claim must run"));

        let stored: String = sqlx::query_scalar("SELECT key_hash FROM template_dedupe")
            .fetch_one(&pool)
            .await
            .expect("the dedupe row must exist");
        assert_ne!(stored, key);
        assert_eq!(stored.len(), 64);
        assert!(stored.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(stored, sha256_hex(key));
    }

    #[tokio::test]
    async fn claim_dedupe_is_atomic_and_expires() {
        let pool = memory_pool().await;
        let now = 1_000;

        let mut tasks = Vec::new();
        for _ in 0..20 {
            let pool = pool.clone();
            tasks.push(tokio::spawn(async move {
                claim_dedupe(&pool, "app", "order_new", "same-key", 600, now).await
            }));
        }

        let mut claimed = 0;
        for task in tasks {
            let result = task.await.expect("the claim task must not panic");
            if result.expect("the claim must not fail") {
                claimed += 1;
            }
        }
        assert_eq!(claimed, 1, "exactly one concurrent claim may win");

        // Still valid one second before expiry.
        assert!(
            !claim_dedupe(&pool, "app", "order_new", "same-key", 600, now + 599)
                .await
                .expect("the claim must run")
        );
        // Expired: the key can be claimed again.
        assert!(
            claim_dedupe(&pool, "app", "order_new", "same-key", 600, now + 600)
                .await
                .expect("the claim must run")
        );
    }

    #[tokio::test]
    async fn release_dedupe_allows_retry() {
        let pool = memory_pool().await;
        let now = 2_000;

        assert!(claim_dedupe(&pool, "app", "order_new", "key-1", 600, now)
            .await
            .expect("the first claim must run"));
        assert!(!claim_dedupe(&pool, "app", "order_new", "key-1", 600, now)
            .await
            .expect("the duplicate claim must run"));

        release_dedupe(&pool, "app", "order_new", "key-1")
            .await
            .expect("the release must run");

        assert!(claim_dedupe(&pool, "app", "order_new", "key-1", 600, now)
            .await
            .expect("the retry claim must run"));
    }

    #[tokio::test]
    async fn touch_template_used_changes_only_last_used_at() {
        let pool = memory_pool().await;
        sqlx::query(
            "INSERT INTO templates (app, key, definition, enabled, version, updated_by, updated_at, last_used_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, NULL)",
        )
        .bind("app")
        .bind("order_new")
        .bind(r#"{"to":{"uid":"x"}}"#)
        .bind(1_i64)
        .bind(3_i64)
        .bind("tester")
        .bind("2020-01-01 00:00:00")
        .execute(&pool)
        .await
        .expect("the template row must be inserted");

        touch_template_used(&pool, "app", "order_new")
            .await
            .expect("the touch must run");

        let row: (Option<String>, i64, String, String) = sqlx::query_as(
            "SELECT last_used_at, version, definition, updated_at FROM templates WHERE app = ? AND key = ?",
        )
        .bind("app")
        .bind("order_new")
        .fetch_one(&pool)
        .await
        .expect("the template row must exist");

        assert!(row.0.is_some(), "last_used_at must be set");
        assert_eq!(row.1, 3, "the version must not change");
        assert_eq!(row.2, r#"{"to":{"uid":"x"}}"#);
        assert_eq!(row.3, "2020-01-01 00:00:00");
    }

    #[test]
    fn template_rate_limiter_window_and_bound() {
        let limiter = TemplateRateLimiter::new();
        let start = Instant::now();
        let key = ("app", "order_new", "uid-1");

        assert!(limiter.allow_at(key, 2, start));
        assert!(limiter.allow_at(key, 2, start + Duration::from_secs(1)));
        assert!(!limiter.allow_at(key, 2, start + Duration::from_secs(2)));

        // Another uid has its own window.
        assert!(limiter.allow_at(("app", "order_new", "uid-2"), 2, start));

        // The first two hits leave the window after one hour.
        assert!(limiter.allow_at(key, 2, start + Duration::from_secs(3601)));

        // The map never exceeds its cap, and a new key still works.
        let limiter = TemplateRateLimiter::new();
        for index in 0..MAX_TEMPLATE_RATE_KEYS {
            let uid = format!("uid-{index}");
            assert!(limiter.allow_at(("app", "order_new", &uid), 5, start));
        }
        assert_eq!(limiter.len(), MAX_TEMPLATE_RATE_KEYS);
        assert!(limiter.allow_at(("app", "order_new", "uid-extra"), 5, start));
        assert!(limiter.len() <= MAX_TEMPLATE_RATE_KEYS);
    }
}

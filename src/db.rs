use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions, SqliteRow};
use sqlx::{Row, SqlitePool};
use std::collections::{HashMap, VecDeque};
use std::env;
use std::path::Path;
use std::str::FromStr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::config::{Quota, Review, ReviewMode};

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

    // T9: company campaigns, exactly as plan 3.14. The raw condition is kept so
    // an approved campaign can be dispatched later; it is never returned by the
    // list endpoint.
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS campaigns (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            app TEXT NOT NULL, entity_id TEXT NOT NULL, sender_uid TEXT NOT NULL,
            title TEXT NOT NULL, body TEXT NOT NULL, image TEXT,
            data TEXT NOT NULL, audience TEXT NOT NULL, condition TEXT NOT NULL,
            status TEXT NOT NULL, review_note TEXT, reviewer TEXT, fcm_response TEXT,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP, decided_at DATETIME, sent_at DATETIME
        );
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS campaign_blocks (
            app TEXT NOT NULL, entity_id TEXT NOT NULL, reason TEXT,
            blocked_by TEXT, created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (app, entity_id)
        );
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_campaigns_entity ON campaigns(app, entity_id, created_at)",
    )
    .execute(pool)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_campaigns_status ON campaigns(app, status)")
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

// ---------- T10-A: dashboard reads ----------

/// Per-template metadata that is not part of the in-memory snapshot.
pub struct TemplateMeta {
    pub key: String,
    pub version: i64,
    pub last_used_at: Option<String>,
    /// `last_used_at` is inside the last 30 days (the delete guard, plan 3.5).
    pub recently_used: bool,
}

/// Template versions and last use. Read-only; the store still owns every write.
pub async fn template_metadata(
    pool: &SqlitePool,
    app: &str,
) -> Result<Vec<TemplateMeta>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT key, version, last_used_at, \
         CASE WHEN last_used_at IS NOT NULL AND last_used_at >= datetime('now', '-30 days') \
         THEN 1 ELSE 0 END AS recently_used \
         FROM templates WHERE app = ? ORDER BY key",
    )
    .bind(app)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| TemplateMeta {
            key: row.get("key"),
            version: row.get("version"),
            last_used_at: row.try_get("last_used_at").unwrap_or_default(),
            recently_used: row.try_get::<i64, _>("recently_used").unwrap_or(0) != 0,
        })
        .collect())
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

// ---------- T9: campaigns ----------

/// One day and one week in UNIX seconds (the quota windows).
const DAY_SECONDS: i64 = 86_400;
const WEEK_SECONDS: i64 = 604_800;
/// `fcm_response` is a short diagnostic, never a token and never a payload.
const MAX_FCM_RESPONSE_CHARS: usize = 2000;

/// Serializes the quota check and the insert. The plan requires one write lock
/// around one transaction, so two concurrent creates cannot both pass the check.
static CAMPAIGN_WRITE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A campaign row about to be inserted.
///
/// The review policy rides with the row because the step's signature passes the
/// quota separately; `sender_is_service` is the trusted sender's bypass.
pub struct NewCampaign {
    pub app: String,
    pub entity_id: String,
    pub sender_uid: String,
    pub title: String,
    pub body: String,
    pub image: Option<String>,
    pub data: String,
    pub audience: String,
    pub condition: String,
    pub review: Review,
    pub sender_is_service: bool,
}

/// The id and status of an inserted campaign.
#[derive(Debug)]
pub struct Inserted {
    pub id: i64,
    pub status: String,
}

/// Why an insert was refused.
#[derive(Debug)]
pub enum InsertError {
    QuotaExceeded { retry_after_seconds: i64 },
    Database(sqlx::Error),
}

impl From<sqlx::Error> for InsertError {
    fn from(error: sqlx::Error) -> Self {
        InsertError::Database(error)
    }
}

/// A stored campaign, including the fields the list endpoint must not expose.
pub struct Campaign {
    pub id: i64,
    pub app: String,
    pub entity_id: String,
    pub sender_uid: String,
    pub title: String,
    pub body: String,
    pub image: Option<String>,
    pub data: String,
    pub audience: String,
    pub condition: String,
    pub status: String,
    pub review_note: Option<String>,
    pub created_at: String,
    pub sent_at: Option<String>,
}

fn campaign_from_row(row: SqliteRow) -> Campaign {
    Campaign {
        id: row.get("id"),
        app: row.get("app"),
        entity_id: row.get("entity_id"),
        sender_uid: row.get("sender_uid"),
        title: row.get("title"),
        body: row.get("body"),
        image: row.get("image"),
        data: row.get("data"),
        audience: row.get("audience"),
        condition: row.get("condition"),
        status: row.get("status"),
        review_note: row.get("review_note"),
        created_at: row.get("created_at"),
        sent_at: row.get("sent_at"),
    }
}

/// Inserts a campaign after checking the entity quota inside one transaction.
///
/// Counted statuses are `pending_review`, `sending` and `sent`; rejected,
/// cancelled and failed campaigns never consume quota.
pub async fn insert_campaign_checked(
    pool: &SqlitePool,
    new: &NewCampaign,
    limits: &Quota,
    now: i64,
) -> Result<Inserted, InsertError> {
    let _guard = CAMPAIGN_WRITE_LOCK.lock().await;
    let mut tx = pool.begin().await?;

    let timestamps: Vec<i64> = sqlx::query_scalar(
        "SELECT CAST(strftime('%s', created_at) AS INTEGER) FROM campaigns \
         WHERE app = ? AND entity_id = ? AND status IN ('pending_review','sending','sent')",
    )
    .bind(&new.app)
    .bind(&new.entity_id)
    .fetch_all(&mut *tx)
    .await?;

    let day_start = now.saturating_sub(DAY_SECONDS);
    let week_start = now.saturating_sub(WEEK_SECONDS);
    let in_day = timestamps.iter().filter(|at| **at >= day_start).count();
    let in_week = timestamps.iter().filter(|at| **at >= week_start).count();

    if in_day as u64 >= u64::from(limits.per_entity_per_day) {
        let oldest = timestamps
            .iter()
            .filter(|at| **at >= day_start)
            .min()
            .copied()
            .unwrap_or(now);
        let retry_after_seconds = (oldest + DAY_SECONDS - now).max(1);
        return Err(InsertError::QuotaExceeded {
            retry_after_seconds,
        });
    }
    if in_week as u64 >= u64::from(limits.per_entity_per_week) {
        let oldest = timestamps
            .iter()
            .filter(|at| **at >= week_start)
            .min()
            .copied()
            .unwrap_or(now);
        let retry_after_seconds = (oldest + WEEK_SECONDS - now).max(1);
        return Err(InsertError::QuotaExceeded {
            retry_after_seconds,
        });
    }

    let reviewed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM campaigns \
         WHERE app = ? AND entity_id = ? AND status IN ('sent','sending')",
    )
    .bind(&new.app)
    .bind(&new.entity_id)
    .fetch_one(&mut *tx)
    .await?;

    let needs_review = if new.sender_is_service {
        false
    } else {
        match new.review.mode {
            ReviewMode::None => false,
            ReviewMode::Always => true,
            ReviewMode::FirstN => reviewed < i64::from(new.review.n.unwrap_or(0)),
        }
    };
    let status = if needs_review {
        "pending_review"
    } else {
        "sending"
    };

    let result = sqlx::query(
        "INSERT INTO campaigns \
         (app, entity_id, sender_uid, title, body, image, data, audience, condition, status) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&new.app)
    .bind(&new.entity_id)
    .bind(&new.sender_uid)
    .bind(&new.title)
    .bind(&new.body)
    .bind(&new.image)
    .bind(&new.data)
    .bind(&new.audience)
    .bind(&new.condition)
    .bind(status)
    .execute(&mut *tx)
    .await?;

    let id = result.last_insert_rowid();
    tx.commit().await?;
    Ok(Inserted {
        id,
        status: status.to_string(),
    })
}

/// Loads one campaign by id.
pub async fn get_campaign(pool: &SqlitePool, id: i64) -> Result<Option<Campaign>, sqlx::Error> {
    let row = sqlx::query("SELECT * FROM campaigns WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(campaign_from_row))
}

/// Lists an app's campaigns, optionally restricted to one entity.
///
/// `limit` is capped at 100; `offset` is passed through.
pub async fn list_campaigns(
    pool: &SqlitePool,
    app: &str,
    entity_id: Option<&str>,
    limit: u32,
    offset: u32,
) -> Result<Vec<Campaign>, sqlx::Error> {
    let limit = i64::from(limit.min(100));
    let offset = i64::from(offset);
    let rows = match entity_id {
        Some(entity_id) => {
            sqlx::query(
                "SELECT * FROM campaigns WHERE app = ? AND entity_id = ? \
                 ORDER BY id DESC LIMIT ? OFFSET ?",
            )
            .bind(app)
            .bind(entity_id)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await?
        }
        None => {
            sqlx::query("SELECT * FROM campaigns WHERE app = ? ORDER BY id DESC LIMIT ? OFFSET ?")
                .bind(app)
                .bind(limit)
                .bind(offset)
                .fetch_all(pool)
                .await?
        }
    };
    Ok(rows.into_iter().map(campaign_from_row).collect())
}

/// Moves a campaign from `pending_review` to `sending` exactly once.
///
/// T9 creates campaigns that already need no review; the T10 approve route uses
/// this to claim a pending campaign.
#[allow(dead_code)]
pub async fn claim_for_sending(pool: &SqlitePool, id: i64) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE campaigns SET status = 'sending' WHERE id = ? AND status = 'pending_review'",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Cancels a pending campaign of the given app and entity.
pub async fn cancel_pending(
    pool: &SqlitePool,
    id: i64,
    app: &str,
    entity_id: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE campaigns SET status = 'cancelled' \
         WHERE id = ? AND app = ? AND entity_id = ? AND status = 'pending_review'",
    )
    .bind(id)
    .bind(app)
    .bind(entity_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Records the delivery outcome. `fcm_response` is truncated and never contains
/// a token; `sent_at` is set only for a successful send.
pub async fn finish_campaign(
    pool: &SqlitePool,
    id: i64,
    status: &str,
    fcm_response: &str,
) -> Result<(), sqlx::Error> {
    let response: String = fcm_response.chars().take(MAX_FCM_RESPONSE_CHARS).collect();
    sqlx::query(
        "UPDATE campaigns SET status = ?1, fcm_response = ?2, \
         sent_at = CASE WHEN ?1 = 'sent' THEN CURRENT_TIMESTAMP ELSE sent_at END \
         WHERE id = ?3",
    )
    .bind(status)
    .bind(&response)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Is this entity blocked from creating campaigns?
pub async fn is_entity_blocked(
    pool: &SqlitePool,
    app: &str,
    entity_id: &str,
) -> Result<bool, sqlx::Error> {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM campaign_blocks WHERE app = ? AND entity_id = ?")
            .bind(app)
            .bind(entity_id)
            .fetch_one(pool)
            .await?;
    Ok(count > 0)
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

    // ---------- T9: campaigns ----------

    fn quota(day: u32, week: u32) -> Quota {
        Quota {
            per_entity_per_day: day,
            per_entity_per_week: week,
        }
    }

    fn review(mode: ReviewMode, n: Option<u32>) -> Review {
        Review { mode, n }
    }

    fn new_campaign(entity: &str, review: Review, sender_is_service: bool) -> NewCampaign {
        NewCampaign {
            app: "app".to_string(),
            entity_id: entity.to_string(),
            sender_uid: "u1".to_string(),
            title: "Title".to_string(),
            body: "Body".to_string(),
            image: None,
            data: "{}".to_string(),
            audience: r#"{"groups":[["users"]]}"#.to_string(),
            condition: "'promos' in topics".to_string(),
            review,
            sender_is_service,
        }
    }

    async fn raw_campaign(pool: &SqlitePool, entity: &str, status: &str, age: i64) -> i64 {
        sqlx::query(
            "INSERT INTO campaigns \
             (app, entity_id, sender_uid, title, body, data, audience, condition, status, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, datetime(?, 'unixepoch'))",
        )
        .bind("app")
        .bind(entity)
        .bind("u1")
        .bind("Title")
        .bind("Body")
        .bind("{}")
        .bind(r#"{"groups":[["users"]]}"#)
        .bind("'promos' in topics")
        .bind(status)
        .bind(crate::unix_seconds() - age)
        .execute(pool)
        .await
        .expect("the raw campaign must insert")
        .last_insert_rowid()
    }

    fn quota_retry(error: InsertError) -> i64 {
        match error {
            InsertError::QuotaExceeded {
                retry_after_seconds,
            } => retry_after_seconds,
            other => panic!("expected a quota error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn campaign_tables_migration_idempotent() {
        let pool = memory_pool().await;
        migrate(&pool).await.expect("the second migration must run");

        let tables: Vec<String> =
            sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'table'")
                .fetch_all(&pool)
                .await
                .expect("the tables must be readable");
        assert!(tables.iter().any(|name| name == "campaigns"));
        assert!(tables.iter().any(|name| name == "campaign_blocks"));

        let indexes: Vec<String> =
            sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'index'")
                .fetch_all(&pool)
                .await
                .expect("the indexes must be readable");
        assert!(indexes.iter().any(|name| name == "idx_campaigns_entity"));
        assert!(indexes.iter().any(|name| name == "idx_campaigns_status"));
    }

    #[tokio::test]
    async fn quota_day_and_week_enforced() {
        let pool = memory_pool().await;

        raw_campaign(&pool, "e1", "sent", 3_600).await;
        let daily = insert_campaign_checked(
            &pool,
            &new_campaign("e1", review(ReviewMode::None, None), false),
            &quota(1, 5),
            crate::unix_seconds(),
        )
        .await
        .expect_err("the daily quota must reject");
        let retry = quota_retry(daily);
        assert!((82_798..=82_800).contains(&retry), "retry was {retry}");

        raw_campaign(&pool, "e2", "sent", 3 * DAY_SECONDS).await;
        raw_campaign(&pool, "e2", "sending", 5 * DAY_SECONDS).await;
        let weekly = insert_campaign_checked(
            &pool,
            &new_campaign("e2", review(ReviewMode::None, None), false),
            &quota(10, 2),
            crate::unix_seconds(),
        )
        .await
        .expect_err("the weekly quota must reject");
        let retry = quota_retry(weekly);
        assert!((172_798..=172_800).contains(&retry), "retry was {retry}");
    }

    #[tokio::test]
    async fn quota_ignores_rejected_cancelled_failed() {
        let pool = memory_pool().await;
        for status in ["rejected", "cancelled", "failed"] {
            raw_campaign(&pool, "e1", status, 100).await;
        }

        let inserted = insert_campaign_checked(
            &pool,
            &new_campaign("e1", review(ReviewMode::None, None), false),
            &quota(1, 5),
            crate::unix_seconds(),
        )
        .await
        .expect("non-counted statuses must not consume quota");
        assert_eq!(inserted.status, "sending");
    }

    #[tokio::test]
    async fn quota_retry_after_is_positive() {
        let pool = memory_pool().await;

        // One second before the window closes: at least one second remains.
        raw_campaign(&pool, "e1", "sent", DAY_SECONDS - 1).await;
        let edge = insert_campaign_checked(
            &pool,
            &new_campaign("e1", review(ReviewMode::None, None), false),
            &quota(1, 5),
            crate::unix_seconds(),
        )
        .await
        .expect_err("the daily quota must reject");
        let retry = quota_retry(edge);
        assert!((1..=2).contains(&retry), "retry was {retry}");

        raw_campaign(&pool, "e2", "sent", 0).await;
        let fresh = insert_campaign_checked(
            &pool,
            &new_campaign("e2", review(ReviewMode::None, None), false),
            &quota(1, 5),
            crate::unix_seconds(),
        )
        .await
        .expect_err("the daily quota must reject");
        let retry = quota_retry(fresh);
        assert!((86_398..=86_400).contains(&retry), "retry was {retry}");
    }

    #[tokio::test]
    async fn quota_insert_is_race_safe() {
        let pool = memory_pool().await;
        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(10));
        let mut tasks = Vec::new();
        for index in 0..10 {
            let pool = pool.clone();
            let barrier = barrier.clone();
            tasks.push(tokio::spawn(async move {
                barrier.wait().await;
                let mut new = new_campaign("e1", review(ReviewMode::None, None), false);
                new.title = format!("Title {index}");
                insert_campaign_checked(&pool, &new, &quota(3, 10), crate::unix_seconds()).await
            }));
        }

        let mut inserted = 0;
        for task in tasks {
            let result = task.await.expect("the insert task must not panic");
            if result.is_ok() {
                inserted += 1;
            }
        }
        assert_eq!(inserted, 3, "the daily quota must hold under concurrency");

        let stored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM campaigns")
            .fetch_one(&pool)
            .await
            .expect("the campaigns must be readable");
        assert_eq!(stored, 3);
    }

    #[tokio::test]
    async fn first_n_review_counts_sent_and_sending_only() {
        let pool = memory_pool().await;
        let pending = raw_campaign(&pool, "e1", "pending_review", 10).await;
        raw_campaign(&pool, "e1", "sent", 10).await;
        raw_campaign(&pool, "e1", "sending", 10).await;

        let limits = quota(50, 50);
        let first = insert_campaign_checked(
            &pool,
            &new_campaign("e1", review(ReviewMode::FirstN, Some(3)), false),
            &limits,
            crate::unix_seconds(),
        )
        .await
        .expect("the insert must run");
        assert_eq!(first.status, "pending_review");

        // A second pending campaign does not increase the reviewed count.
        let second = insert_campaign_checked(
            &pool,
            &new_campaign("e1", review(ReviewMode::FirstN, Some(3)), false),
            &limits,
            crate::unix_seconds(),
        )
        .await
        .expect("the insert must run");
        assert_eq!(second.status, "pending_review");

        sqlx::query("UPDATE campaigns SET status = 'sent' WHERE id = ?")
            .bind(pending)
            .execute(&pool)
            .await
            .expect("the pending row must update");

        let third = insert_campaign_checked(
            &pool,
            &new_campaign("e1", review(ReviewMode::FirstN, Some(3)), false),
            &limits,
            crate::unix_seconds(),
        )
        .await
        .expect("the insert must run");
        assert_eq!(third.status, "sending");
    }

    #[tokio::test]
    async fn review_always_and_none() {
        let pool = memory_pool().await;
        let limits = quota(50, 50);

        let always = insert_campaign_checked(
            &pool,
            &new_campaign("e1", review(ReviewMode::Always, None), false),
            &limits,
            crate::unix_seconds(),
        )
        .await
        .expect("the insert must run");
        assert_eq!(always.status, "pending_review");

        let none = insert_campaign_checked(
            &pool,
            &new_campaign("e2", review(ReviewMode::None, None), false),
            &limits,
            crate::unix_seconds(),
        )
        .await
        .expect("the insert must run");
        assert_eq!(none.status, "sending");

        // The trusted service sender skips review.
        let service = insert_campaign_checked(
            &pool,
            &new_campaign("e3", review(ReviewMode::Always, None), true),
            &limits,
            crate::unix_seconds(),
        )
        .await
        .expect("the insert must run");
        assert_eq!(service.status, "sending");
    }

    #[tokio::test]
    async fn claim_for_sending_only_from_pending() {
        let pool = memory_pool().await;
        let pending = raw_campaign(&pool, "e1", "pending_review", 10).await;

        assert!(claim_for_sending(&pool, pending)
            .await
            .expect("the claim must run"));
        assert!(!claim_for_sending(&pool, pending)
            .await
            .expect("the second claim must run"));

        let sent = raw_campaign(&pool, "e1", "sent", 10).await;
        assert!(!claim_for_sending(&pool, sent)
            .await
            .expect("the claim must run"));
    }

    #[tokio::test]
    async fn cancel_pending_only_from_pending() {
        let pool = memory_pool().await;
        let pending = raw_campaign(&pool, "e1", "pending_review", 10).await;

        assert!(
            !cancel_pending(&pool, pending, "app", "e2")
                .await
                .expect("the cancel must run"),
            "another entity must not cancel the campaign"
        );
        assert!(cancel_pending(&pool, pending, "app", "e1")
            .await
            .expect("the cancel must run"));
        assert!(!cancel_pending(&pool, pending, "app", "e1")
            .await
            .expect("the second cancel must run"));

        let sent = raw_campaign(&pool, "e1", "sent", 10).await;
        assert!(!cancel_pending(&pool, sent, "app", "e1")
            .await
            .expect("the cancel must run"));
    }

    #[tokio::test]
    async fn list_campaigns_scoped_by_entity_and_limit_capped() {
        let pool = memory_pool().await;
        for index in 0..105 {
            raw_campaign(&pool, "e1", "pending_review", index).await;
        }
        raw_campaign(&pool, "e2", "pending_review", 1).await;

        let capped = list_campaigns(&pool, "app", Some("e1"), 500, 0)
            .await
            .expect("the list must run");
        assert_eq!(capped.len(), 100, "the limit must be capped at 100");

        let first_page = list_campaigns(&pool, "app", Some("e1"), 2, 0)
            .await
            .expect("the list must run");
        assert_eq!(first_page.len(), 2);
        assert!(first_page[0].id > first_page[1].id, "newest first");

        let second_page = list_campaigns(&pool, "app", Some("e1"), 2, 2)
            .await
            .expect("the list must run");
        assert_eq!(second_page.len(), 2);
        assert!(second_page[0].id < first_page[1].id);

        let other = list_campaigns(&pool, "app", Some("e2"), 100, 0)
            .await
            .expect("the list must run");
        assert_eq!(other.len(), 1);
        assert!(other.iter().all(|campaign| campaign.entity_id == "e2"));

        let all = list_campaigns(&pool, "app", None, 100, 0)
            .await
            .expect("the list must run");
        assert_eq!(all.len(), 100);
    }
}

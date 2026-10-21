use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::env;
use std::path::Path;
use std::str::FromStr;

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

    Ok(())
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
    sqlx::query(
        "INSERT INTO notifications_history (app, target, title, status, response, payload, sender_uid, sender_email) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(app)
    .bind(target)
    .bind(title)
    .bind(status)
    .bind(response)
    .bind(payload)
    .bind(sender_uid)
    .bind(sender_email)
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

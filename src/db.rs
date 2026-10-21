use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};
use std::env;
use std::path::Path;

pub async fn init_db() -> Result<SqlitePool, sqlx::Error> {
    let db_url = env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://data.db".to_string());
    let db_path = db_url.replace("sqlite://", "");

    // Create the database file if it doesn't exist
    if !Path::new(&db_path).exists() {
        std::fs::File::create(&db_path).map_err(sqlx::Error::Io)?;
    }

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
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

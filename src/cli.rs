//! Command line commands: `import`, `export`, `create-owner` and
//! `reset-owner-password`.
//!
//! No argument parsing crate: only `std::env::args`. They work on `DATABASE_URL`
//! as usual and need neither `credentials/` nor `CORS_ALLOWED_ORIGINS`.

use crate::admin::password;
use crate::config::store::{ConfigError, ConfigStore};
use crate::db;
use sqlx::SqlitePool;

/// Temporary password alphabet: no `0`, `O`, `1`, `l` or `I` to avoid
/// transcription mistakes when the password is copied by hand.
const TEMP_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
const TEMP_LENGTH: usize = 20;

/// The usage message.
pub fn usage() -> String {
    [
        "Usage:",
        "  fcm_microservice import <app> <file.json>   import a configuration document",
        "  fcm_microservice export <app>                 print the configuration as JSON",
        "  fcm_microservice create-owner --email <email>        create the first owner",
        "  fcm_microservice reset-owner-password --email <email> reset an owner password",
    ]
    .join("\n")
}

/// Runs a command line command and returns the exit code.
///
/// Only called when at least one argument exists; an unknown one => code 2.
pub async fn run(args: &[String]) -> i32 {
    let command = args.get(1).map(String::as_str).unwrap_or_default();

    match command {
        "import" => {
            if args.len() != 4 {
                eprintln!("{}", usage());
                return 2;
            }
            import(&args[2], &args[3]).await
        }
        "export" => {
            if args.len() != 3 {
                eprintln!("{}", usage());
                return 2;
            }
            export(&args[2]).await
        }
        "create-owner" => match email_argument(args) {
            Some(email) => create_owner_command(email).await,
            None => {
                eprintln!("{}", usage());
                2
            }
        },
        "reset-owner-password" => match email_argument(args) {
            Some(email) => reset_owner_command(email).await,
            None => {
                eprintln!("{}", usage());
                2
            }
        },
        _ => {
            eprintln!("unknown command '{}'\n{}", command, usage());
            2
        }
    }
}

/// Accepts exactly `<command> --email <value>`.
fn email_argument(args: &[String]) -> Option<&str> {
    if args.len() == 4 && args[2] == "--email" {
        Some(args[3].as_str())
    } else {
        None
    }
}

/// Validates the address loosely and lower-cases it before storage.
pub fn normalize_email(raw: &str) -> Result<String, String> {
    let email = raw.trim().to_lowercase();
    if email.is_empty() || email.len() > 254 || email.chars().any(char::is_whitespace) {
        return Err("the email is not valid".to_string());
    }
    let (local, domain) = email
        .split_once('@')
        .ok_or_else(|| "the email must contain '@'".to_string())?;
    if local.is_empty() || domain.is_empty() || !domain.contains('.') {
        return Err("the email is not valid".to_string());
    }
    if domain.starts_with('.') || domain.ends_with('.') {
        return Err("the email is not valid".to_string());
    }
    Ok(email)
}

/// A random temporary password that the owner must change at first login.
pub fn generate_temp_password() -> String {
    use rand::Rng;

    let mut rng = rand::rngs::OsRng;
    (0..TEMP_LENGTH)
        .map(|_| {
            let index = rng.gen_range(0..TEMP_ALPHABET.len());
            TEMP_ALPHABET[index] as char
        })
        .collect()
}

/// Creates an owner account and returns its temporary password.
pub async fn create_owner(pool: &SqlitePool, raw_email: &str) -> Result<String, String> {
    let email = normalize_email(raw_email)?;
    let existing: Option<i64> = sqlx::query_scalar("SELECT id FROM tool_users WHERE email = ?")
        .bind(&email)
        .fetch_optional(pool)
        .await
        .map_err(|error| format!("database error: {}", error))?;
    if existing.is_some() {
        return Err(format!("an account for '{}' already exists", email));
    }

    let temporary = generate_temp_password();
    let hash = password::hash_password(&temporary).await?;
    sqlx::query(
        "INSERT INTO tool_users (email, password_hash, role, must_change_password) \
         VALUES (?, ?, 'owner', 1)",
    )
    .bind(&email)
    .bind(&hash)
    .execute(pool)
    .await
    .map_err(|error| format!("database error: {}", error))?;

    db::write_audit(
        pool,
        "cli",
        None,
        "admin.create_owner",
        &format!("owner '{}' created", email),
    )
    .await
    .map_err(|error| format!("database error: {}", error))?;

    Ok(temporary)
}

/// Resets an owner password, signs out every session and returns the new
/// temporary password.
pub async fn reset_owner_password(pool: &SqlitePool, raw_email: &str) -> Result<String, String> {
    let email = normalize_email(raw_email)?;
    let row: Option<(i64, String)> =
        sqlx::query_as("SELECT id, role FROM tool_users WHERE email = ?")
            .bind(&email)
            .fetch_optional(pool)
            .await
            .map_err(|error| format!("database error: {}", error))?;
    let (user_id, role) = match row {
        Some(row) => row,
        None => return Err(format!("no account for '{}'", email)),
    };
    if role != "owner" {
        return Err(format!("'{}' is not an owner account", email));
    }

    let temporary = generate_temp_password();
    let hash = password::hash_password(&temporary).await?;

    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| format!("database error: {}", error))?;
    sqlx::query("UPDATE tool_users SET password_hash = ?, must_change_password = 1 WHERE id = ?")
        .bind(&hash)
        .bind(user_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| format!("database error: {}", error))?;
    sqlx::query("DELETE FROM tool_sessions WHERE user_id = ?")
        .bind(user_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| format!("database error: {}", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| format!("database error: {}", error))?;

    db::write_audit(
        pool,
        "cli",
        None,
        "admin.reset_owner_password",
        &format!("owner '{}' password reset; sessions deleted", email),
    )
    .await
    .map_err(|error| format!("database error: {}", error))?;

    Ok(temporary)
}

async fn create_owner_command(email: &str) -> i32 {
    let pool = match db::init_db().await {
        Ok(pool) => pool,
        Err(error) => {
            eprintln!("cannot open the database: {}", error);
            return 1;
        }
    };
    match create_owner(&pool, email).await {
        Ok(temporary) => {
            // The only place a temporary password is ever printed.
            println!("TEMP_PASSWORD={}", temporary);
            0
        }
        Err(message) => {
            eprintln!("{}", message);
            1
        }
    }
}

async fn reset_owner_command(email: &str) -> i32 {
    let pool = match db::init_db().await {
        Ok(pool) => pool,
        Err(error) => {
            eprintln!("cannot open the database: {}", error);
            return 1;
        }
    };
    match reset_owner_password(&pool, email).await {
        Ok(temporary) => {
            // The only place a temporary password is ever printed.
            println!("TEMP_PASSWORD={}", temporary);
            0
        }
        Err(message) => {
            eprintln!("{}", message);
            1
        }
    }
}

async fn import(app: &str, path: &str) -> i32 {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) => {
            eprintln!("cannot read '{}': {}", path, error);
            return 1;
        }
    };
    let document: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(document) => document,
        Err(error) => {
            eprintln!("'{}' is not valid JSON: {}", path, error);
            return 1;
        }
    };

    let pool = match db::init_db().await {
        Ok(pool) => pool,
        Err(error) => {
            eprintln!("cannot open the database: {}", error);
            return 1;
        }
    };
    let store = match ConfigStore::new(pool).await {
        Ok(store) => store,
        Err(error) => {
            eprintln!("cannot open the config store: {}", error);
            return 1;
        }
    };

    match store.import(app, document, "cli").await {
        Ok(()) => {
            println!("OK");
            0
        }
        Err(ConfigError::Invalid(issues)) => {
            for issue in &issues {
                println!("{}: {}", issue.path, issue.message);
            }
            1
        }
        Err(error) => {
            eprintln!("{}", error);
            1
        }
    }
}

async fn export(app: &str) -> i32 {
    let pool = match db::init_db().await {
        Ok(pool) => pool,
        Err(error) => {
            eprintln!("cannot open the database: {}", error);
            return 1;
        }
    };
    let store = match ConfigStore::new(pool).await {
        Ok(store) => store,
        Err(error) => {
            eprintln!("cannot open the config store: {}", error);
            return 1;
        }
    };

    match store.export(app).await {
        Ok(document) => match serde_json::to_string_pretty(&document) {
            Ok(text) => {
                println!("{}", text);
                0
            }
            Err(error) => {
                eprintln!("cannot format the configuration: {}", error);
                1
            }
        },
        Err(ConfigError::NotFound) => {
            eprintln!("the app '{}' is not configured", app);
            1
        }
        Err(error) => {
            eprintln!("{}", error);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin::session;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;

    async fn memory_pool() -> SqlitePool {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .expect("the options must parse")
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("the in-memory database must open");
        crate::db::migrate(&pool)
            .await
            .expect("the migration must run");
        pool
    }

    #[tokio::test]
    async fn create_owner_inserts_owner_with_temp_flag() {
        let pool = memory_pool().await;
        let temporary = create_owner(&pool, " Owner@Example.com ")
            .await
            .expect("the owner must be created");

        assert_eq!(temporary.chars().count(), TEMP_LENGTH);
        assert!(!temporary
            .chars()
            .any(|character| "0O1lI".contains(character)));

        let (email, role, must_change): (String, String, i64) =
            sqlx::query_as("SELECT email, role, must_change_password FROM tool_users")
                .fetch_one(&pool)
                .await
                .expect("the account must exist");
        assert_eq!(email, "owner@example.com");
        assert_eq!(role, "owner");
        assert_eq!(must_change, 1);

        let hash: String = sqlx::query_scalar("SELECT password_hash FROM tool_users")
            .fetch_one(&pool)
            .await
            .expect("the hash must exist");
        assert!(password::verify_password(&hash, &temporary).await);

        let (actor, action, detail): (String, String, Option<String>) =
            sqlx::query_as("SELECT actor, action, detail FROM audit_log")
                .fetch_one(&pool)
                .await
                .expect("the audit row must exist");
        assert_eq!(actor, "cli");
        assert_eq!(action, "admin.create_owner");
        assert!(
            !detail.unwrap_or_default().contains(&temporary),
            "the audit row must not hold the temporary password"
        );
    }

    #[tokio::test]
    async fn create_owner_existing_email_fails_without_changes() {
        let pool = memory_pool().await;
        create_owner(&pool, "owner@example.com")
            .await
            .expect("the owner must be created");
        let before: String = sqlx::query_scalar("SELECT password_hash FROM tool_users")
            .fetch_one(&pool)
            .await
            .expect("the hash must exist");

        let error = create_owner(&pool, "OWNER@example.com")
            .await
            .expect_err("a duplicate must fail");
        assert!(error.contains("already exists"), "error: {}", error);

        let after: String = sqlx::query_scalar("SELECT password_hash FROM tool_users")
            .fetch_one(&pool)
            .await
            .expect("the hash must exist");
        assert_eq!(before, after, "the existing password must not change");

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tool_users")
            .fetch_one(&pool)
            .await
            .expect("the count must exist");
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn reset_owner_password_kills_sessions() {
        let pool = memory_pool().await;
        let old = create_owner(&pool, "owner@example.com")
            .await
            .expect("the owner must be created");
        let user_id: i64 = sqlx::query_scalar("SELECT id FROM tool_users")
            .fetch_one(&pool)
            .await
            .expect("the id must exist");

        let cookie = session::CookieConfig::with_hours(true, 12);
        session::create_session(&pool, &cookie, user_id, None, None)
            .await
            .expect("the first session must be created");
        session::create_session(&pool, &cookie, user_id, None, None)
            .await
            .expect("the second session must be created");

        let new = reset_owner_password(&pool, "owner@example.com")
            .await
            .expect("the reset must work");
        assert_ne!(new, old);
        assert_eq!(new.chars().count(), TEMP_LENGTH);

        let sessions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tool_sessions")
            .fetch_one(&pool)
            .await
            .expect("the count must exist");
        assert_eq!(sessions, 0, "every session must be deleted");

        let (hash, must_change): (String, i64) =
            sqlx::query_as("SELECT password_hash, must_change_password FROM tool_users")
                .fetch_one(&pool)
                .await
                .expect("the account must exist");
        assert_eq!(must_change, 1);
        assert!(password::verify_password(&hash, &new).await);
        assert!(!password::verify_password(&hash, &old).await);
    }

    #[tokio::test]
    async fn reset_owner_password_rejects_manager() {
        let pool = memory_pool().await;
        let hash = password::hash_password("manager password 2026")
            .await
            .expect("hashing must work");
        sqlx::query(
            "INSERT INTO tool_users (email, password_hash, role, must_change_password) \
             VALUES ('manager@example.com', ?, 'manager', 0)",
        )
        .bind(&hash)
        .execute(&pool)
        .await
        .expect("the manager must be inserted");

        let error = reset_owner_password(&pool, "manager@example.com")
            .await
            .expect_err("a manager must be rejected");
        assert!(error.contains("not an owner"), "error: {}", error);

        let after: String =
            sqlx::query_scalar("SELECT password_hash FROM tool_users WHERE email = ?")
                .bind("manager@example.com")
                .fetch_one(&pool)
                .await
                .expect("the hash must exist");
        assert_eq!(hash, after, "the manager password must not change");

        let sessions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tool_sessions")
            .fetch_one(&pool)
            .await
            .expect("the count must exist");
        assert_eq!(sessions, 0);
    }
}

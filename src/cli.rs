//! أوامر سطر الأوامر (T1): `import` و`export`.
//!
//! بلا مكتبة جديدة: `std::env::args` فقط. يعملان على `DATABASE_URL` كالمعتاد،
//! ولا يحتاجان `credentials/` ولا `CORS_ALLOWED_ORIGINS`.

use crate::config::store::{ConfigError, ConfigStore};
use crate::db;
/// رسالة الاستخدام.
pub fn usage() -> String {
    [
        "Usage:",
        "  fcm_microservice import <app> <file.json>   import a configuration document",
        "  fcm_microservice export <app>                 print the configuration as JSON",
    ]
    .join("\n")
}

/// ينفّذ أمر سطر الأوامر ويعيد رمز الخروج.
///
/// يُستدعى فقط عندما هناك وسيط واحد على الأقل؛ الوسيط غير المعروف ⇒ كود 2.
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
        _ => {
            eprintln!("unknown command '{}'\n{}", command, usage());
            2
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

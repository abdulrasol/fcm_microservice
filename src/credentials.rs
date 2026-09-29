use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use yup_oauth2::{authenticator::Authenticator, ServiceAccountAuthenticator};
use hyper::client::HttpConnector;
use hyper_rustls::HttpsConnector;

#[derive(Deserialize)]
struct ServiceAccount {
    project_id: String,
}

pub type AuthMap = Arc<RwLock<HashMap<String, Authenticator<HttpsConnector<HttpConnector>>>>>;
pub type ProjectIdMap = Arc<RwLock<HashMap<String, String>>>;

pub async fn load_credential(
    path: &Path,
    auth_map: AuthMap,
    project_map: ProjectIdMap,
) {
    let file_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();

    if file_name.is_empty() {
        return;
    }

    // Read the project_id from the JSON file
    let content = match tokio::fs::read_to_string(path).await {
        Ok(c) => c,
        Err(_) => return,
    };
    let sa: ServiceAccount = match serde_json::from_str(&content) {
        Ok(s) => s,
        Err(_) => return,
    };

    // Create Authenticator
    match yup_oauth2::read_service_account_key(path).await {
        Ok(secret) => {
            match ServiceAccountAuthenticator::builder(secret).build().await {
                Ok(auth) => {
                    tracing::info!("Loaded credentials for app: {}", file_name);
                    auth_map.write().await.insert(file_name.clone(), auth);
                    project_map.write().await.insert(file_name, sa.project_id);
                }
                Err(e) => tracing::error!("Failed to build auth for {}: {}", file_name, e),
            }
        }
        Err(e) => tracing::error!("Failed to read SA key for {}: {}", file_name, e),
    }
}

pub async fn scan_credentials_dir(
    dir: &Path,
    auth_map: AuthMap,
    project_map: ProjectIdMap,
) {
    if !dir.exists() {
        let _ = tokio::fs::create_dir_all(dir).await;
    }

    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(e) => e,
        Err(_) => return,
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            load_credential(&path, auth_map.clone(), project_map.clone()).await;
        }
    }
}

pub fn watch_credentials_dir(
    dir: PathBuf,
    auth_map: AuthMap,
    project_map: ProjectIdMap,
) {
    tokio::task::spawn_blocking(move || {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = match RecommendedWatcher::new(tx, Config::default()) {
            Ok(w) => w,
            Err(e) => {
                tracing::error!("Failed to initialize watcher: {}", e);
                return;
            }
        };

        if let Err(e) = watcher.watch(&dir, RecursiveMode::NonRecursive) {
            tracing::error!("Failed to watch {}: {}", dir.display(), e);
            return;
        }

        tracing::info!("Watching {} for new credentials", dir.display());

        for res in rx {
            match res {
                Ok(Event { kind: _, paths, .. }) => {
                    for path in paths {
                        if path.extension().and_then(|e| e.to_str()) == Some("json") {
                            // Run the async load_credential in a new tokio task
                            let am = auth_map.clone();
                            let pm = project_map.clone();
                            let p = path.clone();
                            tokio::spawn(async move {
                                // Add a small delay to ensure the file is fully written before reading
                                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                                load_credential(&p, am, pm).await;
                            });
                        }
                    }
                }
                Err(e) => tracing::error!("Watch error: {}", e),
            }
        }
    });
}

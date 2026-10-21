//! Picks the data source for an app from its stored configuration.
//!
//! Sources are built lazily and cached per `(app, config version)`, so a
//! configuration change rebuilds the source and the send routes do not
//! reconstruct it on every request. The routes are wired to this registry in
//! T5.

use super::firestore::{AuthMapTokenProvider, FirestoreDataSource};
use super::http::{HttpDataSource, HttpSourceConfig};
use super::{DataError, DataSource, Doc};
use crate::config::store::ConfigStore;
use crate::config::DataSource as SourceConfig;
use crate::credentials::{AuthMap, ProjectIdMap};
use crate::identity::jwt::EnvSource;
use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};

pub struct DataSourceRegistry {
    config: Arc<ConfigStore>,
    project_map: ProjectIdMap,
    auth_map: AuthMap,
    client: reqwest::Client,
    env: Arc<dyn EnvSource>,
    sources: Mutex<HashMap<(String, i64), Arc<dyn DataSource>>>,
    warned: Mutex<HashSet<String>>,
}

impl DataSourceRegistry {
    pub fn new(
        config: Arc<ConfigStore>,
        project_map: ProjectIdMap,
        auth_map: AuthMap,
        client: reqwest::Client,
        env: Arc<dyn EnvSource>,
    ) -> Self {
        Self {
            config,
            project_map,
            auth_map,
            client,
            env,
            sources: Mutex::new(HashMap::new()),
            warned: Mutex::new(HashSet::new()),
        }
    }

    /// Returns the source for `app`, building and caching it when needed.
    pub async fn source_for(&self, app: &str) -> Result<Arc<dyn DataSource>, DataError> {
        let snapshot = self.config.get(app);
        // Version 0 marks "no configuration"; a later configuration has a real
        // version, so its source is built separately.
        let version = snapshot
            .as_ref()
            .map(|snapshot| snapshot.version)
            .unwrap_or(0);

        if let Some(source) = lock(&self.sources)
            .get(&(app.to_string(), version))
            .cloned()
        {
            return Ok(source);
        }

        let source: Arc<dyn DataSource> = match snapshot {
            Some(snapshot) => match &snapshot.config.data_source {
                SourceConfig::Firestore => Arc::new(self.firestore(app).await?),
                SourceConfig::Http {
                    base_url,
                    secret_env,
                    timeout_ms,
                } => Arc::new(HttpDataSource::new(
                    self.client.clone(),
                    HttpSourceConfig {
                        base_url: base_url.clone(),
                        secret_env: secret_env.clone(),
                        timeout_ms: *timeout_ms,
                    },
                    self.env.clone(),
                )),
                SourceConfig::None => Arc::new(NoDataSource),
            },
            None => {
                let source = self
                    .firestore(app)
                    .await
                    .map_err(|_| DataError::Unavailable("unknown app".to_string()))?;
                self.warn_once(app);
                Arc::new(source)
            }
        };

        let mut sources = lock(&self.sources);
        // Drop older versions of this app so the cache cannot grow forever.
        sources.retain(|(cached_app, _), _| cached_app != app);
        sources.insert((app.to_string(), version), source.clone());
        Ok(source)
    }

    async fn firestore(&self, app: &str) -> Result<FirestoreDataSource, DataError> {
        let project_id = self
            .project_id(app)
            .await
            .ok_or_else(|| DataError::Unavailable("app has no loaded credentials".to_string()))?;
        Ok(FirestoreDataSource::new(
            self.client.clone(),
            Arc::new(AuthMapTokenProvider::new(self.auth_map.clone(), app)),
            project_id,
        ))
    }

    async fn project_id(&self, app: &str) -> Option<String> {
        let map = self.project_map.read().await;
        map.get(app)
            .map(|project| project.trim().to_string())
            .filter(|project| !project.is_empty())
    }

    fn warn_once(&self, app: &str) {
        let mut warned = lock(&self.warned);
        if warned.insert(app.to_string()) {
            tracing::warn!(
                app = %app,
                "no configuration, falling back to the default Firestore data source"
            );
        }
    }
}

/// Defense in depth for `data_source: none`; the validator already forbids the
/// configuration that would use it.
struct NoDataSource;

#[async_trait]
impl DataSource for NoDataSource {
    async fn get_doc(&self, _path: &str) -> Result<Option<Doc>, DataError> {
        Err(DataError::Unavailable(
            "this app has no data source".to_string(),
        ))
    }

    async fn members(
        &self,
        _collection: &str,
        _field: &str,
        _value: &str,
        _limit: usize,
    ) -> Result<Vec<String>, DataError> {
        Err(DataError::Unavailable(
            "this app has no data source".to_string(),
        ))
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datasource::test_support::{json as json_response, TestServer};
    use serde_json::{json, Value};
    use sqlx::sqlite::SqlitePoolOptions;
    use sqlx::SqlitePool;
    use tokio::sync::RwLock;

    const SECRET_ENV_A: &str = "CLIENT_RESOLVER_SECRET_A";
    const SECRET_ENV_B: &str = "CLIENT_RESOLVER_SECRET_B";

    struct StaticEnv {
        values: HashMap<String, String>,
    }

    impl EnvSource for StaticEnv {
        fn get(&self, name: &str) -> Option<String> {
            self.values.get(name).cloned()
        }
    }

    async fn memory_store() -> (Arc<ConfigStore>, SqlitePool) {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("the in-memory database must open");
        crate::db::migrate(&pool)
            .await
            .expect("the migration must run");
        let store = ConfigStore::new(pool.clone())
            .await
            .expect("the store must open");
        (Arc::new(store), pool)
    }

    fn project_map(entries: &[(&str, &str)]) -> ProjectIdMap {
        let map: HashMap<String, String> = entries
            .iter()
            .map(|(app, project)| (app.to_string(), project.to_string()))
            .collect();
        Arc::new(RwLock::new(map))
    }

    fn client() -> reqwest::Client {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("the data source client must build")
    }

    fn env(entries: &[(&str, &str)]) -> Arc<dyn EnvSource> {
        Arc::new(StaticEnv {
            values: entries
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
        })
    }

    fn registry(
        config: Arc<ConfigStore>,
        projects: ProjectIdMap,
        env: Arc<dyn EnvSource>,
    ) -> DataSourceRegistry {
        DataSourceRegistry::new(
            config,
            projects,
            Arc::new(RwLock::new(HashMap::new())),
            client(),
            env,
        )
    }

    fn http_value(base_url: &str, secret_env: &str) -> Value {
        json!({
            "type": "http",
            "base_url": base_url,
            "secret_env": secret_env,
            "timeout_ms": 1000
        })
    }

    async fn save_data_source(store: &ConfigStore, app: &str, value: Value) {
        let version = store.get(app).expect("the app must be cached").version;
        store
            .save_section(app, "data_source", value, "tester", version)
            .await
            .expect("the data source section must be saved");
    }

    async fn create_app(store: &ConfigStore, app: &str) {
        store.create_app(app, "tester").await.expect("create app");
    }

    #[tokio::test]
    async fn registry_builds_source_per_config() {
        let (store, _pool) = memory_store().await;
        create_app(&store, "watt").await;
        save_data_source(
            &store,
            "watt",
            http_value("http://127.0.0.1:9/internal/fcm", SECRET_ENV_A),
        )
        .await;
        let registry = registry(store, project_map(&[]), env(&[]));

        let source = registry
            .source_for("watt")
            .await
            .expect("the app has a configuration");

        match source.get_doc("users/u1").await {
            Err(DataError::Unavailable(message)) => {
                assert_eq!(message, "data source secret is not configured");
            }
            other => panic!("expected the HTTP source, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn registry_rebuilds_when_config_version_changes() {
        let (store, _pool) = memory_store().await;
        create_app(&store, "watt").await;
        let server = TestServer::start(vec![json_response(200, "OK", r#"{"role":"admin"}"#)]).await;
        save_data_source(
            &store,
            "watt",
            http_value(&format!("{}/internal/fcm", server.base_url), SECRET_ENV_A),
        )
        .await;
        let registry = registry(
            store.clone(),
            project_map(&[]),
            env(&[(SECRET_ENV_A, &"a".repeat(32))]),
        );

        let first = registry.source_for("watt").await.expect("first source");
        let document = first
            .get_doc("users/u1")
            .await
            .expect("the local resolver must answer")
            .expect("the document exists");
        assert_eq!(document, json!({ "role": "admin" }));

        save_data_source(
            &store,
            "watt",
            http_value("http://127.0.0.1:9/internal/fcm", SECRET_ENV_B),
        )
        .await;

        let second = registry.source_for("watt").await.expect("second source");
        assert!(
            !Arc::ptr_eq(&first, &second),
            "a new configuration version must rebuild the source"
        );
        match second.get_doc("users/u1").await {
            Err(DataError::Unavailable(message)) => {
                assert_eq!(message, "data source secret is not configured");
            }
            other => panic!("expected the rebuilt source to use the new secret, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn registry_caches_source_between_calls() {
        let (store, _pool) = memory_store().await;
        create_app(&store, "watt").await;
        save_data_source(
            &store,
            "watt",
            http_value("http://127.0.0.1:9/internal/fcm", SECRET_ENV_A),
        )
        .await;
        let registry = registry(store, project_map(&[]), env(&[]));

        let first = registry.source_for("watt").await.expect("first call");
        let second = registry.source_for("watt").await.expect("second call");
        assert!(
            Arc::ptr_eq(&first, &second),
            "the same config version must reuse the source"
        );
    }

    #[tokio::test]
    async fn registry_defaults_to_firestore_without_config() {
        let (store, _pool) = memory_store().await;
        let registry = registry(store, project_map(&[("watt", "test-project")]), env(&[]));

        let first = registry.source_for("watt").await.expect("fallback source");
        let second = registry.source_for("watt").await.expect("cached fallback");
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[tokio::test]
    async fn registry_firestore_without_credentials_is_unavailable() {
        let (store, _pool) = memory_store().await;
        create_app(&store, "watt").await;
        let registry = registry(store, project_map(&[]), env(&[]));

        match registry.source_for("watt").await {
            Err(DataError::Unavailable(message)) => {
                assert_eq!(message, "app has no loaded credentials");
            }
            Err(other) => panic!("expected Unavailable, got {other:?}"),
            Ok(_) => panic!("expected Unavailable, got a source"),
        }
    }

    #[tokio::test]
    async fn registry_none_source_always_unavailable() {
        let (store, _pool) = memory_store().await;
        create_app(&store, "watt").await;
        store
            .save_section(
                "watt",
                "identity",
                json!({
                    "provider": { "type": "firebase" },
                    "role": { "source": "claim", "claim": "role" },
                    "admin_roles": ["admin"]
                }),
                "tester",
                1,
            )
            .await
            .expect("save the identity section");
        save_data_source(&store, "watt", json!({ "type": "none" })).await;
        let registry = registry(store, project_map(&[("watt", "test-project")]), env(&[]));

        let source = registry
            .source_for("watt")
            .await
            .expect("the none source is built");

        match source.get_doc("users/u1").await {
            Err(DataError::Unavailable(message)) => {
                assert_eq!(message, "this app has no data source");
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
        match source.members("users", "role", "admin", 10).await {
            Err(DataError::Unavailable(message)) => {
                assert_eq!(message, "this app has no data source");
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn registry_unknown_app_is_unavailable() {
        let (store, _pool) = memory_store().await;
        let registry = registry(store, project_map(&[]), env(&[]));

        match registry.source_for("watt").await {
            Err(DataError::Unavailable(message)) => assert_eq!(message, "unknown app"),
            Err(other) => panic!("expected Unavailable, got {other:?}"),
            Ok(_) => panic!("expected Unavailable, got a source"),
        }
    }
}

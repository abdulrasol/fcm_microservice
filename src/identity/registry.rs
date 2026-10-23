//! Picks the identity provider for an app from its stored configuration.
//!
//! Providers are built lazily and cached per `(app, config version)`, so a
//! configuration change rebuilds the provider and token verification does not
//! reconstruct it on every request. The send routes are wired to this registry
//! in T5/T8.

use super::firebase::{FirebaseProvider, FirebaseVerifier};
use super::jwks::{
    GoogleJwkProvider, HttpJwkProvider, JwkProvider, JwksCache, UnconfiguredJwkProvider,
};
use super::jwt::{EnvSource, JwtProvider, JwtProviderConfig};
use super::{IdentityError, IdentityProvider};
use crate::config::store::ConfigStore;
use crate::config::IdentityProvider as ProviderConfig;
use crate::credentials::ProjectIdMap;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};

pub struct IdentityRegistry {
    config: Arc<ConfigStore>,
    project_map: ProjectIdMap,
    client: reqwest::Client,
    env: Arc<dyn EnvSource>,
    /// One verifier for every Firebase app: they all use the same Google JWKS.
    firebase: Arc<FirebaseVerifier>,
    providers: Mutex<HashMap<(String, i64), Arc<dyn IdentityProvider>>>,
    warned: Mutex<HashSet<String>>,
}

impl IdentityRegistry {
    pub fn new(
        config: Arc<ConfigStore>,
        project_map: ProjectIdMap,
        client: reqwest::Client,
        env: Arc<dyn EnvSource>,
    ) -> Self {
        let firebase = Arc::new(FirebaseVerifier::with_cache(
            Arc::new(GoogleJwkProvider::new(client.clone())),
            Arc::new(Mutex::new(JwksCache::new())),
        ));
        Self::with_firebase(config, project_map, client, env, firebase)
    }

    pub fn with_firebase(
        config: Arc<ConfigStore>,
        project_map: ProjectIdMap,
        client: reqwest::Client,
        env: Arc<dyn EnvSource>,
        firebase: Arc<FirebaseVerifier>,
    ) -> Self {
        Self {
            config,
            project_map,
            client,
            env,
            firebase,
            providers: Mutex::new(HashMap::new()),
            warned: Mutex::new(HashSet::new()),
        }
    }

    /// Warms the shared Firebase JWKS cache if any app is configured with
    /// the Firebase provider or defaults to it.
    pub async fn warmup_firebase_if_needed(&self) {
        let has_firebase_in_config = self.config.app_names().iter().any(|app| {
            self.config
                .get(app)
                .map(|s| matches!(s.config.identity.provider, ProviderConfig::Firebase))
                .unwrap_or(false)
        });
        let has_firebase_in_creds = {
            let map = self.project_map.read().await;
            !map.is_empty()
        };
        if has_firebase_in_config || has_firebase_in_creds {
            if let Err(error) = self.firebase.warmup().await {
                tracing::warn!("Failed to warm up Firebase JWKS: {:?}", error);
            } else {
                tracing::info!("Firebase JWKS warmed up successfully");
            }
        }
    }

    /// Returns the provider for `app`, building and caching it when needed.
    pub async fn provider_for(
        &self,
        app: &str,
    ) -> Result<Arc<dyn IdentityProvider>, IdentityError> {
        let snapshot = self.config.get(app);
        // Version 0 marks "no configuration"; a later configuration has a real
        // version, so its provider is built separately.
        let version = snapshot
            .as_ref()
            .map(|snapshot| snapshot.version)
            .unwrap_or(0);

        if let Some(provider) = lock(&self.providers)
            .get(&(app.to_string(), version))
            .cloned()
        {
            return Ok(provider);
        }

        let provider: Arc<dyn IdentityProvider> = match snapshot {
            Some(snapshot) => match &snapshot.config.identity.provider {
                ProviderConfig::Firebase => {
                    let project_id = self.project_id(app).await.ok_or_else(|| {
                        IdentityError::Unavailable("app has no loaded credentials".to_string())
                    })?;
                    Arc::new(FirebaseProvider::new(project_id, self.firebase.clone()))
                }
                ProviderConfig::Jwt {
                    algorithms,
                    jwks_url,
                    hs256_secret_env,
                    issuer,
                    audience,
                    uid_claim,
                    name_claim,
                    max_age_seconds,
                } => {
                    let config = JwtProviderConfig {
                        algorithms: algorithms.clone(),
                        jwks_url: jwks_url.clone(),
                        hs256_secret_env: hs256_secret_env.clone(),
                        issuer: issuer.clone(),
                        audience: audience.clone(),
                        uid_claim: uid_claim.clone(),
                        name_claim: name_claim.clone(),
                        max_age_seconds: *max_age_seconds,
                    };
                    let jwks: Arc<dyn JwkProvider> = match jwks_url {
                        Some(url) => {
                            Arc::new(HttpJwkProvider::new(self.client.clone(), url.clone()))
                        }
                        None => Arc::new(UnconfiguredJwkProvider),
                    };
                    Arc::new(JwtProvider::new(&config, self.env.clone(), jwks))
                }
            },
            None => {
                let project_id = self
                    .project_id(app)
                    .await
                    .ok_or(IdentityError::Invalid("unknown app"))?;
                self.warn_once(app);
                Arc::new(FirebaseProvider::new(project_id, self.firebase.clone()))
            }
        };

        let mut providers = lock(&self.providers);
        // Drop older versions of this app so the cache cannot grow forever.
        providers.retain(|(cached_app, _), _| cached_app != app);
        providers.insert((app.to_string(), version), provider.clone());
        Ok(provider)
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
                "no configuration, falling back to the default Firebase provider"
            );
        }
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
    use crate::config::store::ConfigStore;
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
    use serde_json::{json, Value};
    use sqlx::sqlite::SqlitePoolOptions;
    use sqlx::SqlitePool;
    use tokio::sync::RwLock;

    const ISSUER: &str = "https://auth.example.com/";
    const AUDIENCE: &str = "client-mobile-app";
    const SECRET_ENV: &str = "CLIENT_JWT_SECRET";
    const HS256_SECRET: &str = "a-very-long-test-secret-value-1234567890";

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
            .expect("the identity client must build")
    }

    fn registry(config: Arc<ConfigStore>, projects: ProjectIdMap) -> IdentityRegistry {
        IdentityRegistry::new(
            config,
            projects,
            client(),
            Arc::new(StaticEnv {
                values: HashMap::from([(SECRET_ENV.to_string(), HS256_SECRET.to_string())]),
            }),
        )
    }

    fn identity_value(audience: &str) -> Value {
        json!({
            "provider": {
                "type": "jwt",
                "algorithms": ["HS256"],
                "hs256_secret_env": SECRET_ENV,
                "issuer": ISSUER,
                "audience": audience,
                "uid_claim": "sub",
                "max_age_seconds": 3600
            },
            "role": { "source": "claim", "claim": "role" },
            "admin_roles": ["admin"]
        })
    }

    async fn jwt_app(store: &ConfigStore, app: &str, audience: &str) {
        store.create_app(app, "tester").await.expect("create app");
        store
            .save_section(app, "identity", identity_value(audience), "tester", 1)
            .await
            .expect("save the identity section");
    }

    fn hs256_token(audience: &str) -> String {
        let now = jsonwebtoken::get_current_timestamp();
        let claims = json!({
            "sub": "uid-1",
            "iss": ISSUER,
            "aud": audience,
            "exp": now + 3600,
            "iat": now - 10,
        });
        encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(HS256_SECRET.as_bytes()),
        )
        .expect("encode token")
    }

    #[tokio::test]
    async fn registry_builds_provider_per_config() {
        let (store, _pool) = memory_store().await;
        jwt_app(&store, "watt", AUDIENCE).await;
        let registry = registry(store, project_map(&[]));

        let provider = registry
            .provider_for("watt")
            .await
            .expect("the app has a configuration");
        let identity = provider
            .verify(&hs256_token(AUDIENCE))
            .await
            .expect("the provider must use the stored HS256 settings");
        assert_eq!(identity.uid, "uid-1");
    }

    #[tokio::test]
    async fn registry_rebuilds_when_config_version_changes() {
        let (store, _pool) = memory_store().await;
        jwt_app(&store, "watt", AUDIENCE).await;
        let registry = registry(store.clone(), project_map(&[]));

        let first = registry.provider_for("watt").await.expect("first provider");
        let version = store.get("watt").expect("cached app").version;
        store
            .save_section(
                "watt",
                "identity",
                identity_value("second-app"),
                "tester",
                version,
            )
            .await
            .expect("save the new identity section");

        let second = registry
            .provider_for("watt")
            .await
            .expect("second provider");
        assert!(
            !Arc::ptr_eq(&first, &second),
            "a new configuration version must rebuild the provider"
        );
        second
            .verify(&hs256_token("second-app"))
            .await
            .expect("the rebuilt provider must use the new audience");
    }

    #[tokio::test]
    async fn registry_caches_provider_between_calls() {
        let (store, _pool) = memory_store().await;
        jwt_app(&store, "watt", AUDIENCE).await;
        let registry = registry(store, project_map(&[]));

        let first = registry.provider_for("watt").await.expect("first call");
        let second = registry.provider_for("watt").await.expect("second call");
        assert!(
            Arc::ptr_eq(&first, &second),
            "the same config version must reuse the provider"
        );
    }

    #[tokio::test]
    async fn registry_defaults_to_firebase_without_config() {
        let (store, _pool) = memory_store().await;
        let registry = registry(store, project_map(&[("watt", "test-project")]));

        let first = registry
            .provider_for("watt")
            .await
            .expect("fallback provider");
        let second = registry
            .provider_for("watt")
            .await
            .expect("cached fallback");
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[tokio::test]
    async fn registry_firebase_without_credentials_is_unavailable() {
        let (store, _pool) = memory_store().await;
        store
            .create_app("watt", "tester")
            .await
            .expect("create app");
        let registry = registry(store, project_map(&[]));

        match registry.provider_for("watt").await {
            Err(IdentityError::Unavailable(message)) => {
                assert_eq!(message, "app has no loaded credentials");
            }
            Err(other) => panic!("expected Unavailable, got {other:?}"),
            Ok(_) => panic!("expected Unavailable, got a provider"),
        }
    }

    #[tokio::test]
    async fn registry_unknown_app_is_invalid() {
        let (store, _pool) = memory_store().await;
        let registry = registry(store, project_map(&[]));

        match registry.provider_for("watt").await {
            Err(IdentityError::Invalid(message)) => assert_eq!(message, "unknown app"),
            Err(other) => panic!("expected Invalid, got {other:?}"),
            Ok(_) => panic!("expected Invalid, got a provider"),
        }
    }
}

//! Role resolution and web subscription topic patterns.
//!
//! A role comes either from a claim in the verified token or from a document in
//! the app's data source. Data-source roles are cached per
//! `(app, config version, uid)` so a configuration change invalidates them.

use crate::config::{AppConfig, RoleSource};
use crate::datasource::{DataError, DataSource, Doc};
use crate::identity::{claim_at, Identity};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// The part of the app identity configuration the role rules need.
///
/// The whole `config::Identity` is used so the legacy policy below is exactly
/// the default configuration, provider included.
pub use crate::config::Identity as IdentityPolicy;

/// The role source and admin roles of an app that has no stored configuration.
///
/// This must stay byte-for-byte equivalent to the historical behavior:
/// Firebase identities and `users/{uid}.role == "admin"`.
pub fn legacy_policy() -> IdentityPolicy {
    AppConfig::default_firebase().identity
}

/// Why a role could not be resolved right now. Never cached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoleError {
    /// The data source failed or is misconfigured; maps to `503`.
    Unavailable(String),
}

/// Largest number of cached role entries. The cache is cleared when it is full
/// so it cannot grow without bound.
const MAX_CACHE_ENTRIES: usize = 4096;

type CacheKey = (String, i64, String);
type CacheEntry = (Option<String>, Instant);

pub struct RoleResolver {
    cache: Mutex<HashMap<CacheKey, CacheEntry>>,
}

impl Default for RoleResolver {
    fn default() -> Self {
        Self {
            cache: Mutex::new(HashMap::new()),
        }
    }
}

impl RoleResolver {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolves the role of an already verified identity.
    ///
    /// Errors are never cached. Claim roles are read from the token on every
    /// call: two tokens for the same uid may carry different claims.
    pub async fn role(
        &self,
        app: &str,
        version: i64,
        policy: &IdentityPolicy,
        identity: &Identity,
        source: &dyn DataSource,
    ) -> Result<Option<String>, RoleError> {
        let (doc, field, cache_seconds) = match &policy.role {
            RoleSource::Claim { claim } => return Ok(claim_role(&identity.claims, claim)),
            RoleSource::DataSource {
                doc,
                field,
                cache_seconds,
            } => (doc, field, *cache_seconds),
        };

        let key = (app.to_string(), version, identity.uid.clone());
        let ttl = Duration::from_secs(cache_seconds);
        if !ttl.is_zero() {
            if let Some((role, expires_at)) = lock(&self.cache).get(&key) {
                if Instant::now() < *expires_at {
                    return Ok(role.clone());
                }
            }
        }

        let path = doc.replace("{uid}", &identity.uid);
        let document = source.get_doc(&path).await.map_err(|error| {
            tracing::warn!(
                app = %app,
                error = ?error,
                "the role lookup from the data source failed"
            );
            RoleError::Unavailable("role lookup failed".to_string())
        })?;
        let role = document
            .as_ref()
            .and_then(|document| claim_at(document, field))
            .and_then(non_empty_string);

        if !ttl.is_zero() {
            let now = Instant::now();
            let mut cache = lock(&self.cache);
            cache.retain(|_, (_, expires_at)| now < *expires_at);
            if cache.len() >= MAX_CACHE_ENTRIES {
                cache.clear();
            }
            cache.insert(key, (role.clone(), now + ttl));
        }

        Ok(role)
    }
}

/// Source placeholder for claim roles: the resolver returns before touching it.
pub struct NoSource;

#[async_trait]
impl DataSource for NoSource {
    async fn get_doc(&self, _path: &str) -> Result<Option<Doc>, DataError> {
        Err(DataError::Unavailable(
            "a claim role does not need a data source".to_string(),
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
            "a claim role does not need a data source".to_string(),
        ))
    }
}

/// Exact, case-sensitive membership: an empty list means "never an admin".
pub fn is_admin_role(role: Option<&str>, admin_roles: &[String]) -> bool {
    match role {
        Some(role) => admin_roles.iter().any(|candidate| candidate == role),
        None => false,
    }
}

/// Matches a subscription topic against a configured pattern.
///
/// A pattern without a star is an exact match. A star is only meaningful at the
/// end: the text after the prefix must be non-empty and made of
/// `[A-Za-z0-9_-]`. A lone star or an inner star never matches.
pub fn topic_matches(pattern: &str, topic: &str) -> bool {
    if !pattern.contains('*') {
        return pattern == topic;
    }
    let Some(prefix) = pattern.strip_suffix('*') else {
        return false;
    };
    if prefix.is_empty() || prefix.contains('*') {
        return false;
    }
    topic.strip_prefix(prefix).is_some_and(|suffix| {
        !suffix.is_empty()
            && suffix.chars().all(|character| {
                character.is_ascii_alphanumeric() || character == '_' || character == '-'
            })
    })
}

/// Only a non-empty string is a role. Arrays, numbers and null are not
/// supported as role values.
fn claim_role(claims: &Value, claim: &str) -> Option<String> {
    claim_at(claims, claim).and_then(non_empty_string)
}

fn non_empty_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{IdentityProvider, RoleSource};
    use async_trait::async_trait;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FakeSource {
        doc: Mutex<Result<Option<Value>, DataError>>,
        paths: Mutex<Vec<String>>,
        calls: AtomicUsize,
    }

    impl FakeSource {
        fn with_doc(doc: Value) -> Self {
            Self {
                doc: Mutex::new(Ok(Some(doc))),
                paths: Mutex::new(Vec::new()),
                calls: AtomicUsize::new(0),
            }
        }

        fn missing() -> Self {
            Self {
                doc: Mutex::new(Ok(None)),
                paths: Mutex::new(Vec::new()),
                calls: AtomicUsize::new(0),
            }
        }

        fn failing() -> Self {
            Self {
                doc: Mutex::new(Err(DataError::Unavailable(
                    "the backend is down".to_string(),
                ))),
                paths: Mutex::new(Vec::new()),
                calls: AtomicUsize::new(0),
            }
        }

        fn recover(&self, doc: Value) {
            *self.doc.lock().expect("the doc lock") = Ok(Some(doc));
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }

        fn paths(&self) -> Vec<String> {
            self.paths.lock().expect("the paths lock").clone()
        }
    }

    #[async_trait]
    impl DataSource for FakeSource {
        async fn get_doc(&self, path: &str) -> Result<Option<Doc>, DataError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.paths
                .lock()
                .expect("the paths lock")
                .push(path.to_string());
            self.doc.lock().expect("the doc lock").clone()
        }

        async fn members(
            &self,
            _collection: &str,
            _field: &str,
            _value: &str,
            _limit: usize,
        ) -> Result<Vec<String>, DataError> {
            Err(DataError::Unavailable("unused in role tests".to_string()))
        }
    }

    fn identity(uid: &str, claims: Value) -> Identity {
        Identity {
            uid: uid.to_string(),
            email: None,
            name: None,
            claims,
        }
    }

    fn policy(role: RoleSource, admin_roles: &[&str]) -> IdentityPolicy {
        IdentityPolicy {
            provider: IdentityProvider::Firebase,
            role,
            admin_roles: admin_roles.iter().map(|role| (*role).to_string()).collect(),
            role_members: None,
        }
    }

    fn claim_policy(claim: &str) -> IdentityPolicy {
        policy(
            RoleSource::Claim {
                claim: claim.to_string(),
            },
            &["admin"],
        )
    }

    fn source_policy(cache_seconds: u64) -> IdentityPolicy {
        policy(
            RoleSource::DataSource {
                doc: "users/{uid}".to_string(),
                field: "role".to_string(),
                cache_seconds,
            },
            &["admin"],
        )
    }

    #[tokio::test]
    async fn claim_role_reads_nested_claim() {
        let resolver = RoleResolver::new();
        let source = FakeSource::missing();
        let policy = claim_policy("user.role");
        let caller = identity("uid-1", json!({ "user": { "role": "admin" } }));

        let role = resolver
            .role("app", 1, &policy, &caller, &source)
            .await
            .expect("a claim role never fails");
        assert_eq!(role.as_deref(), Some("admin"));
        assert_eq!(source.calls(), 0);

        let top_level = identity("uid-1", json!({ "role": "manager" }));
        let role = resolver
            .role("app", 1, &claim_policy("role"), &top_level, &source)
            .await
            .expect("a claim role never fails");
        assert_eq!(role.as_deref(), Some("manager"));
    }

    #[tokio::test]
    async fn claim_role_non_string_is_none() {
        let resolver = RoleResolver::new();
        let source = FakeSource::missing();
        let policy = claim_policy("role");

        for claims in [
            json!({ "role": 5 }),
            json!({ "role": ["admin"] }),
            json!({ "role": { "name": "admin" } }),
            json!({ "role": "" }),
            json!({ "role": null }),
            json!({ "other": "admin" }),
        ] {
            let role = resolver
                .role("app", 1, &policy, &identity("uid-1", claims), &source)
                .await
                .expect("a claim role never fails");
            assert_eq!(role, None);
        }
    }

    #[tokio::test]
    async fn data_source_role_reads_field_with_uid_substitution() {
        let resolver = RoleResolver::new();
        let source = FakeSource::with_doc(json!({ "profile": { "role": "admin" } }));
        let policy = policy(
            RoleSource::DataSource {
                doc: "users/{uid}".to_string(),
                field: "profile.role".to_string(),
                cache_seconds: 60,
            },
            &["admin"],
        );

        let role = resolver
            .role("app", 1, &policy, &identity("uid-1", json!({})), &source)
            .await
            .expect("the role must be read");
        assert_eq!(role.as_deref(), Some("admin"));
        assert_eq!(source.paths(), vec!["users/uid-1".to_string()]);
    }

    #[tokio::test]
    async fn data_source_missing_doc_is_none() {
        let resolver = RoleResolver::new();
        let source = FakeSource::missing();

        let role = resolver
            .role(
                "app",
                1,
                &source_policy(60),
                &identity("uid-1", json!({})),
                &source,
            )
            .await
            .expect("a missing document is not an error");
        assert_eq!(role, None);
        assert_eq!(source.calls(), 1);
    }

    #[tokio::test]
    async fn data_source_error_is_unavailable_and_not_cached() {
        let resolver = RoleResolver::new();
        let source = FakeSource::failing();

        let error = resolver
            .role(
                "app",
                1,
                &source_policy(60),
                &identity("uid-1", json!({})),
                &source,
            )
            .await
            .expect_err("a failing source must fail closed");
        assert!(matches!(error, RoleError::Unavailable(_)));
        assert_eq!(source.calls(), 1);

        source.recover(json!({ "role": "admin" }));
        let role = resolver
            .role(
                "app",
                1,
                &source_policy(60),
                &identity("uid-1", json!({})),
                &source,
            )
            .await
            .expect("the error must not be cached");
        assert_eq!(role.as_deref(), Some("admin"));
        assert_eq!(source.calls(), 2);
    }

    #[tokio::test]
    async fn role_cache_hits_within_ttl() {
        let resolver = RoleResolver::new();
        let source = FakeSource::with_doc(json!({ "role": "admin" }));
        let policy = source_policy(60);

        for _ in 0..3 {
            let role = resolver
                .role("app", 1, &policy, &identity("uid-1", json!({})), &source)
                .await
                .expect("the role must resolve");
            assert_eq!(role.as_deref(), Some("admin"));
        }
        assert_eq!(source.calls(), 1);
    }

    #[tokio::test]
    async fn role_cache_expires_after_ttl() {
        let resolver = RoleResolver::new();
        let source = FakeSource::with_doc(json!({ "role": "admin" }));
        let policy = source_policy(1);

        resolver
            .role("app", 1, &policy, &identity("uid-1", json!({})), &source)
            .await
            .expect("the role must resolve");
        tokio::time::sleep(Duration::from_millis(1100)).await;
        resolver
            .role("app", 1, &policy, &identity("uid-1", json!({})), &source)
            .await
            .expect("the role must resolve again");
        assert_eq!(source.calls(), 2);
    }

    #[tokio::test]
    async fn role_cache_zero_seconds_disables_cache() {
        let resolver = RoleResolver::new();
        let source = FakeSource::with_doc(json!({ "role": "admin" }));
        let policy = source_policy(0);

        for _ in 0..2 {
            resolver
                .role("app", 1, &policy, &identity("uid-1", json!({})), &source)
                .await
                .expect("the role must resolve");
        }
        assert_eq!(source.calls(), 2);
        assert!(lock(&resolver.cache).is_empty());
    }

    #[tokio::test]
    async fn role_cache_invalidated_by_config_version() {
        let resolver = RoleResolver::new();
        let source = FakeSource::with_doc(json!({ "role": "admin" }));
        let policy = source_policy(60);
        let user = identity("uid-1", json!({}));

        resolver
            .role("app", 1, &policy, &user, &source)
            .await
            .expect("the role must resolve");
        resolver
            .role("app", 2, &policy, &user, &source)
            .await
            .expect("a new version must be looked up again");
        resolver
            .role("app", 1, &policy, &user, &source)
            .await
            .expect("the old version is still cached");
        assert_eq!(source.calls(), 2);
    }

    #[tokio::test]
    async fn role_cache_is_bounded() {
        let resolver = RoleResolver::new();
        let source = FakeSource::with_doc(json!({ "role": "admin" }));
        let policy = source_policy(3600);

        for index in 0..=MAX_CACHE_ENTRIES {
            resolver
                .role(
                    "app",
                    1,
                    &policy,
                    &identity(&format!("uid-{index}"), json!({})),
                    &source,
                )
                .await
                .expect("the role must resolve");
        }

        let size = lock(&resolver.cache).len();
        assert!(size <= MAX_CACHE_ENTRIES);
        assert_eq!(size, 1);
        assert_eq!(source.calls(), MAX_CACHE_ENTRIES + 1);
    }

    #[test]
    fn admin_role_match_is_exact_and_case_sensitive() {
        let admin_roles = vec!["admin".to_string(), "owner".to_string()];
        assert!(is_admin_role(Some("admin"), &admin_roles));
        assert!(is_admin_role(Some("owner"), &admin_roles));
        assert!(!is_admin_role(Some("Admin"), &admin_roles));
        assert!(!is_admin_role(Some(" administrator "), &admin_roles));
        assert!(!is_admin_role(Some("user"), &admin_roles));
        assert!(!is_admin_role(None, &admin_roles));
    }

    #[test]
    fn empty_admin_roles_never_admin() {
        assert!(!is_admin_role(Some("admin"), &[]));
        assert!(!is_admin_role(Some(""), &[]));
        assert!(!is_admin_role(None, &[]));
    }

    #[test]
    fn topic_pattern_exact_and_prefix() {
        assert!(topic_matches("users", "users"));
        assert!(!topic_matches("users", "user"));
        assert!(topic_matches("country_*", "country_us"));
        assert!(topic_matches("country_*", "country_IQ"));
        assert!(!topic_matches("cat_*-v2", "cat_x-v2"));
        assert!(topic_matches("lang_*", "lang_ar"));
        assert!(!topic_matches("users", "users_extra"));
    }

    #[test]
    fn topic_pattern_star_alone_or_middle_never_matches() {
        assert!(!topic_matches("*", "anything"));
        assert!(!topic_matches("a*b", "ab"));
        assert!(!topic_matches("a*b", "a*b"));
        assert!(!topic_matches("*users", "*users"));
        assert!(topic_matches("a*", "ab"));
    }

    #[test]
    fn topic_pattern_requires_non_empty_suffix() {
        assert!(!topic_matches("country_*", "country_"));
        assert!(!topic_matches("country_*", "country"));
        assert!(!topic_matches("country_*", "country_u/"));
        assert!(!topic_matches("country_*", "country_u s"));
        assert!(topic_matches("country_*", "country_u-1"));
    }
}

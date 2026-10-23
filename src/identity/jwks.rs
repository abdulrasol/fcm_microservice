//! JWKS fetching and caching shared by the identity providers.
//!
//! The cache follows `Cache-Control: max-age` (clamped between 60 seconds and
//! six hours). An unknown `kid` triggers at most one refresh per
//! [`MIN_JWK_REFRESH`]; when the cached keys have expired and the refresh is
//! still throttled the caller fails closed with an `Unavailable` error.

use async_trait::async_trait;
use jsonwebtoken::jwk::{JwkSet, KeyAlgorithm};
use jsonwebtoken::DecodingKey;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::sync::Mutex as AsyncMutex;

/// Google's public keys for Firebase Auth ID tokens.
pub const GOOGLE_JWK_URL: &str =
    "https://www.googleapis.com/service_accounts/v1/jwk/securetoken@system.gserviceaccount.com";

pub(crate) const MAX_KID_LENGTH: usize = 128;
pub(crate) const MIN_JWK_REFRESH: Duration = Duration::from_secs(60);
pub(crate) const DEFAULT_JWK_MAX_AGE: Duration = Duration::from_secs(60 * 60);
pub(crate) const MIN_JWK_MAX_AGE: Duration = Duration::from_secs(60);
pub(crate) const MAX_JWK_MAX_AGE: Duration = Duration::from_secs(6 * 60 * 60);

/// Upper bound for one JWKS response body.
const MAX_JWKS_BYTES: usize = 64 * 1024;

/// One downloaded key set and the lifetime the endpoint asked for.
pub struct FetchedJwks {
    pub keys: HashMap<String, DecodingKey>,
    pub max_age: Duration,
}

#[async_trait]
pub trait JwkProvider: Send + Sync {
    async fn fetch_keys(&self) -> Result<FetchedJwks, String>;
}

/// The cached key set plus its freshness bookkeeping.
pub struct JwksCache {
    pub keys: HashMap<String, DecodingKey>,
    pub expires_at: Instant,
    pub last_refresh: Instant,
    pub refresh_gate: Arc<AsyncMutex<()>>,
}

impl JwksCache {
    pub fn new() -> Self {
        Self {
            keys: HashMap::new(),
            expires_at: Instant::now(),
            last_refresh: Instant::now() - MIN_JWK_REFRESH,
            refresh_gate: Arc::new(AsyncMutex::new(())),
        }
    }
}

impl Default for JwksCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Why a key could not be selected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JwksError {
    /// The cached keys expired and the refresh throttle is still active.
    Expired,
    /// The endpoint could not be reached or returned unusable data.
    Fetch(String),
}

/// Returns the key for `kid`, refreshing the cache when allowed.
///
/// With `kid == None` the sole cached key is used; a set with more than one key
/// cannot be resolved without a `kid`.
pub async fn cached_key(
    cache: &Mutex<JwksCache>,
    provider: &dyn JwkProvider,
    kid: Option<&str>,
) -> Result<Option<DecodingKey>, JwksError> {
    let now = Instant::now();
    let (cached, expired, gate) = {
        let cache = lock(cache);
        (
            lookup(&cache, kid),
            now >= cache.expires_at,
            Arc::clone(&cache.refresh_gate),
        )
    };
    if !expired && cached.is_some() {
        return Ok(cached);
    }

    let _guard = gate.lock().await;

    // Double-check the cache under the single-flight gate in case a concurrent
    // fetch completed while this task was waiting.
    let now = Instant::now();
    let (cached, expired) = {
        let cache = lock(cache);
        (lookup(&cache, kid), now >= cache.expires_at)
    };
    if !expired && cached.is_some() {
        return Ok(cached);
    }

    let may_refresh = {
        let cache = lock(cache);
        now.duration_since(cache.last_refresh) >= MIN_JWK_REFRESH
    };
    if !may_refresh {
        return if expired {
            Err(JwksError::Expired)
        } else {
            Ok(None)
        };
    }

    {
        let mut cache = lock(cache);
        cache.last_refresh = now;
    }
    refresh(cache, provider).await?;

    let cache = lock(cache);
    Ok(lookup(&cache, kid))
}

/// Warms the cache by fetching keys if expired or empty, respecting the
/// single-flight gate and refresh throttle.
pub async fn warmup_cache(
    cache: &Mutex<JwksCache>,
    provider: &dyn JwkProvider,
) -> Result<(), JwksError> {
    let now = Instant::now();
    let (is_fresh, gate) = {
        let cache = lock(cache);
        (
            now < cache.expires_at && !cache.keys.is_empty(),
            Arc::clone(&cache.refresh_gate),
        )
    };
    if is_fresh {
        return Ok(());
    }

    let _guard = gate.lock().await;

    let now = Instant::now();
    let is_fresh = {
        let cache = lock(cache);
        now < cache.expires_at && !cache.keys.is_empty()
    };
    if is_fresh {
        return Ok(());
    }

    let may_refresh = {
        let cache = lock(cache);
        now.duration_since(cache.last_refresh) >= MIN_JWK_REFRESH
    };
    if !may_refresh {
        return Err(JwksError::Expired);
    }

    {
        let mut cache = lock(cache);
        cache.last_refresh = now;
    }
    refresh(cache, provider).await
}

/// Downloads a new key set and stores it. The fetch happens without holding the
/// lock, so concurrent verifications are not blocked on the network.
async fn refresh(cache: &Mutex<JwksCache>, provider: &dyn JwkProvider) -> Result<(), JwksError> {
    let fetched = provider.fetch_keys().await.map_err(JwksError::Fetch)?;
    let max_age = fetched.max_age.clamp(MIN_JWK_MAX_AGE, MAX_JWK_MAX_AGE);
    let mut cache = lock(cache);
    cache.keys = fetched.keys;
    cache.expires_at = Instant::now() + max_age;
    Ok(())
}

fn lookup(cache: &JwksCache, kid: Option<&str>) -> Option<DecodingKey> {
    match kid {
        Some(kid) => cache.keys.get(kid).cloned(),
        None if cache.keys.len() == 1 => cache.keys.values().next().cloned(),
        None => None,
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A JWKS endpoint. The client passed to [`HttpJwkProvider::new`] decides the
/// timeout and the redirect policy; the URL must use `https://`.
pub struct HttpJwkProvider {
    client: reqwest::Client,
    url: String,
    /// Per-request timeout; `None` uses the client's own setting.
    timeout: Option<Duration>,
}

impl HttpJwkProvider {
    pub fn new(client: reqwest::Client, url: impl Into<String>) -> Self {
        Self {
            client,
            url: url.into(),
            timeout: None,
        }
    }

    /// A provider with its own timeout, so a long-lived shared client cannot
    /// stretch the JWKS download past the value the identity code chose.
    pub fn with_timeout(
        client: reqwest::Client,
        url: impl Into<String>,
        timeout: Duration,
    ) -> Self {
        Self {
            client,
            url: url.into(),
            timeout: Some(timeout),
        }
    }
}

#[async_trait]
impl JwkProvider for HttpJwkProvider {
    async fn fetch_keys(&self) -> Result<FetchedJwks, String> {
        if !self.url.starts_with("https://") {
            return Err("the JWK endpoint must use https".to_string());
        }

        let mut request = self.client.get(&self.url);
        if let Some(timeout) = self.timeout {
            request = request.timeout(timeout);
        }
        let mut response = request
            .send()
            .await
            .map_err(|_| "could not reach the JWK endpoint".to_string())?;

        let status = response.status();
        if !status.is_success() {
            return Err(format!("the JWK endpoint returned HTTP {status}"));
        }

        if let Some(length) = response.content_length() {
            if length > MAX_JWKS_BYTES as u64 {
                return Err("the JWK response is too large".to_string());
            }
        }

        let max_age = response
            .headers()
            .get(reqwest::header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_cache_max_age)
            .unwrap_or(DEFAULT_JWK_MAX_AGE)
            .clamp(MIN_JWK_MAX_AGE, MAX_JWK_MAX_AGE);

        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "the JWK response could not be read".to_string())?
        {
            if body.len() + chunk.len() > MAX_JWKS_BYTES {
                return Err("the JWK response is too large".to_string());
            }
            body.extend_from_slice(&chunk);
        }

        let set: JwkSet = serde_json::from_slice(&body)
            .map_err(|_| "the JWK response was not valid JSON".to_string())?;

        let mut keys = HashMap::new();
        for jwk in set.keys {
            if !matches!(
                jwk.common.key_algorithm,
                Some(KeyAlgorithm::RS256) | Some(KeyAlgorithm::ES256)
            ) {
                continue;
            }
            let Some(kid) = jwk.common.key_id.clone() else {
                continue;
            };
            if kid.len() > MAX_KID_LENGTH {
                continue;
            }
            if let Ok(key) = DecodingKey::from_jwk(&jwk) {
                keys.insert(kid, key);
            }
        }

        if keys.is_empty() {
            return Err("the JWK response contained no usable signing keys".to_string());
        }

        Ok(FetchedJwks { keys, max_age })
    }
}

/// Google's endpoint for Firebase ID tokens. It is an [`HttpJwkProvider`] with
/// a fixed URL, kept so the existing construction sites stay unchanged.
pub struct GoogleJwkProvider {
    inner: HttpJwkProvider,
}

impl GoogleJwkProvider {
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            inner: HttpJwkProvider::with_timeout(client, GOOGLE_JWK_URL, Duration::from_secs(10)),
        }
    }
}

#[async_trait]
impl JwkProvider for GoogleJwkProvider {
    async fn fetch_keys(&self) -> Result<FetchedJwks, String> {
        self.inner.fetch_keys().await
    }
}

/// A provider with no endpoint, used when a configuration relies on HS256 only.
pub struct UnconfiguredJwkProvider;

#[async_trait]
impl JwkProvider for UnconfiguredJwkProvider {
    async fn fetch_keys(&self) -> Result<FetchedJwks, String> {
        Err("no JWKS endpoint is configured".to_string())
    }
}

pub(crate) fn parse_cache_max_age(value: &str) -> Option<Duration> {
    value.split(',').find_map(|directive| {
        let (name, value) = directive.trim().split_once('=')?;
        if !name.trim().eq_ignore_ascii_case("max-age") {
            return None;
        }
        value.trim().parse::<u64>().ok().map(Duration::from_secs)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct SlowMockProvider {
        keys: HashMap<String, DecodingKey>,
        delay: Duration,
        calls: AtomicUsize,
    }

    #[async_trait]
    impl JwkProvider for SlowMockProvider {
        async fn fetch_keys(&self) -> Result<FetchedJwks, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(self.delay).await;
            Ok(FetchedJwks {
                keys: self.keys.clone(),
                max_age: Duration::from_secs(3600),
            })
        }
    }

    struct FailingMockProvider {
        delay: Duration,
        calls: AtomicUsize,
        error_message: String,
    }

    #[async_trait]
    impl JwkProvider for FailingMockProvider {
        async fn fetch_keys(&self) -> Result<FetchedJwks, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(self.delay).await;
            Err(self.error_message.clone())
        }
    }

    #[tokio::test]
    async fn eight_concurrent_calls_on_cold_cache_all_succeed_and_provider_called_once() {
        let key = DecodingKey::from_secret(b"test-secret-for-jwks-single-flight");
        let provider = Arc::new(SlowMockProvider {
            keys: HashMap::from([("k1".to_string(), key)]),
            delay: Duration::from_millis(200),
            calls: AtomicUsize::new(0),
        });
        let cache = Arc::new(Mutex::new(JwksCache::new()));

        let mut handles = Vec::new();
        for _ in 0..8 {
            let cache = cache.clone();
            let provider = provider.clone();
            handles.push(tokio::spawn(async move {
                cached_key(&cache, provider.as_ref(), Some("k1")).await
            }));
        }

        let mut results = Vec::new();
        for handle in handles {
            let res = handle.await.expect("join handle must succeed");
            results.push(res);
        }

        assert_eq!(results.len(), 8);
        for res in results {
            assert!(res.is_ok(), "every concurrent call must succeed");
            assert!(res.unwrap().is_some(), "key k1 must be found");
        }

        assert_eq!(
            provider.calls.load(Ordering::SeqCst),
            1,
            "provider must be called exactly once for single-flight"
        );
    }

    #[tokio::test]
    async fn fetch_failure_all_concurrent_callers_fail_provider_called_once_and_throttles() {
        let provider = Arc::new(FailingMockProvider {
            delay: Duration::from_millis(50),
            calls: AtomicUsize::new(0),
            error_message: "jwks endpoint down".to_string(),
        });
        let cache = Arc::new(Mutex::new(JwksCache::new()));

        let mut handles = Vec::new();
        for _ in 0..8 {
            let cache = cache.clone();
            let provider = provider.clone();
            handles.push(tokio::spawn(async move {
                cached_key(&cache, provider.as_ref(), Some("k1")).await
            }));
        }

        let mut results = Vec::new();
        for handle in handles {
            let res = handle.await.expect("join handle must succeed");
            results.push(res);
        }

        assert_eq!(results.len(), 8);
        for res in results {
            assert!(
                res.is_err(),
                "every concurrent call must fail when fetch fails"
            );
        }

        assert_eq!(
            provider.calls.load(Ordering::SeqCst),
            1,
            "provider must be called exactly once during concurrent failed fetch"
        );

        // Subsequent call within 60s must be throttled and NOT call the provider again.
        let subsequent = cached_key(&cache, provider.as_ref(), Some("k1")).await;
        assert!(matches!(subsequent, Err(JwksError::Expired)));
        assert_eq!(
            provider.calls.load(Ordering::SeqCst),
            1,
            "provider must not be called again within the throttle window"
        );
    }

    #[tokio::test]
    async fn warmup_cache_fetches_once_and_subsequent_call_skips() {
        let key = DecodingKey::from_secret(b"warmup-test-secret");
        let provider = Arc::new(SlowMockProvider {
            keys: HashMap::from([("k1".to_string(), key)]),
            delay: Duration::from_millis(10),
            calls: AtomicUsize::new(0),
        });
        let cache = Arc::new(Mutex::new(JwksCache::new()));

        warmup_cache(&cache, provider.as_ref())
            .await
            .expect("warmup must succeed");
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);

        // Second warmup call on warm cache must be a no-op.
        warmup_cache(&cache, provider.as_ref())
            .await
            .expect("second warmup must succeed");
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    }
}

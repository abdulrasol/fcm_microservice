//! Small in-memory fixed-window rate limiter.
//!
//! The map is bounded; when it grows too large, expired entries are pruned and,
//! if still saturated, the map is cleared to protect memory.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

const MAX_TRACKED_KEYS: usize = 10_000;

pub struct RateLimiter {
    limit: usize,
    window: Duration,
    entries: Mutex<HashMap<String, VecDeque<Instant>>>,
}

impl RateLimiter {
    pub fn new(limit: usize, window: Duration) -> Self {
        Self {
            limit,
            window,
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// Records a hit for `key` and returns `true` when the request is allowed.
    pub fn check(&self, key: &str) -> bool {
        self.check_at(key, Instant::now())
    }

    fn check_at(&self, key: &str, now: Instant) -> bool {
        let mut entries = lock(&self.entries);

        if entries.len() >= MAX_TRACKED_KEYS && !entries.contains_key(key) {
            entries.retain(|_, hits| {
                hits.back()
                    .map(|last| now.duration_since(*last) < self.window)
                    .unwrap_or(false)
            });
            if entries.len() >= MAX_TRACKED_KEYS {
                entries.clear();
            }
        }

        let hits = entries.entry(key.to_string()).or_default();
        while let Some(first) = hits.front() {
            if now.duration_since(*first) >= self.window {
                hits.pop_front();
            } else {
                break;
            }
        }

        if hits.len() >= self.limit {
            return false;
        }
        hits.push_back(now);
        true
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

    #[test]
    fn enforces_limit_per_key_and_resets_after_window() {
        let limiter = RateLimiter::new(3, Duration::from_secs(60));
        let start = Instant::now();

        assert!(limiter.check_at("ip:1", start));
        assert!(limiter.check_at("ip:1", start + Duration::from_secs(1)));
        assert!(limiter.check_at("ip:1", start + Duration::from_secs(2)));
        assert!(
            !limiter.check_at("ip:1", start + Duration::from_secs(3)),
            "fourth hit inside the window must be rejected"
        );
        assert!(
            limiter.check_at("ip:2", start + Duration::from_secs(3)),
            "other keys have their own budget"
        );
        assert!(limiter.check_at("ip:1", start + Duration::from_secs(61)));
    }

    #[test]
    fn different_keys_are_independent() {
        let limiter = RateLimiter::new(1, Duration::from_secs(60));
        let start = Instant::now();
        assert!(limiter.check_at("uid:a", start));
        assert!(!limiter.check_at("uid:a", start));
        assert!(limiter.check_at("uid:b", start));
    }
}

//! The limit of the legacy `API_KEY` (T13-A).
//!
//! The legacy key is a temporary compatibility path for `/send`. It has its
//! own budget, so a client that still uses it cannot spend the budget of the
//! service key, of an application key or of the IP limits.
//!
//! The module holds no secret: it never sees the key itself, only the fact
//! that a request was accepted because of it.

use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::ratelimit::RateLimiter;

/// The default budget of the legacy key: 60 requests per minute.
pub const DEFAULT_LEGACY_RATE_LIMIT_PER_MINUTE: usize = 60;

/// The window of the legacy budget.
pub const LEGACY_RATE_LIMIT_WINDOW: Duration = Duration::from_secs(60);

/// The window in seconds, for the `Retry-After` header.
pub const LEGACY_RATE_LIMIT_WINDOW_STR: &str = "60";

/// The name of the bucket. The legacy key is one global key, so the bucket
/// does not depend on the address of the caller.
const LEGACY_BUCKET: &str = "legacy";

/// The text of the daily warning. It never holds the key, not even a part.
pub const LEGACY_WARNING: &str = "legacy API key in use; migrate to per-app keys";

/// The text of the refusal when the budget is exhausted.
pub const LEGACY_LIMIT_MESSAGE: &str =
    "too many requests with the legacy API key; migrate to an application key";

/// The result of reading `LEGACY_RATE_LIMIT_PER_MINUTE`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyLimit {
    /// The budget per minute.
    pub limit: usize,
    /// A warning to print at startup when the value was not usable.
    pub warning: Option<String>,
}

/// Reads the budget of the legacy key from the environment.
///
/// A missing, empty, zero or non-numeric value keeps the default and asks
/// for a warning: a silent change of the budget would be surprising.
pub fn legacy_limit_from_env(raw: Option<&str>) -> LegacyLimit {
    let Some(value) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return LegacyLimit {
            limit: DEFAULT_LEGACY_RATE_LIMIT_PER_MINUTE,
            warning: None,
        };
    };

    match value.parse::<usize>() {
        Ok(limit) if limit > 0 => LegacyLimit {
            limit,
            warning: None,
        },
        Ok(_) => LegacyLimit {
            limit: DEFAULT_LEGACY_RATE_LIMIT_PER_MINUTE,
            warning: Some(format!(
                "LEGACY_RATE_LIMIT_PER_MINUTE is zero; the default of {} is used",
                DEFAULT_LEGACY_RATE_LIMIT_PER_MINUTE
            )),
        },
        Err(_) => LegacyLimit {
            limit: DEFAULT_LEGACY_RATE_LIMIT_PER_MINUTE,
            warning: Some(format!(
                "LEGACY_RATE_LIMIT_PER_MINUTE is not a number; the default of {} is used",
                DEFAULT_LEGACY_RATE_LIMIT_PER_MINUTE
            )),
        },
    }
}

/// The counter of the daily warning.
#[derive(Debug, Default)]
struct Usage {
    /// The UTC day of the last warning.
    last_warn_day: Option<u32>,
    /// The uses since the last warning.
    uses_since_warn: u64,
}

/// The guard of the legacy key: the budget and the daily warning.
pub struct LegacyGuard {
    limiter: RateLimiter,
    usage: Mutex<Usage>,
}

impl LegacyGuard {
    /// A guard with the given budget per minute.
    pub fn new(limit: usize) -> Self {
        Self {
            limiter: RateLimiter::new(limit.max(1), LEGACY_RATE_LIMIT_WINDOW),
            usage: Mutex::new(Usage::default()),
        }
    }

    /// Records a use of the legacy key.
    ///
    /// `false` means the budget of the window is exhausted.
    pub fn allow(&self) -> bool {
        self.limiter.check(LEGACY_BUCKET)
    }

    /// Counts a use and returns the number of uses since the last warning
    /// when the warning must be printed now.
    ///
    /// The warning is printed at most once per UTC day. The day is a number
    /// given by the caller, so the logic is testable without waiting.
    pub fn note_use(&self, day: u32) -> Option<u64> {
        let mut usage = lock(&self.usage);
        usage.uses_since_warn += 1;
        if usage.last_warn_day == Some(day) {
            return None;
        }
        usage.last_warn_day = Some(day);
        let count = usage.uses_since_warn;
        usage.uses_since_warn = 0;
        Some(count)
    }
}

/// The message of the daily warning.
pub fn legacy_warning_message(uses_since_warn: u64) -> String {
    format!(
        "{} (uses since the last warning: {})",
        LEGACY_WARNING, uses_since_warn
    )
}

/// The number of the UTC day of a moment.
///
/// The number of the days since the epoch, so two moments of the same UTC day
/// have the same number.
pub fn utc_day(now: SystemTime) -> u32 {
    let seconds = now
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    u32::try_from(seconds / 86_400).unwrap_or(u32::MAX)
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_limit_env_invalid_falls_back_to_default() {
        let fallback = DEFAULT_LEGACY_RATE_LIMIT_PER_MINUTE;

        assert_eq!(
            legacy_limit_from_env(None).limit,
            fallback,
            "a missing value keeps the default"
        );
        assert_eq!(legacy_limit_from_env(None).warning, None);

        let empty = legacy_limit_from_env(Some("   "));
        assert_eq!(empty.limit, fallback);
        assert_eq!(empty.warning, None);

        let zero = legacy_limit_from_env(Some("0"));
        assert_eq!(zero.limit, fallback);
        assert!(zero.warning.is_some(), "a zero value must warn");

        let text = legacy_limit_from_env(Some("many"));
        assert_eq!(text.limit, fallback);
        assert!(text.warning.is_some(), "a non-numeric value must warn");

        let negative = legacy_limit_from_env(Some("-5"));
        assert_eq!(negative.limit, fallback);
        assert!(negative.warning.is_some(), "a negative value must warn");

        let good = legacy_limit_from_env(Some("7"));
        assert_eq!(good.limit, 7);
        assert_eq!(good.warning, None);
    }

    #[test]
    fn legacy_warning_emitted_once_per_day() {
        let guard = LegacyGuard::new(10);

        assert_eq!(guard.note_use(100), Some(1), "the first use warns");
        assert_eq!(guard.note_use(100), None, "the same day does not warn");
        assert_eq!(guard.note_use(100), None);

        assert_eq!(
            guard.note_use(101),
            Some(3),
            "the next day warns with the uses since the last warning"
        );
        assert_eq!(guard.note_use(101), None);
        assert_eq!(
            guard.note_use(102),
            Some(2),
            "the counter holds the uses since the last warning"
        );
    }

    #[test]
    fn legacy_warning_never_contains_key_material() {
        let guard = LegacyGuard::new(10);
        let count = guard.note_use(5).expect("the first use warns");
        let message = legacy_warning_message(count);

        assert!(message.contains("legacy API key"));
        assert!(message.contains('1'));
        for secret in ["secret-key", "AIza", "BEGIN PRIVATE", "private_key"] {
            assert!(
                !message.contains(secret),
                "the warning must not hold any key material"
            );
        }
    }

    #[test]
    fn legacy_budget_is_global_and_finite() {
        let guard = LegacyGuard::new(2);
        assert!(guard.allow());
        assert!(guard.allow());
        assert!(!guard.allow(), "the third use is refused");
    }

    #[test]
    fn utc_day_is_stable_inside_a_day() {
        let day = utc_day(UNIX_EPOCH + Duration::from_secs(86_400 * 10 + 100));
        let same = utc_day(UNIX_EPOCH + Duration::from_secs(86_400 * 10 + 86_399));
        let next = utc_day(UNIX_EPOCH + Duration::from_secs(86_400 * 11));

        assert_eq!(day, same);
        assert_eq!(next, day + 1);
    }
}

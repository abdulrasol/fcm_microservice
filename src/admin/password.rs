//! Password hashing (argon2id) and the dashboard password policy.
//!
//! Hashing is CPU-heavy and runs on `spawn_blocking` so a login never stalls
//! the async runtime.

use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;

pub const MIN_LENGTH: usize = 12;
pub const MAX_LENGTH: usize = 128;

/// Small built-in list; the length rule already rejects most weak guesses, this
/// only stops the obvious ones that are long enough.
const COMMON_PASSWORDS: [&str; 20] = [
    "password1234",
    "123456789012",
    "qwertyuiop12",
    "1q2w3e4r5t6y",
    "passwordpassword",
    "letmein12345",
    "welcome12345",
    "admin1234567",
    "iloveyou1234",
    "abc123456789",
    "monkey123456",
    "dragon123456",
    "football1234",
    "baseball1234",
    "superman1234",
    "trustno12345",
    "master123456",
    "sunshine1234",
    "princess1234",
    "shadow123456",
];

/// Returns the reason the password is rejected, or `Ok(())`.
pub fn validate_password(password: &str, email: &str) -> Result<(), String> {
    let length = password.chars().count();
    if !(MIN_LENGTH..=MAX_LENGTH).contains(&length) {
        return Err(format!(
            "the password must be between {} and {} characters",
            MIN_LENGTH, MAX_LENGTH
        ));
    }

    let lowered = password.to_lowercase();
    let email = email.trim().to_lowercase();
    let local_part = email.split('@').next().unwrap_or_default();
    if !email.is_empty() && (lowered == email || (!local_part.is_empty() && lowered == local_part))
    {
        return Err("the password must not be the email address or its local part".to_string());
    }

    if COMMON_PASSWORDS.contains(&lowered.as_str()) {
        return Err("the password is too common".to_string());
    }

    Ok(())
}

/// Hashes a password into a PHC string with a fresh salt.
pub async fn hash_password(password: &str) -> Result<String, String> {
    let password = password.to_string();
    tokio::task::spawn_blocking(move || hash_blocking(&password))
        .await
        .map_err(|error| format!("the password hashing task failed: {}", error))?
}

fn hash_blocking(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| format!("the password could not be hashed: {}", error))
}

/// Verifies a password against a stored PHC string; a malformed hash never
/// matches, so a corrupt row cannot be logged into.
pub async fn verify_password(hash: &str, password: &str) -> bool {
    let hash = hash.to_string();
    let password = password.to_string();
    // A panicking hashing task is treated as a failed verification.
    tokio::task::spawn_blocking(move || verify_blocking(&hash, &password))
        .await
        .unwrap_or(false)
}

fn verify_blocking(hash: &str, password: &str) -> bool {
    let parsed = match PasswordHash::new(hash) {
        Ok(parsed) => parsed,
        Err(_) => return false,
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// Verifies against `hash`, or against `dummy_hash` when the account does not
/// exist, so an unknown email costs the same argon2 work as a known one.
pub async fn verify_or_dummy(hash: Option<&str>, password: &str, dummy_hash: &str) -> bool {
    let known = hash.is_some();
    let target = hash.unwrap_or(dummy_hash);
    let verified = verify_password(target, password).await;
    known && verified
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_policy_rejects_short_and_email_equal_and_common() {
        let email = "owner@example.com";
        assert!(validate_password("short", email).is_err(), "too short");
        assert!(
            validate_password("exactly11ch", email).is_err(),
            "eleven characters is still too short"
        );
        assert!(
            validate_password("owner@example.com", email).is_err(),
            "the email itself is rejected"
        );
        assert!(
            validate_password("Owner@Example.com", email).is_err(),
            "the email is compared case-insensitively"
        );
        assert!(
            validate_password("owner", email).is_err(),
            "the local part is rejected"
        );
        assert!(
            validate_password("password1234", email).is_err(),
            "a common password is rejected"
        );
        assert!(
            validate_password("password1234", "other@example.com").is_err(),
            "the common list does not depend on the email"
        );
        assert!(
            validate_password(&"a".repeat(129), email).is_err(),
            "too long"
        );

        assert!(validate_password("exactly12chr", email).is_ok());
        assert!(validate_password(&"a".repeat(128), email).is_ok());
        assert!(validate_password("a long passphrase 2026", email).is_ok());
    }

    #[tokio::test]
    async fn hash_then_verify_roundtrip() {
        let hash = hash_password("a long passphrase 2026")
            .await
            .expect("hashing must work");
        assert!(hash.starts_with("$argon2id$"), "argon2id PHC string");
        assert!(verify_password(&hash, "a long passphrase 2026").await);

        let second = hash_password("a long passphrase 2026")
            .await
            .expect("hashing must work");
        assert_ne!(hash, second, "each hash uses a fresh salt");
        assert!(verify_password(&second, "a long passphrase 2026").await);
    }

    #[tokio::test]
    async fn verify_rejects_wrong_password() {
        let hash = hash_password("a long passphrase 2026")
            .await
            .expect("hashing must work");
        assert!(!verify_password(&hash, "a long passphrase 2027").await);
        assert!(!verify_password(&hash, "").await);
        assert!(!verify_password("not a phc string", "a long passphrase 2026").await);
    }

    #[tokio::test]
    async fn unknown_email_still_runs_dummy_verify() {
        let dummy = hash_password("dummy password for timing")
            .await
            .expect("hashing must work");
        assert!(!verify_or_dummy(None, "anything at all", &dummy).await);

        let real = hash_password("a long passphrase 2026")
            .await
            .expect("hashing must work");
        assert!(verify_or_dummy(Some(&real), "a long passphrase 2026", &dummy).await);
        assert!(!verify_or_dummy(Some(&real), "wrong password 2026", &dummy).await);
    }
}

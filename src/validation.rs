//! Request validation for `/send` and topic management.
//!
//! All functions return a clear, user-facing error message and never panic.

use serde_json::Value;

pub(crate) const MAX_TOPIC_LENGTH: usize = 200;
pub(crate) const MAX_CONDITION_LENGTH: usize = 500;
pub(crate) const MAX_CONDITION_TOPICS: usize = 5;
pub(crate) const MAX_TITLE_LENGTH: usize = 200;
pub(crate) const MAX_BODY_LENGTH: usize = 4000;
pub(crate) const MAX_IMAGE_URL_LENGTH: usize = 2048;
pub(crate) const MAX_DATA_BYTES: usize = 4096;
pub(crate) const MAX_DEVICE_TOKEN_LENGTH: usize = 4096;
pub(crate) const MAX_SUBSCRIBE_TOKENS: usize = 10;

/// Topic names accepted by `/send` and saved-topic endpoints.
pub(crate) fn validate_topic_name(topic: &str) -> Result<(), String> {
    let length = topic.len();
    if length == 0 || length > MAX_TOPIC_LENGTH {
        return Err(format!("topic must be 1-{MAX_TOPIC_LENGTH} characters"));
    }
    if !topic.bytes().all(is_allowed_topic_byte) {
        return Err(
            "topic may only contain ASCII letters, digits, '-', '_', '.', '~' and '%' (no '/topics/' prefix)"
                .to_string(),
        );
    }
    Ok(())
}

fn is_allowed_topic_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'%')
}

/// FCM condition validation: length, character allow-list, balanced quotes and
/// at most five topic references.
pub(crate) fn validate_condition(condition: &str) -> Result<(), String> {
    if condition.trim().is_empty() {
        return Err("condition must not be empty".to_string());
    }
    if condition.chars().count() > MAX_CONDITION_LENGTH {
        return Err(format!(
            "condition must be at most {MAX_CONDITION_LENGTH} characters"
        ));
    }
    if !condition.chars().all(is_allowed_condition_char) {
        return Err(
            "condition contains unsupported characters; allowed: ASCII letters, digits, spaces and _ ' \" ( ) | & ! . ~ - %"
                .to_string(),
        );
    }

    let single_quotes = condition.matches('\'').count();
    let double_quotes = condition.matches('"').count();
    if !single_quotes.is_multiple_of(2) || !double_quotes.is_multiple_of(2) {
        return Err("condition has unbalanced quotes".to_string());
    }

    let topics = condition.matches("in topics").count();
    if topics == 0 {
        return Err(
            "condition must reference topics using the form '<topic>' in topics".to_string(),
        );
    }
    if topics > MAX_CONDITION_TOPICS {
        return Err(format!(
            "condition may reference at most {MAX_CONDITION_TOPICS} topics"
        ));
    }

    Ok(())
}

fn is_allowed_condition_char(character: char) -> bool {
    character.is_ascii_alphanumeric()
        || matches!(
            character,
            '_' | '\'' | '"' | '(' | ')' | '|' | '&' | '!' | '.' | '~' | '-' | '%' | ' '
        )
}

fn validate_device_token(token: &str) -> Result<(), String> {
    if token.trim().is_empty() {
        return Err("tokens must not contain empty values".to_string());
    }
    if token.len() > MAX_DEVICE_TOKEN_LENGTH {
        return Err(format!(
            "each device token must be at most {MAX_DEVICE_TOKEN_LENGTH} characters"
        ));
    }
    if token.chars().any(char::is_whitespace) {
        return Err("device tokens must not contain whitespace".to_string());
    }
    Ok(())
}

pub(crate) fn validate_subscribe_tokens(tokens: &[String]) -> Result<(), String> {
    if tokens.is_empty() {
        return Err("tokens must contain at least one device token".to_string());
    }
    if tokens.len() > MAX_SUBSCRIBE_TOKENS {
        return Err(format!(
            "at most {MAX_SUBSCRIBE_TOKENS} device tokens are allowed per request"
        ));
    }
    for token in tokens {
        validate_device_token(token)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn validate_send(
    app: &str,
    topic: Option<&str>,
    token: Option<&str>,
    condition: Option<&str>,
    title: &str,
    body: Option<&str>,
    image: Option<&str>,
    data: Option<&Value>,
) -> Result<(), String> {
    if app.trim().is_empty() {
        return Err("app is required".to_string());
    }

    let targets = [topic.is_some(), token.is_some(), condition.is_some()]
        .into_iter()
        .filter(|present| *present)
        .count();
    if targets != 1 {
        return Err("provide exactly one of topic, token or condition".to_string());
    }

    if let Some(topic) = topic {
        validate_topic_name(topic)?;
    }
    if let Some(token) = token {
        validate_device_token(token)?;
    }
    if let Some(condition) = condition {
        validate_condition(condition)?;
    }

    if title.trim().is_empty() {
        return Err("title is required".to_string());
    }
    if title.chars().count() > MAX_TITLE_LENGTH {
        return Err(format!(
            "title must be at most {MAX_TITLE_LENGTH} characters"
        ));
    }
    if let Some(body) = body {
        if body.chars().count() > MAX_BODY_LENGTH {
            return Err(format!("body must be at most {MAX_BODY_LENGTH} characters"));
        }
    }
    if let Some(image) = image {
        if !image.starts_with("https://") {
            return Err("image must be an https:// URL".to_string());
        }
        if image.len() > MAX_IMAGE_URL_LENGTH {
            return Err(format!(
                "image URL must be at most {MAX_IMAGE_URL_LENGTH} characters"
            ));
        }
        if image.chars().any(char::is_whitespace) {
            return Err("image URL must not contain whitespace".to_string());
        }
    }
    if let Some(data) = data {
        let size = serde_json::to_vec(data)
            .map_err(|_| "data could not be serialized".to_string())?
            .len();
        if size > MAX_DATA_BYTES {
            return Err(format!(
                "data must be at most {MAX_DATA_BYTES} bytes when serialized"
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn topic_boundaries() {
        assert!(validate_topic_name("a").is_ok());
        assert!(validate_topic_name(&"a".repeat(MAX_TOPIC_LENGTH)).is_ok());
        assert!(validate_topic_name(&"a".repeat(MAX_TOPIC_LENGTH + 1)).is_err());
        assert!(validate_topic_name("").is_err());
        assert!(validate_topic_name("news-1_.~%").is_ok());
        for topic in ["/topics/all", "a b", "عربي", "a/b", "a\nb"] {
            assert!(validate_topic_name(topic).is_err(), "{topic} should fail");
        }
    }

    #[test]
    fn condition_boundaries() {
        assert!(validate_condition("'a' in topics").is_ok());
        assert!(validate_condition("'a' in topics && 'b' in topics").is_ok());
        assert!(validate_condition(
            "'a' in topics && 'b' in topics || 'c' in topics && 'd' in topics"
        )
        .is_ok());
        assert!(validate_condition(
            "'a' in topics && 'b' in topics || 'c' in topics && 'd' in topics || 'e' in topics"
        )
        .is_ok());
        assert!(
            validate_condition(
                "'a' in topics && 'b' in topics || 'c' in topics && 'd' in topics || 'e' in topics || 'f' in topics"
            )
            .is_err(),
            "six topics must be rejected"
        );
        assert!(validate_condition("").is_err());
        assert!(validate_condition(&"a".repeat(MAX_CONDITION_LENGTH + 1)).is_err());
        assert!(
            validate_condition("'a in topics").is_err(),
            "unbalanced quote"
        );
        assert!(validate_condition("\"a\" in topics").is_ok());
        assert!(validate_condition("'a' in topics; DROP TABLE").is_err());
        assert!(validate_condition("'a' in topics\n").is_err());
    }

    #[test]
    fn send_payload_boundaries() {
        assert!(validate_send(
            "app",
            Some("all"),
            None,
            None,
            "Title",
            Some("Body"),
            Some("https://example.com/i.png"),
            Some(&json!({"k": "v"})),
        )
        .is_ok());

        assert!(validate_send("app", None, None, None, "Title", None, None, None).is_err());
        assert!(
            validate_send(
                "app",
                Some("all"),
                Some("device"),
                None,
                "Title",
                None,
                None,
                None,
            )
            .is_err(),
            "exactly one target is required"
        );
        assert!(validate_send(
            "app",
            Some("all"),
            None,
            None,
            &"t".repeat(MAX_TITLE_LENGTH + 1),
            None,
            None,
            None,
        )
        .is_err());
        assert!(validate_send(
            "app",
            Some("all"),
            None,
            None,
            "Title",
            Some(&"b".repeat(MAX_BODY_LENGTH + 1)),
            None,
            None,
        )
        .is_err());
        assert!(validate_send(
            "app",
            Some("all"),
            None,
            None,
            "Title",
            None,
            Some("http://example.com/i.png"),
            None,
        )
        .is_err());
        assert!(validate_send(
            "app",
            Some("all"),
            None,
            None,
            "Title",
            None,
            None,
            Some(&json!({"k": "x".repeat(MAX_DATA_BYTES)})),
        )
        .is_err());
        assert!(validate_send("  ", Some("all"), None, None, "Title", None, None, None).is_err());
    }

    #[test]
    fn subscribe_token_limits() {
        assert!(validate_subscribe_tokens(&["token".to_string()]).is_ok());
        assert!(validate_subscribe_tokens(&vec!["t".to_string(); MAX_SUBSCRIBE_TOKENS]).is_ok());
        assert!(
            validate_subscribe_tokens(&vec!["t".to_string(); MAX_SUBSCRIBE_TOKENS + 1]).is_err()
        );
        assert!(validate_subscribe_tokens(&[]).is_err());
        assert!(validate_subscribe_tokens(&[" ".to_string()]).is_err());
        assert!(validate_subscribe_tokens(&["a b".to_string()]).is_err());
        assert!(validate_subscribe_tokens(&["x".repeat(MAX_DEVICE_TOKEN_LENGTH + 1)]).is_err());
    }
}

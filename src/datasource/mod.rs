//! Data sources: read one document and the members of a role from a backend.
//!
//! Templates use logical paths (`store_orders/{id}`) and the source decides
//! where they are read from: Firestore with the app's service account, or a
//! small HTTP contract implemented by the app backend. The registry picks the
//! source per app; the send routes are wired to it in T5, so until then most of
//! this module is reachable only from tests.

#![allow(dead_code)]

pub mod firestore;
pub mod http;
pub mod registry;

use async_trait::async_trait;

/// A plain JSON document (strings, numbers, booleans, nulls, nested objects and
/// arrays).
pub type Doc = serde_json::Value;

/// Why a data source could not answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataError {
    /// The caller passed a path/field/value/limit this source cannot use; maps
    /// to a `400`-class response.
    InvalidPath(String),
    /// The source is down, denied or misconfigured; callers fail closed and map
    /// this to `503`.
    Unavailable(String),
}

#[async_trait]
pub trait DataSource: Send + Sync {
    /// `Ok(None)` means "not found". Any other failure is `Err` (callers fail
    /// closed).
    async fn get_doc(&self, path: &str) -> Result<Option<Doc>, DataError>;

    /// Returns document ids (uids) whose `field` equals `value`, at most `limit`
    /// (1..=200).
    async fn members(
        &self,
        collection: &str,
        field: &str,
        value: &str,
        limit: usize,
    ) -> Result<Vec<String>, DataError>;
}

const MAX_SEGMENT_LENGTH: usize = 200;
const MIN_DOC_SEGMENTS: usize = 2;
const MAX_DOC_SEGMENTS: usize = 6;
const MAX_COLLECTION_LENGTH: usize = 100;
const MAX_FIELD_SEGMENTS: usize = 5;
const MAX_VALUE_LENGTH: usize = 200;
const MAX_LIMIT: usize = 200;

/// Validates a document path and returns its segments.
///
/// A path is an even number of segments (2..=6), e.g. `store_orders/abc123`.
/// Segments may contain any non-control character (Auth0 ids such as
/// `auth0|123` are valid); each segment is percent-encoded when it is sent.
pub fn validate_doc_path(path: &str) -> Result<Vec<&str>, DataError> {
    let segments: Vec<&str> = path.split('/').collect();
    if segments.len() < MIN_DOC_SEGMENTS
        || segments.len() > MAX_DOC_SEGMENTS
        || !segments.len().is_multiple_of(2)
    {
        return Err(DataError::InvalidPath(
            "a document path must have an even number of segments (2..=6)".to_string(),
        ));
    }
    for segment in &segments {
        if segment.is_empty()
            || *segment == "."
            || *segment == ".."
            || segment.len() > MAX_SEGMENT_LENGTH
            || segment.chars().any(char::is_control)
        {
            return Err(DataError::InvalidPath(
                "a document path segment must be 1..=200 bytes, must not be '.' or '..' and must not contain control characters"
                    .to_string(),
            ));
        }
    }
    Ok(segments)
}

/// A collection id: `^[A-Za-z0-9_-]{1,100}$`.
pub fn validate_collection(collection: &str) -> Result<(), DataError> {
    if collection.is_empty()
        || collection.len() > MAX_COLLECTION_LENGTH
        || !collection.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        })
    {
        return Err(DataError::InvalidPath(
            "a collection must match ^[A-Za-z0-9_-]{1,100}$".to_string(),
        ));
    }
    Ok(())
}

/// A field path: 1..=5 dot-separated `[A-Za-z0-9_]+` segments.
pub fn validate_field(field: &str) -> Result<(), DataError> {
    let segments: Vec<&str> = field.split('.').collect();
    if segments.is_empty()
        || segments.len() > MAX_FIELD_SEGMENTS
        || segments.iter().any(|segment| {
            segment.is_empty()
                || !segment
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_')
        })
    {
        return Err(DataError::InvalidPath(
            "a field must be 1..=5 dot-separated [A-Za-z0-9_]+ segments".to_string(),
        ));
    }
    Ok(())
}

/// A role-member value: at most 200 bytes without control characters.
pub fn validate_value(value: &str) -> Result<(), DataError> {
    if value.len() > MAX_VALUE_LENGTH || value.chars().any(char::is_control) {
        return Err(DataError::InvalidPath(
            "a value must be at most 200 bytes and must not contain control characters".to_string(),
        ));
    }
    Ok(())
}

/// The member limit: 1..=200.
pub fn validate_limit(limit: usize) -> Result<(), DataError> {
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(DataError::InvalidPath(
            "limit must be between 1 and 200".to_string(),
        ));
    }
    Ok(())
}

/// Percent-encodes one path segment (or a whole logical path) for a URL.
///
/// Only `A-Z a-z 0-9 _ . ~ -` stay as they are; every other byte becomes `%XX`
/// with uppercase hex, so `store_orders/abc123` becomes
/// `store_orders%2Fabc123` when a separator must be encoded too.
pub(crate) fn percent_encode(segment: &str) -> String {
    let mut encoded = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'~' | b'-') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
pub(crate) mod test_support {
    //! A tiny HTTP/1.1 server for tests: it records raw requests and replies
    //! with prepared responses. It uses only tokio, so no test dependency is
    //! added.

    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    pub struct TestServer {
        pub base_url: String,
        requests: Arc<Mutex<Vec<String>>>,
        task: tokio::task::JoinHandle<()>,
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    impl TestServer {
        /// Replies with each response in order and keeps repeating the last one.
        pub async fn start(responses: Vec<Vec<u8>>) -> Self {
            Self::start_with_delay(responses, Duration::ZERO).await
        }

        pub async fn start_with_delay(responses: Vec<Vec<u8>>, delay: Duration) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("the test server must bind");
            let address = listener.local_addr().expect("the test server address");
            let requests = Arc::new(Mutex::new(Vec::new()));
            let queue = Arc::new(Mutex::new(VecDeque::from(responses)));
            let task = tokio::spawn({
                let requests = requests.clone();
                async move {
                    loop {
                        let Ok((mut stream, _)) = listener.accept().await else {
                            break;
                        };
                        let raw = read_request(&mut stream).await;
                        requests
                            .lock()
                            .expect("the requests lock must not be poisoned")
                            .push(raw);
                        let prepared = {
                            let mut queue = queue
                                .lock()
                                .expect("the responses lock must not be poisoned");
                            if queue.len() > 1 {
                                queue.pop_front()
                            } else {
                                queue.front().cloned()
                            }
                        };
                        let reply = prepared.unwrap_or_else(|| {
                            response(500, "Internal Server Error", "text/plain", "no response")
                        });
                        if !delay.is_zero() {
                            tokio::time::sleep(delay).await;
                        }
                        let _ = stream.write_all(&reply).await;
                        let _ = stream.flush().await;
                    }
                }
            });
            Self {
                base_url: format!("http://{address}"),
                requests,
                task,
            }
        }

        /// Accepts connections and never replies, for timeout tests.
        pub async fn start_silent() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("the test server must bind");
            let address = listener.local_addr().expect("the test server address");
            let requests = Arc::new(Mutex::new(Vec::new()));
            let task = tokio::spawn({
                let requests = requests.clone();
                async move {
                    loop {
                        let Ok((mut stream, _)) = listener.accept().await else {
                            break;
                        };
                        let raw = read_request(&mut stream).await;
                        requests
                            .lock()
                            .expect("the requests lock must not be poisoned")
                            .push(raw);
                    }
                }
            });
            Self {
                base_url: format!("http://{address}"),
                requests,
                task,
            }
        }

        pub fn requests(&self) -> Vec<String> {
            self.requests
                .lock()
                .expect("the requests lock must not be poisoned")
                .clone()
        }
    }

    async fn read_request(stream: &mut TcpStream) -> String {
        let mut buffer = Vec::new();
        let mut chunk = [0u8; 2048];
        loop {
            let read = match stream.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(read) => read,
            };
            buffer.extend_from_slice(&chunk[..read]);
            if let Some(header_end) = find(&buffer, b"\r\n\r\n") {
                let length = content_length(&buffer[..header_end]);
                if buffer.len() >= header_end + 4 + length {
                    break;
                }
            }
        }
        String::from_utf8_lossy(&buffer).to_string()
    }

    fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
    }

    fn content_length(headers: &[u8]) -> usize {
        let text = String::from_utf8_lossy(headers);
        header(&text, "content-length")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0)
    }

    /// Returns `(method, path, raw query)` from a captured request.
    pub fn request_line(raw: &str) -> (String, String, String) {
        let line = raw.lines().next().unwrap_or_default();
        let mut parts = line.split_whitespace();
        let method = parts.next().unwrap_or_default().to_string();
        let target = parts.next().unwrap_or_default();
        match target.split_once('?') {
            Some((path, query)) => (method, path.to_string(), query.to_string()),
            None => (method, target.to_string(), String::new()),
        }
    }

    /// Returns one header value from a captured request (case-insensitive).
    pub fn header(raw: &str, name: &str) -> Option<String> {
        raw.lines()
            .skip(1)
            .take_while(|line| !line.is_empty())
            .find_map(|line| {
                let (key, value) = line.split_once(':')?;
                key.trim()
                    .eq_ignore_ascii_case(name)
                    .then(|| value.trim().to_string())
            })
    }

    pub fn response(status: u16, reason: &str, content_type: &str, body: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    pub fn json(status: u16, reason: &str, body: &str) -> Vec<u8> {
        response(status, reason, "application/json", body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doc_path_accepts_pairs_and_special_ids() {
        assert_eq!(
            validate_doc_path("store_orders/abc123").expect("a pair must be accepted"),
            vec!["store_orders", "abc123"]
        );
        assert_eq!(
            validate_doc_path("users/auth0|123").expect("a special id must be accepted"),
            vec!["users", "auth0|123"]
        );
        assert_eq!(
            validate_doc_path("companies/acme/users/u-1/orders/o-9")
                .expect("three pairs must be accepted")
                .len(),
            6
        );
    }

    #[test]
    fn doc_path_rejects_dot_segments_slashes_and_odd_lengths() {
        for invalid in [
            "",
            "users",
            "users/",
            "/users/u1",
            "users/u1/extra",
            "users/.",
            "users/..",
            "users/u1/..",
        ] {
            assert!(
                matches!(validate_doc_path(invalid), Err(DataError::InvalidPath(_))),
                "{invalid:?} must be rejected"
            );
        }
    }

    #[test]
    fn doc_path_rejects_control_chars_and_long_segments() {
        assert!(matches!(
            validate_doc_path("users/bad\u{0}uid"),
            Err(DataError::InvalidPath(_))
        ));
        assert!(matches!(
            validate_doc_path("users/bad\nuid"),
            Err(DataError::InvalidPath(_))
        ));
        let long = format!("users/{}", "a".repeat(MAX_SEGMENT_LENGTH + 1));
        assert!(matches!(
            validate_doc_path(&long),
            Err(DataError::InvalidPath(_))
        ));
        let longest = format!("users/{}", "a".repeat(MAX_SEGMENT_LENGTH));
        assert!(validate_doc_path(&longest).is_ok());
        let deep = "a/b/c/d/e/f/g/h";
        assert!(matches!(
            validate_doc_path(deep),
            Err(DataError::InvalidPath(_))
        ));
    }

    #[test]
    fn collection_and_field_validation() {
        assert!(validate_collection("users").is_ok());
        assert!(validate_collection("app_users-1").is_ok());
        assert!(validate_collection("").is_err());
        assert!(validate_collection("users/orders").is_err());
        assert!(validate_collection("users orders").is_err());
        assert!(validate_collection(&"a".repeat(MAX_COLLECTION_LENGTH + 1)).is_err());

        assert!(validate_field("role").is_ok());
        assert!(validate_field("app_metadata.role").is_ok());
        assert!(validate_field("a.b.c.d.e").is_ok());
        assert!(validate_field("a.b.c.d.e.f").is_err());
        assert!(validate_field("").is_err());
        assert!(validate_field("role.name").is_ok());
        assert!(validate_field(".role").is_err());
        assert!(validate_field("role.").is_err());
        assert!(validate_field("ro le").is_err());
        assert!(validate_field("ro-le").is_err());

        assert!(validate_value("admin").is_ok());
        assert!(validate_value("").is_ok());
        assert!(validate_value(&"a".repeat(MAX_VALUE_LENGTH + 1)).is_err());
        assert!(validate_value("bad\u{0}value").is_err());

        assert!(validate_limit(1).is_ok());
        assert!(validate_limit(200).is_ok());
        assert!(validate_limit(0).is_err());
        assert!(validate_limit(201).is_err());
    }

    #[test]
    fn percent_encode_segment() {
        assert_eq!(
            percent_encode("store_orders/abc123"),
            "store_orders%2Fabc123"
        );
        assert_eq!(percent_encode("auth0|123"), "auth0%7C123");
        assert_eq!(percent_encode("a b"), "a%20b");
        assert_eq!(percent_encode("A-Z_a.z~0"), "A-Z_a.z~0");
        assert_eq!(percent_encode("é"), "%C3%A9");
        assert_eq!(percent_encode("+&=?"), "%2B%26%3D%3F");
    }
}

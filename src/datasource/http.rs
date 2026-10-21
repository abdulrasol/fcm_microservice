//! HTTP resolver data source.
//!
//! The tool asks the app backend over a small fixed contract:
//!
//! ```text
//! GET {base}/doc?path=<percent-encoded full path>                  -> 200 object, or 404
//! GET {base}/members?collection=<c>&field=<f>&value=<v>&limit=<n>  -> 200 {"uids": [...]}
//! ```
//!
//! Every request is signed with HMAC-SHA256 over
//! `"GET\n<path as sent>\n<raw query as sent>\n<unix seconds>"`. The secret is
//! read from the environment and never sent, logged or reported.

use super::{
    percent_encode, validate_collection, validate_doc_path, validate_field, validate_limit,
    validate_value, DataError, DataSource, Doc,
};
use crate::identity::jwt::EnvSource;
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use reqwest::StatusCode;
use serde_json::Value;
use sha2::Sha256;
use std::sync::Arc;
use std::time::Duration;

const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const MIN_SECRET_BYTES: usize = 32;
const TIMESTAMP_HEADER: &str = "X-FCM-Timestamp";
const SIGNATURE_HEADER: &str = "X-FCM-Signature";
const SECRET_MISSING: &str = "data source secret is not configured";
const INVALID_DOCUMENT: &str = "resolver returned an invalid document";
const INVALID_MEMBERS: &str = "resolver returned an invalid members response";

pub struct HttpSourceConfig {
    pub base_url: String,
    pub secret_env: String,
    pub timeout_ms: u64,
}

pub struct HttpDataSource {
    client: reqwest::Client,
    cfg: HttpSourceConfig,
    env: Arc<dyn EnvSource>,
}

impl HttpDataSource {
    /// The caller supplies a client built with `redirect::Policy::none()`; the
    /// source also rejects a response that was redirected somewhere else.
    pub fn new(client: reqwest::Client, cfg: HttpSourceConfig, env: Arc<dyn EnvSource>) -> Self {
        Self { client, cfg, env }
    }

    async fn fetch(&self, resource: &str, query: &str) -> Result<Option<Vec<u8>>, DataError> {
        validate_base_url(&self.cfg.base_url)?;
        let base = self.cfg.base_url.trim_end_matches('/');
        let url = format!("{base}/{resource}?{query}");
        let path = url_path(&url).to_string();
        let timestamp = now_seconds();
        let canonical = canonical_request("GET", &path, query, timestamp);
        let secret = self.secret()?;
        let signature = sign(&secret, &canonical)?;

        let response = self
            .client
            .get(&url)
            .header(TIMESTAMP_HEADER, timestamp.to_string())
            .header(SIGNATURE_HEADER, signature)
            .timeout(Duration::from_millis(self.cfg.timeout_ms))
            .send()
            .await
            .map_err(|_| DataError::Unavailable("could not reach the resolver".to_string()))?;

        if response.url().path() != path {
            return Err(DataError::Unavailable(
                "the resolver redirected the request".to_string(),
            ));
        }

        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() {
            return Err(DataError::Unavailable(format!(
                "resolver returned HTTP {status}"
            )));
        }

        let body = response.bytes().await.map_err(|_| {
            DataError::Unavailable("resolver returned an invalid response".to_string())
        })?;
        if body.len() > MAX_RESPONSE_BYTES {
            return Err(DataError::Unavailable(
                "the resolver response is larger than 1MB".to_string(),
            ));
        }
        Ok(Some(body.to_vec()))
    }

    fn secret(&self) -> Result<Vec<u8>, DataError> {
        match self.env.get(&self.cfg.secret_env) {
            Some(secret) if secret.len() >= MIN_SECRET_BYTES => Ok(secret.into_bytes()),
            _ => {
                tracing::error!(
                    secret_env = %self.cfg.secret_env,
                    "the resolver secret is missing or shorter than 32 bytes"
                );
                Err(DataError::Unavailable(SECRET_MISSING.to_string()))
            }
        }
    }
}

#[async_trait]
impl DataSource for HttpDataSource {
    async fn get_doc(&self, path: &str) -> Result<Option<Doc>, DataError> {
        let segments = validate_doc_path(path)?;
        let query = format!("path={}", percent_encode(&segments.join("/")));

        let Some(body) = self.fetch("doc", &query).await? else {
            return Ok(None);
        };
        let document: Value = serde_json::from_slice(&body)
            .map_err(|_| DataError::Unavailable(INVALID_DOCUMENT.to_string()))?;
        if !document.is_object() {
            return Err(DataError::Unavailable(INVALID_DOCUMENT.to_string()));
        }
        Ok(Some(document))
    }

    async fn members(
        &self,
        collection: &str,
        field: &str,
        value: &str,
        limit: usize,
    ) -> Result<Vec<String>, DataError> {
        validate_collection(collection)?;
        validate_field(field)?;
        validate_value(value)?;
        validate_limit(limit)?;

        let query = format!(
            "collection={}&field={}&value={}&limit={}",
            percent_encode(collection),
            percent_encode(field),
            percent_encode(value),
            limit
        );

        let Some(body) = self.fetch("members", &query).await? else {
            return Err(DataError::Unavailable(
                "resolver returned 404 for a members query".to_string(),
            ));
        };
        let response: Value = serde_json::from_slice(&body)
            .map_err(|_| DataError::Unavailable(INVALID_MEMBERS.to_string()))?;
        let uids = response
            .get("uids")
            .and_then(Value::as_array)
            .ok_or_else(|| DataError::Unavailable(INVALID_MEMBERS.to_string()))?;

        let mut members = Vec::with_capacity(uids.len());
        for uid in uids {
            let uid = uid
                .as_str()
                .filter(|uid| {
                    !uid.is_empty()
                        && uid.chars().count() <= 128
                        && !uid.chars().any(char::is_control)
                })
                .ok_or_else(|| DataError::Unavailable(INVALID_MEMBERS.to_string()))?;
            members.push(uid.to_string());
        }
        if members.len() > limit {
            tracing::warn!(
                returned = members.len(),
                limit,
                "the resolver returned more uids than the limit; truncating"
            );
            members.truncate(limit);
        }
        Ok(members)
    }
}

/// `"GET" + "\n" + path + "\n" + query + "\n" + timestamp`, signed as sent.
fn canonical_request(method: &str, path: &str, query: &str, timestamp: u64) -> String {
    format!("{method}\n{path}\n{query}\n{timestamp}")
}

fn sign(secret: &[u8], canonical: &str) -> Result<String, DataError> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret)
        .map_err(|_| DataError::Unavailable("the resolver secret is not usable".to_string()))?;
    mac.update(canonical.as_bytes());
    Ok(hex(&mac.finalize().into_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push_str(&format!("{byte:02x}"));
    }
    encoded
}

fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn url_path(url: &str) -> &str {
    let rest = match url.split_once("://") {
        Some((_, rest)) => rest,
        None => url,
    };
    let Some(start) = rest.find('/') else {
        return "/";
    };
    let path = &rest[start..];
    match path.find('?') {
        Some(end) => &path[..end],
        None => path,
    }
}

fn validate_base_url(base_url: &str) -> Result<(), DataError> {
    let https = base_url
        .strip_prefix("https://")
        .is_some_and(|rest| !rest.is_empty());
    let localhost = ["http://localhost", "http://127.0.0.1"]
        .iter()
        .any(|prefix| match base_url.strip_prefix(prefix) {
            Some(rest) => rest.is_empty() || rest.starts_with(':') || rest.starts_with('/'),
            None => false,
        });
    if !(https || localhost) || base_url.contains('?') || base_url.contains('#') {
        return Err(DataError::Unavailable(
            "the resolver base_url must use https:// (http://localhost and http://127.0.0.1 are allowed for development)".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{header, json as json_response, request_line, TestServer};
    use super::*;

    const SECRET_ENV: &str = "CLIENT_RESOLVER_SECRET";
    const SECRET: &str = "resolver-test-secret-0123456789abcdef";

    struct TestEnv {
        values: Vec<(&'static str, String)>,
    }

    impl TestEnv {
        fn new(values: &[(&'static str, &str)]) -> Self {
            Self {
                values: values
                    .iter()
                    .map(|(name, value)| (*name, (*value).to_string()))
                    .collect(),
            }
        }
    }

    impl EnvSource for TestEnv {
        fn get(&self, name: &str) -> Option<String> {
            self.values
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.clone())
        }
    }

    fn client() -> reqwest::Client {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("the test client must build")
    }

    fn source(
        server: &TestServer,
        timeout_ms: u64,
        values: &[(&'static str, &str)],
    ) -> HttpDataSource {
        HttpDataSource::new(
            client(),
            HttpSourceConfig {
                base_url: format!("{}/internal/fcm", server.base_url),
                secret_env: SECRET_ENV.to_string(),
                timeout_ms,
            },
            Arc::new(TestEnv::new(values)),
        )
    }

    /// Independent HMAC-SHA256 helper: it does not call the production signer.
    fn hmac_hex(secret: &[u8], message: &str) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("the test key must be usable");
        mac.update(message.as_bytes());
        mac.finalize()
            .into_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    #[test]
    fn signature_matches_known_test_vector() {
        const CANONICAL: &str = "GET\n/internal/fcm/doc\npath=store_orders%2Fabc123\n1700000000";
        const SIGNATURE: &str = "f97974c3e1b1706c53e625f9f6b324adab12a0143af3348757cc311771f20813";

        assert_eq!(
            canonical_request(
                "GET",
                "/internal/fcm/doc",
                "path=store_orders%2Fabc123",
                1_700_000_000
            ),
            CANONICAL
        );
        assert_eq!(
            hmac_hex(b"test-secret-test-secret-test-secret-00", CANONICAL),
            SIGNATURE
        );
    }

    #[tokio::test]
    async fn http_get_doc_signs_request_and_parses_object() {
        let server = TestServer::start(vec![json_response(
            200,
            "OK",
            r#"{"buyer_id":"u1","order_number":42}"#,
        )])
        .await;
        let source = source(&server, 3000, &[(SECRET_ENV, SECRET)]);

        let document = source
            .get_doc("store_orders/abc123")
            .await
            .expect("the request must succeed")
            .expect("the document exists");
        assert_eq!(
            document,
            serde_json::json!({ "buyer_id": "u1", "order_number": 42 })
        );

        let raw = &server.requests()[0];
        let (method, path, query) = request_line(raw);
        assert_eq!(method, "GET");
        assert_eq!(path, "/internal/fcm/doc");
        assert_eq!(query, "path=store_orders%2Fabc123");

        let timestamp = header(raw, "x-fcm-timestamp").expect("the timestamp header must exist");
        let signature = header(raw, "x-fcm-signature").expect("the signature header must exist");
        assert_eq!(
            signature,
            hmac_hex(
                SECRET.as_bytes(),
                &format!("GET\n{path}\n{query}\n{timestamp}")
            )
        );
        assert_eq!(header(raw, "authorization"), None);
    }

    #[tokio::test]
    async fn http_get_doc_404_is_none() {
        let server = TestServer::start(vec![json_response(
            404,
            "Not Found",
            r#"{"error":"missing"}"#,
        )])
        .await;
        let source = source(&server, 3000, &[(SECRET_ENV, SECRET)]);

        assert_eq!(
            source
                .get_doc("store_orders/missing")
                .await
                .expect("a 404 is not an error"),
            None
        );
    }

    #[tokio::test]
    async fn http_members_404_is_unavailable() {
        let server = TestServer::start(vec![json_response(
            404,
            "Not Found",
            r#"{"error":"missing"}"#,
        )])
        .await;
        let source = source(&server, 3000, &[(SECRET_ENV, SECRET)]);

        match source.members("users", "role", "admin", 10).await {
            Err(DataError::Unavailable(message)) => {
                assert!(message.contains("404"), "{message}");
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn http_non_object_doc_is_unavailable() {
        let server = TestServer::start(vec![json_response(200, "OK", "[1, 2, 3]")]).await;
        let source = source(&server, 3000, &[(SECRET_ENV, SECRET)]);

        match source.get_doc("store_orders/abc123").await {
            Err(DataError::Unavailable(message)) => {
                assert_eq!(message, "resolver returned an invalid document");
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn http_members_truncates_to_limit() {
        let server = TestServer::start(vec![json_response(
            200,
            "OK",
            r#"{"uids":["u1","u2","u3"]}"#,
        )])
        .await;
        let source = source(&server, 3000, &[(SECRET_ENV, SECRET)]);

        let members = source
            .members("users", "role", "admin", 2)
            .await
            .expect("the query must succeed");
        assert_eq!(members, vec!["u1".to_string(), "u2".to_string()]);
    }

    #[tokio::test]
    async fn http_secret_missing_is_unavailable() {
        let server = TestServer::start(vec![json_response(200, "OK", "{}")]).await;
        let source = source(&server, 3000, &[]);

        match source.get_doc("store_orders/abc123").await {
            Err(DataError::Unavailable(message)) => {
                assert_eq!(message, "data source secret is not configured");
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
        assert!(server.requests().is_empty());
    }

    #[tokio::test]
    async fn http_short_secret_is_unavailable() {
        let server = TestServer::start(vec![json_response(200, "OK", "{}")]).await;
        let source = source(&server, 3000, &[(SECRET_ENV, "too-short")]);

        match source.get_doc("store_orders/abc123").await {
            Err(DataError::Unavailable(message)) => {
                assert_eq!(message, "data source secret is not configured");
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
        assert!(server.requests().is_empty());
    }

    #[tokio::test]
    async fn http_refuses_non_https_non_localhost_base_url() {
        let source = HttpDataSource::new(
            client(),
            HttpSourceConfig {
                base_url: "http://example.com/internal/fcm".to_string(),
                secret_env: SECRET_ENV.to_string(),
                timeout_ms: 3000,
            },
            Arc::new(TestEnv::new(&[(SECRET_ENV, SECRET)])),
        );

        match source.get_doc("store_orders/abc123").await {
            Err(DataError::Unavailable(message)) => {
                assert!(message.contains("https"), "{message}");
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }

        let lookalike = HttpDataSource::new(
            client(),
            HttpSourceConfig {
                base_url: "http://localhost.evil.com/internal/fcm".to_string(),
                secret_env: SECRET_ENV.to_string(),
                timeout_ms: 3000,
            },
            Arc::new(TestEnv::new(&[(SECRET_ENV, SECRET)])),
        );
        assert!(matches!(
            lookalike.get_doc("store_orders/abc123").await,
            Err(DataError::Unavailable(_))
        ));
    }

    #[tokio::test]
    async fn http_does_not_follow_redirects() {
        let target =
            TestServer::start(vec![json_response(200, "OK", r#"{"buyer_id":"u1"}"#)]).await;
        let location = format!("{}/steal", target.base_url);
        let redirect = super::super::test_support::response(302, "Found", "text/plain", "");
        let mut redirect_with_location = redirect;
        let marker = b"\r\n\r\n";
        let position = redirect_with_location
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("the response must have a header terminator");
        redirect_with_location.splice(
            position..position,
            format!("\r\nLocation: {location}").into_bytes(),
        );

        let server = TestServer::start(vec![redirect_with_location]).await;
        let source = source(&server, 3000, &[(SECRET_ENV, SECRET)]);

        match source.get_doc("store_orders/abc123").await {
            Err(DataError::Unavailable(message)) => {
                assert!(message.contains("302"), "{message}");
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
        assert!(target.requests().is_empty());
    }

    #[tokio::test]
    async fn http_timeout_is_unavailable() {
        let server = TestServer::start_with_delay(
            vec![json_response(200, "OK", "{}")],
            Duration::from_secs(2),
        )
        .await;
        let source = source(&server, 200, &[(SECRET_ENV, SECRET)]);

        match source.get_doc("store_orders/abc123").await {
            Err(DataError::Unavailable(message)) => {
                assert_eq!(message, "could not reach the resolver");
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn http_secret_never_appears_in_request() {
        let server = TestServer::start(vec![json_response(200, "OK", "{}")]).await;
        let source = source(&server, 3000, &[(SECRET_ENV, SECRET)]);

        source
            .get_doc("store_orders/abc123")
            .await
            .expect("the request must succeed");

        let raw = &server.requests()[0];
        assert!(!raw.contains(SECRET), "the secret must never be sent");
        assert!(raw.contains("x-fcm-signature:"), "{raw}");
    }
}

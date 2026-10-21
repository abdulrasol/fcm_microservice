//! Firestore data source (REST v1).
//!
//! Reads with the app's own service account in the `datastore` scope, exactly
//! like the dashboard read did before the data source interface existed, but
//! generalized to any document path and to a `runQuery` members lookup.

use super::{
    percent_encode, validate_collection, validate_doc_path, validate_field, validate_limit,
    validate_value, DataError, DataSource, Doc,
};
use crate::credentials::AuthMap;
use async_trait::async_trait;
use reqwest::StatusCode;
use serde_json::{json, Map, Value};
use std::sync::Arc;
use std::time::Duration;

const DEFAULT_BASE_URL: &str = "https://firestore.googleapis.com";
const FIRESTORE_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_DEPTH: usize = 8;
const DATASTORE_SCOPE: &str = "https://www.googleapis.com/auth/datastore";
const GUIDANCE: &str = "Firestore denied the request; grant the app service account a role that can read Firestore (for example roles/datastore.viewer) in the Firebase project";

/// Supplies an OAuth access token for Firestore.
#[async_trait]
pub trait AccessTokenProvider: Send + Sync {
    async fn token(&self) -> Result<String, String>;
}

/// Wraps the per-app authenticator that `credentials` already loads.
pub struct AuthMapTokenProvider {
    auth_map: AuthMap,
    app: String,
}

impl AuthMapTokenProvider {
    pub fn new(auth_map: AuthMap, app: impl Into<String>) -> Self {
        Self {
            auth_map,
            app: app.into(),
        }
    }
}

#[async_trait]
impl AccessTokenProvider for AuthMapTokenProvider {
    async fn token(&self) -> Result<String, String> {
        let auth = {
            let map = self.auth_map.read().await;
            map.get(&self.app).cloned()
        }
        .ok_or_else(|| format!("no service account is loaded for app '{}'", self.app))?;

        let token = auth.token(&[DATASTORE_SCOPE]).await.map_err(|_| {
            "could not obtain a service-account access token for Firestore".to_string()
        })?;

        token
            .token()
            .map(ToString::to_string)
            .ok_or_else(|| "service account returned an empty access token".to_string())
    }
}

pub struct FirestoreDataSource {
    client: reqwest::Client,
    tokens: Arc<dyn AccessTokenProvider>,
    project_id: String,
    base_url: String,
}

impl FirestoreDataSource {
    pub fn new(
        client: reqwest::Client,
        tokens: Arc<dyn AccessTokenProvider>,
        project_id: impl Into<String>,
    ) -> Self {
        Self {
            client,
            tokens,
            project_id: project_id.into(),
            base_url: DEFAULT_BASE_URL.to_string(),
        }
    }

    /// Points the source at another endpoint; tests use it with a local server.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    fn base(&self) -> &str {
        self.base_url.trim_end_matches('/')
    }

    fn documents_url(&self, segments: &[&str]) -> String {
        let encoded: Vec<String> = segments
            .iter()
            .map(|segment| percent_encode(segment))
            .collect();
        format!(
            "{}/v1/projects/{}/databases/(default)/documents/{}",
            self.base(),
            self.project_id,
            encoded.join("/")
        )
    }

    async fn access_token(&self) -> Result<String, DataError> {
        self.tokens.token().await.map_err(DataError::Unavailable)
    }

    fn check_status(status: StatusCode) -> Result<(), DataError> {
        if status.is_success() {
            return Ok(());
        }
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(DataError::Unavailable(GUIDANCE.to_string()));
        }
        Err(DataError::Unavailable(format!(
            "Firestore returned HTTP {status}"
        )))
    }
}

#[async_trait]
impl DataSource for FirestoreDataSource {
    async fn get_doc(&self, path: &str) -> Result<Option<Doc>, DataError> {
        let segments = validate_doc_path(path)?;
        let token = self.access_token().await?;

        let response = self
            .client
            .get(self.documents_url(&segments))
            .bearer_auth(token)
            .timeout(FIRESTORE_TIMEOUT)
            .send()
            .await
            .map_err(|_| DataError::Unavailable("could not reach Firestore".to_string()))?;

        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        Self::check_status(status)?;

        let body = response.bytes().await.map_err(|_| {
            DataError::Unavailable("Firestore returned an invalid response".to_string())
        })?;
        if body.len() > MAX_RESPONSE_BYTES {
            return Err(DataError::Unavailable(
                "Firestore response is larger than 1MB".to_string(),
            ));
        }
        let document: Value = serde_json::from_slice(&body).map_err(|_| {
            DataError::Unavailable("Firestore returned an invalid JSON document".to_string())
        })?;
        from_firestore_document(&document).map(Some)
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
        let token = self.access_token().await?;

        // `stringValue` only: a non-string role value is not matched, which is
        // documented as a current limitation.
        let query = json!({
            "structuredQuery": {
                "from": [{ "collectionId": collection }],
                "where": {
                    "fieldFilter": {
                        "field": { "fieldPath": field },
                        "op": "EQUAL",
                        "value": { "stringValue": value }
                    }
                },
                "select": { "fields": [{ "fieldPath": "__name__" }] },
                "limit": limit
            }
        });

        let url = format!(
            "{}/v1/projects/{}/databases/(default)/documents:runQuery",
            self.base(),
            self.project_id
        );
        let response = self
            .client
            .post(url)
            .bearer_auth(token)
            .timeout(FIRESTORE_TIMEOUT)
            .json(&query)
            .send()
            .await
            .map_err(|_| DataError::Unavailable("could not reach Firestore".to_string()))?;

        let status = response.status();
        Self::check_status(status)?;

        let body = response.bytes().await.map_err(|_| {
            DataError::Unavailable("Firestore returned an invalid response".to_string())
        })?;
        if body.len() > MAX_RESPONSE_BYTES {
            return Err(DataError::Unavailable(
                "Firestore response is larger than 1MB".to_string(),
            ));
        }
        let results: Value = serde_json::from_slice(&body).map_err(|_| {
            DataError::Unavailable("Firestore returned an invalid JSON response".to_string())
        })?;
        let entries = results.as_array().ok_or_else(|| {
            DataError::Unavailable("Firestore returned an invalid query response".to_string())
        })?;

        let mut uids = Vec::new();
        for entry in entries {
            let Some(document) = entry.get("document") else {
                continue;
            };
            let Some(name) = document.get("name").and_then(Value::as_str) else {
                continue;
            };
            if let Some(id) = name.rsplit('/').next().filter(|id| !id.is_empty()) {
                uids.push(id.to_string());
            }
        }
        if uids.len() > limit {
            tracing::warn!(
                returned = uids.len(),
                limit,
                "Firestore returned more members than the requested limit; truncating"
            );
            uids.truncate(limit);
        }
        Ok(uids)
    }
}

/// Converts a Firestore REST document (`{"fields": {...}}`) into plain JSON.
///
/// Unknown value types and nesting deeper than [`MAX_DEPTH`] are errors, so a
/// document is never silently truncated.
pub fn from_firestore_document(document: &Value) -> Result<Doc, DataError> {
    match document.get("fields") {
        None | Some(Value::Null) => Ok(Value::Object(Map::new())),
        Some(Value::Object(fields)) => convert_fields(fields, 0),
        Some(_) => Err(unsupported()),
    }
}

fn convert_fields(fields: &Map<String, Value>, depth: usize) -> Result<Value, DataError> {
    let mut converted = Map::with_capacity(fields.len());
    for (name, value) in fields {
        converted.insert(name.clone(), convert_value(value, depth)?);
    }
    Ok(Value::Object(converted))
}

fn convert_value(value: &Value, depth: usize) -> Result<Value, DataError> {
    let entry = value.as_object().ok_or_else(unsupported)?;
    let (tag, raw) = entry.iter().next().ok_or_else(unsupported)?;

    match (tag.as_str(), raw) {
        ("stringValue", Value::String(text))
        | ("timestampValue", Value::String(text))
        | ("referenceValue", Value::String(text))
        | ("bytesValue", Value::String(text)) => Ok(Value::String(text.clone())),
        ("integerValue", Value::String(text)) => match text.parse::<i64>() {
            Ok(number) => Ok(json!(number)),
            Err(_) => Ok(Value::String(text.clone())),
        },
        ("integerValue", Value::Number(number)) => Ok(Value::Number(number.clone())),
        ("doubleValue", Value::Number(number)) => Ok(Value::Number(number.clone())),
        ("booleanValue", Value::Bool(flag)) => Ok(Value::Bool(*flag)),
        ("nullValue", _) => Ok(Value::Null),
        ("geoPointValue", Value::Object(point)) => {
            let latitude = point.get("latitude").and_then(Value::as_f64);
            let longitude = point.get("longitude").and_then(Value::as_f64);
            match (latitude, longitude) {
                (Some(latitude), Some(longitude)) => {
                    Ok(json!({ "latitude": latitude, "longitude": longitude }))
                }
                _ => Err(unsupported()),
            }
        }
        ("mapValue", Value::Object(map)) => {
            if depth >= MAX_DEPTH {
                return Err(depth_error());
            }
            match map.get("fields") {
                None | Some(Value::Null) => Ok(Value::Object(Map::new())),
                Some(Value::Object(fields)) => convert_fields(fields, depth + 1),
                Some(_) => Err(unsupported()),
            }
        }
        ("arrayValue", Value::Object(array)) => {
            if depth >= MAX_DEPTH {
                return Err(depth_error());
            }
            match array.get("values") {
                None | Some(Value::Null) => Ok(Value::Array(Vec::new())),
                Some(Value::Array(values)) => {
                    let mut converted = Vec::with_capacity(values.len());
                    for item in values {
                        converted.push(convert_value(item, depth + 1)?);
                    }
                    Ok(Value::Array(converted))
                }
                Some(_) => Err(unsupported()),
            }
        }
        _ => Err(unsupported()),
    }
}

fn unsupported() -> DataError {
    DataError::Unavailable("unsupported Firestore value type".to_string())
}

fn depth_error() -> DataError {
    DataError::Unavailable("the Firestore document is nested deeper than 8 levels".to_string())
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{header, json, TestServer};
    use super::*;

    struct StaticToken(&'static str);

    #[async_trait]
    impl AccessTokenProvider for StaticToken {
        async fn token(&self) -> Result<String, String> {
            Ok(self.0.to_string())
        }
    }

    fn client() -> reqwest::Client {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("the test client must build")
    }

    fn source(server: &TestServer, project_id: &str) -> FirestoreDataSource {
        FirestoreDataSource::new(
            client(),
            Arc::new(StaticToken("test-access-token")),
            project_id,
        )
        .with_base_url(server.base_url.clone())
    }

    fn document(fields: Value) -> Value {
        json!({ "fields": fields })
    }

    #[test]
    fn firestore_value_conversion_all_types() {
        let fields = json!({
            "s": { "stringValue": "text" },
            "i": { "integerValue": "42" },
            "i_bad": { "integerValue": "not-a-number" },
            "d": { "doubleValue": 1.5 },
            "b": { "booleanValue": true },
            "n": { "nullValue": null },
            "ts": { "timestampValue": "2026-01-01T00:00:00Z" },
            "ref": { "referenceValue": "projects/p/databases/(default)/documents/users/u1" },
            "bytes": { "bytesValue": "aGVsbG8=" },
            "geo": { "geoPointValue": { "latitude": 24.7, "longitude": 46.7 } },
            "map": { "mapValue": { "fields": { "inner": { "stringValue": "v" } } } },
            "empty_map": { "mapValue": {} },
            "array": { "arrayValue": { "values": [{ "integerValue": "1" }, { "stringValue": "two" }] } },
            "empty_array": { "arrayValue": {} }
        });

        let converted =
            from_firestore_document(&document(fields)).expect("every supported type must convert");

        assert_eq!(
            converted,
            json!({
                "s": "text",
                "i": 42,
                "i_bad": "not-a-number",
                "d": 1.5,
                "b": true,
                "n": null,
                "ts": "2026-01-01T00:00:00Z",
                "ref": "projects/p/databases/(default)/documents/users/u1",
                "bytes": "aGVsbG8=",
                "geo": { "latitude": 24.7, "longitude": 46.7 },
                "map": { "inner": "v" },
                "empty_map": {},
                "array": [1, "two"],
                "empty_array": []
            })
        );
    }

    #[test]
    fn firestore_value_conversion_of_an_empty_document() {
        assert_eq!(
            from_firestore_document(
                &json!({ "name": "projects/p/databases/(default)/documents/users/u1" })
            )
            .expect("a document without fields is empty"),
            json!({})
        );
    }

    #[test]
    fn firestore_conversion_rejects_unknown_type() {
        let error = from_firestore_document(&document(json!({
            "x": { "unknownValue": { "anything": 1 } }
        })))
        .expect_err("an unknown type must fail closed");
        assert!(
            matches!(error, DataError::Unavailable(message) if message == "unsupported Firestore value type")
        );
    }

    fn nested_map(levels: usize) -> Value {
        let mut value = json!({ "stringValue": "leaf" });
        for _ in 0..levels {
            value = json!({ "mapValue": { "fields": { "child": value } } });
        }
        value
    }

    #[test]
    fn firestore_conversion_depth_limit_is_an_error() {
        from_firestore_document(&document(json!({ "root": nested_map(8) })))
            .expect("eight nested maps are allowed");

        let error = from_firestore_document(&document(json!({ "root": nested_map(9) })))
            .expect_err("nine nested maps must be rejected");
        assert!(
            matches!(error, DataError::Unavailable(message) if message.contains("deeper than 8"))
        );
    }

    #[tokio::test]
    async fn firestore_get_doc_found() {
        let server = TestServer::start(vec![json(
            200,
            "OK",
            r#"{"fields":{"buyer_id":{"stringValue":"u1"},"total":{"integerValue":"42"}}}"#,
        )])
        .await;
        let source = source(&server, "test-project");

        let document = source
            .get_doc("users/auth0|123")
            .await
            .expect("the request must succeed")
            .expect("the document exists");
        assert_eq!(document, json!({ "buyer_id": "u1", "total": 42 }));

        let raw = &server.requests()[0];
        let (method, path, _) = super::super::test_support::request_line(raw);
        assert_eq!(method, "GET");
        assert_eq!(
            path,
            "/v1/projects/test-project/databases/(default)/documents/users/auth0%7C123"
        );
        assert_eq!(
            header(raw, "authorization").as_deref(),
            Some("Bearer test-access-token")
        );
    }

    #[tokio::test]
    async fn firestore_get_doc_not_found_is_none() {
        let server = TestServer::start(vec![json(404, "Not Found", "{}")]).await;
        let source = source(&server, "test-project");

        assert_eq!(
            source
                .get_doc("users/missing")
                .await
                .expect("a 404 is not an error"),
            None
        );
    }

    #[tokio::test]
    async fn firestore_get_doc_403_is_unavailable_with_guidance() {
        let server = TestServer::start(vec![json(403, "Forbidden", "{}")]).await;
        let source = source(&server, "test-project");

        match source.get_doc("users/u1").await {
            Err(DataError::Unavailable(message)) => {
                assert!(message.contains("roles/datastore.viewer"), "{message}");
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn firestore_get_doc_500_is_unavailable() {
        let server = TestServer::start(vec![json(500, "Internal Server Error", "{}")]).await;
        let source = source(&server, "test-project");

        match source.get_doc("users/u1").await {
            Err(DataError::Unavailable(message)) => {
                assert!(message.contains("500"), "{message}");
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn firestore_response_over_1mb_is_unavailable() {
        let body = "a".repeat(MAX_RESPONSE_BYTES + 1);
        let server = TestServer::start(vec![json(200, "OK", &body)]).await;
        let source = source(&server, "test-project");

        match source.get_doc("users/u1").await {
            Err(DataError::Unavailable(message)) => {
                assert!(message.contains("larger than 1MB"), "{message}");
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn firestore_members_extracts_ids_and_skips_empty_results() {
        let body = r#"[
            {"document":{"name":"projects/p/databases/(default)/documents/users/u1"}},
            {"readTime":"2026-01-01T00:00:00Z"},
            {"document":{"name":"projects/p/databases/(default)/documents/users/u2"}}
        ]"#;
        let server = TestServer::start(vec![json(200, "OK", body)]).await;
        let source = source(&server, "test-project");

        let members = source
            .members("users", "role", "admin", 50)
            .await
            .expect("the query must succeed");
        assert_eq!(members, vec!["u1".to_string(), "u2".to_string()]);
    }

    #[tokio::test]
    async fn firestore_members_sends_expected_query() {
        let server = TestServer::start(vec![json(200, "OK", "[]")]).await;
        let source = source(&server, "test-project");

        source
            .members("users", "app_metadata.role", "admin", 50)
            .await
            .expect("the query must succeed");

        let raw = &server.requests()[0];
        let (method, path, _) = super::super::test_support::request_line(raw);
        assert_eq!(method, "POST");
        assert_eq!(
            path,
            "/v1/projects/test-project/databases/(default)/documents:runQuery"
        );
        for expected in [
            r#""collectionId":"users""#,
            r#""fieldPath":"app_metadata.role""#,
            r#""op":"EQUAL""#,
            r#""stringValue":"admin""#,
            r#""fieldPath":"__name__""#,
            r#""limit":50"#,
        ] {
            assert!(raw.contains(expected), "missing {expected} in {raw}");
        }
    }
}

//! FCM transport, message building and fan-out.
//!
//! The raw `/send` route and the template route share this module. The
//! [`FcmTransport`] trait keeps the network replaceable in tests; the
//! production implementation is [`HttpFcm`].

use crate::credentials::AuthMap;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

/// Scope requested from the service account.
const FCM_SCOPE: &str = "https://www.googleapis.com/auth/firebase.messaging";

/// Parallel FCM calls used by [`send_to_tokens`] when the caller passes `0`.
pub const DEFAULT_CONCURRENCY: usize = 10;

/// Why an FCM call could not produce an HTTP response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendError {
    /// No access token could be obtained for the app.
    Auth,
    /// The request to FCM failed (DNS, connection, TLS, timeout).
    Network,
}

/// The result of one FCM call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FcmOutcome {
    /// FCM accepted the message.
    Accepted { body: String },
    /// FCM rejected the message with an HTTP status.
    Rejected {
        http_status: u16,
        body: String,
        /// The rejected message targeted a device token that FCM says is gone.
        dead_token: bool,
    },
}

/// Where one message goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Destination {
    Token(String),
    Topic(String),
    Condition(String),
}

/// The FCM client: one HTTP call per message.
pub struct HttpFcm {
    client: reqwest::Client,
    auth_map: AuthMap,
}

impl HttpFcm {
    pub fn new(client: reqwest::Client, auth_map: AuthMap) -> Self {
        Self { client, auth_map }
    }
}

#[async_trait]
impl FcmTransport for HttpFcm {
    async fn send(
        &self,
        app: &str,
        project_id: &str,
        message: Value,
    ) -> Result<FcmOutcome, SendError> {
        let auth = {
            let map = self.auth_map.read().await;
            map.get(app).cloned()
        }
        .ok_or(SendError::Auth)?;

        let token = auth
            .token(&[FCM_SCOPE])
            .await
            .map_err(|_| SendError::Auth)?;
        let access_token = token.token().ok_or(SendError::Auth)?;

        let url = format!("https://fcm.googleapis.com/v1/projects/{project_id}/messages:send");
        let response = self
            .client
            .post(&url)
            .bearer_auth(access_token)
            .json(&message)
            .send()
            .await
            .map_err(|_| SendError::Network)?;

        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "Failed to read response body".to_string());

        if status.is_success() {
            Ok(FcmOutcome::Accepted { body })
        } else {
            // A dead token only makes sense for a token-targeted message.
            let token_targeted = message.pointer("/message/token").is_some();
            Ok(FcmOutcome::Rejected {
                http_status: status.as_u16(),
                dead_token: token_targeted && is_dead_token(&body),
                body,
            })
        }
    }
}

/// The transport seam: the raw route and the template route only need this.
#[async_trait]
pub trait FcmTransport: Send + Sync {
    async fn send(
        &self,
        app: &str,
        project_id: &str,
        message: Value,
    ) -> Result<FcmOutcome, SendError>;
}

/// Builds the FCM request body (`{"message": ...}`) for the raw route.
pub fn build_message(
    title: &str,
    body: Option<&str>,
    image: Option<&str>,
    data: &BTreeMap<String, String>,
    analytics_label: Option<&str>,
    dest: &Destination,
) -> Value {
    let mut message = json!({
        "notification": {
            "title": title,
        }
    });

    if let Some(body) = body {
        message["notification"]["body"] = json!(body);
    }
    if let Some(image) = image {
        message["notification"]["image"] = json!(image);
    }

    if !data.is_empty() {
        let mut string_map = serde_json::Map::new();
        for (key, value) in data {
            string_map.insert(key.clone(), Value::String(value.clone()));
        }
        message["data"] = Value::Object(string_map);
    }

    if let Some(label) = analytics_label {
        message["fcm_options"] = json!({ "analytics_label": label });
    }

    match dest {
        Destination::Token(token) => message["token"] = json!(token),
        Destination::Topic(topic) => message["topic"] = json!(topic),
        Destination::Condition(condition) => message["condition"] = json!(condition),
    }

    json!({ "message": message })
}

/// The narrow dead-token rule of T8 (plan 3.8): only an explicit `UNREGISTERED`
/// detail, or an `INVALID_ARGUMENT` that names `message.token`, removes a device.
fn is_dead_token(body: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return false;
    };
    let Some(error) = value.get("error") else {
        return false;
    };
    let details = error.get("details").and_then(Value::as_array);

    if details.is_some_and(|details| {
        details
            .iter()
            .any(|detail| detail.get("errorCode").and_then(Value::as_str) == Some("UNREGISTERED"))
    }) {
        return true;
    }

    if error.get("status").and_then(Value::as_str) != Some("INVALID_ARGUMENT") {
        return false;
    }

    details.is_some_and(|details| {
        details.iter().any(|detail| {
            detail
                .get("fieldViolations")
                .and_then(Value::as_array)
                .is_some_and(|violations| {
                    violations.iter().any(|violation| {
                        violation
                            .get("field")
                            .and_then(Value::as_str)
                            .is_some_and(|field| field.ends_with("message.token"))
                    })
                })
        })
    })
}

/// What a fan-out produced. Tokens are never logged.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Delivery {
    pub delivered: usize,
    pub failed: usize,
    pub dead_tokens: Vec<String>,
}

/// Sends one message per token with a bounded number of parallel calls.
///
/// `make` runs in the caller's task, so the closure does not cross tasks. A
/// panicked task counts as one failure. Nothing is retried.
pub async fn send_to_tokens(
    transport: &Arc<dyn FcmTransport>,
    app: &str,
    project_id: &str,
    tokens: &[String],
    make: impl Fn(&str) -> Value,
    concurrency: usize,
) -> Delivery {
    let limit = if concurrency == 0 {
        DEFAULT_CONCURRENCY
    } else {
        concurrency
    };
    let permits = Arc::new(Semaphore::new(limit));
    let mut tasks = JoinSet::new();

    for token in tokens {
        let permit_source = permits.clone();
        let transport = transport.clone();
        let app = app.to_string();
        let project_id = project_id.to_string();
        let message = make(token);
        let token = token.clone();
        tasks.spawn(async move {
            let _permit = permit_source.acquire().await;
            let outcome = transport.send(&app, &project_id, message).await;
            (token, outcome)
        });
    }

    let mut delivery = Delivery::default();
    while let Some(joined) = tasks.join_next().await {
        match joined {
            Ok((_, Ok(FcmOutcome::Accepted { .. }))) => delivery.delivered += 1,
            Ok((token, Ok(FcmOutcome::Rejected { dead_token, .. }))) => {
                delivery.failed += 1;
                if dead_token {
                    delivery.dead_tokens.push(token);
                }
            }
            Ok((_, Err(_))) | Err(_) => delivery.failed += 1,
        }
    }
    delivery
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use std::time::Duration;

    type Reply = Box<dyn Fn(&str, &Value) -> Result<FcmOutcome, SendError> + Send + Sync>;

    /// A programmable transport that records every message it receives.
    pub(crate) struct FakeTransport {
        reply: Reply,
        sent: Mutex<Vec<Value>>,
        in_flight: AtomicUsize,
        max_in_flight: AtomicUsize,
        delay: Duration,
    }

    impl FakeTransport {
        pub(crate) fn new(
            reply: impl Fn(&str, &Value) -> Result<FcmOutcome, SendError> + Send + Sync + 'static,
        ) -> Arc<Self> {
            Self::with_delay(reply, Duration::ZERO)
        }

        pub(crate) fn with_delay(
            reply: impl Fn(&str, &Value) -> Result<FcmOutcome, SendError> + Send + Sync + 'static,
            delay: Duration,
        ) -> Arc<Self> {
            Arc::new(Self {
                reply: Box::new(reply),
                sent: Mutex::new(Vec::new()),
                in_flight: AtomicUsize::new(0),
                max_in_flight: AtomicUsize::new(0),
                delay,
            })
        }

        pub(crate) fn accepting() -> Arc<Self> {
            Self::new(|_, _| {
                Ok(FcmOutcome::Accepted {
                    body: "{}".to_string(),
                })
            })
        }

        pub(crate) fn messages(&self) -> Vec<Value> {
            self.sent
                .lock()
                .map(|sent| sent.clone())
                .unwrap_or_default()
        }

        pub(crate) fn message_count(&self) -> usize {
            self.messages().len()
        }

        pub(crate) fn max_in_flight(&self) -> usize {
            self.max_in_flight.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl FcmTransport for FakeTransport {
        async fn send(
            &self,
            app: &str,
            _project_id: &str,
            message: Value,
        ) -> Result<FcmOutcome, SendError> {
            let current = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_in_flight.fetch_max(current, Ordering::SeqCst);
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            self.sent
                .lock()
                .map(|mut sent| sent.push(message.clone()))
                .ok();
            let outcome = (self.reply)(app, &message);
            self.in_flight.fetch_sub(1, Ordering::SeqCst);
            outcome
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::FakeTransport;
    use super::*;
    use std::time::Duration;

    #[test]
    fn build_message_matches_existing_shape() {
        let mut data = BTreeMap::new();
        data.insert("action".to_string(), "open_page".to_string());

        let message = build_message(
            "Title",
            Some("Body"),
            Some("https://example.com/i.png"),
            &data,
            Some("label-1"),
            &Destination::Topic("news".to_string()),
        );

        assert_eq!(
            message,
            json!({
                "message": {
                    "notification": {
                        "title": "Title",
                        "body": "Body",
                        "image": "https://example.com/i.png"
                    },
                    "data": { "action": "open_page" },
                    "fcm_options": { "analytics_label": "label-1" },
                    "topic": "news"
                }
            })
        );

        let token = build_message(
            "Title",
            None,
            None,
            &BTreeMap::new(),
            None,
            &Destination::Token("device-token".to_string()),
        );
        assert_eq!(token["message"]["token"], "device-token");
        assert!(token["message"]["notification"].get("body").is_none());

        let condition = build_message(
            "Title",
            None,
            None,
            &BTreeMap::new(),
            None,
            &Destination::Condition("'a' in topics".to_string()),
        );
        assert_eq!(condition["message"]["condition"], "'a' in topics");
    }

    #[test]
    fn dead_token_detects_unregistered_detail() {
        let body = r#"{
            "error": {
                "code": 404,
                "status": "NOT_FOUND",
                "details": [
                    {"@type": "type.googleapis.com/google.firebase.fcm.v1.FcmError",
                     "errorCode": "UNREGISTERED"}
                ]
            }
        }"#;
        assert!(is_dead_token(body));
    }

    #[test]
    fn dead_token_detects_invalid_argument_on_token_field() {
        let body = r#"{
            "error": {
                "status": "INVALID_ARGUMENT",
                "details": [
                    {"fieldViolations": [
                        {"field": "message.token", "description": "invalid"}
                    ]}
                ]
            }
        }"#;
        assert!(is_dead_token(body));
    }

    #[test]
    fn invalid_argument_on_payload_is_not_dead() {
        let payload = r#"{
            "error": {
                "status": "INVALID_ARGUMENT",
                "details": [
                    {"fieldViolations": [
                        {"field": "message.notification.title", "description": "invalid"}
                    ]}
                ]
            }
        }"#;
        assert!(!is_dead_token(payload));
        assert!(!is_dead_token(r#"{"error":{"status":"INVALID_ARGUMENT"}}"#));
        assert!(!is_dead_token("not json"));
        assert!(!is_dead_token(r#"{"name":"projects/p/messages/1"}"#));
    }

    #[tokio::test]
    async fn send_to_tokens_counts_delivered_failed_and_dead() {
        let transport = FakeTransport::new(|_, message| {
            let token = message
                .pointer("/message/token")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match token {
                "token-ok" => Ok(FcmOutcome::Accepted {
                    body: "{}".to_string(),
                }),
                "token-dead" => Ok(FcmOutcome::Rejected {
                    http_status: 404,
                    body: "{}".to_string(),
                    dead_token: true,
                }),
                _ => Ok(FcmOutcome::Rejected {
                    http_status: 400,
                    body: "{}".to_string(),
                    dead_token: false,
                }),
            }
        });

        let tokens = vec![
            "token-ok".to_string(),
            "token-dead".to_string(),
            "token-rejected".to_string(),
        ];
        let delivery = send_to_tokens(
            &(transport.clone() as Arc<dyn FcmTransport>),
            "app",
            "project",
            &tokens,
            |token| json!({"message": {"token": token}}),
            10,
        )
        .await;

        assert_eq!(delivery.delivered, 1);
        assert_eq!(delivery.failed, 2);
        assert_eq!(delivery.dead_tokens, vec!["token-dead".to_string()]);
        assert_eq!(transport.message_count(), 3);
    }

    #[tokio::test]
    async fn send_to_tokens_respects_concurrency_limit() {
        let transport = FakeTransport::with_delay(
            |_, _| {
                Ok(FcmOutcome::Accepted {
                    body: "{}".to_string(),
                })
            },
            Duration::from_millis(20),
        );
        let tokens: Vec<String> = (0..6).map(|index| format!("token-{index}")).collect();

        let delivery = send_to_tokens(
            &(transport.clone() as Arc<dyn FcmTransport>),
            "app",
            "project",
            &tokens,
            |token| json!({"message": {"token": token}}),
            2,
        )
        .await;

        assert_eq!(delivery.delivered, 6);
        assert_eq!(delivery.failed, 0);
        assert!(
            transport.max_in_flight() <= 2,
            "at most two calls may run at once, saw {}",
            transport.max_in_flight()
        );
    }

    #[tokio::test]
    async fn transport_errors_count_as_failed() {
        let transport = FakeTransport::new(|_, message| {
            let token = message
                .pointer("/message/token")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match token {
                "token-ok" => Ok(FcmOutcome::Accepted {
                    body: "{}".to_string(),
                }),
                "token-auth" => Err(SendError::Auth),
                "token-net" => Err(SendError::Network),
                _ => Ok(FcmOutcome::Rejected {
                    http_status: 400,
                    body: "{}".to_string(),
                    dead_token: false,
                }),
            }
        });

        let tokens = vec![
            "token-ok".to_string(),
            "token-auth".to_string(),
            "token-net".to_string(),
        ];
        let delivery = send_to_tokens(
            &(transport as Arc<dyn FcmTransport>),
            "app",
            "project",
            &tokens,
            |token| json!({"message": {"token": token}}),
            10,
        )
        .await;

        assert_eq!(delivery.delivered, 1);
        assert_eq!(delivery.failed, 2);
        assert!(delivery.dead_tokens.is_empty());
    }
}

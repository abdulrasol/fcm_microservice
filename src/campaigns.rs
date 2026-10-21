//! Company campaigns (plan 3.7, T9).
//!
//! A campaign is created from client input, the app configuration and an entity
//! document. Everything here fails closed: an unknown topic, a link, a foreign
//! image host or an action outside `content.actions` never reaches FCM.
//!
//! The audience is built from groups of topics, never from a client-supplied
//! condition. The stored condition is assembled only from quoted, validated
//! topic names, so a value in the title or in a topic cannot change it.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use regex::Regex;
use serde_json::{json, Value};

use crate::config::validate::{is_image_host, is_param_name, is_reserved_data_key};
use crate::config::{Audience, Campaigns, Quota};
use crate::datasource::DataError;
use crate::db::{self, NewCampaign};
use crate::send::{self, FcmOutcome};
use crate::template::{self, RenderError, Sender, Snippet, SnippetMode};
use crate::validation::{self, MAX_CONDITION_LENGTH, MAX_IMAGE_URL_LENGTH};
use crate::AppState;

/// Why a campaign request failed. The HTTP layer maps every variant.
#[derive(Debug)]
pub enum CampaignError {
    BadRequest(String),
    Forbidden,
    Unavailable,
    NotFound,
    Conflict,
    QuotaExceeded { retry_after_seconds: i64 },
    Internal,
}

fn bad(message: impl Into<String>) -> CampaignError {
    CampaignError::BadRequest(message.into())
}

pub(crate) fn render_error(error: RenderError) -> CampaignError {
    match error {
        RenderError::BadRequest(message) => CampaignError::BadRequest(message),
        RenderError::Forbidden => CampaignError::Forbidden,
    }
}

pub(crate) fn storage_error(error: sqlx::Error) -> CampaignError {
    // The SQL error never contains campaign content or a token.
    tracing::error!(error = %error, "campaign storage failed");
    CampaignError::Internal
}

/// `^[A-Za-z0-9_-]{1,64}$`, the entity identifier used in document paths.
pub fn is_entity_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

// ---------- audience ----------

/// Builds the literal FCM condition from groups of topics.
///
/// The output contains only the required topic and quoted topic names; the
/// client never supplies the condition itself.
pub fn build_condition(
    groups: &[Vec<String>],
    cfg: &Audience,
    entity_id: &str,
) -> Result<String, CampaignError> {
    // Size limits come first, before any per-topic work, to bound memory use.
    if groups.is_empty() {
        return Err(bad("audience.groups must not be empty"));
    }
    if groups.len() > 5 {
        return Err(bad("audience.groups must have at most 5 groups"));
    }
    for group in groups {
        if group.is_empty() {
            return Err(bad("an audience group must not be empty"));
        }
        if group.len() > 10 {
            return Err(bad("an audience group must have at most 10 topics"));
        }
    }

    let entity_ok = is_entity_id(entity_id);
    let mut cleaned: Vec<Vec<String>> = Vec::new();
    let mut distinct: BTreeSet<String> = BTreeSet::new();

    for group in groups {
        let mut kept = Vec::new();
        for topic in group {
            // The same character rule as `/send`; this also rejects quotes and
            // anything that could break out of the condition grammar.
            if validation::validate_topic_name(topic).is_err() {
                return Err(bad("topic not allowed"));
            }
            // The required topic is implied; a duplicate is ignored, not counted.
            if *topic == cfg.required_topic {
                continue;
            }
            if kept.contains(topic) {
                continue;
            }
            if !topic_allowed(topic, &cfg.allowed_topics, entity_id, entity_ok) {
                return Err(bad("topic not allowed"));
            }
            distinct.insert(topic.clone());
            kept.push(topic.clone());
        }
        if !kept.is_empty() {
            cleaned.push(kept);
        }
    }

    if 1 + distinct.len() > cfg.max_topics as usize {
        return Err(bad("too many topics"));
    }

    let mut parts = vec![format!("'{}' in topics", cfg.required_topic)];
    for group in &cleaned {
        let alternatives = group
            .iter()
            .map(|topic| format!("'{topic}' in topics"))
            .collect::<Vec<_>>()
            .join(" || ");
        parts.push(format!("({alternatives})"));
    }
    let condition = parts.join(" && ");

    if condition.chars().count() > MAX_CONDITION_LENGTH {
        return Err(bad("the audience condition is too long"));
    }
    validation::validate_condition(&condition)
        .map_err(|_| bad("the audience condition is not valid"))?;
    Ok(condition)
}

fn topic_allowed(topic: &str, patterns: &[String], entity_id: &str, entity_ok: bool) -> bool {
    patterns.iter().any(|pattern| {
        if let Some(expanded) = expand_entity_pattern(pattern, entity_id) {
            return entity_ok && expanded == topic;
        }
        match pattern.strip_suffix('*') {
            Some(prefix) => {
                topic.starts_with(prefix) && {
                    let suffix = &topic[prefix.len()..];
                    !suffix.is_empty()
                        && suffix
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                }
            }
            None => pattern == topic,
        }
    })
}

/// Expands the single `{p.<name>}` placeholder allowed in a topic pattern.
fn expand_entity_pattern(pattern: &str, entity_id: &str) -> Option<String> {
    let start = pattern.find('{')?;
    let end = start + pattern[start..].find('}')?;
    let name = pattern[start + 1..end].strip_prefix("p.")?;
    if !is_param_name(name) {
        return None;
    }
    Some(format!(
        "{}{}{}",
        &pattern[..start],
        entity_id,
        &pattern[end + 1..]
    ))
}

// ---------- content ----------

/// The client-supplied content of a campaign.
pub struct ContentInput<'a> {
    pub title: &'a str,
    pub body: &'a str,
    pub image: Option<&'a str>,
    pub action: &'a str,
    pub action_params: Option<&'a Value>,
}

/// Content that passed every campaign rule and is ready for `/send` validation.
pub struct PreparedContent {
    pub title: String,
    pub body: String,
    pub image: Option<String>,
    pub data: BTreeMap<String, String>,
    /// A rendered `check.doc` path and its unrendered `require` map.
    pub check: Option<(String, BTreeMap<String, String>)>,
    /// The parameters used for rendering, needed by the caller for `require`.
    pub params: BTreeMap<String, String>,
}

/// Applies the content rules of plan 3.7 to one campaign.
///
/// Pure: the action `check` document is read by the caller, so this function
/// never touches the network or the database.
pub fn prepare_content(
    campaigns: &Campaigns,
    entity: &Value,
    sender: &Sender,
    entity_id: &str,
    input: &ContentInput<'_>,
) -> Result<PreparedContent, CampaignError> {
    let content = &campaigns.content;

    validate_text(input.title, content.title_max, false, "title")?;
    validate_text(input.body, content.body_max, true, "body")?;

    // Link detection is best effort (a comment in `contains_link` explains why);
    // the real protection is review, quota and the action allow-list.
    if content.forbid_links && (contains_link(input.title) || contains_link(input.body)) {
        return Err(bad("links are not allowed"));
    }

    if let Some(image) = input.image {
        validate_image(image, &content.image_hosts)?;
    }

    let action = content
        .actions
        .get(input.action)
        .ok_or_else(|| bad("action is not configured"))?;

    let action_params =
        template::validate_param_map(&action.params, input.action_params.unwrap_or(&Value::Null))
            .map_err(render_error)?;

    let mut params = action_params;
    params.insert(campaigns.entity.param.clone(), entity_id.to_string());
    let snippet = Snippet {
        params: &params,
        sender,
        doc: None,
        entity: Some(entity),
    };

    let prefix = template::render_snippet(&content.title_prefix, &snippet, SnippetMode::Text)
        .map_err(render_error)?;
    let title = format!("{prefix}{}", input.title);

    let mut data = BTreeMap::new();
    for (key, definition) in &action.data {
        if is_reserved_data_key(key) {
            return Err(bad(format!("data key '{key}' is reserved")));
        }
        let value = template::render_snippet(definition, &snippet, SnippetMode::Text)
            .map_err(render_error)?;
        data.insert(key.clone(), value);
    }

    let check = match &action.check {
        Some(check) => {
            let path = template::render_snippet(&check.doc, &snippet, SnippetMode::Path)
                .map_err(render_error)?;
            Some((path, check.require.clone()))
        }
        None => None,
    };

    Ok(PreparedContent {
        title,
        body: input.body.to_string(),
        image: input.image.map(str::to_string),
        data,
        check,
        params,
    })
}

fn validate_text(
    value: &str,
    max: u32,
    allow_newline: bool,
    label: &str,
) -> Result<(), CampaignError> {
    if value.trim().is_empty() {
        return Err(bad(format!("{label} must not be empty")));
    }
    if value.chars().count() > max as usize {
        return Err(bad(format!("{label} must be at most {max} characters")));
    }
    if value
        .chars()
        .any(|character| character.is_control() && !(allow_newline && character == '\n'))
    {
        return Err(bad(format!(
            "{label} contains an unsupported control character"
        )));
    }
    Ok(())
}

/// Best-effort link detection for campaign text.
///
/// This is a configured product rule, not a security boundary: a determined
/// sender can write a link that the expression misses. The plan says so
/// explicitly; review and quota are what limit abuse. The regex covers the
/// scheme separator, `www.`, common TLDs, the short-link hosts and e-mail-like
/// values, case-insensitively.
fn contains_link(text: &str) -> bool {
    static LINKS: OnceLock<Option<Regex>> = OnceLock::new();
    let pattern = r"(?i)://|www\.|t\.me/|wa\.me/|bit\.ly|@\S+\.\S+|\b[\p{L}\p{N}-]+\.(?:com|net|org|io|app|co|me|ly|info|biz|xyz|iq|ae|sa|eg|tk|ml|ga|cf|gq|top|site|online|link|click|shop|store)\b";
    // If the constant ever stops compiling, fail closed rather than allow text.
    match LINKS.get_or_init(|| Regex::new(pattern).ok()) {
        Some(regex) => regex.is_match(text),
        None => true,
    }
}

/// The image host must equal an entry of `image_hosts` exactly.
///
/// The host is the text between `https://` and the first `/`, `?` or `#`.
/// `is_image_host` rejects `@`, `\`, spaces, ports and uppercase letters; an
/// empty `image_hosts` rejects every image.
fn validate_image(image: &str, hosts: &[String]) -> Result<(), CampaignError> {
    if hosts.is_empty() {
        return Err(bad("image host is not allowed"));
    }
    if image.len() > MAX_IMAGE_URL_LENGTH {
        return Err(bad("image URL is too long"));
    }
    let rest = match image.strip_prefix("https://") {
        Some(rest) => rest,
        None => return Err(bad("image must use https")),
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = &rest[..end];
    if !is_image_host(host) || !hosts.iter().any(|allowed| allowed == host) {
        return Err(bad("image host is not allowed"));
    }
    Ok(())
}

// ---------- entity and action checks ----------

/// Resolves the entity document and applies `entity.require`.
///
/// A missing document and a failed requirement are the same uniform
/// [`CampaignError::Forbidden`]. The trusted service sender skips only the
/// requirement fields computed from `{sender.*}`; a missing entity is still an
/// error because the entity name is needed in the title.
pub async fn resolve_entity(
    state: &AppState,
    app: &str,
    campaigns: &Campaigns,
    sender: &Sender,
    is_service: bool,
    entity_id: &str,
) -> Result<Value, CampaignError> {
    if !is_entity_id(entity_id) {
        return Err(bad("entity_id must match ^[A-Za-z0-9_-]{1,64}$"));
    }

    let mut params = BTreeMap::new();
    params.insert(campaigns.entity.param.clone(), entity_id.to_string());
    let snippet = Snippet {
        params: &params,
        sender,
        doc: None,
        entity: None,
    };
    let path = template::render_snippet(&campaigns.entity.doc, &snippet, SnippetMode::Path)
        .map_err(render_error)?;

    let source = state
        .datasource
        .source_for(app)
        .await
        .map_err(|_| CampaignError::Unavailable)?;
    let doc = match source.get_doc(&path).await {
        Ok(Some(doc)) => doc,
        Ok(None) => return Err(CampaignError::Forbidden),
        Err(DataError::Unavailable(_)) => return Err(CampaignError::Unavailable),
        Err(DataError::InvalidPath(message)) => return Err(CampaignError::BadRequest(message)),
    };

    let require: BTreeMap<String, String> = if is_service {
        campaigns
            .entity
            .require
            .iter()
            .filter(|(_, value)| !value.contains("{sender."))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    } else {
        campaigns.entity.require.clone()
    };
    let check = Snippet {
        params: &params,
        sender,
        doc: Some(&doc),
        entity: None,
    };
    template::check_require_map(&require, &check, Some(&doc)).map_err(render_error)?;
    Ok(doc)
}

/// Reads and checks an action's `check.doc`, when the action has one.
pub async fn check_action(
    state: &AppState,
    app: &str,
    sender: &Sender,
    entity: &Value,
    prepared: &PreparedContent,
) -> Result<(), CampaignError> {
    let (path, require) = match &prepared.check {
        Some(check) => check,
        None => return Ok(()),
    };

    let source = state
        .datasource
        .source_for(app)
        .await
        .map_err(|_| CampaignError::Unavailable)?;
    let doc = match source.get_doc(path).await {
        Ok(Some(doc)) => doc,
        Ok(None) => return Err(CampaignError::Forbidden),
        Err(DataError::Unavailable(_)) => return Err(CampaignError::Unavailable),
        Err(DataError::InvalidPath(message)) => return Err(CampaignError::BadRequest(message)),
    };

    let snippet = Snippet {
        params: &prepared.params,
        sender,
        doc: Some(&doc),
        entity: Some(entity),
    };
    template::check_require_map(require, &snippet, Some(&doc)).map_err(render_error)
}

/// Runs the quota check and the insert, mapping storage errors for the route.
pub async fn insert_campaign(
    state: &AppState,
    new: &NewCampaign,
    limits: &Quota,
    now: i64,
) -> Result<db::Inserted, CampaignError> {
    db::insert_campaign_checked(&state.db, new, limits, now)
        .await
        .map_err(|error| match error {
            db::InsertError::QuotaExceeded {
                retry_after_seconds,
            } => CampaignError::QuotaExceeded {
                retry_after_seconds,
            },
            db::InsertError::Database(error) => storage_error(error),
        })
}

// ---------- delivery ----------

/// The final status of a dispatched campaign.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CampaignOutcome {
    Sent,
    Failed,
}

impl CampaignOutcome {
    pub fn status(self) -> &'static str {
        match self {
            CampaignOutcome::Sent => "sent",
            CampaignOutcome::Failed => "failed",
        }
    }
}

/// Sends one campaign that is already in `sending` (T9 and, later, T10).
///
/// A failed FCM call marks the campaign `failed`; there is no automatic retry.
/// The history row contains the rendered content and the condition target, never
/// a token.
pub async fn dispatch_campaign(
    state: &AppState,
    id: i64,
) -> Result<CampaignOutcome, CampaignError> {
    let campaign = db::get_campaign(&state.db, id)
        .await
        .map_err(storage_error)?
        .ok_or(CampaignError::NotFound)?;
    if campaign.status != "sending" {
        return Err(CampaignError::Conflict);
    }

    let project_id = {
        let projects = state.project_map.read().await;
        projects.get(&campaign.app).cloned()
    }
    .ok_or(CampaignError::Internal)?;

    let data: BTreeMap<String, String> =
        serde_json::from_str(&campaign.data).map_err(|_| CampaignError::Internal)?;

    let message = send::build_message(
        &campaign.title,
        Some(&campaign.body),
        campaign.image.as_deref(),
        &data,
        None,
        &send::Destination::Condition(campaign.condition.clone()),
    );

    let (outcome, response) = match state.fcm.send(&campaign.app, &project_id, message).await {
        Ok(FcmOutcome::Accepted { .. }) => (CampaignOutcome::Sent, "accepted".to_string()),
        Ok(FcmOutcome::Rejected { http_status, .. }) => {
            (CampaignOutcome::Failed, format!("http {http_status}"))
        }
        Err(_) => (CampaignOutcome::Failed, "transport error".to_string()),
    };

    db::finish_campaign(&state.db, id, outcome.status(), &response)
        .await
        .map_err(storage_error)?;

    let payload = json!({
        "title": campaign.title,
        "body": campaign.body,
        "data": data,
    })
    .to_string();
    let _ = db::log_notification_ext(
        &state.db,
        &campaign.app,
        "condition",
        &campaign.title,
        if outcome == CampaignOutcome::Sent {
            "success"
        } else {
            "failed"
        },
        &response,
        &payload,
        &campaign.sender_uid,
        None,
        "campaign",
        Some(&id.to_string()),
    )
    .await;

    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Param;

    fn campaigns_config() -> Campaigns {
        serde_json::from_value(json!({
            "enabled": true,
            "senders": ["company"],
            "entity": {
                "param": "company_id",
                "doc": "companies/{p.company_id}",
                "require": { "user_id": "{sender.uid}", "status": "approved" },
                "name_field": "name"
            },
            "audience": {
                "allowed_topics": [
                    "country_*", "lang_*", "users", "technicals", "cat_*",
                    "company_{p.company_id}"
                ],
                "required_topic": "promos",
                "max_topics": 5
            },
            "content": {
                "title_prefix": "{entity.name}: ",
                "title_max": 60,
                "body_max": 240,
                "forbid_links": true,
                "image_hosts": ["firebasestorage.googleapis.com"],
                "actions": {
                    "open_company": {
                        "data": {
                            "action": "open_company",
                            "company_id": "{p.company_id}"
                        }
                    },
                    "open_product": {
                        "params": {
                            "product_id": {
                                "type": "pattern",
                                "pattern": "^[A-Za-z0-9_-]{1,64}$"
                            }
                        },
                        "check": {
                            "doc": "store_products/{p.product_id}",
                            "require": {
                                "owner_id": "{sender.uid}",
                                "status": "approved"
                            }
                        },
                        "data": {
                            "action": "open_product",
                            "product_id": "{p.product_id}"
                        }
                    }
                }
            },
            "quota": { "per_entity_per_day": 1, "per_entity_per_week": 5 },
            "review": { "mode": "first_n", "n": 3 }
        }))
        .expect("the campaign configuration must parse")
    }

    fn audience() -> Audience {
        campaigns_config().audience
    }

    fn sender() -> Sender {
        Sender {
            uid: "company-1".to_string(),
            name: Some("Owner".to_string()),
            role: Some("company".to_string()),
        }
    }

    fn entity() -> Value {
        json!({
            "name": "Acme",
            "user_id": "company-1",
            "status": "approved"
        })
    }

    fn input() -> ContentInput<'static> {
        ContentInput {
            title: "Sale",
            body: "Body",
            image: None,
            action: "open_company",
            action_params: None,
        }
    }

    fn assert_bad(result: Result<String, CampaignError>) {
        match result {
            Err(CampaignError::BadRequest(_)) => {}
            other => panic!("expected a bad request, got {other:?}"),
        }
    }

    // ---------- audience ----------

    #[test]
    fn condition_matches_expected_text_literally() {
        let groups = vec![
            vec!["country_iq".to_string()],
            vec!["technicals".to_string(), "users".to_string()],
            vec!["lang_ar".to_string()],
        ];

        let condition =
            build_condition(&groups, &audience(), "company-1").expect("the audience must build");

        assert_eq!(
            condition,
            "'promos' in topics && ('country_iq' in topics) && ('technicals' in topics || 'users' in topics) && ('lang_ar' in topics)"
        );
    }

    #[test]
    fn condition_dedups_and_orders() {
        let groups = vec![vec![
            "users".to_string(),
            "users".to_string(),
            "technicals".to_string(),
        ]];

        let condition =
            build_condition(&groups, &audience(), "company-1").expect("the audience must build");

        assert_eq!(
            condition,
            "'promos' in topics && ('users' in topics || 'technicals' in topics)"
        );
    }

    #[test]
    fn condition_rejects_topic_outside_allowed() {
        match build_condition(&[vec!["spam".to_string()]], &audience(), "company-1") {
            Err(CampaignError::BadRequest(message)) => {
                assert_eq!(message, "topic not allowed");
            }
            other => panic!("expected a bad request, got {other:?}"),
        }
    }

    #[test]
    fn condition_rejects_admins_and_privileged_topics() {
        for topic in ["admins", "all"] {
            assert_bad(build_condition(
                &[vec![topic.to_string()]],
                &audience(),
                "company-1",
            ));
        }
    }

    #[test]
    fn condition_enforces_max_topics_including_required() {
        assert_bad(build_condition(
            &[vec![
                "country_iq".to_string(),
                "users".to_string(),
                "technicals".to_string(),
                "lang_ar".to_string(),
                "cat_x".to_string(),
            ]],
            &audience(),
            "company-1",
        ));
    }

    #[test]
    fn condition_pattern_star_requires_non_empty_suffix() {
        let audience = Audience {
            allowed_topics: vec!["news_*".to_string()],
            required_topic: "promos".to_string(),
            max_topics: 5,
        };

        assert_bad(build_condition(
            &[vec!["news_".to_string()]],
            &audience,
            "company-1",
        ));
        assert_eq!(
            build_condition(&[vec!["news_x".to_string()]], &audience, "company-1")
                .expect("the suffix is valid"),
            "'promos' in topics && ('news_x' in topics)"
        );
    }

    #[test]
    fn condition_entity_placeholder_topic_matches_only_own_entity() {
        let audience = Audience {
            allowed_topics: vec!["company_{p.company_id}".to_string()],
            required_topic: "promos".to_string(),
            max_topics: 5,
        };

        assert_eq!(
            build_condition(&[vec!["company_acme".to_string()]], &audience, "acme")
                .expect("the own entity must match"),
            "'promos' in topics && ('company_acme' in topics)"
        );
        assert_bad(build_condition(
            &[vec!["company_other".to_string()]],
            &audience,
            "acme",
        ));
    }

    #[test]
    fn condition_rejects_bad_entity_id_for_placeholder_topic() {
        let audience = Audience {
            allowed_topics: vec!["company_{p.company_id}".to_string()],
            required_topic: "promos".to_string(),
            max_topics: 5,
        };
        let entity_id = "a".repeat(65);
        let topic = format!("company_{entity_id}");

        assert_bad(build_condition(&[vec![topic]], &audience, &entity_id));
    }

    #[test]
    fn condition_empty_groups_and_oversized_groups_rejected() {
        assert_bad(build_condition(&[], &audience(), "company-1"));
        assert_bad(build_condition(
            &[vec![], vec![], vec![], vec![], vec![], vec![]],
            &audience(),
            "company-1",
        ));
        assert_bad(build_condition(
            &[vec![
                "users".to_string(),
                "users".to_string(),
                "users".to_string(),
                "users".to_string(),
                "users".to_string(),
                "users".to_string(),
                "users".to_string(),
                "users".to_string(),
                "users".to_string(),
                "users".to_string(),
                "users".to_string(),
            ]],
            &audience(),
            "company-1",
        ));
    }

    #[test]
    fn condition_never_contains_unquoted_input() {
        for payload in ["x' in topics || 'admins", "a') || ('b"] {
            assert_bad(build_condition(
                &[vec![payload.to_string()]],
                &audience(),
                "company-1",
            ));
        }

        let condition = build_condition(&[vec!["users".to_string()]], &audience(), "company-1")
            .expect("the audience must build");
        assert_eq!(condition, "'promos' in topics && ('users' in topics)");
    }

    // ---------- content ----------

    #[test]
    fn title_prefix_comes_from_entity_name() {
        let prepared = prepare_content(
            &campaigns_config(),
            &entity(),
            &sender(),
            "company-1",
            &input(),
        )
        .expect("the content must pass");

        assert_eq!(prepared.title, "Acme: Sale");
        assert_eq!(
            prepared.data.get("company_id").map(String::as_str),
            Some("company-1")
        );
    }

    #[test]
    fn title_and_body_limits_enforced() {
        let mut config = campaigns_config();
        config.content.title_max = 5;
        config.content.body_max = 4;

        let long_title = ContentInput {
            title: "123456",
            ..input()
        };
        assert!(prepare_content(&config, &entity(), &sender(), "company-1", &long_title).is_err());

        let long_body = ContentInput {
            body: "12345",
            ..input()
        };
        assert!(prepare_content(&config, &entity(), &sender(), "company-1", &long_body).is_err());

        let blank_title = ContentInput {
            title: "   ",
            ..input()
        };
        assert!(prepare_content(&config, &entity(), &sender(), "company-1", &blank_title).is_err());

        let fits = ContentInput {
            title: "12345",
            body: "1234",
            ..input()
        };
        assert!(prepare_content(&config, &entity(), &sender(), "company-1", &fits).is_ok());
    }

    #[test]
    fn links_rejected_in_title_and_body() {
        let config = campaigns_config();
        for title in [
            "see https://example.com",
            "www.example.com",
            "t.me/channel",
            "wa.me/964",
            "bit.ly/abc",
            "write to a@b.com",
        ] {
            let content = ContentInput { title, ..input() };
            assert!(
                prepare_content(&config, &entity(), &sender(), "company-1", &content).is_err(),
                "title '{title}' must be rejected"
            );
        }

        for body in [
            "see https://example.com",
            "www.example.com",
            "write to a@b.com",
        ] {
            let content = ContentInput { body, ..input() };
            assert!(
                prepare_content(&config, &entity(), &sender(), "company-1", &content).is_err(),
                "body '{body}' must be rejected"
            );
        }
    }

    #[test]
    fn links_rejected_case_insensitive_and_dotted_domains() {
        let config = campaigns_config();
        for body in [
            "Visit EXAMPLE.COM",
            "a.b.Example.Co",
            "sub.domain.biz",
            "WWW.Example.Com",
        ] {
            let content = ContentInput { body, ..input() };
            assert!(
                prepare_content(&config, &entity(), &sender(), "company-1", &content).is_err(),
                "body '{body}' must be rejected"
            );
        }
    }

    #[test]
    fn arabic_text_without_links_accepted() {
        let config = campaigns_config();
        let content = ContentInput {
            title: "عرض خاص",
            body: "خصم اليوم",
            ..input()
        };

        let prepared = prepare_content(&config, &entity(), &sender(), "company-1", &content)
            .expect("Arabic text without links must pass");
        assert_eq!(prepared.title, "Acme: عرض خاص");
        assert_eq!(prepared.body, "خصم اليوم");
    }

    #[test]
    fn image_exact_host_only() {
        let config = campaigns_config();

        let exact = ContentInput {
            image: Some("https://firebasestorage.googleapis.com/v0/b/x"),
            ..input()
        };
        assert!(prepare_content(&config, &entity(), &sender(), "company-1", &exact).is_ok());

        let external = ContentInput {
            image: Some("https://evil.example/x.png"),
            ..input()
        };
        assert!(prepare_content(&config, &entity(), &sender(), "company-1", &external).is_err());
    }

    #[test]
    fn image_rejects_userinfo_port_backslash_and_subdomain() {
        let config = campaigns_config();
        for image in [
            "https://user@firebasestorage.googleapis.com/x",
            "https://firebasestorage.googleapis.com:443/x",
            "https://firebasestorage.googleapis.com\\x",
            "https://a.firebasestorage.googleapis.com/x",
        ] {
            let content = ContentInput {
                image: Some(image),
                ..input()
            };
            assert!(
                prepare_content(&config, &entity(), &sender(), "company-1", &content).is_err(),
                "image '{image}' must be rejected"
            );
        }
    }

    #[test]
    fn image_rejected_when_no_hosts_configured() {
        let mut config = campaigns_config();
        config.content.image_hosts.clear();
        let content = ContentInput {
            image: Some("https://firebasestorage.googleapis.com/x"),
            ..input()
        };

        assert!(prepare_content(&config, &entity(), &sender(), "company-1", &content).is_err());
    }

    #[test]
    fn action_must_be_configured() {
        let content = ContentInput {
            action: "not_configured",
            ..input()
        };

        assert!(prepare_content(
            &campaigns_config(),
            &entity(),
            &sender(),
            "company-1",
            &content
        )
        .is_err());
    }

    #[test]
    fn action_params_validated() {
        let config = campaigns_config();
        let product = ContentInput {
            action: "open_product",
            ..input()
        };
        assert!(
            prepare_content(&config, &entity(), &sender(), "company-1", &product).is_err(),
            "a missing parameter must fail"
        );

        let values = json!({ "product_id": "p1" });
        let valid = ContentInput {
            action: "open_product",
            action_params: Some(&values),
            ..input()
        };
        let prepared = prepare_content(&config, &entity(), &sender(), "company-1", &valid)
            .expect("a valid parameter must pass");
        assert_eq!(
            prepared.data.get("product_id").map(String::as_str),
            Some("p1")
        );
        assert_eq!(
            prepared.check.as_ref().map(|(path, _)| path.as_str()),
            Some("store_products/p1")
        );

        let unknown = json!({ "product_id": "p1", "extra": "x" });
        let invalid = ContentInput {
            action: "open_product",
            action_params: Some(&unknown),
            ..input()
        };
        assert!(prepare_content(&config, &entity(), &sender(), "company-1", &invalid).is_err());
    }

    #[test]
    fn action_data_rendered_from_config_only() {
        let mut config = campaigns_config();
        let action = config
            .content
            .actions
            .get_mut("open_product")
            .expect("the action must exist");
        action
            .params
            .insert("product_id".to_string(), Param::Text { max: 64 });
        action
            .data
            .insert("name".to_string(), "{entity.name}".to_string());

        // A client value that looks like a placeholder must stay literal.
        let values = json!({ "product_id": "{entity.name}" });
        let content = ContentInput {
            action: "open_product",
            action_params: Some(&values),
            ..input()
        };

        let prepared = prepare_content(&config, &entity(), &sender(), "company-1", &content)
            .expect("the content must pass");
        assert_eq!(
            prepared.data.get("product_id").map(String::as_str),
            Some("{entity.name}")
        );
        assert_eq!(prepared.data.get("name").map(String::as_str), Some("Acme"));
        assert!(prepared.check.is_some());
    }
}

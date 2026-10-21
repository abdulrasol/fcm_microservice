//! Configuration and template validation (T1).
//!
//! Every error carries its rule code in brackets, e.g. `[V3]`, so the cause is clear.
//! `validate_all` returns **all** errors and never stops at the first one.

use crate::config::placeholder::{self, Ref};
use crate::config::{
    AppCheckMode, AppConfig, Campaigns, DataSource, IdentityProvider, Param, ReviewMode,
    RoleSource, Target, Template,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// A single validation error.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Issue {
    /// The field path inside the configuration, e.g. `templates.order_new.to`.
    pub path: String,
    pub message: String,
}

impl Issue {
    pub fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Issue {
            path: path.into(),
            message: message.into(),
        }
    }
}

// ---------- shared patterns ----------

/// `^[A-Za-z0-9_-]{1,40}$`
pub fn is_app_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 40
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// `^[a-z0-9_]{1,32}$` (roles)
pub fn is_role(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// `^[A-Z][A-Z0-9_]{0,63}$` (environment variable names)
pub fn is_env_name(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) if first.is_ascii_uppercase() => {}
        _ => return false,
    }
    value.len() <= 64
        && value
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// `^[a-z][a-z0-9_]{0,39}$` (template key)
pub fn is_template_key(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() => {}
        _ => return false,
    }
    value.len() <= 40
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// `^[a-z][a-z0-9_]{0,29}$` (parameter or variable name)
pub fn is_param_name(value: &str) -> bool {
    placeholder::is_name(value)
}

/// `^[A-Za-z0-9_.~%-]{1,100}$` (literal topic)
pub fn is_plain_topic(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value.chars().all(|c| {
            c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '~' || c == '%' || c == '-'
        })
}

/// `^[A-Za-z0-9_.~%-]{1,100}\*?$` (topic pattern: literal or one trailing star)
pub fn is_topic_pattern(value: &str) -> bool {
    let body = value.strip_suffix('*').unwrap_or(value);
    is_plain_topic(body)
}

/// `^[a-z0-9.-]{3,100}$` (image host name, no scheme and no path)
pub fn is_image_host(value: &str) -> bool {
    value.len() >= 3
        && value.len() <= 100
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
}

/// `data` keys reserved by FCM.
pub fn is_reserved_data_key(key: &str) -> bool {
    key == "from"
        || key == "notification"
        || key == "message_type"
        || key.starts_with("google.")
        || key.starts_with("gcm.")
}

// ---------- general validation ----------

/// [V1] the app name.
pub fn validate_app_name(app: &str) -> Vec<Issue> {
    if is_app_name(app) {
        Vec::new()
    } else {
        vec![Issue::new(
            "app",
            "[V1] app name must match ^[A-Za-z0-9_-]{1,40}$".to_string(),
        )]
    }
}

/// Validates the whole configuration and every template, returning **all** errors.
pub fn validate_all(cfg: &AppConfig, templates: &BTreeMap<String, Template>) -> Vec<Issue> {
    let mut issues = Vec::new();

    validate_identity(cfg, &mut issues);
    validate_data_source(cfg, &mut issues);

    if cfg.devices.max_per_uid < 1 || cfg.devices.max_per_uid > 20 {
        issues.push(Issue::new(
            "devices.max_per_uid",
            "[V6] devices.max_per_uid must be between 1 and 20".to_string(),
        ));
    }

    for (index, topic) in cfg.web_subscribe.anonymous.iter().enumerate() {
        if !is_topic_pattern(topic) {
            issues.push(Issue::new(
                format!("web_subscribe.anonymous[{}]", index),
                format!("[V7] topic pattern '{}' is not valid", topic),
            ));
        }
    }
    for (index, topic) in cfg.web_subscribe.signed_in.iter().enumerate() {
        if !is_topic_pattern(topic) {
            issues.push(Issue::new(
                format!("web_subscribe.signed_in[{}]", index),
                format!("[V7] topic pattern '{}' is not valid", topic),
            ));
        }
    }

    if cfg.app_check.mode != AppCheckMode::Off {
        if !matches!(cfg.identity.provider, IdentityProvider::Firebase) {
            issues.push(Issue::new(
                "app_check.mode",
                "[V8] App Check requires the 'firebase' identity provider".to_string(),
            ));
        }
        match cfg.app_check.project_number.as_deref() {
            None => issues.push(Issue::new(
                "app_check.project_number",
                "[V8] app_check.project_number is required when App Check is enabled".to_string(),
            )),
            Some(number) if number.is_empty() || !number.chars().all(|c| c.is_ascii_digit()) => {
                issues.push(Issue::new(
                    "app_check.project_number",
                    "[V8] app_check.project_number must contain digits only".to_string(),
                ))
            }
            Some(_) => {}
        }
    }

    if cfg.campaigns.enabled {
        validate_campaigns(cfg, &mut issues);
    }

    for (key, template) in templates {
        issues.extend(validate_template(key, template, cfg));
    }

    issues
}

// ---------- identity ----------

fn validate_identity(cfg: &AppConfig, issues: &mut Vec<Issue>) {
    if let IdentityProvider::Jwt {
        algorithms,
        jwks_url,
        hs256_secret_env,
        issuer,
        audience,
        uid_claim,
        name_claim,
        max_age_seconds,
    } = &cfg.identity.provider
    {
        if algorithms.is_empty() {
            issues.push(Issue::new(
                "identity.provider.algorithms",
                "[V2] algorithms must not be empty".to_string(),
            ));
        }
        let has_hs = algorithms.iter().any(|algorithm| algorithm.is_hs());
        let has_asymmetric = algorithms.iter().any(|algorithm| !algorithm.is_hs());
        if has_hs && has_asymmetric {
            issues.push(Issue::new(
                "identity.provider.algorithms",
                "[V2] HS256 must not be mixed with RS256/ES256".to_string(),
            ));
        }
        if has_hs {
            match hs256_secret_env {
                None => issues.push(Issue::new(
                    "identity.provider.hs256_secret_env",
                    "[V2] hs256_secret_env is required when HS256 is used".to_string(),
                )),
                Some(name) if !is_env_name(name) => issues.push(Issue::new(
                    "identity.provider.hs256_secret_env",
                    "[V2] hs256_secret_env must match ^[A-Z][A-Z0-9_]{0,63}$".to_string(),
                )),
                Some(_) => {}
            }
            if jwks_url.is_some() {
                issues.push(Issue::new(
                    "identity.provider.jwks_url",
                    "[V2] jwks_url must not be set when HS256 is used".to_string(),
                ));
            }
        } else {
            match jwks_url {
                None => issues.push(Issue::new(
                    "identity.provider.jwks_url",
                    "[V2] jwks_url is required for RS256/ES256".to_string(),
                )),
                Some(url) if !url.starts_with("https://") => issues.push(Issue::new(
                    "identity.provider.jwks_url",
                    "[V2] jwks_url must start with https://".to_string(),
                )),
                Some(_) => {}
            }
        }
        if issuer.trim().is_empty() {
            issues.push(Issue::new(
                "identity.provider.issuer",
                "[V2] issuer must not be empty".to_string(),
            ));
        }
        if audience.trim().is_empty() {
            issues.push(Issue::new(
                "identity.provider.audience",
                "[V2] audience must not be empty".to_string(),
            ));
        }
        if *max_age_seconds < 60 || *max_age_seconds > 86_400 {
            issues.push(Issue::new(
                "identity.provider.max_age_seconds",
                "[V2] max_age_seconds must be between 60 and 86400".to_string(),
            ));
        }
        if !placeholder::is_path(uid_claim) {
            issues.push(Issue::new(
                "identity.provider.uid_claim",
                "[V2] uid_claim must be a dotted path".to_string(),
            ));
        }
        if let Some(name) = name_claim {
            if !placeholder::is_path(name) {
                issues.push(Issue::new(
                    "identity.provider.name_claim",
                    "[V2] name_claim must be a dotted path".to_string(),
                ));
            }
        }
    }

    for (index, role) in cfg.identity.admin_roles.iter().enumerate() {
        if !is_role(role) {
            issues.push(Issue::new(
                format!("identity.admin_roles[{}]", index),
                format!("[V3] role '{}' must match ^[a-z0-9_]{{1,32}}$", role),
            ));
        }
    }

    match &cfg.identity.role {
        RoleSource::Claim { claim } => {
            if !placeholder::is_path(claim) {
                issues.push(Issue::new(
                    "identity.role.claim",
                    "[V2] role claim must be a dotted path".to_string(),
                ));
            }
        }
        RoleSource::DataSource {
            doc,
            field,
            cache_seconds,
        } => {
            if matches!(cfg.data_source, DataSource::None) {
                issues.push(Issue::new(
                    "identity.role",
                    "[V4] a role taken from the data source requires data_source != 'none'"
                        .to_string(),
                ));
            }
            if doc.trim().is_empty() {
                issues.push(Issue::new(
                    "identity.role.doc",
                    "[V4] role doc must not be empty".to_string(),
                ));
            }
            if field.trim().is_empty() {
                issues.push(Issue::new(
                    "identity.role.field",
                    "[V4] role field must not be empty".to_string(),
                ));
            }
            if *cache_seconds > 3600 {
                issues.push(Issue::new(
                    "identity.role.cache_seconds",
                    "[V4] cache_seconds must be <= 3600".to_string(),
                ));
            }
        }
    }

    if let Some(role_members) = &cfg.identity.role_members {
        if matches!(cfg.data_source, DataSource::None) {
            issues.push(Issue::new(
                "identity.role_members",
                "[V4] role_members requires data_source != 'none'".to_string(),
            ));
        }
        if role_members.max < 1 || role_members.max > 200 {
            issues.push(Issue::new(
                "identity.role_members.max",
                "[V4] role_members.max must be between 1 and 200".to_string(),
            ));
        }
        if role_members.cache_seconds > 3600 {
            issues.push(Issue::new(
                "identity.role_members.cache_seconds",
                "[V4] role_members.cache_seconds must be <= 3600".to_string(),
            ));
        }
        if role_members.collection.trim().is_empty() {
            issues.push(Issue::new(
                "identity.role_members.collection",
                "[V4] role_members.collection must not be empty".to_string(),
            ));
        }
        if role_members.field.trim().is_empty() {
            issues.push(Issue::new(
                "identity.role_members.field",
                "[V4] role_members.field must not be empty".to_string(),
            ));
        }
    }
}

fn validate_data_source(cfg: &AppConfig, issues: &mut Vec<Issue>) {
    if let DataSource::Http {
        base_url,
        secret_env,
        timeout_ms,
    } = &cfg.data_source
    {
        let development =
            base_url.starts_with("http://localhost") || base_url.starts_with("http://127.0.0.1");
        if !(base_url.starts_with("https://") || development) {
            issues.push(Issue::new(
                "data_source.base_url",
                "[V5] base_url must start with https:// (http://localhost and http://127.0.0.1 are allowed for development)"
                    .to_string(),
            ));
        }
        if !is_env_name(secret_env) {
            issues.push(Issue::new(
                "data_source.secret_env",
                "[V5] secret_env must match ^[A-Z][A-Z0-9_]{0,63}$".to_string(),
            ));
        }
        if *timeout_ms < 200 || *timeout_ms > 10_000 {
            issues.push(Issue::new(
                "data_source.timeout_ms",
                "[V5] timeout_ms must be between 200 and 10000".to_string(),
            ));
        }
    }
}

// ---------- templates ----------

/// Validates one template in the context of the whole configuration.
pub fn validate_template(key: &str, template: &Template, cfg: &AppConfig) -> Vec<Issue> {
    let mut issues = Vec::new();
    let base = format!("templates.{}", key);

    if !is_template_key(key) {
        issues.push(Issue::new(
            &base,
            "[T0] template key must match ^[a-z][a-z0-9_]{0,39}$".to_string(),
        ));
    }

    // [T1] senders
    if template.senders.is_empty() {
        issues.push(Issue::new(
            format!("{}.senders", base),
            "[T1] senders must not be empty".to_string(),
        ));
    }
    // [V3] sender roles
    for (index, sender) in template.senders.iter().enumerate() {
        if !is_role(sender) {
            issues.push(Issue::new(
                format!("{}.senders[{}]", base, index),
                format!("[V3] sender '{}' must match ^[a-z0-9_]{{1,32}}$", sender),
            ));
        }
    }

    // [T2] parameters
    for (name, param) in &template.params {
        let path = format!("{}.params.{}", base, name);
        if !is_param_name(name) {
            issues.push(Issue::new(
                &path,
                "[T2] parameter name must match ^[a-z][a-z0-9_]{0,29}$".to_string(),
            ));
        }
        match param {
            Param::Text { max } => {
                if *max < 1 || *max > 1000 {
                    issues.push(Issue::new(
                        &path,
                        "[T2] text.max must be between 1 and 1000".to_string(),
                    ));
                }
            }
            Param::Pattern { pattern, .. } => {
                if pattern.len() > 200 {
                    issues.push(Issue::new(
                        &path,
                        "[T2] pattern length must be 200 characters or less".to_string(),
                    ));
                }
                if regex::Regex::new(pattern).is_err() {
                    issues.push(Issue::new(
                        &path,
                        "[T2] pattern is not a valid regular expression".to_string(),
                    ));
                }
            }
            Param::Enum { values } => {
                if values.is_empty() || values.len() > 50 {
                    issues.push(Issue::new(
                        &path,
                        "[T2] enum.values must hold between 1 and 50 values".to_string(),
                    ));
                }
                for (index, value) in values.iter().enumerate() {
                    if value.trim().is_empty() {
                        issues.push(Issue::new(
                            format!("{}.values[{}]", path, index),
                            "[T2] enum.values must not contain empty values".to_string(),
                        ));
                    }
                }
            }
            Param::Int { min, max } => {
                if let (Some(low), Some(high)) = (min, max) {
                    if low > high {
                        issues.push(Issue::new(
                            &path,
                            "[T2] int.min must be less than or equal to int.max".to_string(),
                        ));
                    }
                }
            }
        }
    }

    // [T3] references inside the texts
    check_template_placeholders(
        &template.title,
        &format!("{}.title", base),
        template,
        &mut issues,
    );
    if template.title.trim().is_empty() {
        issues.push(Issue::new(
            format!("{}.title", base),
            "[T3] title must not be empty".to_string(),
        ));
    }
    check_template_placeholders(
        &template.body,
        &format!("{}.body", base),
        template,
        &mut issues,
    );
    if template.body.trim().is_empty() {
        issues.push(Issue::new(
            format!("{}.body", base),
            "[T3] body must not be empty".to_string(),
        ));
    }
    if let Some(image) = &template.image {
        check_template_placeholders(image, &format!("{}.image", base), template, &mut issues);
    }
    for (key, value) in &template.data {
        check_template_placeholders(
            value,
            &format!("{}.data.{}", base, key),
            template,
            &mut issues,
        );
    }
    check_template_placeholders(
        template.to.value(),
        &format!("{}.to.{}", base, template.to.kind()),
        template,
        &mut issues,
    );
    if let Some(context) = &template.context {
        check_template_placeholders(
            &context.doc,
            &format!("{}.context.doc", base),
            template,
            &mut issues,
        );
        for (key, value) in &context.require {
            check_template_placeholders(
                value,
                &format!("{}.context.require.{}", base, key),
                template,
                &mut issues,
            );
        }
    }
    if let Some(dedupe) = &template.dedupe {
        check_template_placeholders(
            &dedupe.key,
            &format!("{}.dedupe.key", base),
            template,
            &mut issues,
        );
    }
    for (name, var) in &template.vars {
        let path = format!("{}.vars.{}.from", base, name);
        if var.from.contains('{') || var.from.contains('}') {
            issues.push(Issue::new(
                &path,
                "[T3] vars.from must be a plain path without braces".to_string(),
            ));
        } else if !(var.from.starts_with("doc.")
            || var.from.starts_with("p.")
            || var.from.starts_with("sender."))
        {
            issues.push(Issue::new(
                &path,
                "[T3] vars.from must start with 'doc.', 'p.' or 'sender.'".to_string(),
            ));
        } else {
            let remainder = var
                .from
                .split_once('.')
                .map(|(_, rest)| rest)
                .unwrap_or_default();
            if !placeholder::is_path(remainder) {
                issues.push(Issue::new(
                    &path,
                    "[T3] vars.from must be a valid dotted path".to_string(),
                ));
            }
        }
        for (key, value) in &var.map {
            let path = format!("{}.vars.{}.map.{}", base, name, key);
            check_map_value(value, &path, template, &mut issues);
        }
    }

    // [T5] context and vars from the document need a data source
    if matches!(cfg.data_source, DataSource::None) {
        if template.context.is_some() {
            issues.push(Issue::new(
                format!("{}.context", base),
                "[T5] context is not allowed when data_source is 'none'".to_string(),
            ));
        }
        for (name, var) in &template.vars {
            if var.from.starts_with("doc.") {
                issues.push(Issue::new(
                    format!("{}.vars.{}.from", base, name),
                    "[T5] document variables are not allowed when data_source is 'none'"
                        .to_string(),
                ));
            }
        }
    }

    // [T6] reserved data keys
    for key in template.data.keys() {
        if is_reserved_data_key(key) {
            issues.push(Issue::new(
                format!("{}.data.{}", base, key),
                "[T6] data key is reserved for FCM and must not be used".to_string(),
            ));
        }
    }

    // [T7] limits
    if let Some(dedupe) = &template.dedupe {
        if dedupe.seconds < 1 || dedupe.seconds > 86_400 {
            issues.push(Issue::new(
                format!("{}.dedupe.seconds", base),
                "[T7] dedupe.seconds must be between 1 and 86400".to_string(),
            ));
        }
    }
    if let Some(rate) = &template.rate {
        if rate.per_uid_per_hour < 1 || rate.per_uid_per_hour > 120 {
            issues.push(Issue::new(
                format!("{}.rate.per_uid_per_hour", base),
                "[T7] rate.per_uid_per_hour must be between 1 and 120".to_string(),
            ));
        }
    }

    // [T4] a single field in `to` -- guaranteed by the enum definition.

    // Safety guards for templates that are not admin-only
    if !is_admin_only(template, cfg) {
        validate_non_admin_guards(key, template, cfg, &base, &mut issues);
    }

    issues
}

/// Every sender is an admin, so G1..G5 do not apply.
fn is_admin_only(template: &Template, cfg: &AppConfig) -> bool {
    if template.senders.is_empty() || cfg.identity.admin_roles.is_empty() {
        return false;
    }
    template
        .senders
        .iter()
        .all(|sender| cfg.identity.admin_roles.contains(sender))
}

fn validate_non_admin_guards(
    key: &str,
    template: &Template,
    cfg: &AppConfig,
    base: &str,
    issues: &mut Vec<Issue>,
) {
    let target_is_broadcast = matches!(template.to, Target::Topic(_) | Target::Condition(_));

    // [G1] free broadcast from a broadcast source: no text parameter, no target from a parameter
    if target_is_broadcast {
        if template.params.values().any(|param| param.is_text()) {
            issues.push(Issue::new(
                format!("{}.params", base),
                "[G1] a template sent to a topic/condition must not define a 'text' parameter"
                    .to_string(),
            ));
        }
        if target_uses_param(&template.to) {
            issues.push(Issue::new(
                format!("{}.to.{}", base, template.to.kind()),
                "[G1] the target of a topic/condition template must not be built from a client parameter"
                    .to_string(),
            ));
        }
    }

    // [G2] uid/role must not be built from a parameter
    if matches!(template.to, Target::Uid(_) | Target::Role(_)) && target_uses_param(&template.to) {
        issues.push(Issue::new(
            format!("{}.to.{}", base, template.to.kind()),
            "[G2] the target must not be built from a client parameter".to_string(),
        ));
    }

    // [G3] a rate limit is mandatory for non-admin templates
    if template.rate.is_none() {
        issues.push(Issue::new(
            format!("{}.rate", base),
            "[G3] a rate limit is required for templates that are not admin-only".to_string(),
        ));
    }

    // [G4] context needs a non-empty require with {sender.uid}
    if let Some(context) = &template.context {
        if context.require.is_empty() {
            issues.push(Issue::new(
                format!("{}.context.require", base),
                "[G4] context.require must not be empty".to_string(),
            ));
        } else if !context
            .require
            .values()
            .any(|value| value.contains("{sender.uid}"))
        {
            issues.push(Issue::new(
                format!("{}.context.require", base),
                "[G4] context.require must contain at least one '{sender.uid}' check".to_string(),
            ));
        }
    }

    // [G5] the 'all' topic is not allowed for templates that are not admin-only
    if let Target::Topic(topic) = &template.to {
        if topic == "all" {
            issues.push(Issue::new(
                format!("{}.to.topic", base),
                "[G5] the 'all' topic is not allowed for templates that are not admin-only"
                    .to_string(),
            ));
        }
    }
    if let Target::Condition(condition) = &template.to {
        if condition.contains("'all' in topics") {
            issues.push(Issue::new(
                format!("{}.to.condition", base),
                "[G5] a condition targeting the 'all' topic is not allowed for templates that are not admin-only"
                    .to_string(),
            ));
        }
    }

    // [G6] a role target must be a literal admin role: otherwise a non-admin
    // could broadcast text to every member of an ordinary role.
    if let Target::Role(role) = &template.to {
        if !cfg.identity.admin_roles.contains(role) {
            issues.push(Issue::new(
                format!("{}.to.role", base),
                "[G6] a non-admin template may only target an admin role".to_string(),
            ));
        }
    }

    let _ = key;
}

/// Does the target contain a parameter reference `p.*`?
fn target_uses_param(target: &Target) -> bool {
    match placeholder::extract(target.value()) {
        Ok(found) => found
            .iter()
            .any(|placeholder| matches!(placeholder.r, Ref::Param(_))),
        // Malformed text is reported as [T3]; do not emit a second G error.
        Err(_) => false,
    }
}

fn check_template_placeholders(
    text: &str,
    path: &str,
    template: &Template,
    issues: &mut Vec<Issue>,
) {
    let found = match placeholder::extract(text) {
        Ok(found) => found,
        Err(message) => {
            issues.push(Issue::new(
                path,
                format!("[T3] invalid placeholder: {}", message),
            ));
            return;
        }
    };

    for item in found {
        match &item.r {
            Ref::Param(name) => {
                if !template.params.contains_key(name) {
                    issues.push(Issue::new(
                        path,
                        format!("[T3] parameter '{}' is not defined in params", name),
                    ));
                }
            }
            Ref::Doc(_) => {
                if template.context.is_none() {
                    issues.push(Issue::new(
                        path,
                        "[T3] a document reference requires a 'context' section".to_string(),
                    ));
                }
            }
            Ref::Entity(_) => {
                issues.push(Issue::new(
                    path,
                    "[T3] entity references are not allowed in templates".to_string(),
                ));
            }
            Ref::Var(name) => {
                if !template.vars.contains_key(name) {
                    issues.push(Issue::new(
                        path,
                        format!("[T3] variable '{}' is not defined in vars", name),
                    ));
                }
            }
            Ref::Sender(_) => {}
        }
    }
}

/// `vars.map` values must not reference other variables.
fn check_map_value(text: &str, path: &str, template: &Template, issues: &mut Vec<Issue>) {
    let found = match placeholder::extract(text) {
        Ok(found) => found,
        Err(message) => {
            issues.push(Issue::new(
                path,
                format!("[T3] invalid placeholder: {}", message),
            ));
            return;
        }
    };

    for item in found {
        match &item.r {
            Ref::Var(name) => {
                issues.push(Issue::new(
                    path,
                    format!(
                        "[T3] a vars.map value must not reference the variable '{}'",
                        name
                    ),
                ));
            }
            Ref::Param(name) => {
                if !template.params.contains_key(name) {
                    issues.push(Issue::new(
                        path,
                        format!("[T3] parameter '{}' is not defined in params", name),
                    ));
                }
            }
            Ref::Doc(_) => {
                if template.context.is_none() {
                    issues.push(Issue::new(
                        path,
                        "[T3] a document reference requires a 'context' section".to_string(),
                    ));
                }
            }
            Ref::Entity(_) => {
                issues.push(Issue::new(
                    path,
                    "[T3] entity references are not allowed in templates".to_string(),
                ));
            }
            Ref::Sender(_) => {}
        }
    }
}

// ---------- campaigns ----------

fn validate_campaigns(cfg: &AppConfig, issues: &mut Vec<Issue>) {
    let campaigns: &Campaigns = &cfg.campaigns;
    let entity_params: BTreeSet<String> = std::iter::once(campaigns.entity.param.clone()).collect();

    // [C1] senders and the company
    if campaigns.senders.is_empty() {
        issues.push(Issue::new(
            "campaigns.senders",
            "[C1] senders must not be empty".to_string(),
        ));
    }
    for (index, sender) in campaigns.senders.iter().enumerate() {
        if !is_role(sender) {
            issues.push(Issue::new(
                format!("campaigns.senders[{}]", index),
                format!("[V3] sender '{}' must match ^[a-z0-9_]{{1,32}}$", sender),
            ));
        }
    }
    if !is_param_name(&campaigns.entity.param) {
        issues.push(Issue::new(
            "campaigns.entity.param",
            "[C1] entity.param must match ^[a-z][a-z0-9_]{0,29}$".to_string(),
        ));
    }
    let entity_placeholder = format!("{{p.{}}}", campaigns.entity.param);
    if !campaigns.entity.doc.contains(&entity_placeholder) {
        issues.push(Issue::new(
            "campaigns.entity.doc",
            "[C1] entity.doc must contain the '{p.<param>}' placeholder".to_string(),
        ));
    }
    if campaigns.entity.require.is_empty() {
        issues.push(Issue::new(
            "campaigns.entity.require",
            "[C1] entity.require must not be empty".to_string(),
        ));
    } else if !campaigns
        .entity
        .require
        .values()
        .any(|value| value.contains("{sender.uid}"))
    {
        issues.push(Issue::new(
            "campaigns.entity.require",
            "[C1] entity.require must contain at least one '{sender.uid}' check".to_string(),
        ));
    }
    if campaigns.entity.name_field.trim().is_empty() {
        issues.push(Issue::new(
            "campaigns.entity.name_field",
            "[C1] entity.name_field must not be empty".to_string(),
        ));
    }
    check_campaign_placeholders(
        &campaigns.entity.doc,
        "campaigns.entity.doc",
        &entity_params,
        "C1",
        issues,
    );
    for (key, value) in &campaigns.entity.require {
        check_campaign_placeholders(
            value,
            &format!("campaigns.entity.require.{}", key),
            &entity_params,
            "C1",
            issues,
        );
    }

    // [C2] audience
    if campaigns.audience.allowed_topics.is_empty() {
        issues.push(Issue::new(
            "campaigns.audience.allowed_topics",
            "[C2] allowed_topics must not be empty".to_string(),
        ));
    }
    for (index, pattern) in campaigns.audience.allowed_topics.iter().enumerate() {
        let path = format!("campaigns.audience.allowed_topics[{}]", index);
        if !is_allowed_topic_pattern(pattern, &campaigns.entity.param) {
            issues.push(Issue::new(
                &path,
                format!("[C2] topic pattern '{}' is not valid", pattern),
            ));
        }
        let literal = literal_part(pattern);
        if literal.len() < 3 {
            issues.push(Issue::new(
                &path,
                "[C2] the literal part of a topic pattern must be at least 3 characters"
                    .to_string(),
            ));
        }
        if literal == campaigns.audience.required_topic {
            issues.push(Issue::new(
                &path,
                "[C2] an allowed topic pattern must not be the required topic itself".to_string(),
            ));
        }
    }

    // [C3] required subject and topic count
    if !is_plain_topic(&campaigns.audience.required_topic) {
        issues.push(Issue::new(
            "campaigns.audience.required_topic",
            "[C3] required_topic must be a literal topic without '*'".to_string(),
        ));
    }
    if campaigns.audience.max_topics < 2 || campaigns.audience.max_topics > 5 {
        issues.push(Issue::new(
            "campaigns.audience.max_topics",
            "[C3] max_topics must be between 2 and 5".to_string(),
        ));
    }

    // [C4] content
    match placeholder::extract(&campaigns.content.title_prefix) {
        Ok(found) => {
            for item in found {
                if !matches!(item.r, Ref::Entity(_)) {
                    issues.push(Issue::new(
                        "campaigns.content.title_prefix",
                        "[C4] title_prefix may only use '{entity.*}' placeholders".to_string(),
                    ));
                }
            }
        }
        Err(message) => issues.push(Issue::new(
            "campaigns.content.title_prefix",
            format!("[C4] invalid placeholder: {}", message),
        )),
    }
    if campaigns.content.title_max < 1 || campaigns.content.title_max > 200 {
        issues.push(Issue::new(
            "campaigns.content.title_max",
            "[C4] title_max must be between 1 and 200".to_string(),
        ));
    }
    if campaigns.content.body_max < 1 || campaigns.content.body_max > 1_000 {
        issues.push(Issue::new(
            "campaigns.content.body_max",
            "[C4] body_max must be between 1 and 1000".to_string(),
        ));
    }
    for (index, host) in campaigns.content.image_hosts.iter().enumerate() {
        if !is_image_host(host) {
            issues.push(Issue::new(
                format!("campaigns.content.image_hosts[{}]", index),
                format!("[C4] image host '{}' is not valid", host),
            ));
        }
    }

    // [C5] actions
    if campaigns.content.actions.is_empty() {
        issues.push(Issue::new(
            "campaigns.content.actions",
            "[C5] actions must not be empty".to_string(),
        ));
    }
    for (name, action) in &campaigns.content.actions {
        let mut allowed: BTreeSet<String> = entity_params.clone();
        for param_name in action.params.keys() {
            allowed.insert(param_name.clone());
        }
        for (key, value) in &action.data {
            let path = format!("campaigns.content.actions.{}.data.{}", name, key);
            if is_reserved_data_key(key) {
                issues.push(Issue::new(
                    &path,
                    "[T6] data key is reserved for FCM and must not be used".to_string(),
                ));
            }
            check_campaign_placeholders(value, &path, &allowed, "C5", issues);
        }
        if let Some(check) = &action.check {
            let path = format!("campaigns.content.actions.{}.check", name);
            check_campaign_placeholders(&check.doc, &path, &allowed, "C5", issues);
            if !check
                .require
                .values()
                .any(|value| value.contains("{sender.uid}"))
            {
                issues.push(Issue::new(
                    format!("{}.require", path),
                    "[C5] an action check must contain a '{sender.uid}' check".to_string(),
                ));
            }
        }
    }

    // [C6] quota
    if campaigns.quota.per_entity_per_day < 1 {
        issues.push(Issue::new(
            "campaigns.quota.per_entity_per_day",
            "[C6] per_entity_per_day must be at least 1".to_string(),
        ));
    }
    if campaigns.quota.per_entity_per_week < campaigns.quota.per_entity_per_day {
        issues.push(Issue::new(
            "campaigns.quota.per_entity_per_week",
            "[C6] per_entity_per_week must not be lower than per_entity_per_day".to_string(),
        ));
    }

    // [C7] review
    if campaigns.review.mode == ReviewMode::FirstN {
        match campaigns.review.n {
            None => issues.push(Issue::new(
                "campaigns.review.n",
                "[C7] review.n is required when review.mode is 'first_n'".to_string(),
            )),
            Some(n) if !(1..=50).contains(&n) => issues.push(Issue::new(
                "campaigns.review.n",
                "[C7] review.n must be between 1 and 50".to_string(),
            )),
            Some(_) => {}
        }
    }
}

/// An allowed topic pattern: V7 literal, or V7 with at most one parameter reference.
fn is_allowed_topic_pattern(pattern: &str, entity_param: &str) -> bool {
    if !pattern.contains('{') {
        return is_topic_pattern(pattern);
    }
    let placeholder = format!("{{p.{}}}", entity_param);
    let mut rest = pattern.to_string();
    let mut count = 0;
    while let Some(position) = rest.find('{') {
        let after = rest[position..].to_string();
        let end = match after.find('}') {
            Some(end) => end,
            None => return false,
        };
        if after[..=end] != placeholder {
            return false;
        }
        count += 1;
        rest = format!("{}{}", &rest[..position], &after[end + 1..]);
    }
    count > 0 && count <= 1 && is_plain_topic(&rest)
}

/// The literal part of a topic pattern (without the star and the braces).
fn literal_part(pattern: &str) -> String {
    let mut literal = pattern.to_string();
    if let Some(position) = literal.find('{') {
        literal.truncate(position);
    }
    if literal.ends_with('*') {
        literal.pop();
    }
    literal
}

/// In campaigns: `p.*` from a fixed set and `sender.*` only.
fn check_campaign_placeholders(
    text: &str,
    path: &str,
    allowed_params: &BTreeSet<String>,
    code: &str,
    issues: &mut Vec<Issue>,
) {
    let found = match placeholder::extract(text) {
        Ok(found) => found,
        Err(message) => {
            issues.push(Issue::new(
                path,
                format!("[{}] invalid placeholder: {}", code, message),
            ));
            return;
        }
    };

    for item in found {
        match &item.r {
            Ref::Param(name) => {
                if !allowed_params.contains(name) {
                    issues.push(Issue::new(
                        path,
                        format!("[{}] parameter '{}' is not allowed here", code, name),
                    ));
                }
            }
            Ref::Sender(_) => {}
            _ => issues.push(Issue::new(
                path,
                format!(
                    "[{}] only '{{p.*}}' and '{{sender.*}}' placeholders are allowed here",
                    code
                ),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigDocument;

    const EXAMPLE: &str = include_str!("../../policies/watt.example.json");

    fn example_parts() -> (AppConfig, BTreeMap<String, Template>) {
        let document: ConfigDocument =
            serde_json::from_str(EXAMPLE).expect("the example policy must parse");
        document.into_parts()
    }

    fn example_config() -> AppConfig {
        example_parts().0
    }

    fn issue_codes(issues: &[Issue]) -> Vec<&str> {
        issues
            .iter()
            .map(|issue| issue.message.split_whitespace().next().unwrap_or_default())
            .collect()
    }

    fn has_code(issues: &[Issue], code: &str) -> bool {
        issues.iter().any(|issue| issue.message.contains(code))
    }

    #[test]
    fn example_policy_is_valid() {
        let (config, templates) = example_parts();
        let issues = validate_all(&config, &templates);
        assert!(
            issues.is_empty(),
            "the example policy must be valid, got: {:?}",
            issues
        );
    }

    #[test]
    fn unknown_field_is_rejected() {
        let raw = serde_json::json!({
            "description": "x",
            "senders": ["admin"],
            "to": { "role": "admin" },
            "title": "t",
            "body": "b",
            "totally_unknown": 1
        });
        let error =
            serde_json::from_value::<Template>(raw).expect_err("an unknown field must be rejected");
        assert!(error.to_string().contains("unknown"), "error: {}", error);
    }

    #[test]
    fn undefined_param_ref_rejected() {
        let mut template = Template {
            description: "t".to_string(),
            enabled: true,
            senders: vec!["admin".to_string()],
            params: BTreeMap::new(),
            context: None,
            vars: BTreeMap::new(),
            to: Target::Role("admin".to_string()),
            title: "order {p.order_id}".to_string(),
            body: "b".to_string(),
            image: None,
            data: BTreeMap::new(),
            dedupe: None,
            rate: None,
        };
        template.enabled = true;
        let issues = validate_template("t", &template, &example_config());
        assert!(
            has_code(&issues, "[T3]"),
            "issues: {:?}",
            issue_codes(&issues)
        );
    }

    #[test]
    fn doc_ref_without_context_rejected() {
        let template = Template {
            description: "t".to_string(),
            enabled: true,
            senders: vec!["admin".to_string()],
            params: BTreeMap::new(),
            context: None,
            vars: BTreeMap::new(),
            to: Target::Role("admin".to_string()),
            title: "order {doc.order_number}".to_string(),
            body: "b".to_string(),
            image: None,
            data: BTreeMap::new(),
            dedupe: None,
            rate: None,
        };
        let issues = validate_template("t", &template, &example_config());
        assert!(
            has_code(&issues, "[T3]"),
            "issues: {:?}",
            issue_codes(&issues)
        );
        assert!(issues.iter().any(|issue| issue.message.contains("context")));
    }

    #[test]
    fn undefined_var_ref_rejected() {
        let template = Template {
            description: "t".to_string(),
            enabled: true,
            senders: vec!["admin".to_string()],
            params: BTreeMap::new(),
            context: None,
            vars: BTreeMap::new(),
            to: Target::Role("admin".to_string()),
            title: "state {status_text}".to_string(),
            body: "b".to_string(),
            image: None,
            data: BTreeMap::new(),
            dedupe: None,
            rate: None,
        };
        let issues = validate_template("t", &template, &example_config());
        assert!(
            has_code(&issues, "[T3]"),
            "issues: {:?}",
            issue_codes(&issues)
        );
        assert!(issues.iter().any(|issue| issue.message.contains("vars")));
    }

    #[test]
    fn jwt_none_algorithm_rejected() {
        let raw = serde_json::json!({
            "type": "jwt",
            "algorithms": ["none"],
            "jwks_url": "https://auth.example.com/jwks",
            "issuer": "https://auth.example.com/",
            "audience": "app"
        });
        assert!(
            serde_json::from_value::<IdentityProvider>(raw).is_err(),
            "the 'none' algorithm must not deserialize"
        );
    }

    #[test]
    fn jwt_mixed_hs_rs_rejected() {
        let mut config = example_config();
        config.identity.provider = IdentityProvider::Jwt {
            algorithms: vec![
                crate::config::JwtAlgorithm::RS256,
                crate::config::JwtAlgorithm::HS256,
            ],
            jwks_url: Some("https://auth.example.com/jwks".to_string()),
            hs256_secret_env: Some("CLIENT_JWT_SECRET".to_string()),
            issuer: "https://auth.example.com/".to_string(),
            audience: "app".to_string(),
            uid_claim: "sub".to_string(),
            name_claim: None,
            max_age_seconds: 3600,
        };
        let issues = validate_all(&config, &BTreeMap::new());
        assert!(
            has_code(&issues, "[V2]"),
            "issues: {:?}",
            issue_codes(&issues)
        );
    }

    #[test]
    fn jwt_hs256_requires_secret_env() {
        let mut config = example_config();
        config.identity.provider = IdentityProvider::Jwt {
            algorithms: vec![crate::config::JwtAlgorithm::HS256],
            jwks_url: None,
            hs256_secret_env: None,
            issuer: "https://auth.example.com/".to_string(),
            audience: "app".to_string(),
            uid_claim: "sub".to_string(),
            name_claim: None,
            max_age_seconds: 3600,
        };
        let issues = validate_all(&config, &BTreeMap::new());
        assert!(
            has_code(&issues, "[V2]"),
            "issues: {:?}",
            issue_codes(&issues)
        );
    }

    #[test]
    fn jwt_rs256_requires_https_jwks() {
        let mut config = example_config();
        config.identity.provider = IdentityProvider::Jwt {
            algorithms: vec![crate::config::JwtAlgorithm::RS256],
            jwks_url: Some("http://auth.example.com/jwks".to_string()),
            hs256_secret_env: None,
            issuer: "https://auth.example.com/".to_string(),
            audience: "app".to_string(),
            uid_claim: "sub".to_string(),
            name_claim: None,
            max_age_seconds: 3600,
        };
        let issues = validate_all(&config, &BTreeMap::new());
        assert!(
            has_code(&issues, "[V2]"),
            "issues: {:?}",
            issue_codes(&issues)
        );
    }

    #[test]
    fn role_from_data_source_requires_data_source() {
        let mut config = example_config();
        config.data_source = DataSource::None;
        let issues = validate_all(&config, &BTreeMap::new());
        assert!(
            has_code(&issues, "[V4]"),
            "issues: {:?}",
            issue_codes(&issues)
        );
    }

    #[test]
    fn max_topics_over_5_rejected() {
        let (mut config, templates) = example_parts();
        config.campaigns.audience.max_topics = 6;
        let issues = validate_all(&config, &templates);
        assert!(
            has_code(&issues, "[C3]"),
            "issues: {:?}",
            issue_codes(&issues)
        );
    }

    #[test]
    fn bad_pattern_regex_rejected() {
        let mut template = Template {
            description: "t".to_string(),
            enabled: true,
            senders: vec!["admin".to_string()],
            params: BTreeMap::new(),
            context: None,
            vars: BTreeMap::new(),
            to: Target::Role("admin".to_string()),
            title: "t".to_string(),
            body: "b".to_string(),
            image: None,
            data: BTreeMap::new(),
            dedupe: None,
            rate: None,
        };
        template.params.insert(
            "order_id".to_string(),
            Param::Pattern {
                pattern: "([unclosed".to_string(),
                max: None,
            },
        );
        let issues = validate_template("t", &template, &example_config());
        assert!(
            has_code(&issues, "[T2]"),
            "issues: {:?}",
            issue_codes(&issues)
        );
    }

    #[test]
    fn reserved_data_keys_rejected() {
        let mut template = Template {
            description: "t".to_string(),
            enabled: true,
            senders: vec!["admin".to_string()],
            params: BTreeMap::new(),
            context: None,
            vars: BTreeMap::new(),
            to: Target::Role("admin".to_string()),
            title: "t".to_string(),
            body: "b".to_string(),
            image: None,
            data: BTreeMap::new(),
            dedupe: None,
            rate: None,
        };
        template
            .data
            .insert("notification".to_string(), "x".to_string());
        template
            .data
            .insert("google.c".to_string(), "x".to_string());
        let issues = validate_template("t", &template, &example_config());
        assert!(
            has_code(&issues, "[T6]"),
            "issues: {:?}",
            issue_codes(&issues)
        );
    }

    fn broadcast_template(senders: Vec<&str>, params: BTreeMap<String, Param>) -> Template {
        Template {
            description: "t".to_string(),
            enabled: true,
            senders: senders.into_iter().map(str::to_string).collect(),
            params,
            context: None,
            vars: BTreeMap::new(),
            to: Target::Topic("all_users".to_string()),
            title: "t".to_string(),
            body: "b".to_string(),
            image: None,
            data: BTreeMap::new(),
            dedupe: None,
            rate: None,
        }
    }

    #[test]
    fn g1_non_admin_topic_with_text_param_rejected() {
        let mut params = BTreeMap::new();
        params.insert("message".to_string(), Param::Text { max: 100 });
        let template = broadcast_template(vec!["user"], params);
        let issues = validate_template("t", &template, &example_config());
        assert!(
            has_code(&issues, "[G1]"),
            "issues: {:?}",
            issue_codes(&issues)
        );
    }

    #[test]
    fn g1_admin_only_topic_with_text_param_allowed() {
        let mut params = BTreeMap::new();
        params.insert("message".to_string(), Param::Text { max: 100 });
        let template = broadcast_template(vec!["admin"], params);
        let issues = validate_template("t", &template, &example_config());
        assert!(
            !has_code(&issues, "[G1]"),
            "an admin-only template is exempt, got: {:?}",
            issue_codes(&issues)
        );
    }

    #[test]
    fn g2_uid_from_param_rejected() {
        let template = Template {
            description: "t".to_string(),
            enabled: true,
            senders: vec!["user".to_string()],
            params: BTreeMap::new(),
            context: None,
            vars: BTreeMap::new(),
            to: Target::Uid("{p.target}".to_string()),
            title: "t".to_string(),
            body: "b".to_string(),
            image: None,
            data: BTreeMap::new(),
            dedupe: None,
            rate: Some(crate::config::Rate {
                per_uid_per_hour: 10,
            }),
        };
        let issues = validate_template("t", &template, &example_config());
        assert!(
            has_code(&issues, "[G2]"),
            "issues: {:?}",
            issue_codes(&issues)
        );
    }

    #[test]
    fn g3_rate_required_for_non_admin() {
        let template = Template {
            description: "t".to_string(),
            enabled: true,
            senders: vec!["user".to_string()],
            params: BTreeMap::new(),
            context: None,
            vars: BTreeMap::new(),
            to: Target::Role("admin".to_string()),
            title: "t".to_string(),
            body: "b".to_string(),
            image: None,
            data: BTreeMap::new(),
            dedupe: None,
            rate: None,
        };
        let issues = validate_template("t", &template, &example_config());
        assert!(
            has_code(&issues, "[G3]"),
            "issues: {:?}",
            issue_codes(&issues)
        );
    }

    #[test]
    fn g4_context_without_sender_uid_rejected() {
        let mut require = BTreeMap::new();
        require.insert("owner_id".to_string(), "{doc.owner_id}".to_string());
        let template = Template {
            description: "t".to_string(),
            enabled: true,
            senders: vec!["user".to_string()],
            params: BTreeMap::new(),
            context: Some(crate::config::Context {
                doc: "store_orders/{p.order_id}".to_string(),
                require,
            }),
            vars: BTreeMap::new(),
            to: Target::Uid("{doc.seller_id}".to_string()),
            title: "t".to_string(),
            body: "b".to_string(),
            image: None,
            data: BTreeMap::new(),
            dedupe: None,
            rate: Some(crate::config::Rate {
                per_uid_per_hour: 10,
            }),
        };
        let issues = validate_template("t", &template, &example_config());
        assert!(
            has_code(&issues, "[G4]"),
            "issues: {:?}",
            issue_codes(&issues)
        );
    }

    #[test]
    fn g5_topic_all_rejected_for_non_admin() {
        let template = Template {
            description: "t".to_string(),
            enabled: true,
            senders: vec!["user".to_string()],
            params: BTreeMap::new(),
            context: None,
            vars: BTreeMap::new(),
            to: Target::Topic("all".to_string()),
            title: "t".to_string(),
            body: "b".to_string(),
            image: None,
            data: BTreeMap::new(),
            dedupe: None,
            rate: Some(crate::config::Rate {
                per_uid_per_hour: 10,
            }),
        };
        let issues = validate_template("t", &template, &example_config());
        assert!(
            has_code(&issues, "[G5]"),
            "issues: {:?}",
            issue_codes(&issues)
        );
    }

    #[test]
    fn c1_entity_requires_sender_uid() {
        let (mut config, templates) = example_parts();
        config.campaigns.entity.require.clear();
        let issues = validate_all(&config, &templates);
        assert!(
            has_code(&issues, "[C1]"),
            "issues: {:?}",
            issue_codes(&issues)
        );
    }

    #[test]
    fn c2_wildcard_star_alone_rejected() {
        let (mut config, templates) = example_parts();
        config.campaigns.audience.allowed_topics = vec!["*".to_string()];
        let issues = validate_all(&config, &templates);
        assert!(
            has_code(&issues, "[C2]"),
            "issues: {:?}",
            issue_codes(&issues)
        );
    }

    #[test]
    fn c3_max_topics_range() {
        let (mut config, templates) = example_parts();
        config.campaigns.audience.max_topics = 1;
        let issues = validate_all(&config, &templates);
        assert!(
            has_code(&issues, "[C3]"),
            "issues: {:?}",
            issue_codes(&issues)
        );

        let (mut config, templates) = example_parts();
        config.campaigns.audience.max_topics = 5;
        let issues = validate_all(&config, &templates);
        assert!(
            !has_code(&issues, "[C3]"),
            "max_topics = 5 is allowed, got: {:?}",
            issue_codes(&issues)
        );
    }

    #[test]
    fn c5_action_check_requires_sender_uid() {
        let (mut config, templates) = example_parts();
        let action = config
            .campaigns
            .content
            .actions
            .get_mut("open_product")
            .expect("the example has the open_product action");
        action.check = Some(crate::config::Check {
            doc: "store_products/{p.product_id}".to_string(),
            require: BTreeMap::new(),
        });
        let issues = validate_all(&config, &templates);
        assert!(
            has_code(&issues, "[C5]"),
            "issues: {:?}",
            issue_codes(&issues)
        );
    }

    #[test]
    fn c7_first_n_requires_n() {
        let (mut config, templates) = example_parts();
        config.campaigns.review.n = None;
        let issues = validate_all(&config, &templates);
        assert!(
            has_code(&issues, "[C7]"),
            "issues: {:?}",
            issue_codes(&issues)
        );
    }

    #[test]
    fn app_check_requires_firebase_provider() {
        let (mut config, templates) = example_parts();
        config.app_check.mode = crate::config::AppCheckMode::Enforce;
        config.app_check.project_number = Some("919552016311".to_string());
        config.identity.provider = IdentityProvider::Jwt {
            algorithms: vec![crate::config::JwtAlgorithm::RS256],
            jwks_url: Some("https://auth.example.com/jwks".to_string()),
            hs256_secret_env: None,
            issuer: "https://auth.example.com/".to_string(),
            audience: "app".to_string(),
            uid_claim: "sub".to_string(),
            name_claim: None,
            max_age_seconds: 3600,
        };
        let issues = validate_all(&config, &templates);
        assert!(
            has_code(&issues, "[V8]"),
            "issues: {:?}",
            issue_codes(&issues)
        );
    }

    fn non_admin_role_template(role: &str) -> Template {
        Template {
            description: "t".to_string(),
            enabled: true,
            senders: vec!["user".to_string()],
            params: BTreeMap::new(),
            context: None,
            vars: BTreeMap::new(),
            to: Target::Role(role.to_string()),
            title: "t".to_string(),
            body: "b".to_string(),
            image: None,
            data: BTreeMap::new(),
            dedupe: None,
            rate: Some(crate::config::Rate {
                per_uid_per_hour: 10,
            }),
        }
    }

    #[test]
    fn g6_non_admin_role_target_must_be_admin_role() {
        let design = non_admin_role_template("member");
        let issues = validate_template("t", &design, &example_config());
        assert!(
            has_code(&issues, "[G6]"),
            "issues: {:?}",
            issue_codes(&issues)
        );
    }

    #[test]
    fn g6_admin_role_target_allowed() {
        let design = non_admin_role_template("admin");
        let issues = validate_template("t", &design, &example_config());
        assert!(
            !has_code(&issues, "[G6]"),
            "issues: {:?}",
            issue_codes(&issues)
        );
    }
}

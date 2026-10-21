//! Template rendering engine (plan 3.5, 3.6, T7).
//!
//! The only path from a stored template and client input to a notification is
//! [`validate_params`] -> [`context_path`] -> [`render`]. Every function here is
//! pure: no network, no database, no environment access, no logging.
//!
//! Substitution happens in one pass over [`placeholder::segments`]: a value is
//! copied into the output as data and is never parsed as a template again.
//!
//! The engine is wired into sending in T8 and into preview in T10; until then
//! its public items are intentionally unused.
#![allow(dead_code)]

use std::collections::BTreeMap;

use serde_json::Value;

use crate::config::placeholder::{self, Placeholder, Ref, Segment, SenderField};
use crate::config::{Param, Template};

/// A raw client value may not exceed this many bytes before any processing.
pub const MAX_RAW_VALUE_BYTES: usize = 4096;
/// A saved pattern may not exceed this length; it is re-checked per request.
pub const MAX_PATTERN_BYTES: usize = 200;
/// Upper bound for the compiled regular expression.
pub const REGEX_SIZE_LIMIT: usize = 100_000;
/// Largest absolute value the `|int` filter accepts.
pub const INT_FILTER_LIMIT: f64 = 1e15;

/// The sender of a notification, taken from the identity token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sender {
    pub uid: String,
    pub name: Option<String>,
    pub role: Option<String>,
}

/// Client parameters that passed [`validate_params`], already stringified.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedParams(BTreeMap<String, String>);

impl ValidatedParams {
    /// The validated text of a parameter, if it was provided.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }
}

/// Why rendering failed. `Forbidden` is deliberately uniform (plan 3.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenderError {
    /// Invalid or missing input, a missing field, an unsupported variable
    /// value, or an exceeded limit. The caller maps it to `400`.
    BadRequest(String),
    /// A `context.require` check failed or the context document is missing.
    /// The caller maps it to a uniform `403`.
    Forbidden,
}

/// The recipient of the rendered notification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolvedTarget {
    Uid(String),
    Role(String),
    Topic(String),
    Condition(String),
}

/// The final notification produced from a template.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rendered {
    pub title: String,
    pub body: String,
    pub image: Option<String>,
    pub data: BTreeMap<String, String>,
    pub target: ResolvedTarget,
    pub dedupe_key: Option<String>,
    pub dedupe_seconds: Option<u32>,
}

fn bad(message: impl Into<String>) -> RenderError {
    RenderError::BadRequest(message.into())
}

// ---------- step 1: parameters ----------

/// Validates the client parameters against `template.params`.
///
/// Unknown and missing parameters are errors; nothing is silently dropped.
pub fn validate_params(template: &Template, raw: &Value) -> Result<ValidatedParams, RenderError> {
    let empty = serde_json::Map::new();
    let provided = match raw {
        Value::Object(map) => map,
        Value::Null => &empty,
        _ => return Err(bad("params must be an object")),
    };

    for key in provided.keys() {
        if !template.params.contains_key(key) {
            return Err(bad(format!("unknown parameter '{key}'")));
        }
    }

    let mut values = BTreeMap::new();
    for (name, param) in &template.params {
        let raw_value = provided
            .get(name)
            .ok_or_else(|| bad(format!("missing parameter '{name}'")))?;
        values.insert(name.clone(), validate_param(name, param, raw_value)?);
    }
    Ok(ValidatedParams(values))
}

fn validate_param(name: &str, param: &Param, value: &Value) -> Result<String, RenderError> {
    if raw_size(value) > MAX_RAW_VALUE_BYTES {
        return Err(bad(format!("parameter '{name}' is too large")));
    }

    match param {
        Param::Text { max } => {
            let text = as_text(name, value)?;
            if text.is_empty() {
                return Err(bad(format!("parameter '{name}' must not be empty")));
            }
            if text.chars().count() > *max as usize {
                return Err(bad(format!(
                    "parameter '{name}' must be at most {max} characters"
                )));
            }
            if text
                .chars()
                .any(|character| character.is_control() && character != '\n' && character != '\t')
            {
                return Err(bad(format!(
                    "parameter '{name}' contains an unsupported control character"
                )));
            }
            Ok(text.to_string())
        }
        Param::Pattern { pattern, max } => {
            let text = as_text(name, value)?;
            if text.chars().any(char::is_control) {
                return Err(bad(format!(
                    "parameter '{name}' contains an unsupported control character"
                )));
            }
            if pattern.len() > MAX_PATTERN_BYTES {
                return Err(bad(format!(
                    "pattern of parameter '{name}' is longer than {MAX_PATTERN_BYTES} bytes"
                )));
            }
            if let Some(max) = max {
                if text.chars().count() > *max as usize {
                    return Err(bad(format!(
                        "parameter '{name}' must be at most {max} characters"
                    )));
                }
            }
            let regex = build_anchored_regex(pattern, name)?;
            if !regex.is_match(text) {
                return Err(bad(format!(
                    "parameter '{name}' does not match its pattern"
                )));
            }
            Ok(text.to_string())
        }
        Param::Enum { values } => {
            let text = as_text(name, value)?;
            if !values.iter().any(|allowed| allowed == text) {
                return Err(bad(format!(
                    "parameter '{name}' must be one of the defined values"
                )));
            }
            Ok(text.to_string())
        }
        Param::Int { min, max } => {
            let number = match value {
                Value::Number(number) => number
                    .as_i64()
                    .ok_or_else(|| bad(format!("parameter '{name}' must be an integer")))?,
                Value::String(text) => text
                    .parse::<i64>()
                    .map_err(|_| bad(format!("parameter '{name}' must be an integer")))?,
                _ => return Err(bad(format!("parameter '{name}' must be an integer"))),
            };
            if let Some(min) = min {
                if number < *min {
                    return Err(bad(format!("parameter '{name}' must be at least {min}")));
                }
            }
            if let Some(max) = max {
                if number > *max {
                    return Err(bad(format!("parameter '{name}' must be at most {max}")));
                }
            }
            Ok(number.to_string())
        }
    }
}

fn raw_size(value: &Value) -> usize {
    match value {
        Value::String(text) => text.len(),
        other => other.to_string().len(),
    }
}

fn as_text<'a>(name: &str, value: &'a Value) -> Result<&'a str, RenderError> {
    value
        .as_str()
        .ok_or_else(|| bad(format!("parameter '{name}' must be a string")))
}

fn build_anchored_regex(pattern: &str, name: &str) -> Result<regex::Regex, RenderError> {
    // Anchored on both ends: `is_match` must cover the whole value.
    let anchored = format!("^(?:{pattern})$");
    regex::RegexBuilder::new(&anchored)
        .size_limit(REGEX_SIZE_LIMIT)
        .build()
        .map_err(|_| bad(format!("pattern of parameter '{name}' is not usable")))
}

// ---------- step 2: the context document path ----------

/// Renders the context document path (`None` without a `context` section).
///
/// Only parameter and sender references are allowed here; every substituted
/// value must be a single path segment.
pub fn context_path(
    template: &Template,
    params: &ValidatedParams,
    sender: &Sender,
) -> Result<Option<String>, RenderError> {
    let context = match &template.context {
        Some(context) => context,
        None => return Ok(None),
    };
    let mut renderer = Renderer {
        template,
        params,
        sender,
        doc: None,
        entity: None,
        vars: BTreeMap::new(),
    };
    let path = renderer.render_text(&context.doc, Mode::Path, true)?;
    Ok(Some(path))
}

// ---------- substitution ----------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Title, body, data and target: plain text substitution.
    Text,
    /// `context.doc`: one path segment, no `/`, `\`, `..` or control chars.
    Path,
}

struct Renderer<'a> {
    template: &'a Template,
    params: &'a ValidatedParams,
    sender: &'a Sender,
    doc: Option<&'a Value>,
    entity: Option<&'a Value>,
    vars: BTreeMap<String, String>,
}

impl Renderer<'_> {
    fn render_text(
        &mut self,
        text: &str,
        mode: Mode,
        allow_vars: bool,
    ) -> Result<String, RenderError> {
        let segments = placeholder::segments(text)
            .map_err(|message| bad(format!("invalid template placeholder: {message}")))?;
        let mut out = String::new();
        for segment in segments {
            match segment {
                Segment::Text(literal) => out.push_str(&literal),
                Segment::Ref(reference) => {
                    let value = self.resolve(&reference, mode, allow_vars)?;
                    out.push_str(&value);
                }
            }
        }
        Ok(out)
    }

    fn resolve(
        &mut self,
        reference: &Placeholder,
        mode: Mode,
        allow_vars: bool,
    ) -> Result<String, RenderError> {
        let value = self.source_value(&reference.r, mode, allow_vars)?;
        let text = if reference.int_filter {
            apply_int_filter(&value)?
        } else {
            scalar_to_text(&value)?
        };
        if mode == Mode::Path {
            validate_path_value(&text)?;
        } else {
            reject_control_chars(&text)?;
        }
        Ok(text)
    }

    fn source_value(
        &mut self,
        reference: &Ref,
        mode: Mode,
        allow_vars: bool,
    ) -> Result<Value, RenderError> {
        match reference {
            Ref::Param(name) => self
                .params
                .get(name)
                .map(|value| Value::String(value.to_string()))
                .ok_or_else(|| bad(format!("parameter '{name}' was not validated"))),
            Ref::Sender(field) => {
                let value = match field {
                    SenderField::Uid => Some(self.sender.uid.clone()),
                    SenderField::Name => self.sender.name.clone(),
                    SenderField::Role => self.sender.role.clone(),
                };
                value
                    .map(Value::String)
                    .ok_or_else(|| bad("sender field is not available"))
            }
            Ref::Doc(path) => {
                let doc = self
                    .doc
                    .ok_or_else(|| bad("template needs a context document"))?;
                lookup(doc, path).cloned()
            }
            Ref::Entity(path) => {
                let entity = self
                    .entity
                    .ok_or_else(|| bad("template needs an entity document"))?;
                lookup(entity, path).cloned()
            }
            Ref::Var(name) => {
                if !allow_vars {
                    return Err(bad("a vars map value must not reference a variable"));
                }
                self.variable(name, mode).map(Value::String)
            }
        }
    }

    fn variable(&mut self, name: &str, mode: Mode) -> Result<String, RenderError> {
        if let Some(value) = self.vars.get(name) {
            return Ok(value.clone());
        }
        let definition = self
            .template
            .vars
            .get(name)
            .cloned()
            .ok_or_else(|| bad(format!("variable '{name}' is not defined")))?;
        let source = parse_var_source(&definition.from)?;
        let value = self.source_value(&source.r, mode, false)?;
        let key = scalar_to_text(&value)?;
        let mapped = definition
            .map
            .get(&key)
            .cloned()
            .ok_or_else(|| bad("unsupported value"))?;
        let rendered = self.render_text(&mapped, mode, false)?;
        // Computed lazily, once, and only when a template references it.
        self.vars.insert(name.to_string(), rendered.clone());
        Ok(rendered)
    }
}

/// Parses `vars.from`, a plain reference without braces.
fn parse_var_source(from: &str) -> Result<Placeholder, RenderError> {
    if from.contains('{') || from.contains('}') {
        return Err(bad("vars.from must be a plain reference without braces"));
    }
    let reference = if let Some(rest) = from.strip_prefix("doc.") {
        if !placeholder::is_path(rest) {
            return Err(bad("vars.from is not a valid dotted path"));
        }
        Ref::Doc(rest.to_string())
    } else if let Some(rest) = from.strip_prefix("p.") {
        if !placeholder::is_name(rest) {
            return Err(bad("vars.from is not a valid parameter name"));
        }
        Ref::Param(rest.to_string())
    } else if let Some(rest) = from.strip_prefix("sender.") {
        match rest {
            "uid" => Ref::Sender(SenderField::Uid),
            "name" => Ref::Sender(SenderField::Name),
            "role" => Ref::Sender(SenderField::Role),
            _ => return Err(bad("vars.from has an unknown sender field")),
        }
    } else if let Some(rest) = from.strip_prefix("entity.") {
        if !placeholder::is_path(rest) {
            return Err(bad("vars.from is not a valid dotted path"));
        }
        Ref::Entity(rest.to_string())
    } else {
        return Err(bad(
            "vars.from must start with 'doc.', 'p.', 'sender.' or 'entity.'",
        ));
    };
    Ok(Placeholder {
        r: reference,
        int_filter: false,
    })
}

/// Reads a dotted path of object keys. Array indices are not supported.
fn lookup<'a>(value: &'a Value, path: &str) -> Result<&'a Value, RenderError> {
    let mut current = value;
    for segment in path.split('.') {
        current = current
            .get(segment)
            .ok_or_else(|| bad(format!("document field '{path}' is missing")))?;
    }
    Ok(current)
}

/// Converts a JSON value to text: no silent default for null/object/array.
fn scalar_to_text(value: &Value) -> Result<String, RenderError> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Bool(flag) => Ok(flag.to_string()),
        Value::Number(number) => {
            if let Some(integer) = number.as_i64() {
                Ok(integer.to_string())
            } else if let Some(integer) = number.as_u64() {
                Ok(integer.to_string())
            } else if let Some(float) = number.as_f64() {
                if float.is_finite() {
                    Ok(format!("{float}"))
                } else {
                    Err(bad("value is not a finite number"))
                }
            } else {
                Err(bad("value is not supported"))
            }
        }
        Value::Null => Err(bad("value is missing")),
        Value::Array(_) | Value::Object(_) => Err(bad("value is not a scalar")),
    }
}

/// `|int`: rounds half away from zero, bounded by [`INT_FILTER_LIMIT`].
fn apply_int_filter(value: &Value) -> Result<String, RenderError> {
    let number = match value {
        Value::Number(number) => number
            .as_f64()
            .ok_or_else(|| bad("value is not a number"))?,
        Value::String(text) => text
            .parse::<f64>()
            .map_err(|_| bad("value is not a number"))?,
        _ => return Err(bad("value is not a number")),
    };
    if !number.is_finite() || number.abs() > INT_FILTER_LIMIT {
        return Err(bad("value is out of range for the 'int' filter"));
    }
    let rounded = number.round();
    if rounded == 0.0 {
        return Ok("0".to_string());
    }
    Ok(format!("{rounded}"))
}

fn reject_control_chars(value: &str) -> Result<(), RenderError> {
    if value
        .chars()
        .any(|character| character.is_control() && character != '\n' && character != '\t')
    {
        return Err(bad("value contains an unsupported control character"));
    }
    Ok(())
}

fn validate_path_value(value: &str) -> Result<(), RenderError> {
    if value.is_empty() {
        return Err(bad("path value must not be empty"));
    }
    if value.contains('/') || value.contains('\\') || value.contains("..") {
        return Err(bad("path value contains a forbidden character"));
    }
    if value.chars().any(char::is_control) {
        return Err(bad("path value contains a control character"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Context, Target};
    use serde_json::json;

    fn base_template() -> Template {
        Template {
            description: String::new(),
            enabled: true,
            senders: vec!["user".to_string()],
            params: BTreeMap::new(),
            context: None,
            vars: BTreeMap::new(),
            to: Target::Topic("news".to_string()),
            title: "Hello".to_string(),
            body: "Body".to_string(),
            image: None,
            data: BTreeMap::new(),
            dedupe: None,
            rate: None,
        }
    }

    fn sender() -> Sender {
        Sender {
            uid: "user-1".to_string(),
            name: Some("Sara".to_string()),
            role: Some("user".to_string()),
        }
    }

    fn params(pairs: &[(&str, &str)]) -> ValidatedParams {
        ValidatedParams(
            pairs
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
        )
    }

    fn text_param(max: u32) -> Param {
        Param::Text { max }
    }

    fn expect_bad<T: std::fmt::Debug>(result: Result<T, RenderError>) -> String {
        match result {
            Err(RenderError::BadRequest(message)) => message,
            other => panic!("expected a bad request, got {other:?}"),
        }
    }

    fn expect_forbidden<T: std::fmt::Debug>(result: Result<T, RenderError>) -> RenderError {
        match result {
            Err(RenderError::Forbidden) => RenderError::Forbidden,
            other => panic!("expected Forbidden, got {other:?}"),
        }
    }

    // ----- parameters -----

    #[test]
    fn param_text_counts_chars_not_bytes() {
        let mut template = base_template();
        template.params.insert("name".to_string(), text_param(5));

        // Five Arabic letters are ten bytes, but five characters.
        let ok = validate_params(&template, &json!({ "name": "مرحبا" })).expect("5 chars fit");
        assert_eq!(ok.get("name"), Some("مرحبا"));

        let too_long = validate_params(&template, &json!({ "name": "مرحبا!" }));
        assert!(expect_bad(too_long).contains("at most 5 characters"));
    }

    #[test]
    fn param_text_rejects_empty_and_control_chars() {
        let mut template = base_template();
        template.params.insert("name".to_string(), text_param(20));

        expect_bad(validate_params(&template, &json!({ "name": "" })));
        expect_bad(validate_params(&template, &json!({ "name": "a\u{0}b" })));
        expect_bad(validate_params(&template, &json!({ "name": "a\u{7f}b" })));

        let allowed = validate_params(&template, &json!({ "name": "line\nnext\tcell" }))
            .expect("newline and tab are allowed");
        assert_eq!(allowed.get("name"), Some("line\nnext\tcell"));
    }

    #[test]
    fn param_pattern_is_anchored_both_ends() {
        let mut template = base_template();
        template.params.insert(
            "code".to_string(),
            Param::Pattern {
                pattern: "[a-z]+".to_string(),
                max: None,
            },
        );

        let ok = validate_params(&template, &json!({ "code": "abc" })).expect("full match");
        assert_eq!(ok.get("code"), Some("abc"));

        // A partial search would accept this value; the anchored regex must not.
        expect_bad(validate_params(&template, &json!({ "code": "abc1" })));
        expect_bad(validate_params(&template, &json!({ "code": "1abc" })));

        let injected = validate_params(&template, &json!({ "code": "abc\n" }));
        assert!(expect_bad(injected).contains("control character"));
    }

    #[test]
    fn param_pattern_oversized_regex_rejected() {
        let mut template = base_template();
        template.params.insert(
            "code".to_string(),
            Param::Pattern {
                pattern: "a".repeat(MAX_PATTERN_BYTES + 1),
                max: None,
            },
        );
        expect_bad(validate_params(&template, &json!({ "code": "a" })));

        // Small pattern, huge compiled program: the size limit must reject it.
        let mut template = base_template();
        template.params.insert(
            "code".to_string(),
            Param::Pattern {
                pattern: "(a{1000}){1000}".to_string(),
                max: None,
            },
        );
        expect_bad(validate_params(&template, &json!({ "code": "a" })));
    }

    #[test]
    fn param_enum_exact_match_only() {
        let mut template = base_template();
        template.params.insert(
            "status".to_string(),
            Param::Enum {
                values: vec!["open".to_string(), "closed".to_string()],
            },
        );

        expect_bad(validate_params(&template, &json!({ "status": "Open" })));
        expect_bad(validate_params(&template, &json!({ "status": " open" })));
        let ok = validate_params(&template, &json!({ "status": "closed" })).expect("exact match");
        assert_eq!(ok.get("status"), Some("closed"));
    }

    #[test]
    fn param_int_bounds_and_rejects_float() {
        let mut template = base_template();
        template.params.insert(
            "count".to_string(),
            Param::Int {
                min: Some(1),
                max: Some(10),
            },
        );

        let ok = validate_params(&template, &json!({ "count": 7 })).expect("in range");
        assert_eq!(ok.get("count"), Some("7"));
        let from_text = validate_params(&template, &json!({ "count": "3" })).expect("numeric text");
        assert_eq!(from_text.get("count"), Some("3"));

        expect_bad(validate_params(&template, &json!({ "count": 0 })));
        expect_bad(validate_params(&template, &json!({ "count": 11 })));
        expect_bad(validate_params(&template, &json!({ "count": 1.5 })));
        expect_bad(validate_params(&template, &json!({ "count": "1.5" })));
        expect_bad(validate_params(&template, &json!({ "count": true })));
    }

    #[test]
    fn param_unknown_key_rejected() {
        let mut template = base_template();
        template.params.insert("code".to_string(), text_param(10));

        let error = expect_bad(validate_params(
            &template,
            &json!({ "code": "a", "extra": "b" }),
        ));
        assert!(error.contains("unknown parameter"), "message: {error}");
    }

    #[test]
    fn param_missing_rejected() {
        let mut template = base_template();
        template.params.insert("code".to_string(), text_param(10));

        expect_bad(validate_params(&template, &json!({})));
        expect_bad(validate_params(&template, &Value::Null));

        // No parameters: null and an empty object are both accepted.
        let empty = base_template();
        assert!(validate_params(&empty, &Value::Null).is_ok());
        assert!(validate_params(&empty, &json!({})).is_ok());
    }

    #[test]
    fn param_value_over_4096_bytes_rejected() {
        let mut template = base_template();
        template
            .params
            .insert("code".to_string(), text_param(10_000));

        let giant = "a".repeat(MAX_RAW_VALUE_BYTES + 1);
        expect_bad(validate_params(&template, &json!({ "code": giant })));
    }

    // ----- context path and require -----

    #[test]
    fn context_path_rejects_slash_dotdot_and_backslash_in_values() {
        let mut template = base_template();
        template.params.insert("id".to_string(), text_param(100));
        template.context = Some(Context {
            doc: "store_orders/{p.id}".to_string(),
            require: BTreeMap::new(),
        });

        for value in ["../other", "a/b", "a\\b", "a\u{0}b", ""] {
            let error = context_path(&template, &params(&[("id", value)]), &sender());
            expect_bad(error);
        }

        let path = context_path(&template, &params(&[("id", "abc-123")]), &sender())
            .expect("safe segment");
        assert_eq!(path.as_deref(), Some("store_orders/abc-123"));
    }

    #[test]
    fn context_path_none_without_context() {
        let template = base_template();
        let path = context_path(&template, &params(&[]), &sender()).expect("no context");
        assert_eq!(path, None);
    }
}

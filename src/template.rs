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

use std::collections::BTreeMap;

use serde_json::Value;

use crate::config::placeholder::{self, Placeholder, Ref, Segment, SenderField};
use crate::config::validate::{is_reserved_data_key, is_role};
use crate::config::{Context, Param, Target, Template};
use crate::identity::is_valid_uid;
use crate::validation::{MAX_BODY_LENGTH, MAX_DATA_BYTES, MAX_IMAGE_URL_LENGTH, MAX_TITLE_LENGTH};

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

// ---------- step 3: the final notification ----------

/// Produces the final notification from a template and its context.
///
/// `context.require` is evaluated first, so a missing or mismatching document
/// is always a uniform [`RenderError::Forbidden`] and never a `BadRequest`.
pub fn render(
    template: &Template,
    params: &ValidatedParams,
    sender: &Sender,
    doc: Option<&Value>,
    entity: Option<&Value>,
) -> Result<Rendered, RenderError> {
    let mut renderer = Renderer {
        template,
        params,
        sender,
        doc,
        entity,
        vars: BTreeMap::new(),
    };

    if let Some(context) = &template.context {
        check_require(&mut renderer, context)?;
    }

    let title = renderer.render_text(&template.title, Mode::Text, true)?;
    if title.trim().is_empty() {
        return Err(bad("title is empty after rendering"));
    }
    let body = renderer.render_text(&template.body, Mode::Text, true)?;
    let image = match &template.image {
        Some(definition) => Some(renderer.render_text(definition, Mode::Text, true)?),
        None => None,
    };

    let mut data = BTreeMap::new();
    for (key, definition) in &template.data {
        if is_reserved_data_key(key) {
            return Err(bad(format!("data key '{key}' is reserved")));
        }
        data.insert(
            key.clone(),
            renderer.render_text(definition, Mode::Text, true)?,
        );
    }

    let target_text = renderer.render_text(template.to.value(), Mode::Text, true)?;
    let target = match &template.to {
        Target::Uid(_) => {
            if !is_valid_uid(&target_text) {
                return Err(bad("uid target is not valid"));
            }
            ResolvedTarget::Uid(target_text)
        }
        Target::Role(_) => {
            if !is_role(&target_text) {
                return Err(bad("role target must match ^[a-z0-9_]{1,32}$"));
            }
            ResolvedTarget::Role(target_text)
        }
        Target::Topic(_) => ResolvedTarget::Topic(target_text),
        Target::Condition(_) => ResolvedTarget::Condition(target_text),
    };

    let (dedupe_key, dedupe_seconds) = match &template.dedupe {
        Some(dedupe) => (
            Some(renderer.render_text(&dedupe.key, Mode::Text, true)?),
            Some(dedupe.seconds),
        ),
        None => (None, None),
    };

    enforce_limits(&title, &body, image.as_deref(), &data)?;

    Ok(Rendered {
        title,
        body,
        image,
        data,
        target,
        dedupe_key,
        dedupe_seconds,
    })
}

fn enforce_limits(
    title: &str,
    body: &str,
    image: Option<&str>,
    data: &BTreeMap<String, String>,
) -> Result<(), RenderError> {
    if title.chars().count() > MAX_TITLE_LENGTH {
        return Err(bad(format!(
            "title must be at most {MAX_TITLE_LENGTH} characters"
        )));
    }
    if body.chars().count() > MAX_BODY_LENGTH {
        return Err(bad(format!(
            "body must be at most {MAX_BODY_LENGTH} characters"
        )));
    }
    if let Some(image) = image {
        if image.len() > MAX_IMAGE_URL_LENGTH {
            return Err(bad(format!(
                "image URL must be at most {MAX_IMAGE_URL_LENGTH} characters"
            )));
        }
    }
    // Same measure as `validation::validate_send`: the serialized JSON size.
    let size = serde_json::to_vec(data)
        .map_err(|_| bad("data could not be serialized"))?
        .len();
    if size > MAX_DATA_BYTES {
        return Err(bad(format!(
            "data must be at most {MAX_DATA_BYTES} bytes when serialized"
        )));
    }
    Ok(())
}

fn check_require(renderer: &mut Renderer<'_>, context: &Context) -> Result<(), RenderError> {
    let doc = renderer.doc;
    for (field, definition) in &context.require {
        // Require values are computed from parameters and the sender only.
        let expected = renderer.render_text(definition, Mode::Text, false)?;
        let document = doc.ok_or(RenderError::Forbidden)?;
        let actual = match lookup(document, field) {
            Ok(value) => scalar_to_text(value).map_err(|_| RenderError::Forbidden)?,
            Err(_) => return Err(RenderError::Forbidden),
        };
        if actual != expected {
            return Err(RenderError::Forbidden);
        }
    }
    Ok(())
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
    use crate::config::ConfigDocument;
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

    // ----- render -----

    #[test]
    fn render_substitutes_all_sources() {
        let mut template = base_template();
        template.title = "{p.subject} from {sender.name} ({sender.role})".to_string();
        template.body = "{doc.text} for {entity.name}".to_string();
        template
            .data
            .insert("id".to_string(), "{sender.uid}".to_string());
        template.to = Target::Uid("{doc.owner}".to_string());

        let doc = json!({ "text": "body text", "owner": "owner-9" });
        let entity = json!({ "name": "ACME" });
        let validated = params(&[("subject", "Order")]);
        let rendered = render(&template, &validated, &sender(), Some(&doc), Some(&entity))
            .expect("all sources resolve");

        assert_eq!(rendered.title, "Order from Sara (user)");
        assert_eq!(rendered.body, "body text for ACME");
        assert_eq!(rendered.data.get("id").map(String::as_str), Some("user-1"));
        assert_eq!(rendered.target, ResolvedTarget::Uid("owner-9".to_string()));
    }

    #[test]
    fn p_value_with_placeholder_is_not_reinterpreted() {
        let mut template = base_template();
        template.title = "{p.text}".to_string();
        let validated = params(&[("text", "{doc.secret}")]);
        let doc = json!({ "secret": "leaked" });

        let rendered = render(&template, &validated, &sender(), Some(&doc), None)
            .expect("a substituted value is data");
        assert_eq!(rendered.title, "{doc.secret}");
    }

    #[test]
    fn doc_value_with_placeholder_is_not_reinterpreted() {
        let mut template = base_template();
        template.body = "{doc.note}".to_string();
        let validated = params(&[("text", "leaked")]);
        let doc = json!({ "note": "{p.text}" });

        let rendered = render(&template, &validated, &sender(), Some(&doc), None)
            .expect("a document value is data");
        assert_eq!(rendered.body, "{p.text}");
    }

    #[test]
    fn int_filter_rounds_150000_point_0() {
        let mut template = base_template();
        template.body = "total {doc.total|int}".to_string();
        let doc = json!({ "total": 150000.0 });

        let rendered = render(&template, &params(&[]), &sender(), Some(&doc), None)
            .expect("integral float renders");
        assert_eq!(rendered.body, "total 150000");
    }

    #[test]
    fn int_filter_rounds_half_away_from_zero() {
        let mut template = base_template();
        template.body = "{doc.up|int}|{doc.down|int}|{doc.text|int}".to_string();
        let doc = json!({ "up": 2.5, "down": -2.5, "text": "150000.6" });

        let rendered = render(&template, &params(&[]), &sender(), Some(&doc), None)
            .expect("halves round away from zero");
        assert_eq!(rendered.body, "3|-3|150001");
    }

    #[test]
    fn int_filter_rejects_non_numeric_and_huge() {
        let mut template = base_template();
        template.body = "{doc.value|int}".to_string();

        for value in [json!("abc"), json!(1e16), json!(-1e16), json!(true)] {
            let doc = json!({ "value": value });
            expect_bad(render(&template, &params(&[]), &sender(), Some(&doc), None));
        }

        let null_doc = json!({ "value": null });
        expect_bad(render(
            &template,
            &params(&[]),
            &sender(),
            Some(&null_doc),
            None,
        ));
    }

    #[test]
    fn float_without_filter_prints_integer_form() {
        let mut template = base_template();
        template.body = "{doc.total} {doc.rate}".to_string();
        let doc = json!({ "total": 150000.0, "rate": 1.25 });

        let rendered = render(&template, &params(&[]), &sender(), Some(&doc), None)
            .expect("floats render with their shortest form");
        assert_eq!(rendered.body, "150000 1.25");
    }

    #[test]
    fn missing_or_null_or_object_field_is_bad_request() {
        let mut template = base_template();
        template.body = "{doc.value}".to_string();

        for doc in [
            json!({}),
            json!({ "value": null }),
            json!({ "value": { "nested": 1 } }),
            json!({ "value": [1, 2] }),
        ] {
            expect_bad(render(&template, &params(&[]), &sender(), Some(&doc), None));
        }

        // Array indices are not object keys, so this is also a bad request.
        let array_doc = json!({ "value": [{ "x": 1 }] });
        let mut indexed = base_template();
        indexed.body = "{doc.value.0.x}".to_string();
        expect_bad(render(
            &indexed,
            &params(&[]),
            &sender(),
            Some(&array_doc),
            None,
        ));
    }

    #[test]
    fn arabic_text_is_preserved_exactly() {
        let mut template = base_template();
        template.title = "تحديث الطلب #{doc.number}".to_string();
        template.body = "{doc.message}".to_string();
        let doc = json!({
            "number": 42,
            "message": "تم شحن طلبك وهو في الطريق إليك — شكراً",
        });

        let rendered = render(&template, &params(&[]), &sender(), Some(&doc), None)
            .expect("arabic text renders");
        assert_eq!(rendered.title, "تحديث الطلب #42");
        assert_eq!(rendered.body, "تم شحن طلبك وهو في الطريق إليك — شكراً");
        assert_eq!(rendered.title.chars().count(), 15);
    }

    #[test]
    fn control_chars_in_substituted_value_rejected() {
        let mut template = base_template();
        template.body = "{doc.value}".to_string();

        let bad_doc = json!({ "value": "a\u{0}b" });
        expect_bad(render(
            &template,
            &params(&[]),
            &sender(),
            Some(&bad_doc),
            None,
        ));

        let good_doc = json!({ "value": "a\nb\tc" });
        let rendered = render(&template, &params(&[]), &sender(), Some(&good_doc), None)
            .expect("newline and tab survive");
        assert_eq!(rendered.body, "a\nb\tc");
    }

    #[test]
    fn title_empty_after_render_rejected() {
        let mut template = base_template();
        template.title = "{doc.title}".to_string();
        template.body = "Body".to_string();
        let doc = json!({ "title": "" });

        expect_bad(render(&template, &params(&[]), &sender(), Some(&doc), None));
    }

    #[test]
    fn title_body_limits_enforced() {
        let mut template = base_template();
        template.title = "{doc.title}".to_string();
        template.body = "{doc.body}".to_string();

        let long_title = json!({ "title": "t".repeat(MAX_TITLE_LENGTH + 1), "body": "b" });
        expect_bad(render(
            &template,
            &params(&[]),
            &sender(),
            Some(&long_title),
            None,
        ));

        let long_body = json!({ "title": "t", "body": "b".repeat(MAX_BODY_LENGTH + 1) });
        expect_bad(render(
            &template,
            &params(&[]),
            &sender(),
            Some(&long_body),
            None,
        ));

        let at_limit = json!({
            "title": "t".repeat(MAX_TITLE_LENGTH),
            "body": "b".repeat(MAX_BODY_LENGTH),
        });
        assert!(render(&template, &params(&[]), &sender(), Some(&at_limit), None).is_ok());
    }

    #[test]
    fn data_total_size_limit_enforced() {
        let mut template = base_template();
        template
            .data
            .insert("payload".to_string(), "{doc.payload}".to_string());
        let doc = json!({ "payload": "x".repeat(MAX_DATA_BYTES) });

        expect_bad(render(&template, &params(&[]), &sender(), Some(&doc), None));
    }

    #[test]
    fn reserved_data_key_rejected() {
        for key in ["from", "notification", "message_type", "google.x", "gcm.y"] {
            let mut template = base_template();
            template.data.insert(key.to_string(), "value".to_string());
            let error = expect_bad(render(&template, &params(&[]), &sender(), None, None));
            assert!(error.contains("reserved"), "key {key}: {error}");
        }
    }

    #[test]
    fn target_uid_must_be_valid_uid() {
        let mut template = base_template();
        template.to = Target::Uid("{doc.uid}".to_string());

        expect_bad(render(
            &template,
            &params(&[]),
            &sender(),
            Some(&json!({ "uid": "" })),
            None,
        ));
        expect_bad(render(
            &template,
            &params(&[]),
            &sender(),
            Some(&json!({ "uid": "u".repeat(129) })),
            None,
        ));

        let rendered = render(
            &template,
            &params(&[]),
            &sender(),
            Some(&json!({ "uid": "user-42" })),
            None,
        )
        .expect("valid uid");
        assert_eq!(rendered.target, ResolvedTarget::Uid("user-42".to_string()));
    }

    #[test]
    fn target_role_charset_enforced() {
        let mut template = base_template();
        template.to = Target::Role("{doc.role}".to_string());

        for role in ["Admin", "admin role", ""] {
            expect_bad(render(
                &template,
                &params(&[]),
                &sender(),
                Some(&json!({ "role": role })),
                None,
            ));
        }
        let too_long = "a".repeat(33);
        expect_bad(render(
            &template,
            &params(&[]),
            &sender(),
            Some(&json!({ "role": too_long })),
            None,
        ));

        let rendered = render(
            &template,
            &params(&[]),
            &sender(),
            Some(&json!({ "role": "admin_1" })),
            None,
        )
        .expect("valid role");
        assert_eq!(rendered.target, ResolvedTarget::Role("admin_1".to_string()));
    }

    #[test]
    fn dedupe_key_rendered() {
        let mut template = base_template();
        template.dedupe = Some(crate::config::Dedupe {
            key: "{p.order_id}:{doc.status}".to_string(),
            seconds: 600,
        });
        let doc = json!({ "status": "shipped" });

        let rendered = render(
            &template,
            &params(&[("order_id", "abc")]),
            &sender(),
            Some(&doc),
            None,
        )
        .expect("dedupe key renders");
        assert_eq!(rendered.dedupe_key.as_deref(), Some("abc:shipped"));
        assert_eq!(rendered.dedupe_seconds, Some(600));

        let without = base_template();
        let plain = render(&without, &params(&[]), &sender(), None, None).expect("no dedupe");
        assert_eq!(plain.dedupe_key, None);
        assert_eq!(plain.dedupe_seconds, None);
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

    fn template_with_require() -> Template {
        let mut template = base_template();
        template
            .params
            .insert("order_id".to_string(), text_param(64));
        template.context = Some(Context {
            doc: "store_orders/{p.order_id}".to_string(),
            require: BTreeMap::from([("buyer_id".to_string(), "{sender.uid}".to_string())]),
        });
        template.to = Target::Uid("{doc.buyer_id}".to_string());
        template.body = "{doc.note}".to_string();
        template
    }

    #[test]
    fn require_match_passes() {
        let template = template_with_require();
        let doc = json!({ "buyer_id": "user-1", "note": "ok" });

        let rendered = render(
            &template,
            &params(&[("order_id", "a1")]),
            &sender(),
            Some(&doc),
            None,
        )
        .expect("require matches");
        assert_eq!(rendered.body, "ok");
    }

    #[test]
    fn require_mismatch_is_forbidden() {
        let template = template_with_require();
        let doc = json!({ "buyer_id": "someone-else", "note": "ok" });

        expect_forbidden(render(
            &template,
            &params(&[("order_id", "a1")]),
            &sender(),
            Some(&doc),
            None,
        ));
    }

    #[test]
    fn require_missing_doc_is_forbidden() {
        let template = template_with_require();
        expect_forbidden(render(
            &template,
            &params(&[("order_id", "a1")]),
            &sender(),
            None,
            None,
        ));
    }

    #[test]
    fn require_missing_field_is_forbidden() {
        let template = template_with_require();
        let doc = json!({ "note": "ok" });

        expect_forbidden(render(
            &template,
            &params(&[("order_id", "a1")]),
            &sender(),
            Some(&doc),
            None,
        ));
    }

    #[test]
    fn forbidden_is_uniform_for_all_three_cases() {
        let template = template_with_require();

        let mismatch = render(
            &template,
            &params(&[("order_id", "a1")]),
            &sender(),
            Some(&json!({ "buyer_id": "other" })),
            None,
        );
        let missing_doc = render(
            &template,
            &params(&[("order_id", "a1")]),
            &sender(),
            None,
            None,
        );
        let missing_field = render(
            &template,
            &params(&[("order_id", "a1")]),
            &sender(),
            Some(&json!({ "note": "x" })),
            None,
        );

        let a = expect_forbidden(mismatch);
        let b = expect_forbidden(missing_doc);
        let c = expect_forbidden(missing_field);
        assert_eq!(a, b);
        assert_eq!(b, c);
        assert_eq!(a, RenderError::Forbidden);
    }

    #[test]
    fn require_runs_before_other_substitution() {
        let template = template_with_require();
        // `note` is missing, so rendering the body would be a bad request; the
        // failing require must win and stay a uniform Forbidden.
        let doc = json!({ "buyer_id": "other" });

        expect_forbidden(render(
            &template,
            &params(&[("order_id", "a1")]),
            &sender(),
            Some(&doc),
            None,
        ));
    }

    // ----- vars -----

    fn template_with_var(map: &[(&str, &str)], from: &str) -> Template {
        let mut template = base_template();
        template.vars.insert(
            "status_text".to_string(),
            crate::config::Var {
                from: from.to_string(),
                map: map
                    .iter()
                    .map(|(key, value)| (key.to_string(), value.to_string()))
                    .collect(),
            },
        );
        template
    }

    #[test]
    fn var_maps_doc_value() {
        let mut template = template_with_var(&[("shipped", "on the way")], "doc.status");
        template.body = "{status_text}".to_string();
        let doc = json!({ "status": "shipped" });

        let rendered = render(&template, &params(&[]), &sender(), Some(&doc), None)
            .expect("var maps the document value");
        assert_eq!(rendered.body, "on the way");
    }

    #[test]
    fn var_unknown_value_is_bad_request_without_echo() {
        let mut template = template_with_var(&[("shipped", "on the way")], "doc.status");
        template.body = "{status_text}".to_string();
        let doc = json!({ "status": "top-secret-status" });

        let error = expect_bad(render(&template, &params(&[]), &sender(), Some(&doc), None));
        assert!(error.contains("unsupported value"), "message: {error}");
        assert!(
            !error.contains("top-secret-status"),
            "the value must not be echoed: {error}"
        );
    }

    #[test]
    fn var_map_value_may_use_other_sources() {
        let mut template = template_with_var(
            &[("yes", "order {doc.number} for {sender.name} ({p.channel})")],
            "doc.flag",
        );
        template.body = "{status_text}".to_string();
        template
            .params
            .insert("channel".to_string(), text_param(20));
        let doc = json!({ "flag": "yes", "number": 42 });

        let rendered = render(
            &template,
            &params(&[("channel", "app")]),
            &sender(),
            Some(&doc),
            None,
        )
        .expect("map value renders other sources");
        assert_eq!(rendered.body, "order 42 for Sara (app)");
    }

    #[test]
    fn var_referencing_var_rejected() {
        let mut template = template_with_var(&[("yes", "value {status_text}")], "doc.flag");
        template.body = "{status_text}".to_string();
        let doc = json!({ "flag": "yes" });

        let error = expect_bad(render(&template, &params(&[]), &sender(), Some(&doc), None));
        assert!(error.contains("variable"), "message: {error}");
    }

    #[test]
    fn var_computed_lazily_and_only_when_used() {
        let mut template = template_with_var(&[("yes", "used")], "doc.missing");
        // A second, broken variable is defined but never referenced.
        template.vars.insert(
            "unused".to_string(),
            crate::config::Var {
                from: "doc.missing".to_string(),
                map: BTreeMap::new(),
            },
        );
        template.body = "{status_text}".to_string();
        // The used variable succeeds even though the unused one cannot resolve.
        template.vars.insert(
            "status_text".to_string(),
            crate::config::Var {
                from: "doc.flag".to_string(),
                map: BTreeMap::from([("yes".to_string(), "used".to_string())]),
            },
        );
        let doc = json!({ "flag": "yes" });

        let rendered = render(&template, &params(&[]), &sender(), Some(&doc), None)
            .expect("the unused variable is never computed");
        assert_eq!(rendered.body, "used");
    }

    // ----- the shipped example -----

    #[test]
    fn watt_example_templates_render() {
        let document: ConfigDocument =
            serde_json::from_str(include_str!("../policies/watt.example.json"))
                .expect("the example policy must parse");
        let (_, templates) = document.into_parts();

        // admin_alert: no context, role target.
        let admin_alert = templates.get("admin_alert").expect("admin_alert exists");
        let admin_params = validate_params(
            admin_alert,
            &json!({
                "title": "New brand",
                "body": "ACME added brand X",
                "route": "/brands/acme",
            }),
        )
        .expect("admin_alert params are valid");
        let admin =
            render(admin_alert, &admin_params, &sender(), None, None).expect("admin_alert renders");
        assert_eq!(admin.title, "New brand");
        assert_eq!(admin.body, "ACME added brand X");
        assert_eq!(
            admin.data.get("target").map(String::as_str),
            Some("/brands/acme")
        );
        assert_eq!(admin.target, ResolvedTarget::Role("admin".to_string()));

        // order_new: the buyer sends it, the seller receives it.
        let order_new = templates.get("order_new").expect("order_new exists");
        let order_params = validate_params(order_new, &json!({ "order_id": "abc123" }))
            .expect("order_new params are valid");
        let order_doc = json!({
            "order_number": 42,
            "buyer_id": "user-1",
            "buyer_name": "Ahmed",
            "seller_id": "seller-7",
            "seller_name": "Sara",
            "total": 150000.0,
            "currency": "IQD",
        });
        let path = context_path(order_new, &order_params, &sender()).expect("context path");
        assert_eq!(path.as_deref(), Some("store_orders/abc123"));
        let created = render(order_new, &order_params, &sender(), Some(&order_doc), None)
            .expect("order_new renders");
        assert_eq!(created.title, "طلب جديد #42");
        assert_eq!(created.body, "طلب جديد من Ahmed بقيمة 150000 IQD");
        assert_eq!(created.target, ResolvedTarget::Uid("seller-7".to_string()));
        assert_eq!(
            created.data.get("target").map(String::as_str),
            Some("/store/orders/abc123")
        );
        assert_eq!(created.dedupe_key.as_deref(), Some("abc123"));
        assert_eq!(created.dedupe_seconds, Some(600));

        // order_status: the seller sends it, the buyer receives it.
        let order_status = templates.get("order_status").expect("order_status exists");
        let status_params = validate_params(order_status, &json!({ "order_id": "abc123" }))
            .expect("order_status params are valid");
        let seller = Sender {
            uid: "seller-7".to_string(),
            name: Some("Sara".to_string()),
            role: Some("company".to_string()),
        };
        let status_doc = json!({
            "order_number": 42,
            "buyer_id": "user-1",
            "seller_id": "seller-7",
            "seller_name": "Sara",
            "status": "shipped",
        });
        let status_path =
            context_path(order_status, &status_params, &seller).expect("context path");
        assert_eq!(status_path.as_deref(), Some("store_orders/abc123"));
        let status = render(
            order_status,
            &status_params,
            &seller,
            Some(&status_doc),
            None,
        )
        .expect("order_status renders");
        assert_eq!(status.title, "تحديث الطلب #42");
        assert_eq!(status.body, "تم شحن طلبك #42 وهو في الطريق إليك");
        assert_eq!(status.target, ResolvedTarget::Uid("user-1".to_string()));
        assert_eq!(
            status.data.get("new_status").map(String::as_str),
            Some("shipped")
        );
        assert_eq!(status.dedupe_key.as_deref(), Some("abc123:shipped"));
    }

    #[test]
    fn renderer_has_no_io_dependencies() {
        let source = include_str!("template.rs");
        // Split so this test does not match its own needle.
        let forbidden = [
            "reqw".to_string() + "est",
            "sql".to_string() + "x",
            "tokio".to_string() + "::fs",
            "std::".to_string() + "env",
        ];
        for needle in forbidden {
            assert!(
                !source.contains(&needle),
                "template.rs must stay free of I/O: found {needle}"
            );
        }
    }
}

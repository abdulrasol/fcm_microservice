//! Template language parser (plan 3.6).
//!
//! Supported forms: `{p.NAME}`, `{sender.uid|name|role}`, `{doc.PATH}`,
//! `{entity.PATH}` and `{VARNAME}`, with an optional `|int` filter after the reference.
//!
//! No escape characters: an unclosed `{`, a stray `}` or malformed content is
//! an error that points at the position (fail-closed).

/// The reference inside `{}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ref {
    /// `{p.name}` -- a parameter supplied by the client.
    Param(String),
    /// `{sender.uid}` -- from the token.
    Sender(SenderField),
    /// `{doc.path}` -- from the `context.doc` document.
    Doc(String),
    /// `{entity.path}` -- from the company document (campaigns).
    Entity(String),
    /// `{name}` -- from the `vars` map.
    Var(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SenderField {
    Uid,
    Name,
    Role,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placeholder {
    pub r: Ref,
    /// Was the `|int` filter written after the reference?
    pub int_filter: bool,
}

/// One piece of a parsed text: literal text or a placeholder, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Segment {
    Text(String),
    Ref(Placeholder),
}

const MAX_NAME_LEN: usize = 30;
const MAX_PATH_SEGMENTS: usize = 5;

/// Extracts every placeholder from a text.
///
/// Returns `Err` with a message that pinpoints the position on any syntax error.
pub fn extract(s: &str) -> Result<Vec<Placeholder>, String> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;

    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                let open = i;
                let close = match find_close(bytes, i) {
                    Some(pos) => pos,
                    None => return Err(format!("unclosed '{{' at position {}", open)),
                };
                let inner = &s[open + 1..close];
                if inner.contains('{') {
                    return Err(format!(
                        "nested '{{' inside placeholder at position {}",
                        open
                    ));
                }
                out.push(parse_placeholder(inner, open)?);
                i = close + 1;
            }
            b'}' => {
                return Err(format!("stray '}}' at position {}", i));
            }
            _ => i += 1,
        }
    }

    Ok(out)
}

/// Is the text literal, without any placeholder?
///
/// Text with a syntax error is not "literal", so this returns `false` (fail-closed).
pub fn is_literal(s: &str) -> bool {
    matches!(extract(s), Ok(placeholders) if placeholders.is_empty())
}

/// Splits a text into its literal segments and placeholders, in order.
///
/// Same rules and the same error messages as [`extract`]: every check is
/// delegated to it, so the two entry points can never disagree.
pub fn segments(s: &str) -> Result<Vec<Segment>, String> {
    let placeholders = extract(s)?;
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut found = placeholders.into_iter();
    let mut cursor = 0usize;
    let mut text_start = 0usize;

    while cursor < bytes.len() {
        if bytes[cursor] == b'{' {
            let close = match find_close(bytes, cursor) {
                Some(position) => position,
                // Unreachable after a successful `extract`; fail closed anyway.
                None => return Err(format!("unclosed '{{' at position {}", cursor)),
            };
            if cursor > text_start {
                out.push(Segment::Text(s[text_start..cursor].to_string()));
            }
            match found.next() {
                Some(placeholder) => out.push(Segment::Ref(placeholder)),
                // Unreachable after a successful `extract`; fail closed anyway.
                None => {
                    return Err(format!("unexpected placeholder at position {}", cursor));
                }
            }
            cursor = close + 1;
            text_start = cursor;
        } else {
            cursor += 1;
        }
    }

    if text_start < bytes.len() {
        out.push(Segment::Text(s[text_start..].to_string()));
    }
    Ok(out)
}

/// Finds the matching `}` (nested braces are not supported).
fn find_close(bytes: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn parse_placeholder(inner: &str, position: usize) -> Result<Placeholder, String> {
    // Filter: the last `|` is the separator (a reference never contains `|`).
    let (reference, int_filter) = match inner.rfind('|') {
        Some(pos) => {
            let filter = &inner[pos + 1..];
            if filter != "int" {
                return Err(format!(
                    "unknown filter '{}' at position {} (only 'int' is supported)",
                    filter, position
                ));
            }
            (&inner[..pos], true)
        }
        None => (inner, false),
    };

    if reference.is_empty() {
        return Err(format!("empty placeholder at position {}", position));
    }

    let r = if let Some(rest) = reference.strip_prefix("p.") {
        if !is_name(rest) {
            return Err(format!(
                "invalid parameter name '{}' at position {}",
                rest, position
            ));
        }
        Ref::Param(rest.to_string())
    } else if let Some(rest) = reference.strip_prefix("sender.") {
        let field = match rest {
            "uid" => SenderField::Uid,
            "name" => SenderField::Name,
            "role" => SenderField::Role,
            other => {
                return Err(format!(
                    "unknown sender field '{}' at position {} (uid|name|role)",
                    other, position
                ))
            }
        };
        Ref::Sender(field)
    } else if let Some(rest) = reference.strip_prefix("doc.") {
        if !is_path(rest) {
            return Err(format!(
                "invalid document path '{}' at position {}",
                rest, position
            ));
        }
        Ref::Doc(rest.to_string())
    } else if let Some(rest) = reference.strip_prefix("entity.") {
        if !is_path(rest) {
            return Err(format!(
                "invalid entity path '{}' at position {}",
                rest, position
            ));
        }
        Ref::Entity(rest.to_string())
    } else {
        if !is_name(reference) {
            return Err(format!(
                "invalid placeholder reference '{}' at position {}",
                reference, position
            ));
        }
        Ref::Var(reference.to_string())
    };

    Ok(Placeholder { r, int_filter })
}

/// A parameter or variable name: `[a-z][a-z0-9_]{0,29}`.
pub fn is_name(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() => {}
        _ => return false,
    }
    if value.len() > MAX_NAME_LEN {
        return false;
    }
    value
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// A dotted path: at most 5 segments of `[A-Za-z_][A-Za-z0-9_]*`.
pub fn is_path(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    let segments: Vec<&str> = value.split('.').collect();
    if segments.len() > MAX_PATH_SEGMENTS {
        return false;
    }
    segments.iter().all(|segment| {
        let mut chars = segment.chars();
        match chars.next() {
            Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
            _ => return false,
        }
        segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_parses_all_forms() {
        let found = extract(
            "order {p.order_id} from {sender.uid} named {sender.name} role {sender.role} \
             number {doc.order_number} sum {doc.total|int} shop {entity.name} state {status_text}",
        )
        .expect("all forms must parse");

        assert_eq!(found.len(), 8);
        assert_eq!(found[0].r, Ref::Param("order_id".to_string()));
        assert_eq!(found[1].r, Ref::Sender(SenderField::Uid));
        assert_eq!(found[2].r, Ref::Sender(SenderField::Name));
        assert_eq!(found[3].r, Ref::Sender(SenderField::Role));
        assert_eq!(found[4].r, Ref::Doc("order_number".to_string()));
        assert_eq!(found[5].r, Ref::Doc("total".to_string()));
        assert_eq!(found[6].r, Ref::Entity("name".to_string()));
        assert_eq!(found[7].r, Ref::Var("status_text".to_string()));
        assert!(!found[0].int_filter);
    }

    #[test]
    fn placeholder_rejects_stray_braces() {
        let unclosed = extract("text {p.name").expect_err("unclosed brace must fail");
        assert!(unclosed.contains("unclosed"), "message: {}", unclosed);

        let stray = extract("text } more").expect_err("stray brace must fail");
        assert!(stray.contains("stray"), "message: {}", stray);

        let nested = extract("{p.{name}}").expect_err("nested brace must fail");
        assert!(nested.contains("nested"), "message: {}", nested);
    }

    #[test]
    fn placeholder_rejects_unknown_filter() {
        let error = extract("{doc.total|float}").expect_err("unknown filter must fail");
        assert!(error.contains("unknown filter"), "message: {}", error);
        assert!(error.contains("float"), "message: {}", error);
    }

    #[test]
    fn placeholder_int_filter_flag() {
        let found = extract("{doc.total|int}").expect("int filter must parse");
        assert_eq!(found.len(), 1);
        assert!(found[0].int_filter);
        assert_eq!(found[0].r, Ref::Doc("total".to_string()));

        let plain = extract("{doc.total}").expect("plain ref must parse");
        assert!(!plain[0].int_filter);
    }

    #[test]
    fn placeholder_rejects_bad_names_and_paths() {
        assert!(extract("{p.OrderId}").is_err(), "uppercase name must fail");
        assert!(extract("{p.}").is_err(), "empty name must fail");
        assert!(extract("{doc.}").is_err(), "empty path must fail");
        assert!(
            extract("{doc.a.b.c.d.e.f}").is_err(),
            "6 segments must fail"
        );
        assert!(
            extract("{sender.email}").is_err(),
            "unknown sender field must fail"
        );
    }

    #[test]
    fn placeholder_is_literal() {
        assert!(is_literal("text without placeholders"));
        assert!(!is_literal("text {p.name}"));
        assert!(!is_literal("text {broken"), "malformed text is not literal");
    }

    #[test]
    fn segments_preserves_order_and_literals() {
        let parts = segments("a {p.order_id} b {doc.total|int} c").expect("all forms must parse");
        assert_eq!(parts.len(), 5);
        assert_eq!(parts[0], Segment::Text("a ".to_string()));
        assert_eq!(
            parts[1],
            Segment::Ref(Placeholder {
                r: Ref::Param("order_id".to_string()),
                int_filter: false,
            })
        );
        assert_eq!(parts[2], Segment::Text(" b ".to_string()));
        assert_eq!(
            parts[3],
            Segment::Ref(Placeholder {
                r: Ref::Doc("total".to_string()),
                int_filter: true,
            })
        );
        assert_eq!(parts[4], Segment::Text(" c".to_string()));

        assert!(segments("").expect("empty text must parse").is_empty());
        assert_eq!(
            segments("plain").expect("literal text must parse"),
            vec![Segment::Text("plain".to_string())]
        );
        assert_eq!(
            segments("{sender.uid}").expect("one placeholder must parse"),
            vec![Segment::Ref(Placeholder {
                r: Ref::Sender(SenderField::Uid),
                int_filter: false,
            })]
        );
        // Arabic literals and values keep their exact bytes.
        assert_eq!(
            segments("طلب {p.order_id}")
                .expect("arabic literal must parse")
                .first(),
            Some(&Segment::Text("طلب ".to_string()))
        );
    }

    #[test]
    fn segments_same_errors_as_extract() {
        let malformed = [
            "text {p.name",
            "text } more",
            "{p.{name}}",
            "{doc.total|float}",
            "{p.OrderId}",
            "{p.}",
            "{doc.}",
            "{doc.a.b.c.d.e.f}",
            "{sender.email}",
        ];
        for text in malformed {
            let from_extract = extract(text).expect_err("extract must fail");
            let from_segments = segments(text).expect_err("segments must fail");
            assert_eq!(
                from_extract, from_segments,
                "segments must reuse the extract error for {:?}",
                text
            );
        }
    }
}

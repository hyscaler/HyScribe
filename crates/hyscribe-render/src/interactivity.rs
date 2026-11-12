// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Interactivity API's server-side directive processing,
//! `WP_Interactivity_API::process_directives()`, which runs on the markup
//! of each root interactive block.
//!
//! The `interactive`, `context`, `bind`, `class`, `style` and `text`
//! directives are processed, against the server state ported blocks
//! register: the accordion's `isOpen`, derived from its item's context.
//! `each` and `router-region` are not ported yet, so markup that mentions
//! them is left as rendered.

use crate::formatting::find;
use crate::layout::is_void;
use hyscribe_html::{Attribute, TagProcessor};
use hyscribe_kses::esc_html;
use hyscribe_phpser::{Key, Value, is_empty_string, json_decode, json_encode, php_trim};

/// The server state of `namespace` as `wp_interactivity_state()` holds it,
/// closures evaluated against the namespace's `context`: the accordion
/// item's `isOpen` is its `openByDefault`.
fn derived_state(namespace: &[u8], context: &Value) -> Value {
    if namespace != b"core/accordion" {
        return Value::Array(Vec::new());
    }
    let open = context
        .get_str("openByDefault")
        .cloned()
        .unwrap_or(Value::Null);
    Value::Array(vec![(Key::String(b"isOpen".to_vec()), open)])
}

/// The directive processors, in the order WordPress registers them.
#[derive(Clone, Copy)]
enum Directive {
    Interactive,
    Context,
    Bind,
    Class,
    Style,
    Text,
}

const DIRECTIVES: [(&[u8], Directive); 6] = [
    (b"interactive", Directive::Interactive),
    (b"context", Directive::Context),
    (b"bind", Directive::Bind),
    (b"class", Directive::Class),
    (b"style", Directive::Style),
    (b"text", Directive::Text),
];

/// Elements the tag processor reads as text up to their closer, which it
/// never visits; void elements have none either.
const SPECIAL: [&[u8]; 8] = [
    b"SCRIPT",
    b"IFRAME",
    b"NOEMBED",
    b"NOFRAMES",
    b"STYLE",
    b"TEXTAREA",
    b"TITLE",
    b"XMP",
];

/// One directive attribute, as `get_directive_entries()` reads it.
struct Entry {
    namespace: Option<Vec<u8>>,
    value: Value,
    suffix: Option<Vec<u8>>,
    unique_id: Option<Vec<u8>>,
}

/// `process_directives()`: the markup with its directives applied, or as
/// it was when its tags do not balance.
pub(crate) fn process_directives(html: &[u8]) -> Vec<u8> {
    let unported = [&b"data-wp-each"[..], b"data-wp-router-region"];
    if find(html, 0, b"data-wp-").is_none()
        || unported.iter().any(|name| find(html, 0, name).is_some())
    {
        return html.to_vec();
    }
    let mut state = Directives {
        namespaces: Vec::new(),
        contexts: Vec::new(),
    };
    state.run(html).unwrap_or_else(|| html.to_vec())
}

struct Directives {
    /// The namespace stack; PHP pushes `false` where there is none.
    namespaces: Vec<Option<Vec<u8>>>,
    contexts: Vec<Value>,
}

impl Directives {
    /// `_process_directives()`.
    fn run(&mut self, html: &[u8]) -> Option<Vec<u8>> {
        let mut p = TagProcessor::new(html);
        let mut tag_stack: Vec<(Vec<u8>, Vec<Directive>)> = Vec::new();
        while p.next_tag_visiting_closers() {
            let tag = p.get_tag().unwrap_or_default();
            if tag == b"SVG" || tag == b"MATH" {
                skip_to_tag_closer(&mut p, &tag);
                continue;
            }
            let closer = p.is_tag_closer();
            let visits_closer = visits_closer(&tag);
            let directives = if closer {
                match tag_stack.pop() {
                    Some((opener, directives)) if opener == tag => directives,
                    _ => return None,
                }
            } else {
                let directives = present_directives(&p);
                if visits_closer {
                    tag_stack.push((tag.clone(), directives.clone()));
                }
                directives
            };
            if !closer && let Some(text) = self.enter(&mut p, &directives) {
                // The content is replaced at once, so the tags in it are
                // never visited; the closer then leaves the tag.
                let after_opener = p.parsed_up_to();
                skip_to_tag_closer(&mut p, &tag);
                let before_closer = p.token_start().filter(|_| p.is_tag_closer())?;
                p.replace_range(after_opener, before_closer, &text);
                tag_stack.pop();
                for directive in directives.iter().rev() {
                    self.exit(*directive);
                }
                continue;
            }
            if closer || !visits_closer {
                for directive in directives.iter().rev() {
                    self.exit(*directive);
                }
            }
        }
        tag_stack.is_empty().then(|| p.get_updated_html().to_vec())
    }

    /// Each processor's work on entering a tag; the content a `text`
    /// directive gives it, if any.
    fn enter(&mut self, p: &mut TagProcessor, directives: &[Directive]) -> Option<Vec<u8>> {
        let mut text = None;
        for directive in directives {
            match directive {
                Directive::Interactive => self.interactive(p),
                Directive::Context => self.context(p),
                Directive::Bind => self.bind(p),
                Directive::Class => self.class(p),
                Directive::Style => self.style(p),
                Directive::Text => text = self.text(p),
            }
        }
        text
    }

    /// The exit half of the processors that have one.
    fn exit(&mut self, directive: Directive) {
        match directive {
            Directive::Interactive => {
                self.namespaces.pop();
            }
            Directive::Context => {
                self.contexts.pop();
            }
            _ => {}
        }
    }

    /// `data_wp_interactive_processor()` on entering a tag.
    fn interactive(&mut self, p: &mut TagProcessor) {
        let namespace = match p.get_attribute(b"data-wp-interactive") {
            Some(Attribute::Value(value)) => match json_decode(&value) {
                Some(decoded @ Value::Array(_)) => decoded
                    .get_str("namespace")
                    .filter(|v| **v != Value::Null)
                    .map(Value::to_php_string),
                _ => Some(value),
            },
            _ => None,
        };
        let valid = namespace
            .filter(|ns| !is_empty_string(ns) && ns.first().is_some_and(|&b| is_namespace_byte(b)));
        let pushed = match valid {
            Some(ns) => Some(ns),
            None => self.namespaces.last().cloned().flatten(),
        };
        self.namespaces.push(pushed);
    }

    /// `data_wp_context_processor()` on entering a tag.
    fn context(&mut self, p: &mut TagProcessor) {
        let mut context = self
            .contexts
            .last()
            .and_then(Value::as_array)
            .unwrap_or_default()
            .to_vec();
        for entry in self.entries(p, b"context") {
            let (None, Some(namespace)) = (entry.suffix, entry.namespace) else {
                continue;
            };
            let value = match entry.value {
                Value::Array(entries) => entries,
                _ => Vec::new(),
            };
            let replacement = vec![(Key::from_php_string(namespace), Value::Array(value))];
            context = array_replace_recursive(context, replacement);
        }
        self.contexts.push(Value::Array(context));
    }

    /// `data_wp_bind_processor()`.
    fn bind(&self, p: &mut TagProcessor) {
        for entry in self.entries(p, b"bind") {
            let Some(suffix) = entry.suffix.as_ref().filter(|_| entry.unique_id.is_none()) else {
                continue;
            };
            if suffix.is_empty() || suffix.starts_with(b"on") {
                continue;
            }
            let result = match self.evaluate(&entry) {
                Value::Array(_) => Value::Null,
                number @ (Value::Int(_) | Value::Float(_)) => {
                    json_encode(&number).map_or(Value::Null, Value::String)
                }
                other => other,
            };
            let aria_like = suffix.len() > 5 && suffix.get(4) == Some(&b'-');
            match result {
                Value::Null => {
                    p.remove_attribute(suffix);
                }
                Value::Bool(false) if !aria_like => {
                    p.remove_attribute(suffix);
                }
                Value::Bool(flag) if aria_like => {
                    p.set_attribute(suffix, if flag { b"true" } else { b"false" });
                }
                Value::Bool(_) => {
                    p.set_boolean_attribute(suffix);
                }
                other => {
                    p.set_attribute(suffix, &other.to_php_string());
                }
            }
        }
    }

    /// `data_wp_class_processor()`.
    fn class(&self, p: &mut TagProcessor) {
        for entry in self.entries(p, b"class") {
            let Some(suffix) = entry.suffix.clone().filter(|s| !is_empty_string(s)) else {
                continue;
            };
            let class_name = match &entry.unique_id {
                Some(id) => [&suffix[..], b"---", id].concat(),
                None => suffix,
            };
            if self.evaluate(&entry).is_empty() {
                p.remove_class(&class_name);
            } else {
                p.add_class(&class_name);
            }
        }
    }

    /// `data_wp_style_processor()`.
    fn style(&self, p: &mut TagProcessor) {
        for entry in self.entries(p, b"style") {
            let Some(property) = entry.suffix.clone().filter(|s| !is_empty_string(s)) else {
                continue;
            };
            if entry.unique_id.is_some() {
                continue;
            }
            let value = self.evaluate(&entry);
            let current = match p.get_attribute(b"style") {
                Some(Attribute::Value(style)) => style,
                _ => Vec::new(),
            };
            if value.is_empty() && is_empty_string(&current) {
                continue;
            }
            let merged = merge_style_property(&current, &property, &value);
            if is_empty_string(&merged) {
                p.remove_attribute(b"style");
            } else {
                p.set_attribute(b"style", &merged);
            }
        }
    }

    /// `data_wp_text_processor()`: the first plain entry's string or number,
    /// escaped, or nothing, for a tag whose content it replaces.
    fn text(&self, p: &mut TagProcessor) -> Option<Vec<u8>> {
        let entry = self.entries(p, b"text").into_iter().find(|entry| {
            entry.suffix.is_none() && entry.unique_id.is_none() && !entry.value.is_empty()
        })?;
        let tag = p.get_tag().unwrap_or_default();
        if !visits_closer(&tag) {
            return None;
        }
        // `set_content_between_balanced_tags()` escapes again, to the same.
        Some(match self.evaluate(&entry) {
            text @ (Value::String(_) | Value::Int(_) | Value::Float(_)) => {
                esc_html(&text.to_php_string()).into_bytes()
            }
            _ => Vec::new(),
        })
    }

    /// `get_directive_entries()`: the tag's `data-wp-<prefix>` directives,
    /// sorted by suffix and unique ID.
    fn entries(&self, p: &mut TagProcessor, prefix: &[u8]) -> Vec<Entry> {
        let names = p
            .get_attribute_names_with_prefix(&[&b"data-wp-"[..], prefix].concat())
            .unwrap_or_default();
        let default_namespace = self.namespaces.last().cloned().flatten();
        let mut entries: Vec<Entry> = names
            .iter()
            .filter_map(|name| {
                let (parsed_prefix, suffix, unique_id) = parse_directive_name(name)?;
                if parsed_prefix != prefix {
                    return None;
                }
                let (namespace, value) =
                    extract_directive_value(p.get_attribute(name), default_namespace.clone());
                Some(Entry {
                    namespace,
                    value,
                    suffix,
                    unique_id,
                })
            })
            .collect();
        entries.sort_by(|a, b| (&a.suffix, &a.unique_id).cmp(&(&b.suffix, &b.unique_id)));
        entries
    }

    /// `evaluate()`: the value at the entry's path, in the server state or
    /// the context.
    fn evaluate(&self, entry: &Entry) -> Value {
        let Some(namespace) = entry.namespace.as_ref().filter(|ns| !is_empty_string(ns)) else {
            return Value::Null;
        };
        if entry.value.is_empty() {
            return Value::Null;
        }
        let path = entry.value.to_php_string();
        let (negate, path) = match path.strip_prefix(b"!") {
            Some(rest) => (true, rest),
            None => (false, path.as_slice()),
        };
        let key = Key::from_php_string(namespace.clone());
        let context = self
            .contexts
            .last()
            .and_then(|context| context.get(&key))
            .cloned()
            .unwrap_or(Value::Array(Vec::new()));
        let mut current = Value::Array(vec![
            (
                Key::String(b"state".to_vec()),
                derived_state(namespace, &context),
            ),
            (Key::String(b"context".to_vec()), context),
        ]);
        for segment in path.split(|&b| b == b'.') {
            if segment == b"length" {
                match &current {
                    Value::Array(entries) if is_list(entries) => {
                        current = Value::Int(i64::try_from(entries.len()).unwrap_or(i64::MAX));
                        break;
                    }
                    Value::String(text) => {
                        current = Value::Int(i64::try_from(text.len()).unwrap_or(i64::MAX));
                        break;
                    }
                    _ => {}
                }
            }
            match current.get(&Key::from_php_string(segment.to_vec())) {
                Some(next) => current = next.clone(),
                _ => {
                    current = Value::Null;
                    break;
                }
            }
        }
        if negate {
            Value::Bool(current.is_empty())
        } else {
            current
        }
    }
}

/// The registered directives among the tag's attributes, in registration
/// order.
fn present_directives(p: &TagProcessor) -> Vec<Directive> {
    let names = p
        .get_attribute_names_with_prefix(b"data-wp-")
        .unwrap_or_default();
    let prefixes: Vec<Vec<u8>> = names
        .iter()
        .filter_map(|name| parse_directive_name(name).map(|(prefix, _, _)| prefix))
        .collect();
    DIRECTIVES
        .iter()
        .filter(|(name, _)| prefixes.iter().any(|prefix| prefix == name))
        .map(|(_, directive)| *directive)
        .collect()
}

/// `has_and_visits_its_closer_tag()`: neither void nor read as text up to
/// its closer.
fn visits_closer(tag: &[u8]) -> bool {
    !is_void(tag) && !SPECIAL.contains(&tag)
}

/// `skip_to_tag_closer()`: past the closer that balances the current tag.
fn skip_to_tag_closer(p: &mut TagProcessor, tag: &[u8]) {
    let mut depth = 1usize;
    while depth > 0 && p.next_tag_visiting_closers() {
        if p.get_tag().as_deref() != Some(tag) || p.has_self_closing_flag() {
            continue;
        }
        if p.is_tag_closer() {
            depth -= 1;
        } else {
            depth += 1;
        }
    }
}

/// `parse_directive_name()`: prefix, suffix and unique ID of
/// `data-wp-<prefix>--<suffix>---<id>`.
#[allow(clippy::type_complexity)]
fn parse_directive_name(name: &[u8]) -> Option<(Vec<u8>, Option<Vec<u8>>, Option<Vec<u8>>)> {
    let name = name.get(8..).unwrap_or_default();
    // PHP also rejects an empty name and one that starts with `-`, but no
    // directive has either prefix, so they come to the same.
    if !name
        .iter()
        .all(|&b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return None;
    }
    let Some(at) = find(name, 0, b"--") else {
        return Some((name.to_vec(), None, None));
    };
    let prefix = name.get(..at).unwrap_or_default().to_vec();
    let remaining = name.get(at..).unwrap_or_default();
    let non_empty = |text: &[u8]| (!text.is_empty()).then(|| text.to_vec());
    if remaining.iter().take_while(|&&b| b == b'-').count() == 3 {
        return Some((
            prefix,
            None,
            non_empty(remaining.get(3..).unwrap_or_default()),
        ));
    }
    let suffix = remaining.get(2..).unwrap_or_default();
    if let Some(id_at) = find(suffix, 0, b"---")
        && suffix.get(id_at + 3) != Some(&b'-')
    {
        let id = suffix.get(id_at + 3..).unwrap_or_default();
        return Some((
            prefix,
            non_empty(suffix.get(..id_at).unwrap_or_default()),
            non_empty(id),
        ));
    }
    Some((prefix, non_empty(suffix), None))
}

/// `extract_directive_value()`: the namespace (`ns::value`, or the default)
/// and the value, JSON decoded where it is JSON.
fn extract_directive_value(
    value: Option<Attribute>,
    default_namespace: Option<Vec<u8>>,
) -> (Option<Vec<u8>>, Value) {
    let value = match value {
        Some(Attribute::Value(value)) => value,
        _ => return (default_namespace, Value::Null),
    };
    // An empty namespace before `::` is taken too, unlike PHP's regex; it
    // evaluates to null either way.
    let namespace_len = value.iter().take_while(|&&b| is_namespace_byte(b)).count();
    let explicit = value.get(namespace_len..namespace_len + 2) == Some(b"::")
        && value.get(namespace_len + 2).is_some_and(|&b| b != b'\n');
    let (namespace, value) = if explicit {
        (
            Some(value.get(..namespace_len).unwrap_or_default().to_vec()),
            value.get(namespace_len + 2..).unwrap_or_default().to_vec(),
        )
    } else {
        (default_namespace, value)
    };
    let value = match json_decode(&value) {
        Some(decoded) => decoded,
        _ => Value::String(value),
    };
    (namespace, value)
}

/// `[\w\-_\/]`.
fn is_namespace_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'/')
}

/// `merge_style_property()`, with PHP's `explode( ':' )` that keeps only
/// the text between the first and second colon as the value.
fn merge_style_property(style: &[u8], property: &[u8], value: &Value) -> Vec<u8> {
    let value = (!value.is_empty()).then(|| {
        let text = value.to_php_string();
        let trimmed = php_trim(&text);
        let end = trimmed
            .iter()
            .rposition(|&b| b != b';')
            .map_or(0, |i| i + 1);
        trimmed.get(..end).unwrap_or_default().to_vec()
    });
    let mut out = Vec::new();
    for assignment in style.split(|&b| b == b';') {
        if is_empty_string(php_trim(assignment)) {
            continue;
        }
        let mut parts = assignment.split(|&b| b == b':');
        let name = php_trim(parts.next().unwrap_or_default());
        let assigned = php_trim(parts.next().unwrap_or_default());
        if name != property {
            out.extend_from_slice(name);
            out.push(b':');
            out.extend_from_slice(assigned);
            out.push(b';');
        }
    }
    if let Some(value) = value.filter(|v| !is_empty_string(v)) {
        out.extend_from_slice(property);
        out.push(b':');
        out.extend(value);
        out.push(b';');
    }
    out
}

/// `array_replace_recursive()`.
pub(crate) fn array_replace_recursive(
    mut entries: Vec<(Key, Value)>,
    replacements: Vec<(Key, Value)>,
) -> Vec<(Key, Value)> {
    for (key, value) in replacements {
        match (entries.iter_mut().find(|(k, _)| *k == key), value) {
            (Some((_, Value::Array(slot))), Value::Array(inner)) => {
                *slot = array_replace_recursive(std::mem::take(slot), inner);
            }
            (Some((_, slot)), value) => *slot = value,
            (None, value) => entries.push((key, value)),
        }
    }
    entries
}

/// The values as a list, keyed from 0.
pub(crate) fn list(values: Vec<Value>) -> Vec<(Key, Value)> {
    (0..).map(Key::Int).zip(values).collect()
}

/// `array_is_list()`.
fn is_list(entries: &[(Key, Value)]) -> bool {
    entries
        .iter()
        .zip(0..)
        .all(|((key, _), i)| *key == Key::Int(i))
}

#[cfg(test)]
mod tests;

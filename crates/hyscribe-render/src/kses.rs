// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_kses()`, ported from WordPress 7.1.3's `wp-includes/kses.php` with
//! the default `pre_kses` filters (`wp_pre_kses_less_than()` and
//! `wp_pre_kses_block_attributes()`), and `wp_kses_post()` over the exported
//! `wp_kses_allowed_html( 'post' )`.
//!
//! WordPress filters comments and invalid declarations again until they
//! stop changing. The port caps that recursion with a depth limit and a
//! work budget, and drops a token that runs out of either, so that no input
//! can make it unbounded.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::LazyLock;

use hyscribe_html::{Attribute, TagProcessor, decode_attribute, escape};
use hyscribe_kses::{
    ALLOWED_PROTOCOLS, esc_attr, esc_html, safecss_filter_attr, wp_kses_bad_protocol,
    wp_kses_no_null_keep_slash_zero, wp_kses_normalize_entities,
};
use hyscribe_phpser::{Key, Value, is_empty_string, json_decode};

use crate::blocks::{Block, is_pcre_space, parse_blocks, serialize_blocks};
use crate::formatting::{Next, find};
use crate::template::replace;

/// Nested comments and declarations deeper than this are dropped.
const MAX_DEPTH: usize = 8;

/// Bytes of repeated filtering allowed for an input of `len` bytes.
fn budget(len: usize) -> usize {
    len.saturating_mul(16) + 4096
}

/// `wp_kses_uri_attributes()`.
const URI_ATTRIBUTES: [&[u8]; 17] = [
    b"action",
    b"archive",
    b"background",
    b"cite",
    b"classid",
    b"codebase",
    b"data",
    b"formaction",
    b"href",
    b"icon",
    b"longdesc",
    b"manifest",
    b"poster",
    b"profile",
    b"src",
    b"usemap",
    b"xmlns",
];

static POST: LazyLock<AllowedHtml> = LazyLock::new(|| {
    AllowedHtml::from_json(include_bytes!(
        "../../../wordpress/wp-includes/kses-post.json"
    ))
});

/// A limit in an `$allowed_html` attribute entry.
enum Check {
    /// `'required' => true`: the tag goes without this attribute.
    Required,
    Valueless(Vec<u8>),
    Values(Vec<Vec<u8>>),
    /// `'value_callback' => '_wp_kses_allow_pdf_objects'`.
    PdfObject,
    /// A limit no core list uses, not ported: it rejects the attribute.
    Unported,
}

/// An `$allowed_html` array: tag names, each with its allowed attributes.
pub(crate) struct AllowedHtml(BTreeMap<Vec<u8>, BTreeMap<Vec<u8>, Vec<Check>>>);

impl AllowedHtml {
    /// Anything but a JSON object of tags allows nothing.
    pub(crate) fn from_json(json: &[u8]) -> AllowedHtml {
        let Some(Value::Array(tags)) = json_decode(json) else {
            return AllowedHtml(BTreeMap::new());
        };
        let attributes = |spec: Value| match spec {
            Value::Array(attributes) => attributes
                .into_iter()
                .map(|(name, spec)| (name.to_php_string(), checks(spec)))
                .collect(),
            _ => BTreeMap::new(),
        };
        AllowedHtml(
            tags.into_iter()
                .map(|(tag, spec)| (tag.to_php_string(), attributes(spec)))
                .collect(),
        )
    }
}

/// The limits of one attribute entry; `true` has none.
fn checks(spec: Value) -> Vec<Check> {
    let Value::Array(limits) = spec else {
        return Vec::new();
    };
    limits
        .into_iter()
        .filter_map(|(name, limit)| {
            let name = name.to_php_string();
            // `wp_kses_attr()` looks `required` up exactly; the other
            // limits match any case.
            if name == b"required" {
                return (limit == Value::Bool(true)).then_some(Check::Required);
            }
            Some(match (name.to_ascii_lowercase().as_slice(), limit) {
                (b"required", _) => return None,
                (b"valueless", Value::String(expected)) => {
                    Check::Valueless(expected.to_ascii_lowercase())
                }
                (b"values", Value::Array(values)) => Check::Values(
                    values
                        .into_iter()
                        .filter_map(|(_, value)| value.as_bytes().map(<[u8]>::to_vec))
                        .collect(),
                ),
                (b"value_callback", Value::String(callback))
                    if callback == b"_wp_kses_allow_pdf_objects" =>
                {
                    Check::PdfObject
                }
                _ => Check::Unported,
            })
        })
        .collect()
}

/// `wp_kses_post( $data )`. `upload_host` is the host of `wp_upload_dir()`'s
/// URL, with `:port` where it has one: embedded PDFs must come from there.
pub fn wp_kses_post(content: &[u8], upload_host: &[u8]) -> Vec<u8> {
    wp_kses(content, &POST, upload_host)
}

/// `wp_kses( $content, $allowed_html )` with the default protocols.
pub(crate) fn wp_kses(content: &[u8], allowed: &AllowedHtml, upload_host: &[u8]) -> Vec<u8> {
    Kses {
        allowed,
        upload_host,
        budget: budget(content.len()),
    }
    .kses(content, true, 0)
}

struct Kses<'a> {
    allowed: &'a AllowedHtml,
    upload_host: &'a [u8],
    budget: usize,
}

impl Kses<'_> {
    /// One `wp_kses()` call. `blocks` is false inside `filter_block_content()`,
    /// which unhooks `wp_pre_kses_block_attributes()` while it runs.
    fn kses(&mut self, content: &[u8], blocks: bool, depth: usize) -> Vec<u8> {
        let content = wp_kses_normalize_entities(&wp_kses_no_null_keep_slash_zero(content));
        let content = wp_pre_kses_less_than(&content);
        let content = if blocks {
            self.filter_block_content(&content, depth)
        } else {
            content
        };
        self.split(&content, blocks, depth)
    }

    /// `wp_kses_split()`: every comment, declaration and tag-like span goes
    /// through `wp_kses_split2()`, the text between them stays.
    fn split(&mut self, content: &[u8], blocks: bool, depth: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(content.len());
        let mut closers = Next::new(content, b"-->");
        let mut newlines = Next::new(content, b"\n");
        let mut at = 0;
        while let Some(start) = content
            .get(at..)
            .and_then(|rest| rest.iter().position(|&b| b == b'<' || b == b'>'))
            .map(|i| at + i)
        {
            out.extend_from_slice(content.get(at..start).unwrap_or_default());
            let end = token_end(content, start, &mut closers, &mut newlines);
            let token = content.get(start..end).unwrap_or_default();
            out.extend(self.split2(token, blocks, depth));
            at = end;
        }
        out.extend_from_slice(content.get(at..).unwrap_or_default());
        out
    }

    /// `wp_kses_split2()`.
    fn split2(&mut self, token: &[u8], blocks: bool, depth: usize) -> Vec<u8> {
        // wp_kses_stripslashes()
        let token = replace(token, b"\\\"", b"\"");
        if token.first() != Some(&b'<') {
            return b"&gt;".to_vec();
        }
        if let Some(opener) = invalid_markup(&token) {
            let inner = token.get(2..token.len() - 1).unwrap_or_default();
            return match self.refilter(inner, blocks, depth) {
                Some(inner) => [&b"<"[..], &[opener], &inner, b">"].concat(),
                None => Vec::new(),
            };
        }
        if token.starts_with(b"<!--") {
            let inner = replace(&replace(&token, b"<!--", b""), b"-->", b"");
            let inner = match self.refilter(&inner, blocks, depth) {
                Some(inner) if !inner.is_empty() => inner,
                _ => return Vec::new(),
            };
            let mut inner = collapse_dashes(&inner);
            // `-$`, where `$` also matches before a final newline.
            if inner.ends_with(b"-") {
                inner.pop();
            } else if inner.ends_with(b"-\n") {
                inner.remove(inner.len() - 2);
            }
            return [&b"<!--"[..], &inner, b"-->"].concat();
        }
        let Some((closer, name, attributes)) = element(&token) else {
            return Vec::new();
        };
        let Some(allowed) = self.allowed.0.get(&name.to_ascii_lowercase()) else {
            return Vec::new();
        };
        if closer {
            return [&b"</"[..], name, b">"].concat();
        }
        self.attr(name, attributes, allowed)
    }

    /// `wp_kses()` again until the content stops changing, as `wp_kses_split2()`
    /// does for comments and invalid declarations; `None` past the limits.
    fn refilter(&mut self, content: &[u8], blocks: bool, depth: usize) -> Option<Vec<u8>> {
        if depth >= MAX_DEPTH {
            return None;
        }
        let mut content = content.to_vec();
        loop {
            self.budget = self.budget.checked_sub(content.len() + 1)?;
            let next = self.kses(&content, blocks, depth + 1);
            if next == content {
                return Some(content);
            }
            content = next;
        }
    }

    /// `wp_kses_attr()`.
    fn attr(
        &self,
        element: &[u8],
        attributes: &[u8],
        allowed: &BTreeMap<Vec<u8>, Vec<Check>>,
    ) -> Vec<u8> {
        let xhtml_slash: &[u8] = match attributes.iter().rposition(|&b| !is_pcre_space(b)) {
            Some(last) if attributes.get(last) == Some(&b'/') => b" /",
            _ => b"",
        };
        let mut required: BTreeSet<&[u8]> = allowed
            .iter()
            .filter(|(_, checks)| checks.iter().any(|c| matches!(c, Check::Required)))
            .map(|(name, _)| name.as_slice())
            .collect();
        let stripped = if xhtml_slash.is_empty() {
            [&b"<"[..], element, b">"].concat()
        } else {
            Vec::new()
        };
        let mut kept = Vec::new();
        for attribute in wp_kses_hair(attributes) {
            let is_required = required.remove(attribute.name.as_slice());
            match self.attr_check(&attribute, allowed) {
                Some(whole) => {
                    kept.push(b' ');
                    kept.extend(whole);
                }
                None if is_required => return stripped,
                None => {}
            }
        }
        if !required.is_empty() {
            return stripped;
        }
        // PHP strips `<` and `>` here, but none are left: the token ends at its
        // first `>`, wp_pre_kses_less_than() escaped any `<`, values are encoded.
        [&b"<"[..], element, &kept, xhtml_slash, b">"].concat()
    }

    /// `wp_kses_attr_check()`: the attribute as it may stay, or `None`.
    fn attr_check(
        &self,
        attribute: &Hair,
        allowed: &BTreeMap<Vec<u8>, Vec<Check>>,
    ) -> Option<Vec<u8>> {
        let name = attribute.name.as_slice();
        let checks = allowed.get(name).or_else(|| {
            let suffix = name.strip_prefix(b"data-")?;
            let valid = !suffix.is_empty()
                && suffix.iter().all(|&b| {
                    b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-'
                });
            allowed.get(&b"data-*"[..]).filter(|_| valid)
        })?;
        let mut value = attribute.value.clone();
        let mut whole = attribute.whole.clone();
        if name == b"style" {
            let decoded = decode_attribute(&value);
            let filtered = safecss_filter_attr(&decoded);
            if is_empty_string(&filtered) {
                return None;
            }
            if filtered != decoded {
                let encoded = esc_attr(&filtered).into_bytes();
                // str_replace() swaps every copy of the value, even one
                // inside the name.
                whole = replace(&whole, &value, &encoded);
                value = encoded;
            }
        }
        checks
            .iter()
            .all(|check| self.check_attr_val(check, &value, attribute.valueless))
            .then_some(whole)
    }

    /// `wp_kses_check_attr_val()`.
    fn check_attr_val(&self, check: &Check, value: &[u8], valueless: bool) -> bool {
        match check {
            Check::Required => true,
            Check::Valueless(expected) => expected == if valueless { b"y" } else { b"n" },
            Check::Values(values) => values.contains(&value.to_ascii_lowercase()),
            Check::PdfObject => self.allow_pdf_object(value),
            Check::Unported => false,
        }
    }

    /// `_wp_kses_allow_pdf_objects()`.
    fn allow_pdf_object(&self, url: &[u8]) -> bool {
        let on_upload_host = |scheme: &[u8]| {
            url.strip_prefix(scheme)
                .and_then(|rest| rest.strip_prefix(self.upload_host))
                .is_some_and(|rest| rest.starts_with(b"/"))
        };
        !url.contains(&b'?')
            && !url.contains(&b'#')
            && url.ends_with(b".pdf")
            && (on_upload_host(b"http://") || on_upload_host(b"https://"))
    }

    /// `filter_block_content()`, as `wp_pre_kses_block_attributes()` runs it:
    /// the attributes of every block filtered, the blocks serialized again.
    fn filter_block_content(&mut self, text: &[u8], depth: usize) -> Vec<u8> {
        let mut blocks = parse_blocks(&fix_comment_closers(text));
        for block in &mut blocks {
            self.filter_block_kses(block, depth);
        }
        serialize_blocks(&blocks)
    }

    /// `filter_block_kses()`.
    fn filter_block_kses(&mut self, block: &mut Block, depth: usize) {
        let template_part = block.name.as_deref() == Some("core/template-part");
        if let Some(attrs) = block.attrs.take() {
            block.attrs = Some(self.filter_array(&attrs, template_part, depth));
        }
        for inner in &mut block.inner_blocks {
            self.filter_block_kses(inner, depth);
        }
    }

    /// `filter_block_kses_value()`.
    fn filter_value(&mut self, value: &Value, template_part: bool, depth: usize) -> Value {
        match value {
            Value::Array(entries) => Value::Array(self.filter_array(entries, template_part, depth)),
            Value::String(text) => Value::String(self.kses(text, false, depth)),
            other => other.clone(),
        }
    }

    /// `filter_block_kses_value()` for an array: keys and values filtered.
    fn filter_array(
        &mut self,
        entries: &[(Key, Value)],
        template_part: bool,
        depth: usize,
    ) -> Vec<(Key, Value)> {
        // PHP's ordered array: unset keys leave a gap, set keys keep their
        // place or go last, and an index finds each key.
        let mut out: Vec<Option<(Key, Value)>> = entries.iter().cloned().map(Some).collect();
        let mut at: HashMap<Key, usize> = HashMap::new();
        for (i, (key, _)) in entries.iter().enumerate() {
            at.entry(key.clone()).or_insert(i);
        }
        for (key, value) in entries {
            let filtered_key = match key {
                Key::String(name) => Key::from_php_string(self.kses(name, false, depth)),
                Key::Int(_) => key.clone(),
            };
            let mut filtered = self.filter_value(value, template_part, depth);
            // filter_block_core_template_part_attributes()
            if template_part
                && filtered_key == Key::String(b"tagName".to_vec())
                && !filtered.is_empty()
            {
                let allowed = filtered
                    .as_bytes()
                    .is_some_and(|tag| self.allowed.0.contains_key(tag));
                if !allowed {
                    filtered = Value::String(Vec::new());
                }
            }
            if filtered_key != *key
                && let Some(i) = at.remove(key)
                && let Some(entry) = out.get_mut(i)
            {
                *entry = None;
            }
            match at.get(&filtered_key).and_then(|&i| out.get_mut(i)) {
                Some(Some((_, slot))) => *slot = filtered,
                _ => {
                    at.insert(filtered_key.clone(), out.len());
                    out.push(Some((filtered_key, filtered)));
                }
            }
        }
        out.into_iter().flatten().collect()
    }
}

/// The end of the `wp_kses_split()` token at `start`, a `<` or `>`.
fn token_end(content: &[u8], start: usize, closers: &mut Next, newlines: &mut Next) -> usize {
    let len = content.len();
    let tail = content.get(start..).unwrap_or_default();
    let next_gt = |from: usize| {
        content
            .get(from..)
            .and_then(|rest| rest.iter().position(|&b| b == b'>'))
            .map(|i| from + i + 1)
    };
    if tail.first() == Some(&b'>') {
        return start + 1;
    }
    // `<!--.*?(-->|$)`: `.` stops at a line feed, `$` also matches before a
    // final one.
    if tail.starts_with(b"<!--") {
        let line_end = newlines.from(start + 4).unwrap_or(len);
        match closers.from(start + 4) {
            Some(closer) if closer + 3 <= line_end => return closer + 3,
            _ if line_end + 1 >= len => return line_end,
            _ => {}
        }
    }
    // `</[^a-zA-Z][^>]*>`
    if tail.get(1) == Some(&b'/')
        && tail.get(2).is_some_and(|b| !b.is_ascii_alphabetic())
        && let Some(end) = next_gt(start + 3)
    {
        return end;
    }
    // `<![^>]*>`
    if tail.get(1) == Some(&b'!')
        && let Some(end) = next_gt(start + 2)
    {
        return end;
    }
    // `<[^>]*(>|$)`
    next_gt(start).unwrap_or(len)
}

/// The opener of a `wp_kses_split2()` token that matches
/// `^(?:</[^a-zA-Z][^>]*>|<![a-z][^>]*>)$`.
fn invalid_markup(token: &[u8]) -> Option<u8> {
    let [_, opener, first, rest @ ..] = token else {
        return None;
    };
    let valid = match opener {
        b'/' => !first.is_ascii_alphabetic(),
        b'!' => first.is_ascii_lowercase(),
        _ => false,
    };
    // Tokens end at their first `>`, so it must be the last byte.
    let closed = rest
        .iter()
        .position(|&b| b == b'>')
        .is_some_and(|at| at + 1 == rest.len());
    (valid && closed).then_some(*opener)
}

/// `%^<\s*(/\s*)?([a-zA-Z0-9-]+)([^>]*)>?$%` on a token that starts with
/// `<`: whether the tag closes, its name and its attributes.
fn element(token: &[u8]) -> Option<(bool, &[u8], &[u8])> {
    let body = token.get(1..).unwrap_or_default();
    let body = skip_spaces(body.strip_suffix(b">").unwrap_or(body));
    let (closer, body) = match body.strip_prefix(b"/") {
        Some(rest) => (true, skip_spaces(rest)),
        None => (false, body),
    };
    let name_len = body
        .iter()
        .take_while(|&&b| b.is_ascii_alphanumeric() || b == b'-')
        .count();
    let name = body.get(..name_len).unwrap_or_default();
    let attributes = body.get(name_len..).unwrap_or_default();
    (!name.is_empty()).then_some((closer, name, attributes))
}

fn skip_spaces(text: &[u8]) -> &[u8] {
    let spaces = text.iter().take_while(|&&b| is_pcre_space(b)).count();
    text.get(spaces..).unwrap_or_default()
}

/// `preg_replace( '/--+/', '-', $content )`.
fn collapse_dashes(content: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(content.len());
    for &b in content {
        if b != b'-' || out.last() != Some(&b'-') {
            out.push(b);
        }
    }
    out
}

/// One attribute from `wp_kses_hair()`.
struct Hair {
    name: Vec<u8>,
    value: Vec<u8>,
    whole: Vec<u8>,
    valueless: bool,
}

/// `wp_kses_hair()`: the attributes as the HTML API reads them, URLs
/// through `wp_kses_bad_protocol()`, values encoded again.
fn wp_kses_hair(attributes: &[u8]) -> Vec<Hair> {
    let mut processor = TagProcessor::new(&[&b"<wp "[..], attributes, b">"].concat());
    processor.next_tag(None, None);
    let names = processor
        .get_attribute_names_with_prefix(b"")
        .unwrap_or_default();
    names
        .into_iter()
        .map(|name| match processor.get_attribute(&name) {
            Some(Attribute::Value(value)) => {
                let value = if URI_ATTRIBUTES.contains(&name.as_slice()) {
                    wp_kses_bad_protocol(&value, &ALLOWED_PROTOCOLS)
                } else {
                    value
                };
                let recoded = escape(&value);
                Hair {
                    whole: [&name[..], b"=\"", &recoded, b"\""].concat(),
                    name,
                    value: recoded,
                    valueless: false,
                }
            }
            _ => Hair {
                whole: name.clone(),
                name,
                value: Vec::new(),
                valueless: true,
            },
        })
        .collect()
}

/// `wp_pre_kses_less_than()`: escapes every `<` that no `>` closes before
/// the next `<`.
fn wp_pre_kses_less_than(content: &[u8]) -> Vec<u8> {
    // `$` in `%<[^>]*?((?=<)|>|$)%` also matches before a final newline.
    let dollar = content.len() - usize::from(content.ends_with(b"\n"));
    let mut out = Vec::with_capacity(content.len());
    let mut at = 0;
    while let Some(start) = content
        .get(at..)
        .and_then(|rest| rest.iter().position(|&b| b == b'<'))
        .map(|i| at + i)
    {
        out.extend_from_slice(content.get(at..start).unwrap_or_default());
        let stop = content
            .get(start + 1..)
            .and_then(|rest| rest.iter().position(|&b| b == b'<' || b == b'>'))
            .map_or(dollar, |i| (start + 1 + i).min(dollar));
        if content.get(stop) == Some(&b'>') {
            out.extend_from_slice(content.get(start..=stop).unwrap_or_default());
            at = stop + 1;
        } else {
            let span = content.get(start..stop).unwrap_or_default();
            out.extend_from_slice(esc_html(span).as_bytes());
            at = stop;
        }
    }
    out.extend_from_slice(content.get(at..).unwrap_or_default());
    out
}

/// `filter_block_content()`'s `%<!--(.*?)--->%` replacement: a comment
/// closed by three dashes gets two, its trailing dashes trimmed.
fn fix_comment_closers(text: &[u8]) -> Vec<u8> {
    let mut closers = Next::new(text, b"--->");
    let mut newlines = Next::new(text, b"\n");
    let mut out = Vec::with_capacity(text.len());
    let mut at = 0;
    let mut from = 0;
    while let Some(start) = find(text, from, b"<!--") {
        let line_end = newlines.from(start + 4).unwrap_or(text.len());
        match closers.from(start + 4) {
            Some(closer) if closer + 4 <= line_end => {
                out.extend_from_slice(text.get(at..start).unwrap_or_default());
                let inner = text.get(start + 4..closer).unwrap_or_default();
                let kept = inner.iter().rposition(|&b| b != b'-').map_or(0, |i| i + 1);
                out.extend_from_slice(b"<!--");
                out.extend_from_slice(inner.get(..kept).unwrap_or_default());
                out.extend_from_slice(b"-->");
                at = closer + 4;
                from = at;
            }
            _ => from = start + 1,
        }
    }
    out.extend_from_slice(text.get(at..).unwrap_or_default());
    out
}

#[cfg(test)]
mod tests;

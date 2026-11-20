// SPDX-License-Identifier: AGPL-3.0-or-later

//! Block bindings: attributes a block takes from a source, such as post
//! meta or a synced pattern's overrides, written into its markup.

use hyscribe_html::{HtmlProcessor, TagProcessor};
use hyscribe_kses::{esc_html, esc_url};
use hyscribe_phpser::{Key, Value, lookup};

use crate::blocks::Block;
use crate::core_blocks::{Context, merged, post_data, prepared, public};
use crate::kses::wp_kses_post;
use crate::permalinks::term_link;
use crate::registry::block_type;

/// `get_block_bindings_supported_attributes()`.
fn supported_attributes(name: &str) -> &'static [&'static str] {
    match name {
        "core/paragraph" | "core/heading" | "core/list-item" => &["content"],
        "core/image" => &["id", "url", "title", "alt", "caption"],
        "core/button" => &["url", "text", "linkTarget", "rel"],
        "core/post-date" => &["datetime"],
        "core/navigation-link" | "core/navigation-submenu" => &["url"],
        _ => &[],
    }
}

/// `WP_Block::process_block_bindings()`: the attributes a block's bindings
/// compute, in order, given the `pattern/overrides` context.
pub(crate) fn computed_attributes(
    context: &Context<'_>,
    block: &Block,
    overrides: Option<&Value>,
) -> Vec<(Key, Value)> {
    let name = block.name.as_deref().unwrap_or_default();
    let supported = supported_attributes(name);
    let parsed = block.attrs.as_deref().unwrap_or_default();
    let metadata = lookup(parsed, "metadata");
    let Some(bindings @ Value::Array(_)) = metadata.and_then(|m| m.get_str("bindings")) else {
        return Vec::new();
    };
    let attributes = prepared(name, parsed);
    let mut computed = Vec::new();
    let default = bindings.get_path(&["__default", "source"]);
    let bindings = if matches!(default, Some(Value::String(s)) if s == b"core/pattern-overrides") {
        // Every supported attribute not bound otherwise is overridable.
        let expanded = Value::Array(
            supported
                .iter()
                .map(|attribute| {
                    let binding = bindings
                        .get_str(attribute)
                        .filter(|b| **b != Value::Null)
                        .cloned()
                        .unwrap_or_else(|| {
                            Value::Array(vec![(
                                Key::String(b"source".to_vec()),
                                Value::String(b"core/pattern-overrides".to_vec()),
                            )])
                        });
                    (Key::String(attribute.as_bytes().to_vec()), binding)
                })
                .collect(),
        );
        let bindings = [(Key::String(b"bindings".to_vec()), expanded.clone())];
        let metadata = merged(metadata.cloned().unwrap_or_default(), &bindings);
        computed.push((Key::String(b"metadata".to_vec()), metadata));
        expanded
    } else {
        bindings.clone()
    };
    for (key, binding) in bindings.as_array().unwrap_or_default() {
        let Key::String(attribute) = key else {
            continue;
        };
        let Some(attribute) = supported
            .iter()
            .find(|a| a.as_bytes() == attribute.as_slice())
        else {
            continue;
        };
        let Some(Value::String(source)) = binding.get_str("source") else {
            continue;
        };
        // The sources read only array keys, so any other `args` reads as empty.
        let args = binding.get_str("args").unwrap_or(&Value::Null);
        let value = source_value(
            context,
            source,
            name,
            &attributes,
            overrides,
            args,
            attribute,
        );
        if let Some(value) = value {
            computed.push((key.clone(), value));
        }
    }
    computed
}

/// The registered `source`'s `get_value_callback`, `None` for another.
fn source_value(
    context: &Context<'_>,
    source: &[u8],
    name: &str,
    attributes: &Value,
    overrides: Option<&Value>,
    args: &Value,
    attribute: &str,
) -> Option<Value> {
    match source {
        b"core/pattern-overrides" => pattern_override(attributes, overrides, attribute),
        b"core/post-data" => post_data_value(context, name, attributes, args),
        b"core/post-meta" => post_meta(context, args),
        b"core/term-data" => term_data(context, name, attributes, args),
        _ => None,
    }
}

/// `_block_bindings_pattern_overrides_get_value()`: the synced pattern's
/// value for this block, by its name, and this attribute.
fn pattern_override(
    attributes: &Value,
    overrides: Option<&Value>,
    attribute: &str,
) -> Option<Value> {
    let name = attributes
        .get_path(&["metadata", "name"])
        .filter(|name| !name.is_empty())?;
    let key = match name {
        Value::String(_) | Value::Int(_) => Key::from_php_string(name.to_php_string()),
        _ => return None,
    };
    overrides?
        .get(&key)?
        .get_str(attribute)
        .filter(|value| **value != Value::Null)
        .cloned()
}

/// The `field`, or else `key`, argument, unless empty.
fn field(args: &Value, names: &[&str]) -> Option<Vec<u8>> {
    names
        .iter()
        .find_map(|name| args.get_str(name).filter(|v| !v.is_empty()))
        .map(Value::to_php_string)
}

fn is_navigation(name: &str) -> bool {
    matches!(name, "core/navigation-link" | "core/navigation-submenu")
}

/// `_block_bindings_post_data_get_value()`: a navigation link's own post,
/// or the `postId` context's.
fn post_data_value(
    context: &Context<'_>,
    name: &str,
    attributes: &Value,
    args: &Value,
) -> Option<Value> {
    let field = field(args, &["field", "key"])?;
    let post = if is_navigation(name) {
        let id = attributes.get_str("id").filter(|id| !id.is_empty())?;
        u64::try_from(id.to_int())
            .ok()
            .and_then(|id| context.site.post(id))
    } else {
        context.post()
    };
    post_data(context, post, &field).map(Value::String)
}

/// `_block_bindings_post_meta_get_value()`: only the meta core registers
/// for the REST API, which on a public post is `footnotes`.
fn post_meta(context: &Context<'_>, args: &Value) -> Option<Value> {
    let key = field(args, &["key"])?;
    let post = public(context.site, context.post())?;
    let registered = key == b"footnotes" && matches!(post.post_type.as_str(), "post" | "page");
    if !registered {
        return None;
    }
    let meta = context
        .site
        .post_meta(post.id, "footnotes")
        .unwrap_or_default();
    Some(Value::String(meta))
}

/// `_block_bindings_term_data_get_value()`: a navigation link's own term,
/// or the term template's, in a publicly queryable taxonomy.
fn term_data(context: &Context<'_>, name: &str, attributes: &Value, args: &Value) -> Option<Value> {
    let field = field(args, &["field"])?;
    let (taxonomy, id) = if is_navigation(name) {
        let kind = attributes
            .get_str("type")
            .map(Value::to_php_string)
            .unwrap_or_default();
        let taxonomy = if kind == b"tag" {
            b"post_tag".to_vec()
        } else {
            kind
        };
        let id = attributes.get_str("id").map_or(0, Value::to_int);
        (
            String::from_utf8_lossy(&taxonomy).into_owned(),
            u64::try_from(id).unwrap_or(0),
        )
    } else {
        context.terms.term.clone()?
    };
    if id == 0 || !matches!(taxonomy.as_str(), "category" | "post_tag" | "post_format") {
        return None;
    }
    let term = context.site.term(id, &taxonomy)?;
    let text = |text: String| Some(Value::String(text.into_bytes()));
    match field.as_slice() {
        b"id" => text(esc_html(id.to_string().as_bytes())),
        b"name" => text(esc_html(&term.name)),
        b"link" => Some(Value::String(esc_url(&term_link(
            context.site,
            &taxonomy,
            &term,
        )))),
        b"slug" => text(esc_html(&term.slug)),
        b"description" => Some(Value::String(wp_kses_post(
            &term.description,
            &context.upload_host(),
        ))),
        b"parent" => text(esc_html(term.parent.to_string().as_bytes())),
        b"count" => text(esc_html(term.count.to_string().as_bytes())),
        _ => None,
    }
}

/// `WP_Block::replace_html()`: the bound attribute written where the block
/// type's schema reads it from.
pub(crate) fn replace_html(
    context: &Context<'_>,
    name: &str,
    content: &[u8],
    attribute: &Key,
    value: &Value,
    inner_blocks: &[usize],
) -> Vec<u8> {
    // Every attribute bindings compute has a schema.
    let schema = block_type(name)
        .and_then(|t| t.attributes.iter().find(|(key, _)| key == attribute))
        .map(|(_, schema)| schema);
    let field = |name| {
        schema
            .and_then(|s| s.get_str(name))
            .map(Value::to_php_string)
    };
    let selector = field("selector").unwrap_or_default();
    let selector = String::from_utf8_lossy(&selector);
    match field("source").as_deref() {
        Some(b"html" | b"rich-text") => {
            let text = wp_kses_post(&value.to_php_string(), &context.upload_host());
            replace_selected_rich_text(content, &selector, &text, inner_blocks)
        }
        Some(b"attribute") => {
            let mut p = TagProcessor::new(content);
            if !p.next_tag(Some(selector.as_bytes()), None) {
                return content.to_vec();
            }
            let name = field("attribute").unwrap_or_default();
            match value {
                Value::Bool(true) => p.set_boolean_attribute(&name),
                Value::Bool(false) => p.remove_attribute(&name),
                _ => p.set_attribute(&name, &value.to_php_string()),
            };
            p.get_updated_html().to_vec()
        }
        _ => content.to_vec(),
    }
}

/// The rich text branch of `WP_Block::replace_html()`: `text` in the first
/// element matching one of the comma-separated `selectors`.
fn replace_selected_rich_text(
    content: &[u8],
    selectors: &str,
    text: &[u8],
    inner_blocks: &[usize],
) -> Vec<u8> {
    // PHP seeks back to the first tag between selectors, which parsing
    // afresh matches, but never past a bail, which `seek()` keeps.
    for selector in selectors.split(',') {
        let mut p = HtmlProcessor::create_fragment(content);
        p.next_tag(None);
        let first = p
            .get_tag()
            .is_some_and(|tag| tag.eq_ignore_ascii_case(selector));
        if first || p.next_tag(Some(selector)) {
            return replace_rich_text(p, content, text, inner_blocks)
                .unwrap_or_else(|| content.to_vec());
        }
        if p.get_last_error().is_some() {
            break;
        }
    }
    content.to_vec()
}

/// `replace_rich_text()`: the content up to the closer or first inner block.
/// PHP cannot bookmark implied tokens: an opener reads as 0, a closer as the opener.
fn replace_rich_text(
    mut p: HtmlProcessor,
    content: &[u8],
    text: &[u8],
    inner_blocks: &[usize],
) -> Option<Vec<u8>> {
    // PHP first rejects an element without a closer, which the walk below
    // rejects too: it never visits such a closer.
    let depth = p.get_current_depth();
    let tag = p.get_tag();
    let (opener, length) = p.token_span().unwrap_or_default();
    let start = opener + length;
    while p.next_token() && p.get_current_depth() >= depth {}
    if !p.is_tag_closer() || p.get_tag() != tag {
        return None;
    }
    let closer = p.token_span().map_or(opener, |(start, _)| start);
    let end = inner_blocks
        .iter()
        .copied()
        .find(|offset| (start..closer).contains(offset))
        .unwrap_or(closer);
    let head = content.get(..start).unwrap_or_default();
    let tail = content.get(end..).unwrap_or_default();
    Some([head, text, tail].concat())
}

#[cfg(test)]
mod tests;

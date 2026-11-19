// SPDX-License-Identifier: AGPL-3.0-or-later

//! A template's blocks as the templates controller serves them: each
//! template part block named after the active theme, as
//! `apply_block_hooks_to_content()` names it where no block is hooked, and
//! pattern blocks replaced by their patterns, as `resolve_pattern_blocks()`
//! replaces them.

use std::sync::OnceLock;

use hyscribe_phpser::{Key, Value, is_empty_string, json_decode, lookup, php_trim};
use hyscribe_render::blocks::{Block, parse_blocks, serialize_blocks};
use hyscribe_render::formatting::sanitize_text_field;
use hyscribe_render::theme::under_site;

use crate::response::{RestError, text};

const PATTERNS: &[u8] = include_bytes!("../../../../themes/twentytwentyfive/patterns.json");

/// The prefix of the theme's own patterns, every one of which is exported.
const THEME_PATTERNS: &[u8] = b"twentytwentyfive/";

/// `$attrs[ $key ] = $value`, in place when it is set.
fn set(attrs: &mut Vec<(Key, Value)>, key: &str, value: Value) {
    let key = Key::String(key.as_bytes().to_vec());
    match attrs.iter_mut().find(|(known, _)| *known == key) {
        Some((_, known)) => *known = value,
        None => attrs.push((key, value)),
    }
}

/// `_inject_theme_attribute_in_template_part_block()` on every block.
fn inject_theme(blocks: &mut [Block], theme: &[u8]) {
    for block in blocks {
        if block.name.as_deref() == Some("core/template-part") {
            let attrs = block.attrs.get_or_insert_with(Vec::new);
            if lookup(attrs, "theme").is_none_or(|theme| *theme == Value::Null) {
                set(attrs, "theme", text(theme));
            }
        }
        inject_theme(&mut block.inner_blocks, theme);
    }
}

/// `apply_block_hooks_to_content()` of a template, or of a template part
/// in a template part block that `remove_serialized_parent_block()` takes
/// off again.
pub(crate) fn hooked(content: &[u8], part: bool, theme: &[u8]) -> Vec<u8> {
    if part {
        return in_parent(content, b"template-part", theme);
    }
    let mut blocks = parse_blocks(content);
    inject_theme(&mut blocks, theme);
    serialize_blocks(&blocks)
}

/// `apply_block_hooks_to_content()` of `content` in a `parent` block, which
/// `remove_serialized_parent_block()` takes off again.
pub(crate) fn in_parent(content: &[u8], parent: &[u8], theme: &[u8]) -> Vec<u8> {
    let wrapped = if is_empty_string(content) {
        [&b"<!-- wp:"[..], parent, b" /-->"].concat()
    } else {
        [
            &b"<!-- wp:"[..],
            parent,
            b" -->",
            content,
            b"<!-- /wp:",
            parent,
            b" -->",
        ]
        .concat()
    };
    let mut blocks = parse_blocks(&wrapped);
    inject_theme(&mut blocks, theme);
    let serialized = serialize_blocks(&blocks);
    let find = |needle: &[u8]| serialized.windows(needle.len()).position(|w| w == needle);
    let start = find(b"-->").map_or(3, |at| at + 3);
    let end = serialized
        .windows(4)
        .rposition(|w| w == b"<!--")
        .unwrap_or(0);
    serialized.get(start..end).unwrap_or_default().to_vec()
}

/// A theme pattern: its title, content, and its description and
/// categories where it has them.
pub(crate) fn pattern(slug: &[u8]) -> Option<&'static Value> {
    static DATA: OnceLock<Value> = OnceLock::new();
    let data = DATA.get_or_init(|| json_decode(PATTERNS).unwrap_or_default());
    data.get(&Key::String(slug.to_vec()))
}

/// `sanitize_text_field()` of a value, each item of an array: the theme's
/// patterns hold strings and lists of strings.
fn sanitized(value: &Value) -> Value {
    let clean = |item: &Value| text(sanitize_text_field(&item.to_php_string()));
    match value {
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|(key, item)| (key.clone(), clean(item)))
                .collect(),
        ),
        _ => clean(value),
    }
}

/// The pattern's name, description and categories added to the metadata of
/// its single root block, an array where a theme pattern has it; a theme
/// pattern's fields are never null.
fn name_root(block: &mut Block, slug: &[u8], pattern: &Value) {
    let attrs = block.attrs.get_or_insert_with(Vec::new);
    let metadata = lookup(attrs, "metadata").and_then(Value::as_array);
    let mut metadata = metadata.map(<[_]>::to_vec).unwrap_or_default();
    set(&mut metadata, "patternName", text(slug));
    for (key, field) in [
        ("name", "title"),
        ("description", "description"),
        ("categories", "categories"),
    ] {
        let value = pattern.get_str(field).or_else(|| lookup(&metadata, key));
        if let Some(value) = value.filter(|value| !value.is_empty()) {
            let value = sanitized(value);
            set(&mut metadata, key, value);
        }
    }
    set(attrs, "metadata", Value::Array(metadata));
}

/// The slug of a pattern block, if it names one.
fn pattern_slug(block: &Block) -> Option<Vec<u8>> {
    let slug = block
        .attrs
        .as_deref()
        .and_then(|attrs| lookup(attrs, "slug"));
    let slug = slug
        .filter(|slug| !slug.is_empty())
        .map(Value::to_php_string);
    slug.filter(|_| block.name.as_deref() == Some("core/pattern"))
}

/// Whether a pattern block names a pattern not the theme's: core's or the
/// pattern directory's, which are not ported.
fn names_foreign_pattern(blocks: &[Block]) -> bool {
    blocks.iter().any(|block| {
        pattern_slug(block).is_some_and(|slug| !slug.starts_with(THEME_PATTERNS))
            || names_foreign_pattern(&block.inner_blocks)
    })
}

/// `resolve_pattern_blocks()` of blocks naming only the theme's patterns,
/// with the inner content of the block whose inner blocks these are. No
/// theme pattern holds itself, so none is skipped as recursive.
fn resolve(
    mut blocks: Vec<Block>,
    mut inner: Option<&mut Vec<Option<Vec<u8>>>>,
    siteurl: &[u8],
) -> Vec<Block> {
    let mut i = 0;
    while let Some(block) = blocks.get_mut(i) {
        let found = pattern_slug(block).and_then(|slug| pattern(&slug).map(|found| (slug, found)));
        let Some((slug, pattern)) = found else {
            if block.name.as_deref() != Some("core/pattern") {
                let children = std::mem::take(&mut block.inner_blocks);
                block.inner_blocks = resolve(children, Some(&mut block.inner_content), siteurl);
            }
            i += 1;
            continue;
        };
        let content = pattern.get_str("content").map(Value::to_php_string);
        let content = under_site(&content.unwrap_or_default(), siteurl);
        let mut inserted = parse_blocks(php_trim(&content));
        if let [root] = inserted.as_mut_slice() {
            name_root(root, &slug, pattern);
        }
        let inserted = resolve(inserted, None, siteurl);
        let count = inserted.len();
        blocks.splice(i..=i, inserted);
        // The inner content of a block with inner blocks is never empty.
        if let Some(chunks) = inner.as_deref_mut() {
            let nulls = chunks
                .iter()
                .enumerate()
                .filter(|(_, chunk)| chunk.is_none());
            let at = nulls.map(|(at, _)| at).nth(i);
            at.into_iter().for_each(|at| {
                chunks.splice(at..=at, std::iter::repeat_n(None, count));
            });
        }
        i += count;
    }
    blocks
}

/// A template's content with its patterns resolved, serialized again.
pub(crate) fn resolve_patterns(content: &[u8], siteurl: &[u8]) -> Result<Vec<u8>, RestError> {
    let blocks = parse_blocks(content);
    if names_foreign_pattern(&blocks) {
        return Err(RestError::not_ported());
    }
    Ok(serialize_blocks(&resolve(blocks, None, siteurl)))
}

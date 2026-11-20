// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_Block_Pattern_Categories_Controller` and
//! `WP_REST_Block_Patterns_Controller`: `/wp/v2/block-patterns/categories`
//! and `/wp/v2/block-patterns/patterns`.

use hyscribe_phpser::{Key, Value};

use crate::fields::{Properties, fields_for_response, is_field_included};
use crate::request::Params;
use crate::response::{Response, RestError, list, text};
use hyscribe_render::theme::{Theme, under_site};

use crate::templates::content::resolve_patterns;
use crate::types::can_edit_any;
use crate::{Context, Endpoint};

use super::core;
use super::themes::bundled;

/// A pattern's keys as registered, in the order `prepare_item_for_response()`
/// adds them, and as the route names them below (every one in every context).
const ITEM_KEYS: [&str; 12] = [
    "name",
    "title",
    "content",
    "description",
    "viewportWidth",
    "inserter",
    "categories",
    "keywords",
    "blockTypes",
    "postTypes",
    "templateTypes",
    "source",
];
const PATTERN_PROPERTIES: &Properties = &[
    ("name", &["view", "edit", "embed"]),
    ("title", &["view", "edit", "embed"]),
    ("content", &["view", "edit", "embed"]),
    ("description", &["view", "edit", "embed"]),
    ("viewport_width", &["view", "edit", "embed"]),
    ("inserter", &["view", "edit", "embed"]),
    ("categories", &["view", "edit", "embed"]),
    ("keywords", &["view", "edit", "embed"]),
    ("block_types", &["view", "edit", "embed"]),
    ("post_types", &["view", "edit", "embed"]),
    ("template_types", &["view", "edit", "embed"]),
    ("source", &["view", "edit", "embed"]),
];

/// `WP_REST_Block_Patterns_Controller::$categories_migration`.
const MIGRATION: [(&[u8], &str); 3] = [
    (b"buttons", "call-to-action"),
    (b"columns", "text"),
    (b"query", "posts"),
];

const PROPERTIES: &Properties = &[
    ("name", &["view", "edit", "embed"]),
    ("label", &["view", "edit", "embed"]),
    ("description", &["view", "edit", "embed"]),
];

pub(super) fn endpoints(regex: &str) -> Option<Vec<Endpoint>> {
    match regex {
        "/wp/v2/block-patterns/categories" => Some(vec![Endpoint {
            args: Vec::new(),
            permission: |context, _| permission(context, "block pattern categories"),
            callback: Some(categories),
        }]),
        // PHP first fetches the patterns of the wordpress.org pattern
        // directory and caches the answer in a site transient. HyScribe
        // makes no calls to wordpress.org: it gives the registered patterns,
        // as PHP does when the directory cannot be reached.
        "/wp/v2/block-patterns/patterns" => Some(vec![Endpoint {
            args: Vec::new(),
            permission: |context, _| permission(context, "block patterns"),
            callback: Some(patterns),
        }]),
        _ => None,
    }
}

/// `get_items_permissions_check()`.
fn permission(context: &Context, what: &str) -> Result<(), RestError> {
    if can_edit_any(context) {
        return Ok(());
    }
    let message = format!("Sorry, you are not allowed to view the registered {what}.");
    Err(context.forbidden("rest_cannot_view", &message))
}

/// `get_items()` of the categories core and then the theme register.
fn categories(context: &Context, params: &Params) -> Result<Response, RestError> {
    if context.head {
        return Ok(Response::new(Value::Array(Vec::new()), 200));
    }
    let wanted = fields_for_response(params, PROPERTIES);
    let data = core("pattern_categories")
        .iter()
        .chain(context.site.theme().registered("pattern_categories"))
        .map(|(_, category)| {
            let kept: Vec<(Key, Value)> = PROPERTIES
                .iter()
                .filter(|(name, _)| is_field_included(name, &wanted))
                .filter_map(|(name, _)| {
                    let value = category.get_str(name);
                    value.map(|value| (Key::String(name.as_bytes().to_vec()), value.clone()))
                })
                .collect();
            // Every property is in every context, so the context's filter
            // keeps them all.
            Value::Array(kept)
        })
        .collect();
    Ok(Response::new(list(data), 200))
}

/// `migrate_pattern_categories()`: the old core categories renamed.
pub(super) fn migrated(categories: &Value) -> Value {
    let Value::Array(items) = categories else {
        return categories.clone();
    };
    let rename = |category: &Value| {
        let renamed = MIGRATION
            .iter()
            .find(|(old, _)| category.as_bytes() == Some(old));
        renamed.map_or_else(|| category.clone(), |(_, new)| text(new.as_bytes()))
    };
    Value::Array(
        items
            .iter()
            .map(|(key, category)| (key.clone(), rename(category)))
            .collect(),
    )
}

/// `get_items()`: the patterns core and then the theme register.
fn patterns(context: &Context, params: &Params) -> Result<Response, RestError> {
    if !bundled(context) {
        return Err(RestError::not_ported());
    }
    if context.head {
        return Ok(Response::new(Value::Array(Vec::new()), 200));
    }
    let wanted = fields_for_response(params, PATTERN_PROPERTIES);
    let registered: Vec<&Value> = (core("patterns")
        .iter()
        .chain(context.site.theme().registered("patterns")))
    .map(|(_, item)| item)
    .collect();
    let siteurl = context.option("siteurl").unwrap_or_default();
    prepared(context.site.theme(), &registered, &wanted, &siteurl)
        .map(|data| Response::new(list(data), 200))
}

/// `prepare_item_for_response()` of each registered pattern, with its pattern
/// blocks resolved.
pub(super) fn prepared(
    theme: &Theme,
    registered: &[&Value],
    wanted: &[Vec<u8>],
    siteurl: &[u8],
) -> Result<Vec<Value>, RestError> {
    let mut data = Vec::new();
    for item in registered.iter().copied() {
        // A theme's patterns keep their content in the export of its files.
        let name = item.get_str("name").and_then(Value::as_bytes);
        let source = name
            .and_then(|name| theme.pattern_entry(name))
            .unwrap_or(item);
        let content = source.get_str("content").map(Value::to_php_string);
        let content = under_site(&content.unwrap_or_default(), siteurl);
        let content = text(resolve_patterns(theme, &content, siteurl)?);
        let kept: Vec<(Key, Value)> = ITEM_KEYS
            .iter()
            .zip(PATTERN_PROPERTIES)
            .filter(|(_, (rest, _))| is_field_included(rest, wanted))
            .filter_map(|(registered, (rest, _))| {
                let value = match *registered {
                    "content" => Some(&content),
                    _ => item.get_str(registered),
                }?;
                let value = match *registered {
                    "categories" => migrated(value),
                    _ => value.clone(),
                };
                Some((Key::String(rest.as_bytes().to_vec()), value))
            })
            .collect();
        data.push(Value::Array(kept));
    }
    Ok(data)
}

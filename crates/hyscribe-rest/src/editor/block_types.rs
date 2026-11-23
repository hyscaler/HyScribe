// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_Block_Types_Controller`: `/wp/v2/block-types`, by namespace
//! and by name.

use hyscribe_phpser::{Key, Value, add_query_arg, is_empty_string};

use crate::fields::{Properties, fields_for_response, is_field_included, links_wanted};
use crate::request::{Params, string_arg};
use crate::response::{Response, RestError, list};
use crate::types::{can_edit_any, context_arg};
use crate::{Context, Endpoint};

use super::core;
use hyscribe_render::theme::Theme;

const EVERY: &[&str] = &["embed", "view", "edit"];

/// The schema's properties, each in every context.
const PROPERTIES: &Properties = &[
    ("api_version", EVERY),
    ("title", EVERY),
    ("name", EVERY),
    ("description", EVERY),
    ("icon", EVERY),
    ("attributes", EVERY),
    ("provides_context", EVERY),
    ("uses_context", EVERY),
    ("selectors", EVERY),
    ("supports", EVERY),
    ("category", EVERY),
    ("is_dynamic", EVERY),
    ("editor_script_handles", EVERY),
    ("script_handles", EVERY),
    ("view_script_handles", EVERY),
    ("view_script_module_ids", EVERY),
    ("editor_style_handles", EVERY),
    ("style_handles", EVERY),
    ("view_style_handles", EVERY),
    ("styles", EVERY),
    ("variations", EVERY),
    ("textdomain", EVERY),
    ("parent", EVERY),
    ("ancestor", EVERY),
    ("allowed_blocks", EVERY),
    ("keywords", EVERY),
    ("example", EVERY),
    ("block_hooks", EVERY),
    ("editor_script", EVERY),
    ("script", EVERY),
    ("view_script", EVERY),
    ("editor_style", EVERY),
    ("style", EVERY),
];

/// The block whose variations' callback reads the theme's data, which
/// registers its style partials as block styles.
const READS_THEME_DATA: &[u8] = b"core/template-part";

/// An argument typed `string`, sanitized by its schema.
pub(super) fn endpoints(regex: &str) -> Option<Vec<Endpoint>> {
    let collection = || Endpoint {
        args: vec![context_arg(), string_arg("namespace")],
        permission: |context, _| read_permission(context),
        callback: Some(items),
    };
    match regex {
        "/wp/v2/block-types" | "/wp/v2/block-types/(?P<namespace>[a-zA-Z0-9_-]+)" => {
            Some(vec![collection()])
        }
        "/wp/v2/block-types/(?P<namespace>[a-zA-Z0-9_-]+)/(?P<name>[a-zA-Z0-9_-]+)" => {
            Some(vec![Endpoint {
                args: vec![string_arg("name"), string_arg("namespace"), context_arg()],
                permission: item_permission,
                callback: Some(item),
            }])
        }
        _ => None,
    }
}

/// `check_read_permission()`.
fn read_permission(context: &Context) -> Result<(), RestError> {
    if can_edit_any(context) {
        return Ok(());
    }
    let message = "Sorry, you are not allowed to manage block types.";
    Err(context.forbidden("rest_block_type_cannot_view", message))
}

fn field<'a>(block: &'a Value, name: &str) -> &'a [u8] {
    block
        .get_str(name)
        .and_then(Value::as_bytes)
        .unwrap_or_default()
}

/// `get_block()`: the registered block type named by the request.
fn block(params: &Params) -> Result<&'static Value, RestError> {
    let name = [params.text("namespace"), params.text("name")].join(&b'/');
    core("block_types")
        .iter()
        .map(|(_, block)| block)
        .find(|block| field(block, "name") == name)
        .ok_or_else(|| RestError::status("rest_block_type_invalid", "Invalid block type.", 404))
}

/// `get_item_permissions_check()`.
fn item_permission(context: &Context, params: &Params) -> Result<(), RestError> {
    read_permission(context).and_then(|()| block(params).map(|_| ()))
}

/// `get_items()`: a namespace's block types, or all.
fn items(context: &Context, params: &Params) -> Result<Response, RestError> {
    if context.head {
        return Ok(Response::new(Value::Array(Vec::new()), 200));
    }
    let namespace = params.text("namespace");
    let mut partials = false;
    let mut data = Vec::new();
    for (_, block) in core("block_types") {
        let name = field(block, "name");
        let prefix = name.split(|&b| b == b'/').next().unwrap_or_default();
        if !is_empty_string(&namespace) && prefix != namespace {
            continue;
        }
        let (response, read) = prepare(context, params, block, partials);
        partials |= read;
        data.push(response.into_data());
    }
    Ok(Response::new(list(data), 200))
}

/// `get_item()`.
fn item(context: &Context, params: &Params) -> Result<Response, RestError> {
    block(params).map(|block| prepare(context, params, block, false).0)
}

/// The block styles registered for `name`: the theme's, then its partials'
/// once they are.
fn registered_styles<'a>(
    theme: &'a Theme,
    name: &'a [u8],
    partials: bool,
) -> impl Iterator<Item = Value> + 'a {
    let partial = theme
        .registered("partial_styles")
        .iter()
        .filter(move |_| partials);
    theme
        .registered("block_styles")
        .iter()
        .chain(partial)
        .filter(move |(_, entry)| entry.get(&Key::Int(0)).and_then(Value::as_bytes) == Some(name))
        .filter_map(|(_, entry)| entry.get(&Key::Int(1)).cloned())
}

/// `prepare_item_for_response()`, and whether it read the theme's data
/// with `partials` not yet registered.
fn prepare(context: &Context, params: &Params, block: &Value, partials: bool) -> (Response, bool) {
    if context.head {
        return (Response::new(Value::Array(Vec::new()), 200), false);
    }
    let wanted = fields_for_response(params, PROPERTIES);
    let name = field(block, "name");
    let included = |field: &[u8]| is_field_included(&String::from_utf8_lossy(field), &wanted);
    let data: Vec<(Key, Value)> = block
        .as_array()
        .unwrap_or_default()
        .iter()
        .filter(|(key, _)| included(&key.to_php_string()))
        .map(|(key, value)| {
            // The variations the theme changes are its own.
            if key.to_php_string() == b"variations" {
                let themed = context.site.theme().registered("block_variations");
                let own = themed
                    .iter()
                    .find(|(block, _)| block.to_php_string() == name);
                return (key.clone(), own.map_or(value, |(_, own)| own).clone());
            }
            if key.to_php_string() != b"styles" {
                return (key.clone(), value.clone());
            }
            let own = value.as_array().unwrap_or_default().iter();
            let styles = own.map(|(_, style)| style.clone()).chain(registered_styles(
                context.site.theme(),
                name,
                partials,
            ));
            (key.clone(), list(styles.collect()))
        })
        .collect();
    let reads = name == READS_THEME_DATA && included(b"variations");
    let mut response = Response::new(Value::Array(data), 200);
    if links_wanted(&wanted) {
        let name = String::from_utf8_lossy(name);
        let namespace = name.split('/').next().unwrap_or_default();
        response.add_link("collection", context.rest_url("/wp/v2/block-types"));
        crate::self_link(
            context,
            &mut response,
            &format!("/wp/v2/block-types/{name}"),
        );
        response.add_link(
            "up",
            context.rest_url(&format!("/wp/v2/block-types/{namespace}")),
        );
        if block.get_str("is_dynamic") == Some(&Value::Bool(true)) {
            let renderer = context.rest_url(&format!("/wp/v2/block-renderer/{name}"));
            let href = add_query_arg(&[(b"context", Some(b"edit"))], renderer.as_bytes());
            let href = String::from_utf8_lossy(&href).into_owned();
            response.add_link("https://api.w.org/render-block", href);
        }
    }
    (response, reads)
}

// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_Post_Types_Controller`: `/wp/v2/types` and
//! `/wp/v2/types/<type>`, over the post types exported by
//! `reference/wordpress/export.sh`.

use std::sync::OnceLock;

use hyscribe_phpser::{Key, Value, json_decode};

use crate::fields::{Properties, fields_for_response, filter_by_context, is_field_included};
use crate::request::{Arg, Params, Sanitize};
use crate::response::{Response, RestError, object, text};
use crate::{Context, Endpoint};

const POST_TYPES: &[u8] = include_bytes!("../../../wordpress/wp-includes/rest-post-types.json");

/// The schema's properties and their contexts.
const PROPERTIES: &Properties = &[
    ("capabilities", &["edit"]),
    ("description", &["view", "edit"]),
    ("hierarchical", &["view", "edit"]),
    ("viewable", &["edit"]),
    ("labels", &["edit"]),
    ("name", &["view", "edit", "embed"]),
    ("slug", &["view", "edit", "embed"]),
    ("supports", &["edit"]),
    ("has_archive", &["view", "edit"]),
    ("taxonomies", &["view", "edit"]),
    ("rest_base", &["view", "edit", "embed"]),
    ("rest_namespace", &["view", "edit", "embed"]),
    ("visibility", &["edit"]),
    ("icon", &["view", "edit", "embed"]),
    ("template", &["view", "edit", "embed"]),
    ("template_lock", &["view", "edit", "embed"]),
];

/// Every registered post type by name: the fields of those shown in REST,
/// in the edit context, and `false` for the others.
fn post_types() -> &'static [(Key, Value)] {
    static TYPES: OnceLock<Value> = OnceLock::new();
    TYPES
        .get_or_init(|| json_decode(POST_TYPES).unwrap_or(Value::Null))
        .as_array()
        .unwrap_or_default()
}

fn shown_in_rest() -> impl Iterator<Item = (&'static Key, &'static Value)> {
    post_types()
        .iter()
        .filter(|(_, fields)| matches!(fields, Value::Array(_)))
        .map(|(name, fields)| (name, fields))
}

fn field<'a>(fields: &'a Value, name: &str) -> &'a [u8] {
    fields
        .get_path(&[name])
        .and_then(Value::as_bytes)
        .unwrap_or_default()
}

/// `rest_get_route_for_post_type_items()`: the route of a post type shown
/// in REST.
pub(crate) fn items_route(name: &str) -> Option<String> {
    let found = shown_in_rest().find(|(key, _)| key.to_php_string() == name.as_bytes());
    found.map(|(_, fields)| {
        format!(
            "/{}/{}",
            String::from_utf8_lossy(field(fields, "rest_namespace")),
            String::from_utf8_lossy(field(fields, "rest_base"))
        )
    })
}

/// `current_user_can( $type->cap->edit_posts )`.
fn can_edit(context: &Context, fields: &Value) -> bool {
    let cap = fields
        .get_path(&["capabilities", "edit_posts"])
        .and_then(Value::as_bytes);
    context.can(&String::from_utf8_lossy(cap.unwrap_or_default()))
}

fn wants_edit(params: &Params) -> bool {
    params.context() == b"edit"
}

fn cannot_edit(code: &'static str, context: &Context) -> RestError {
    let message = "Sorry, you are not allowed to edit posts in this post type.";
    RestError::status(code, message, context.authorization_required())
}

/// `get_context_param( array( 'default' => 'view' ) )`.
pub(crate) fn context_arg() -> Arg {
    Arg {
        name: "context",
        schema: object([
            ("type", text("string")),
            (
                "enum",
                Value::Array(vec![
                    (Key::Int(0), text("view")),
                    (Key::Int(1), text("embed")),
                    (Key::Int(2), text("edit")),
                ]),
            ),
            ("default", text("view")),
        ]),
        validate: true,
        sanitize: Sanitize::Key,
    }
}

/// The endpoint `index` of the route `regex`, if ported.
pub(crate) fn endpoints(regex: &str) -> Option<Vec<Endpoint>> {
    match regex {
        "/wp/v2/types" => Some(vec![Endpoint {
            args: vec![context_arg()],
            permission: items_permission,
            callback: Some(items),
        }]),
        "/wp/v2/types/(?P<type>[\\w-]+)" => Some(vec![Endpoint {
            args: vec![
                Arg {
                    name: "type",
                    schema: object([("type", text("string"))]),
                    validate: false,
                    sanitize: Sanitize::Schema,
                },
                context_arg(),
            ],
            permission: |_, _| Ok(()),
            callback: Some(item),
        }]),
        _ => None,
    }
}

/// `get_items_permissions_check()`.
fn items_permission(context: &Context, params: &Params) -> Result<(), RestError> {
    if !wants_edit(params) || shown_in_rest().any(|(_, fields)| can_edit(context, fields)) {
        return Ok(());
    }
    Err(cannot_edit("rest_cannot_view", context))
}

/// `get_items()`.
fn items(context: &Context, params: &Params) -> Result<Response, RestError> {
    if context.head {
        return Ok(Response::new(Value::Array(Vec::new()), 200));
    }
    let data = shown_in_rest()
        .filter(|(_, fields)| !wants_edit(params) || can_edit(context, fields))
        .map(|(name, fields)| (name.clone(), prepare(context, params, fields).into_data()))
        .collect();
    Ok(Response::new(Value::Array(data), 200))
}

/// `get_item()`.
fn item(context: &Context, params: &Params) -> Result<Response, RestError> {
    let name = params
        .get("type")
        .map(Value::to_php_string)
        .unwrap_or_default();
    let Some((_, fields)) = post_types()
        .iter()
        .find(|(key, _)| key.to_php_string() == name)
    else {
        return Err(RestError::status(
            "rest_type_invalid",
            "Invalid post type.",
            404,
        ));
    };
    if !matches!(fields, Value::Array(_)) {
        let status = context.authorization_required();
        return Err(RestError::status(
            "rest_cannot_read_type",
            "Cannot view post type.",
            status,
        ));
    }
    if wants_edit(params) && !can_edit(context, fields) {
        return Err(cannot_edit("rest_forbidden_context", context));
    }
    Ok(prepare(context, params, fields))
}

/// `prepare_item_for_response()`.
fn prepare(context: &Context, params: &Params, fields: &Value) -> Response {
    if context.head {
        return Response::new(Value::Array(Vec::new()), 200);
    }
    let wanted = fields_for_response(params, PROPERTIES);
    let data: Vec<(Key, Value)> = fields
        .as_array()
        .unwrap_or_default()
        .iter()
        .filter(|(name, _)| {
            is_field_included(&String::from_utf8_lossy(&name.to_php_string()), &wanted)
        })
        .cloned()
        .collect();
    let shown = params.context();
    let mut response = Response::new(
        Value::Array(filter_by_context(data, PROPERTIES, &shown)),
        200,
    );
    if is_field_included("_links", &wanted) || is_field_included("_embedded", &wanted) {
        response.add_link("collection", context.rest_url("/wp/v2/types"));
        let slug = String::from_utf8_lossy(field(fields, "slug"));
        let items = items_route(&slug).unwrap_or_default();
        response.add_link("https://api.w.org/items", context.rest_url(&items));
    }
    response
}

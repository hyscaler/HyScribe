// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_Taxonomies_Controller`: `/wp/v2/taxonomies` and
//! `/wp/v2/taxonomies/<taxonomy>`, over the taxonomies exported by
//! `reference/wordpress/export.sh`.

use std::sync::OnceLock;

use hyscribe_phpser::{Key, Value, json_decode};

use crate::fields::{Properties, fields_for_response, filter_by_context, links_wanted};
use crate::request::{Arg, Params, Sanitize};
use crate::response::{Response, RestError, object, text};
use crate::types::context_arg;
use crate::{Context, Endpoint};

const TAXONOMIES: &[u8] = include_bytes!("../../../wordpress/wp-includes/rest-taxonomies.json");

/// The schema's properties and their contexts.
const PROPERTIES: &Properties = &[
    ("capabilities", &["edit"]),
    ("description", &["view", "edit"]),
    ("hierarchical", &["view", "edit"]),
    ("labels", &["edit"]),
    ("name", &["view", "edit", "embed"]),
    ("slug", &["view", "edit", "embed"]),
    ("show_cloud", &["edit"]),
    ("types", &["view", "edit"]),
    ("rest_base", &["view", "edit", "embed"]),
    ("rest_namespace", &["view", "edit", "embed"]),
    ("visibility", &["edit"]),
];

/// Every registered taxonomy by name: the fields of those shown in REST,
/// in the edit context, and `false` for the others.
pub(crate) fn taxonomies() -> &'static [(Key, Value)] {
    static TAXONOMIES_DATA: OnceLock<Value> = OnceLock::new();
    TAXONOMIES_DATA
        .get_or_init(|| json_decode(TAXONOMIES).unwrap_or(Value::Null))
        .as_array()
        .unwrap_or_default()
}

/// The fields of the taxonomy `name`, if shown in REST.
pub(crate) fn shown(name: &str) -> Option<&'static Value> {
    taxonomies()
        .iter()
        .find(|(key, fields)| key.to_php_string() == name.as_bytes() && is_shown(fields))
        .map(|(_, fields)| fields)
}

/// Whether the taxonomy `name` is registered.
pub(crate) fn registered(name: &str) -> bool {
    taxonomies()
        .iter()
        .any(|(key, _)| key.to_php_string() == name.as_bytes())
}

fn is_shown(fields: &Value) -> bool {
    matches!(fields, Value::Array(_))
}

/// A field of a taxonomy's, as text.
pub(crate) fn field(fields: &Value, path: &[&str]) -> String {
    let value = fields.get_path(path).map(Value::to_php_string);
    String::from_utf8_lossy(&value.unwrap_or_default()).into_owned()
}

/// `current_user_can( $taxonomy->cap->assign_terms )`.
fn can_assign(context: &Context, fields: &Value) -> bool {
    context.can(&field(fields, &["capabilities", "assign_terms"]))
}

fn wants_edit(params: &Params) -> bool {
    params.context() == b"edit"
}

fn cannot_manage(code: &'static str, context: &Context) -> RestError {
    let message = "Sorry, you are not allowed to manage terms in this taxonomy.";
    context.forbidden(code, message)
}

/// The taxonomies `type` names, `get_object_taxonomies()`, or all.
fn of_type(params: &Params) -> impl Iterator<Item = &'static (Key, Value)> {
    let ty = params
        .get("type")
        .map(Value::to_php_string)
        .filter(|ty| !Value::String(ty.clone()).is_empty());
    taxonomies().iter().filter(move |(_, fields)| {
        let types = fields.get_str("types").and_then(Value::as_array);
        ty.as_ref().is_none_or(|ty| {
            types
                .unwrap_or_default()
                .iter()
                .any(|(_, name)| name.to_php_string() == *ty)
        })
    })
}

/// The endpoint `index` of the route `regex`, if ported.
pub(crate) fn endpoints(regex: &str) -> Option<Vec<Endpoint>> {
    let string = || object([("type", text("string"))]);
    match regex {
        "/wp/v2/taxonomies" => Some(vec![Endpoint {
            args: vec![
                context_arg(),
                Arg {
                    name: "type",
                    schema: string(),
                    validate: false,
                    sanitize: Sanitize::Schema,
                },
            ],
            permission: items_permission,
            callback: Some(items),
        }]),
        "/wp/v2/taxonomies/(?P<taxonomy>[\\w-]+)" => Some(vec![Endpoint {
            args: vec![
                Arg {
                    name: "taxonomy",
                    schema: string(),
                    validate: false,
                    sanitize: Sanitize::Schema,
                },
                context_arg(),
            ],
            permission: item_permission,
            callback: Some(item),
        }]),
        _ => None,
    }
}

/// `get_items_permissions_check()`.
fn items_permission(context: &Context, params: &Params) -> Result<(), RestError> {
    let assigns = |(_, fields): &&(Key, Value)| is_shown(fields) && can_assign(context, fields);
    if !wants_edit(params) || of_type(params).any(|item| assigns(&item)) {
        return Ok(());
    }
    Err(cannot_manage("rest_cannot_view", context))
}

/// `get_items()`.
fn items(context: &Context, params: &Params) -> Result<Response, RestError> {
    if context.head {
        return Ok(Response::new(Value::Array(Vec::new()), 200));
    }
    let data: Vec<(Key, Value)> = of_type(params)
        .filter(|(_, fields)| is_shown(fields))
        .filter(|(_, fields)| !wants_edit(params) || can_assign(context, fields))
        .map(|(name, fields)| (name.clone(), prepare(context, params, fields).into_data()))
        .collect();
    if data.is_empty() {
        let empty = Value::Object {
            class: b"stdClass".to_vec(),
            properties: Vec::new(),
        };
        return Ok(Response::new(empty, 200));
    }
    Ok(Response::new(Value::Array(data), 200))
}

fn requested(params: &Params) -> Option<&'static (Key, Value)> {
    let name = params.text("taxonomy");
    taxonomies()
        .iter()
        .find(|(key, _)| key.to_php_string() == name)
}

/// `get_item_permissions_check()`.
fn item_permission(context: &Context, params: &Params) -> Result<(), RestError> {
    match requested(params) {
        Some((_, fields)) if !is_shown(fields) => Err(forbidden(context)),
        Some((_, fields)) if wants_edit(params) && !can_assign(context, fields) => {
            Err(cannot_manage("rest_forbidden_context", context))
        }
        _ => Ok(()),
    }
}

/// A permission check's `false`.
pub(crate) fn forbidden(context: &Context) -> RestError {
    let message = "Sorry, you are not allowed to do that.";
    context.forbidden("rest_forbidden", message)
}

/// `get_item()`.
fn item(context: &Context, params: &Params) -> Result<Response, RestError> {
    let Some((_, fields)) = requested(params) else {
        return Err(RestError::status(
            "rest_taxonomy_invalid",
            "Invalid taxonomy.",
            404,
        ));
    };
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
            let name = String::from_utf8_lossy(&name.to_php_string()).into_owned();
            wanted.iter().any(|field| *field == name.as_bytes())
        })
        .cloned()
        .collect();
    let mut response = Response::new(
        Value::Array(filter_by_context(data, PROPERTIES, &params.context())),
        200,
    );
    if links_wanted(&wanted) {
        response.add_link("collection", context.rest_url("/wp/v2/taxonomies"));
        let items = format!(
            "/{}/{}",
            field(fields, &["rest_namespace"]),
            field(fields, &["rest_base"])
        );
        response.add_link("https://api.w.org/items", context.rest_url(&items));
    }
    response
}

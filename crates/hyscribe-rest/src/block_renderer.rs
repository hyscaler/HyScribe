// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_Block_Renderer_Controller`: `/wp/v2/block-renderer/<name>`, a
//! dynamic block rendered by `hyscribe-render` with the attributes given,
//! checked against the block's attribute schemas, for the editor's server
//! side rendered previews. Blocks whose rendering here is not checked
//! against PHP's are answered 501.

use hyscribe_phpser::Value;
use hyscribe_render::registry::block_type;
use hyscribe_render::rest::Rendering;

use crate::posts::render_request;
use crate::request::{Arg, Params, Sanitize, arg, typed};
use crate::response::{Response, RestError, object, text};
use crate::schema::{Checked, sanitize_value_from_schema, validate_value_from_schema};
use crate::{Context, Endpoint};

/// The route's regex.
const ROUTE: &str = "/wp/v2/block-renderer/(?P<name>[a-z0-9-]+/[a-z0-9-]+)";

/// The dynamic blocks rendered here as PHP renders them outside a page,
/// which `reference/rest/renderer.list` records.
const RENDERED: &[&str] = &[
    "core/archives",
    "core/categories",
    "core/home-link",
    "core/latest-comments",
    "core/latest-posts",
    "core/page-list",
    "core/post-author-name",
    "core/post-date",
    "core/post-excerpt",
    "core/post-terms",
    "core/post-title",
    "core/search",
    "core/site-tagline",
    "core/site-title",
];

pub(crate) fn endpoints(regex: &str) -> Option<Vec<Endpoint>> {
    (regex == ROUTE).then(|| {
        vec![Endpoint {
            args: vec![
                typed("name", r#"{"type":"string"}"#),
                arg(
                    "context",
                    r#"{"type":"string","enum":["edit"],"default":"view"}"#,
                    true,
                    Sanitize::Key,
                ),
                Arg {
                    name: "attributes",
                    schema: object([
                        ("type", text("object")),
                        ("default", Value::Array(Vec::new())),
                    ]),
                    validate: false,
                    sanitize: Sanitize::Params(attributes),
                },
                typed("post_id", r#"{"type":"integer"}"#),
            ],
            permission,
            callback: Some(item),
        }]
    })
}

/// The schema of the attributes of the block `params` names, as the
/// argument's callbacks build it; none for a block not registered.
fn schema(params: &Params) -> Option<Value> {
    let name = String::from_utf8_lossy(&params.text("name")).into_owned();
    let block = block_type(&name)?;
    Some(object([
        ("type", text("object")),
        ("properties", Value::Array(block.attributes.clone())),
        ("additionalProperties", Value::Bool(false)),
    ]))
}

/// The argument's `validate_callback`, then its `sanitize_callback`: a
/// block not registered takes any value, which the callback refuses.
fn attributes(params: &Params, value: &Value) -> Checked<Value> {
    let Some(schema) = schema(params) else {
        return Ok(Value::Bool(true));
    };
    // The callbacks name no parameter.
    validate_value_from_schema(value, &schema, "")?;
    sanitize_value_from_schema(value, &schema, "")
}

/// `isset( $request['post_id'] ) ? (int) $request['post_id'] : 0`.
fn post_id(params: &Params) -> i64 {
    params.get("post_id").map_or(0, Value::to_int)
}

/// `get_item_permissions_check()`, for a block rendered here.
fn permission(context: &Context, params: &Params) -> Result<(), RestError> {
    let name = String::from_utf8_lossy(&params.text("name")).into_owned();
    if crate::editor::is_dynamic(&name) && !RENDERED.contains(&name.as_str()) {
        return Err(RestError::not_ported());
    }
    let id = post_id(params);
    if id > 0 {
        let found = u64::try_from(id)
            .ok()
            .and_then(|id| context.site.post(id))
            .is_some_and(|post| context.can_for("edit_post", post.id));
        if !found {
            let message = "Sorry, you are not allowed to read blocks of this post.";
            return Err(context.forbidden("block_cannot_read", message));
        }
    } else if !context.can("edit_posts") {
        let message = "Sorry, you are not allowed to read blocks as this user.";
        return Err(context.forbidden("block_cannot_read", message));
    }
    Ok(())
}

/// `get_item()`: the block rendered with the post the global one.
fn item(context: &Context, params: &Params) -> Result<Response, RestError> {
    let name = String::from_utf8_lossy(&params.text("name")).into_owned();
    if !crate::editor::is_dynamic(&name) {
        return Err(RestError::status("block_invalid", "Invalid block.", 404));
    }
    // Sanitized as an object, an array.
    let attributes = params.get("attributes").and_then(Value::as_array);
    let attributes = attributes.unwrap_or_default().to_vec();
    // No post has the ID 0, so it renders as none.
    let post = u64::try_from(post_id(params)).ok();
    let request = render_request(context);
    let mut rendering = Rendering::new(context.site.theme(), context.site.reads(), &request);
    let rendered = rendering.render_block(&name, attributes, post);
    Ok(Response::new(
        object([("rendered", Value::String(rendered))]),
        200,
    ))
}

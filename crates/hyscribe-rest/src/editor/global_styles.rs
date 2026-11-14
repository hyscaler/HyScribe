// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_Global_Styles_Controller`: the active theme's global settings
//! and styles, `/wp/v2/global-styles/themes/<stylesheet>`, its style
//! variations, `.../variations`, and a site's global styles post,
//! `/wp/v2/global-styles/<id>`. Twenty Twenty-Five's styles name no theme
//! file, so they link to none.

use std::sync::OnceLock;

use hyscribe_model::Post;
use hyscribe_phpser::{Key, Value, json_decode};
use hyscribe_render::post::admin_title;
use hyscribe_render::theme::{Theme, twentytwentyfive};

use crate::fields::{Properties, fields_for_response, is_field_included};
use crate::request::{Arg, Params, Sanitize};
use crate::response::{Response, RestError, list, object, text};
use crate::types::can_edit_any;
use crate::{Context, Endpoint};

/// The schema's properties shown in the theme's global styles.
const PROPERTIES: &Properties = &[
    ("styles", &["view", "edit"]),
    ("settings", &["view", "edit"]),
];

/// The schema's properties of a global styles post.
const POST_PROPERTIES: &Properties = &[
    ("id", &["embed", "view", "edit"]),
    ("styles", &["view", "edit"]),
    ("settings", &["view", "edit"]),
    ("title", &["embed", "view", "edit"]),
];

fn theme() -> &'static Theme {
    static THEME: OnceLock<Theme> = OnceLock::new();
    THEME.get_or_init(twentytwentyfive)
}

fn stylesheet_arg(sanitize: Sanitize) -> Vec<Arg> {
    vec![Arg {
        name: "stylesheet",
        schema: object([("type", text("string"))]),
        validate: false,
        sanitize,
    }]
}

pub(super) fn endpoints(regex: &str) -> Option<Vec<Endpoint>> {
    match regex {
        "/wp/v2/global-styles/themes/(?P<stylesheet>[\\/\\s%\\w\\.\\(\\)\\[\\]\\@_\\-]+)/variations" => {
            Some(vec![Endpoint {
                args: stylesheet_arg(Sanitize::Schema),
                permission,
                callback: Some(variations),
            }])
        }
        "/wp/v2/global-styles/themes/(?P<stylesheet>[^\\/:<>\\*\\?\"\\|]+(?:\\/[^\\/:<>\\*\\?\"\\|]+)?)" => {
            Some(vec![Endpoint {
                args: stylesheet_arg(Sanitize::Urldecode),
                permission,
                callback: Some(item),
            }])
        }
        "/wp/v2/global-styles/(?P<id>[\\/\\d+]+)" => Some(vec![
            Endpoint {
                args: vec![Arg {
                    name: "id",
                    schema: object([("type", text("integer"))]),
                    validate: false,
                    sanitize: Sanitize::Schema,
                }],
                permission: post_permission,
                callback: Some(post_item),
            },
            Endpoint {
                args: Vec::new(),
                permission: update_permission,
                callback: None,
            },
        ]),
        _ => None,
    }
}

/// `get_post()`: the global styles post of the request's ID.
fn styles_post(context: &Context, params: &Params) -> Result<Post, RestError> {
    let id = params.get("id").map_or(0, Value::to_int);
    let post = u64::try_from(id).ok().and_then(|id| context.site.post(id));
    post.filter(|post| post.post_type == "wp_global_styles")
        .ok_or_else(|| {
            let message = "No global styles config exists with that ID.";
            RestError::status("rest_global_styles_not_found", message, 404)
        })
}

/// `check_update_permission()`: the post type is shown in REST.
fn can_update(context: &Context, post: &Post) -> bool {
    context.can_for("edit_post", post.id)
}

/// `get_item_permissions_check()`.
fn post_permission(context: &Context, params: &Params) -> Result<(), RestError> {
    let status = context.authorization_required();
    let edit = params.get("context") == Some(&text("edit"));
    styles_post(context, params).and_then(|post| {
        if edit && !can_update(context, &post) {
            let message = "Sorry, you are not allowed to edit this global style.";
            Err(RestError::status("rest_forbidden_context", message, status))
        } else if !context.can_for("read_post", post.id) {
            let message = "Sorry, you are not allowed to view this global style.";
            Err(RestError::status("rest_cannot_view", message, status))
        } else {
            Ok(())
        }
    })
}

/// `update_item_permissions_check()`.
fn update_permission(context: &Context, params: &Params) -> Result<(), RestError> {
    let message = "Sorry, you are not allowed to edit this global style.";
    let status = context.authorization_required();
    styles_post(context, params).and_then(|post| {
        if can_update(context, &post) {
            Ok(())
        } else {
            Err(RestError::status("rest_cannot_edit", message, status))
        }
    })
}

/// `$config[ $name ]` of a user theme.json, or an empty object.
fn config_part(config: Option<&Value>, name: &str) -> Value {
    let part = config.and_then(|config| config.get_str(name));
    part.filter(|part| !part.is_empty())
        .cloned()
        .unwrap_or_else(|| Value::Object {
            class: b"stdClass".to_vec(),
            properties: Vec::new(),
        })
}

/// The `theme-file` links a user theme.json's background images would
/// need, which are not ported.
fn names_files(value: &Value) -> bool {
    value
        .as_array()
        .unwrap_or_default()
        .iter()
        .any(|(key, value)| key.to_php_string() == b"backgroundImage" || names_files(value))
}

/// `get_item()`: `prepare_item_for_response()` of the post.
fn post_item(context: &Context, params: &Params) -> Result<Response, RestError> {
    styles_post(context, params).and_then(|post| post_response(context, params, &post))
}

/// `prepare_item_for_response()` of a global styles post.
fn post_response(context: &Context, params: &Params, post: &Post) -> Result<Response, RestError> {
    let raw = json_decode(&post.content).unwrap_or_default();
    let user = raw.get_str("isGlobalStylesUserThemeJSON") == Some(&Value::Bool(true));
    let config = user.then(|| theme().global_styles().user_theme_json(raw));
    if config.as_ref().is_some_and(names_files) {
        return Err(RestError::not_ported());
    }
    let wanted = fields_for_response(params, POST_PROPERTIES);
    let included = |field: &str| is_field_included(field, &wanted);
    let mut data = Vec::new();
    let mut add = |name: &str, value: Value| {
        data.push((Key::String(name.as_bytes().to_vec()), value));
    };
    if included("id") {
        add("id", Value::Int(i64::try_from(post.id).unwrap_or_default()));
    }
    if included("title") {
        let raw = || text(post.title.clone());
        let rendered = || text(admin_title(post));
        let parts: [(&str, &dyn Fn() -> Value); 2] = [("raw", &raw), ("rendered", &rendered)];
        let title = parts
            .into_iter()
            .filter(|(part, _)| included(&format!("title.{part}")))
            .map(|(part, value)| (Key::String(part.as_bytes().to_vec()), value()));
        add("title", Value::Array(title.collect()));
    }
    for name in ["settings", "styles"] {
        if included(name) {
            add(name, config_part(config.as_ref(), name));
        }
    }
    // `get_fields_for_response()` kept the context's fields.
    let mut response = Response::new(Value::Array(data), 200);
    if included("_links") || included("_embedded") {
        let route = format!("/wp/v2/global-styles/{}", post.id);
        crate::self_link(context, &mut response, &route);
        let about = context.rest_url("/wp/v2/types/wp_global_styles");
        response.add_link("about", about);
        let revisions = context.site.revisions(post.id);
        let count = i64::try_from(revisions.len()).unwrap_or_default();
        let history = object([
            ("count", Value::Int(count)),
            (
                "href",
                text(context.rest_url(&format!("{route}/revisions"))),
            ),
        ]);
        response.add_link_attributes("version-history", history);
        // `wp_global_styles` maps `publish_posts` to itself.
        let actions = [
            ("https://api.w.org/action-publish", "publish_posts"),
            ("https://api.w.org/action-edit-css", "edit_css"),
        ];
        let allowed = actions.into_iter().filter(|(_, cap)| context.can(cap));
        allowed.for_each(|(rel, _)| response.add_link(rel, context.rest_url(&route)));
    }
    Ok(response)
}

/// `get_theme_item_permissions_check()`: who may `edit_theme_options` may
/// edit posts too.
fn permission(context: &Context, _: &Params) -> Result<(), RestError> {
    if can_edit_any(context) {
        return Ok(());
    }
    let message = "Sorry, you are not allowed to access the global styles on this site.";
    let status = context.authorization_required();
    Err(RestError::status(
        "rest_cannot_read_global_styles",
        message,
        status,
    ))
}

/// The stylesheet asked for, if it is the active theme's: only its global
/// styles are served.
fn active(context: &Context, params: &Params) -> Result<Vec<u8>, RestError> {
    let stylesheet = params
        .get("stylesheet")
        .map(Value::to_php_string)
        .unwrap_or_default();
    if context.option("stylesheet").as_ref() == Some(&stylesheet) {
        return Ok(stylesheet);
    }
    Err(RestError::status(
        "rest_theme_not_found",
        "Theme not found.",
        404,
    ))
}

/// `get_theme_item()`.
fn item(context: &Context, params: &Params) -> Result<Response, RestError> {
    let stylesheet = active(context, params)?;
    let wanted = fields_for_response(params, PROPERTIES);
    // The theme has both settings and styles.
    let global = theme().global_styles();
    let data = [("settings", global.settings()), ("styles", global.styles())]
        .into_iter()
        .filter(|(name, _)| is_field_included(name, &wanted))
        .map(|(name, value)| (Key::String(name.as_bytes().to_vec()), value.clone()));
    let mut response = Response::new(Value::Array(data.collect()), 200);
    if is_field_included("_links", &wanted) || is_field_included("_embedded", &wanted) {
        let stylesheet = String::from_utf8_lossy(&stylesheet);
        let route = format!("/wp/v2/global-styles/themes/{stylesheet}");
        crate::self_link(context, &mut response, &route);
    }
    Ok(response)
}

/// `get_theme_items()`.
fn variations(context: &Context, params: &Params) -> Result<Response, RestError> {
    active(context, params)?;
    Ok(Response::new(list(theme().style_variations()), 200))
}

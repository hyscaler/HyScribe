// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_Themes_Controller`: `/wp/v2/themes` and
//! `/wp/v2/themes/<stylesheet>`, served for the active theme, the bundled
//! Twenty Twenty-Five, as `reference/rest/registered.php` exports it. Its
//! links name its `wp_global_styles` post, which reading them creates when
//! there is none, as a write plan the answer is made after; the
//! other themes, which HyScribe does not have, are not ported.

use hyscribe_phpser::{Key, Value};

use crate::fields::{is_field_included, links_wanted, narrowed};
use crate::posts::write::{Outcome, Plan, Write, Writing};
use crate::request::{Arg, Params, Sanitize};
use crate::response::{Response, RestError, list, object, text};
use crate::types::can_edit_any;
use crate::{Context, Endpoint, placed, self_link};

/// The bundled theme's stylesheet.
const STYLESHEET: &str = "twentytwentyfive";

pub(super) fn endpoints(regex: &str) -> Option<Vec<Endpoint>> {
    match regex {
        "/wp/v2/themes" => Some(vec![Endpoint {
            args: vec![Arg {
                name: "status",
                schema: object([
                    ("type", text("array")),
                    (
                        "items",
                        object([
                            ("enum", list(vec![text("active"), text("inactive")])),
                            ("type", text("string")),
                        ]),
                    ),
                ]),
                validate: false,
                sanitize: Sanitize::Schema,
            }],
            permission: items_permission,
            callback: None,
        }]),
        ITEM_ROUTE => Some(vec![Endpoint {
            args: vec![Arg {
                name: "stylesheet",
                schema: object([("type", text("string"))]),
                validate: false,
                sanitize: Sanitize::Urldecode,
            }],
            permission: item_permission,
            callback: None,
        }]),
        _ => None,
    }
}

/// The route of a single theme.
const ITEM_ROUTE: &str =
    "/wp/v2/themes/(?P<stylesheet>[^\\/:<>\\*\\?\"\\|]+(?:\\/[^\\/:<>\\*\\?\"\\|]+)?)";

/// The reads of themes, which write the active theme's global styles post
/// when their links need one: [`endpoints`] leaves their answers to it.
pub(crate) fn writing(regex: &str, _: usize) -> Option<Writing> {
    (regex == "/wp/v2/themes" || regex == ITEM_ROUTE).then_some(Writing::plain(plan, finish))
}

/// The plan of a read: the post it creates, for the bundled active theme
/// when its links are wanted and the site has none, else nothing.
fn plan(context: &Context, params: &Params) -> Result<Plan, RestError> {
    let active = [(Key::Int(0), text("active"))];
    let asked = match params.route == ITEM_ROUTE {
        true => params.text("stylesheet") == STYLESHEET.as_bytes(),
        false => params.get("status") == Some(&Value::Array(active.to_vec())),
    };
    let creates = asked
        && bundled(context)
        && links_wanted(&narrowed(params, Vec::new()))
        && styles_post(context).is_none();
    let write = Write::StylesPost(creates.then(|| STYLESHEET.to_string()));
    Ok(Plan::new(context, write, Value::Null))
}

/// The answer once the post is there: [`items`] or [`item`], as the
/// callbacks of other reads give it.
fn finish(
    context: &Context,
    params: &mut Params,
    _: Plan,
    _: Outcome,
) -> Result<Response, RestError> {
    match params.route == ITEM_ROUTE {
        true => item(context, params),
        false => items(context, params),
    }
}

fn cannot_view(context: &Context) -> RestError {
    let message = "Sorry, you are not allowed to view themes.";
    context.forbidden("rest_cannot_view_themes", message)
}

fn manages_themes(context: &Context) -> bool {
    context.can("switch_themes") || context.can("manage_network_themes")
}

/// `check_read_active_theme_permission()`.
fn read_active_theme(context: &Context) -> Result<(), RestError> {
    if can_edit_any(context) {
        return Ok(());
    }
    let message = "Sorry, you are not allowed to view the active theme.";
    Err(context.forbidden("rest_cannot_view_active_theme", message))
}

/// `get_items_permissions_check()`.
fn items_permission(context: &Context, params: &Params) -> Result<(), RestError> {
    if manages_themes(context) {
        return Ok(());
    }
    let active = [(Key::Int(0), text("active"))];
    if params.get("status") == Some(&Value::Array(active.to_vec())) {
        return read_active_theme(context);
    }
    Err(cannot_view(context))
}

/// `get_item_permissions_check()`: the active theme is the one its
/// stylesheet names.
fn item_permission(context: &Context, params: &Params) -> Result<(), RestError> {
    if manages_themes(context) {
        return Ok(());
    }
    let stylesheet = params.get("stylesheet").map(Value::to_php_string);
    if stylesheet == context.option("stylesheet") {
        return read_active_theme(context);
    }
    Err(cannot_view(context))
}

/// Whether the bundled theme is the site's.
fn bundled(context: &Context) -> bool {
    context.option("stylesheet").as_deref() == Some(STYLESHEET.as_bytes())
}

/// `get_items()`: the active theme alone.
fn items(context: &Context, params: &Params) -> Result<Response, RestError> {
    let active = [(Key::Int(0), text("active"))];
    if params.get("status") != Some(&Value::Array(active.to_vec())) {
        return Err(RestError::not_ported());
    }
    let theme = prepared(context, params)?.into_data();
    let mut response = Response::new(list(vec![theme]), 200);
    response.header("X-WP-Total", "1");
    response.header("X-WP-TotalPages", "1");
    Ok(response)
}

/// `get_item()`.
fn item(context: &Context, params: &Params) -> Result<Response, RestError> {
    if params.text("stylesheet") != STYLESHEET.as_bytes() {
        return Err(RestError::not_ported());
    }
    prepared(context, params)
}

/// The fields given part by part: the headers' `raw` and `rendered`, and
/// each theme support.
const PARTED: [&str; 7] = [
    "author",
    "author_uri",
    "description",
    "name",
    "tags",
    "theme_uri",
    "theme_supports",
];

/// `prepare_item_for_response()` of the active theme: the fields wanted,
/// and of those given part by part, the parts wanted.
fn prepared(context: &Context, params: &Params) -> Result<Response, RestError> {
    if !bundled(context) {
        return Err(RestError::not_ported());
    }
    let theme = super::theme("rest");
    let fields = narrowed(
        params,
        theme.iter().map(|(key, _)| key.to_php_string()).collect(),
    );
    let siteurl = context.option("siteurl").unwrap_or_default();
    let siteurl = String::from_utf8_lossy(&siteurl);
    let data = theme.iter().filter_map(|(key, value)| {
        let name = String::from_utf8_lossy(&key.to_php_string()).into_owned();
        let value = if PARTED.contains(&name.as_str()) {
            let parts = value.as_array().unwrap_or_default().iter();
            let wanted = parts.filter(|(part, _)| {
                let part = String::from_utf8_lossy(&part.to_php_string()).into_owned();
                is_field_included(&format!("{name}.{part}"), &fields)
            });
            Some(Value::Array(wanted.cloned().collect())).filter(|parts| !parts.is_empty())
        } else {
            is_field_included(&name, &fields).then(|| value.clone())
        }?;
        let value = placed(value, b"%siteurl%", &|path| format!("{siteurl}{path}"));
        Some((key.clone(), value))
    });
    let mut response = Response::new(Value::Array(data.collect()), 200);
    if links_wanted(&fields) {
        let id = styles_post(context).ok_or_else(RestError::not_ported)?;
        self_link(
            context,
            &mut response,
            &format!("/wp/v2/themes/{STYLESHEET}"),
        );
        response.add_link("collection", context.rest_url("/wp/v2/themes"));
        let styles = context.rest_url(&format!("/wp/v2/global-styles/{id}"));
        response.add_link("https://api.w.org/user-global-styles", styles);
        let allow = if context.can("export") {
            vec![text("GET")]
        } else {
            Vec::new()
        };
        response.add_link_with(
            "https://api.w.org/export-theme",
            context.rest_url("/wp-block-editor/v1/export"),
            vec![("targetHints", object([("allow", list(allow))]))],
        );
    }
    Ok(response)
}

/// `get_user_global_styles_post_id()` without the post it creates: the
/// newest published `wp_global_styles` post of the theme's `wp_theme` term.
fn styles_post(context: &Context) -> Option<u64> {
    let posts = context.site.posts_of_type("wp_global_styles").into_iter();
    let mut published = posts.filter(|post| post.status == "publish");
    let found = published.find(|post| {
        let terms = context.site.post_terms(post.id, "wp_theme");
        hyscribe_render::site_templates::names_term(&terms, STYLESHEET.as_bytes())
    });
    found.map(|post| post.id)
}

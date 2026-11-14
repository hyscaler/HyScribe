// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_Global_Styles_Controller`'s theme routes: the active theme's
//! global settings and styles, `/wp/v2/global-styles/themes/<stylesheet>`,
//! and its style variations, `.../variations`. The site's own global
//! styles post, `/wp/v2/global-styles/<id>`, is not ported. Twenty
//! Twenty-Five's styles name no theme file, so they link to none.

use std::sync::OnceLock;

use hyscribe_phpser::{Key, Value};
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
                callback: variations,
            }])
        }
        "/wp/v2/global-styles/themes/(?P<stylesheet>[^\\/:<>\\*\\?\"\\|]+(?:\\/[^\\/:<>\\*\\?\"\\|]+)?)" => {
            Some(vec![Endpoint {
                args: stylesheet_arg(Sanitize::Urldecode),
                permission,
                callback: item,
            }])
        }
        _ => None,
    }
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

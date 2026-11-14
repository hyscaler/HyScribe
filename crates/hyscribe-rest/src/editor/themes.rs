// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_Themes_Controller`'s permission checks: `/wp/v2/themes` and
//! `/wp/v2/themes/<stylesheet>`. Every answer they let through is not
//! ported: the active theme's links name its `wp_global_styles` post, which
//! reading them creates when there is none, and the other themes
//! are not installed in HyScribe.

use hyscribe_phpser::{Key, Value};

use crate::request::{Arg, Params, Sanitize};
use crate::response::{RestError, list, object, text};
use crate::types::can_edit_any;
use crate::{Context, Endpoint};

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
            callback: |_, _| Err(RestError::not_ported()),
        }]),
        "/wp/v2/themes/(?P<stylesheet>[^\\/:<>\\*\\?\"\\|]+(?:\\/[^\\/:<>\\*\\?\"\\|]+)?)" => {
            Some(vec![Endpoint {
                args: vec![Arg {
                    name: "stylesheet",
                    schema: object([("type", text("string"))]),
                    validate: false,
                    sanitize: Sanitize::Urldecode,
                }],
                permission: item_permission,
                callback: |_, _| Err(RestError::not_ported()),
            }])
        }
        _ => None,
    }
}

fn cannot_view(context: &Context) -> RestError {
    let message = "Sorry, you are not allowed to view themes.";
    RestError::status(
        "rest_cannot_view_themes",
        message,
        context.authorization_required(),
    )
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
    let status = context.authorization_required();
    Err(RestError::status(
        "rest_cannot_view_active_theme",
        message,
        status,
    ))
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

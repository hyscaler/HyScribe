// SPDX-License-Identifier: AGPL-3.0-or-later

//! The routes of synced patterns and navigation: `/wp/v2/blocks` and
//! `/wp/v2/wp_pattern_category`, `/wp/v2/navigation`, and the classic
//! menus' `/wp/v2/menus`, `/wp/v2/menu-items` and `/wp/v2/menu-locations`,
//! read only.

use hyscribe_phpser::Value;

use crate::posts::{Kind, type_endpoints};
use crate::request::{Params, typed};
use crate::response::{Response, RestError};
use crate::terms::taxonomy_endpoints;
use crate::types::context_arg;
use crate::{Context, Endpoint, unserialized};

/// The endpoints of the route `regex`, if one of these.
pub(crate) fn endpoints(regex: &str) -> Option<Vec<Endpoint>> {
    Some(match regex {
        "/wp/v2/blocks" => type_endpoints(Kind::Block, false),
        "/wp/v2/blocks/(?P<id>[\\d]+)" => type_endpoints(Kind::Block, true),
        "/wp/v2/navigation" => type_endpoints(Kind::Navigation, false),
        "/wp/v2/navigation/(?P<id>[\\d]+)" => type_endpoints(Kind::Navigation, true),
        "/wp/v2/menu-items" => type_endpoints(Kind::MenuItem, false),
        "/wp/v2/menu-items/(?P<id>[\\d]+)" => type_endpoints(Kind::MenuItem, true),
        "/wp/v2/wp_pattern_category" => taxonomy_endpoints("wp_pattern_category", false),
        "/wp/v2/wp_pattern_category/(?P<id>[\\d]+)" => {
            taxonomy_endpoints("wp_pattern_category", true)
        }
        "/wp/v2/menus" => taxonomy_endpoints("nav_menu", false),
        "/wp/v2/menus/(?P<id>[\\d]+)" => taxonomy_endpoints("nav_menu", true),
        "/wp/v2/menu-locations" => vec![Endpoint {
            args: vec![context_arg()],
            permission: locations_permission,
            callback: Some(|_, _| Ok(Response::new(Value::Array(Vec::new()), 200))),
        }],
        "/wp/v2/menu-locations/(?P<location>[\\w-]+)" => vec![Endpoint {
            args: vec![typed("location", r#"{"type":"string"}"#), context_arg()],
            permission: locations_permission,
            callback: Some(|_, _| {
                let message = "Invalid menu location.";
                Err(RestError::status(
                    "rest_menu_location_invalid",
                    message,
                    404,
                ))
            }),
        }],
        _ => return None,
    })
}

/// `WP_REST_Menu_Locations_Controller::check_has_read_only_access()`.
/// HyScribe runs no theme's PHP, so no theme registers a location: the
/// collection is empty and every location invalid.
fn locations_permission(context: &Context, _: &Params) -> Result<(), RestError> {
    if context.can("edit_theme_options") {
        return Ok(());
    }
    let message = "Sorry, you are not allowed to view menu locations.";
    Err(context.forbidden("rest_cannot_view", message))
}

/// The entry `name` of an array option, if both are set.
fn option_entry(context: &Context, option: &str, name: &str) -> Option<Value> {
    let stored = unserialized(&context.option(option)?);
    stored.get_str(name).cloned()
}

/// `get_menu_locations()`: the locations `get_nav_menu_locations()`
/// assigns the menu `id`, by the theme's mods.
pub(crate) fn menu_locations(context: &Context, id: u64) -> Vec<Vec<u8>> {
    let stylesheet = context.option("stylesheet").unwrap_or_default();
    let mods = format!("theme_mods_{}", String::from_utf8_lossy(&stylesheet));
    let locations = option_entry(context, &mods, "nav_menu_locations");
    let assigned = locations
        .as_ref()
        .and_then(Value::as_array)
        .unwrap_or_default();
    let menu = Value::Int(id.cast_signed());
    assigned
        .iter()
        .filter(|(_, assigned)| *assigned == menu)
        .map(|(location, _)| location.to_php_string())
        .collect()
}

/// `get_menu_auto_add()`: whether the `nav_menu_options` option adds new
/// pages to the menu `id`.
pub(crate) fn auto_add(context: &Context, id: u64) -> bool {
    let menus = option_entry(context, "nav_menu_options", "auto_add");
    let menus = menus.as_ref().and_then(Value::as_array).unwrap_or_default();
    menus
        .iter()
        .any(|(_, menu)| *menu == Value::Int(id.cast_signed()))
}

#[cfg(test)]
mod tests;

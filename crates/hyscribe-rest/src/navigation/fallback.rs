// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_Navigation_Fallback_Controller`:
//! `/wp-block-editor/v1/navigation-fallback`. A read that writes: where no
//! `wp_navigation` post exists, `WP_Navigation_Fallback::get_fallback()`
//! makes one, as the navigation block of a page view does.

use hyscribe_phpser::Value;
use hyscribe_render::{Fallback, navigation_fallback};

use crate::fields::{Properties, fields_for_response, is_field_included, links_wanted};
use crate::posts::render_request;
use crate::posts::write::{Outcome, Plan, Write, Writing};
use crate::request::Params;
use crate::response::{Response, RestError, int, object};
use crate::{Context, Endpoint, self_link_with};

const ROUTE: &str = "/wp-block-editor/v1/navigation-fallback";

const PROPERTIES: &Properties = &[("id", &["view", "edit", "embed"])];

pub(super) fn endpoints(regex: &str) -> Option<Vec<Endpoint>> {
    (regex == ROUTE).then(|| {
        vec![Endpoint {
            args: Vec::new(),
            permission,
            callback: None,
        }]
    })
}

pub(crate) fn writing(regex: &str, _: usize) -> Option<Writing> {
    (regex == ROUTE).then_some(Writing::plain(plan, finish))
}

/// `get_item_permissions_check()`. The type's `create_posts` and `edit_posts`
/// are both `edit_theme_options`, so its `edit` context refusal
/// (`rest_forbidden_context`) never refuses one who passed the first check.
fn permission(context: &Context, _: &Params) -> Result<(), RestError> {
    if context.can("edit_theme_options") && context.can("edit_posts") {
        return Ok(());
    }
    let message = "Sorry, you are not allowed to create Navigation Menus as this user.";
    Err(context.forbidden("rest_cannot_create", message))
}

/// The plan of `get_item()`: the post to make where there is none, no
/// answer where the filter declines one, and 501 for a classic menu not
/// ported.
fn plan(context: &Context, _: &Params) -> Result<Plan, RestError> {
    match navigation_fallback(context.site.reads(), &render_request(context)) {
        Fallback::Latest(id, _) => Ok(Plan::new(context, Write::NavigationFallback(None), int(id))),
        Fallback::Create(post) => {
            let write = Write::NavigationFallback(Some(Box::new(post)));
            Ok(Plan::new(context, write, Value::Null))
        }
        Fallback::Declined => {
            let message = "No fallback menu found.";
            Err(RestError::status("no_fallback_menu", message, 404))
        }
        Fallback::NotPorted => Err(RestError::not_ported()),
    }
}

/// `prepare_item_for_response()` of the post found or made.
fn finish(
    context: &Context,
    params: &mut Params,
    plan: Plan,
    outcome: Outcome,
) -> Result<Response, RestError> {
    let id = match outcome {
        Outcome::Written(id) if id != 0 => id,
        _ => u64::try_from(plan.previous.to_int()).unwrap_or_default(),
    };
    let fields = fields_for_response(params, PROPERTIES);
    let data = match is_field_included("id", &fields) {
        true => object([("id", int(id))]),
        false => Value::Array(Vec::new()),
    };
    let mut response = Response::new(data, 200);
    if links_wanted(&fields) {
        let route = format!("/wp/v2/navigation/{id}");
        self_link_with(context, &mut response, &route, true);
    }
    Ok(response)
}

#[cfg(test)]
mod tests;

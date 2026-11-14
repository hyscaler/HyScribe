// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_Block_Pattern_Categories_Controller` and
//! `WP_REST_Block_Patterns_Controller`: `/wp/v2/block-patterns/categories`
//! and `/wp/v2/block-patterns/patterns`.

use hyscribe_phpser::{Key, Value};

use crate::fields::{Properties, fields_for_response, is_field_included};
use crate::request::Params;
use crate::response::{Response, RestError, list};
use crate::types::can_edit_any;
use crate::{Context, Endpoint};

use super::{core, theme};

const PROPERTIES: &Properties = &[
    ("name", &["view", "edit", "embed"]),
    ("label", &["view", "edit", "embed"]),
    ("description", &["view", "edit", "embed"]),
];

pub(super) fn endpoints(regex: &str) -> Option<Vec<Endpoint>> {
    match regex {
        "/wp/v2/block-patterns/categories" => Some(vec![Endpoint {
            args: Vec::new(),
            permission: |context, _| permission(context, "block pattern categories"),
            callback: Some(categories),
        }]),
        // Listing the patterns first fetches those of the wordpress.org
        // pattern directory and caches the answer, failed or not, in a site
        // transient: not ported.
        "/wp/v2/block-patterns/patterns" => Some(vec![Endpoint {
            args: Vec::new(),
            permission: |context, _| permission(context, "block patterns"),
            callback: Some(|_, _| Err(RestError::not_ported())),
        }]),
        _ => None,
    }
}

/// `get_items_permissions_check()`.
fn permission(context: &Context, what: &str) -> Result<(), RestError> {
    if can_edit_any(context) {
        return Ok(());
    }
    let message = format!("Sorry, you are not allowed to view the registered {what}.");
    Err(context.forbidden("rest_cannot_view", &message))
}

/// `get_items()` of the categories core and then the theme register.
fn categories(context: &Context, params: &Params) -> Result<Response, RestError> {
    if context.head {
        return Ok(Response::new(Value::Array(Vec::new()), 200));
    }
    let wanted = fields_for_response(params, PROPERTIES);
    let data = core("pattern_categories")
        .iter()
        .chain(theme("pattern_categories"))
        .map(|(_, category)| {
            let kept: Vec<(Key, Value)> = PROPERTIES
                .iter()
                .filter(|(name, _)| is_field_included(name, &wanted))
                .filter_map(|(name, _)| {
                    let value = category.get_str(name);
                    value.map(|value| (Key::String(name.as_bytes().to_vec()), value.clone()))
                })
                .collect();
            // Every property is in every context, so the context's filter
            // keeps them all.
            Value::Array(kept)
        })
        .collect();
    Ok(Response::new(list(data), 200))
}

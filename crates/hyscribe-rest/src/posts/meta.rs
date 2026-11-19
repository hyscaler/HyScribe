// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_Meta_Fields::update_value()` of a post's `meta`: the keys core
//! registers for the types whose writes are ported. Posts and pages have
//! `footnotes`; synced patterns have it and `wp_pattern_sync_status`. PHP
//! checks and writes the meta after the post is written, so a value it
//! refuses is not ported: nothing is written and the answer is 501.

use hyscribe_kses::sanitize_key;
use hyscribe_phpser::{Value, json_decode, json_encode, lookup};

use super::Kind;
use super::write::as_stored;
use crate::Context;
use crate::request::Params;
use crate::response::{RestError, list, object, text};

/// What the `meta` of a save writes, in the order PHP's registered fields
/// come: a key and its value, `null` to delete it.
pub(super) fn updates(
    context: &Context,
    params: &Params,
    kind: Kind,
) -> Result<Vec<(&'static str, Value)>, RestError> {
    let Some(meta) = params.get("meta").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    let mut updates = Vec::new();
    if kind == Kind::Block {
        match lookup(meta, "wp_pattern_sync_status") {
            None => {}
            Some(status @ Value::String(text)) if text == b"partial" || text == b"unsynced" => {
                updates.push(("wp_pattern_sync_status", status.clone()));
            }
            Some(_) => return Err(RestError::not_ported()),
        }
    }
    match lookup(meta, "footnotes") {
        None => {}
        Some(Value::Null) => updates.push(("footnotes", Value::Null)),
        Some(Value::String(footnotes)) => {
            updates.push(("footnotes", text(filtered(context, footnotes)?)));
        }
        Some(_) => return Err(RestError::not_ported()),
    }
    Ok(updates)
}

/// `_wp_filter_post_meta_footnotes()`, which `sanitize_meta()` applies to
/// the footnotes of a user without `unfiltered_html`: each footnote with a
/// content and an ID kept, its ID a key and its content as kses leaves it,
/// as JSON.
fn filtered(context: &Context, footnotes: &[u8]) -> Result<Vec<u8>, RestError> {
    if context.can("unfiltered_html") {
        return Ok(footnotes.to_vec());
    }
    let Some(Value::Array(decoded)) = json_decode(footnotes) else {
        return Ok(Vec::new());
    };
    let mut kept = Vec::new();
    for (_, footnote) in &decoded {
        let Value::Array(parts) = footnote else {
            continue;
        };
        let given = |name| lookup(parts, name).filter(|value| !value.is_empty());
        let (Some(content), Some(id)) = (given("content"), given("id")) else {
            continue;
        };
        // What PHP would not take as text is not ported.
        let (Value::String(content), Value::String(_) | Value::Int(_)) = (content, id) else {
            return Err(RestError::not_ported());
        };
        let (content, _) = as_stored(context, false, content.clone(), b"");
        let id = sanitize_key(&id.to_php_string());
        kept.push(object([("id", text(id)), ("content", text(content))]));
    }
    // Valid UTF-8 all through, as the footnotes decoded.
    Ok(json_encode(&list(kept)).unwrap_or_default())
}

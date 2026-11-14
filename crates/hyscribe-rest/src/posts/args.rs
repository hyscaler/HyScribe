// SPDX-License-Identifier: AGPL-3.0-or-later

//! The arguments `WP_REST_Posts_Controller` registers: the collection's
//! (`get_collection_params()`) and a single item's.

use hyscribe_phpser::{Value, json_decode};

use super::Kind;
use crate::Context;
use crate::request::{Arg, Sanitize, arg, string_arg, typed};
use crate::response::{list, text};
use crate::schema::{Checked, Refusal, validate_value_from_schema};
use crate::types::context_arg;

const IDS: &str = r#"{"type":"array","items":{"type":"integer"},"default":[]}"#;
const STATUSES: &str = r#"{"default":"publish","type":"array","items":{"enum":["publish","future","draft","pending","private","trash","auto-draft","inherit","request-pending","request-confirmed","request-failed","request-completed","any"],"type":"string"}}"#;
const ATTACHMENT_STATUSES: &str = r#"{"default":"inherit","type":"array","items":{"enum":["inherit","private","trash"],"type":"string"}}"#;

/// The `status` argument's schema for `kind`.
fn statuses_schema(kind: Kind) -> &'static str {
    match kind {
        Kind::Attachment => ATTACHMENT_STATUSES,
        _ => STATUSES,
    }
}

/// `get_collection_params()` of the type, and the menu items' changes.
pub(super) fn collection(kind: Kind) -> Vec<Arg> {
    let mut context = context_arg();
    context.schema =
        json_decode(br#"{"type":"string","enum":["view","embed","edit"],"default":"view"}"#)
            .unwrap_or(Value::Null);
    // `format` (`date-time`) is not ported: the endpoint answers 501.
    let date = |name| arg(name, "{}", false, Sanitize::Endpoint);
    let mut args = vec![
        context,
        arg(
            "page",
            r#"{"type":"integer","default":1,"minimum":1}"#,
            true,
            Sanitize::Absint,
        ),
        arg(
            "per_page",
            if kind == Kind::MenuItem {
                r#"{"type":"integer","default":100,"minimum":1,"maximum":100}"#
            } else {
                r#"{"type":"integer","default":10,"minimum":1,"maximum":100}"#
            },
            true,
            Sanitize::Absint,
        ),
        arg("search", r#"{"type":"string"}"#, true, Sanitize::TextField),
        date("after"),
        date("modified_after"),
    ];
    if kind.supports("author") {
        args.extend([typed("author", IDS), typed("author_exclude", IDS)]);
    }
    args.extend([
        date("before"),
        date("modified_before"),
        typed("exclude", IDS),
        typed("include", IDS),
    ]);
    let mut orderby = r#"["author","date","id","include","modified","parent","relevance","slug","include_slugs","title""#.to_string();
    if kind == Kind::Page {
        args.push(typed("menu_order", r#"{"type":"integer"}"#));
    }
    if matches!(kind, Kind::Page | Kind::MenuItem) {
        orderby.push_str(r#","menu_order""#);
    }
    let (order, by) = match kind {
        Kind::MenuItem => ("asc", "menu_order"),
        _ => ("desc", "date"),
    };
    args.extend([
        typed("search_semantics", r#"{"type":"string","enum":["exact"]}"#),
        typed("offset", r#"{"type":"integer"}"#),
        typed(
            "order",
            &format!(r#"{{"type":"string","default":"{order}","enum":["asc","desc"]}}"#),
        ),
        typed(
            "orderby",
            &format!(r#"{{"type":"string","default":"{by}","enum":{orderby}]}}"#),
        ),
    ]);
    if matches!(kind, Kind::Page | Kind::Attachment) {
        args.extend([typed("parent", IDS), typed("parent_exclude", IDS)]);
    }
    args.extend([
        typed(
            "search_columns",
            r#"{"default":[],"type":"array","items":{"enum":["post_title","post_content","post_excerpt"],"type":"string"}}"#,
        ),
        typed("slug", r#"{"type":"array","items":{"type":"string"}}"#),
        arg(
            "status",
            statuses_schema(kind),
            false,
            Sanitize::Site(match kind {
                Kind::Post => |context, value| statuses(context, value, Kind::Post),
                Kind::Page => |context, value| statuses(context, value, Kind::Page),
                Kind::Block => |context, value| statuses(context, value, Kind::Block),
                Kind::Navigation => |context, value| statuses(context, value, Kind::Navigation),
                Kind::MenuItem => |context, value| statuses(context, value, Kind::MenuItem),
                Kind::Attachment => |context, value| statuses(context, value, Kind::Attachment),
            }),
        ),
    ]);
    if kind == Kind::Attachment {
        args.extend(crate::media::collection_args());
    }
    if !kind.taxonomies().is_empty() {
        args.push(typed(
            "tax_relation",
            r#"{"type":"string","enum":["AND","OR"]}"#,
        ));
    }
    // `oneOf` is not ported: the endpoint answers 501.
    args.extend(
        limits(kind)
            .iter()
            .map(|name| arg(name, "{}", false, Sanitize::Endpoint)),
    );
    if kind == Kind::MenuItem {
        args.push(typed("menu_order", r#"{"type":"integer"}"#));
    }
    if kind == Kind::Post {
        args.extend([
            typed("sticky", r#"{"type":"boolean"}"#),
            typed("ignore_sticky", r#"{"type":"boolean","default":true}"#),
            typed(
                "format",
                r#"{"type":"array","uniqueItems":true,"items":{"enum":["standard","aside","chat","gallery","link","image","quote","status","video","audio"],"type":"string"}}"#,
            ),
        ]);
    }
    args
}

/// The `id` of a single item's route.
pub(crate) fn id() -> Arg {
    typed("id", r#"{"type":"integer"}"#)
}

/// A single item's arguments for GET.
pub(super) fn item() -> Vec<Arg> {
    vec![
        id(),
        context_arg(),
        typed("excerpt_length", r#"{"type":"integer"}"#),
        string_arg("password"),
    ]
}

/// The type's arguments that limit it to terms, each taxonomy's.
fn limits(kind: Kind) -> &'static [&'static str] {
    match kind {
        Kind::Post => &["categories", "categories_exclude", "tags", "tags_exclude"],
        Kind::Block => &["wp_pattern_category", "wp_pattern_category_exclude"],
        Kind::MenuItem => &["menus", "menus_exclude"],
        Kind::Page | Kind::Navigation | Kind::Attachment => &[],
    }
}

/// The type's arguments whose schemas use keywords not ported.
pub(super) fn not_ported(kind: Kind) -> impl Iterator<Item = &'static str> {
    let dates = ["after", "modified_after", "before", "modified_before"];
    dates.into_iter().chain(limits(kind).iter().copied())
}

/// `sanitize_post_statuses()`: `wp_parse_slug_list()`, each status but
/// the type's default allowed only to those who can edit the type's posts, or
/// read its private ones for `private`.
fn statuses(context: &Context, statuses: &Value, kind: Kind) -> Checked<Value> {
    let parsed = crate::schema::parse_slug_list(statuses);
    let schema = json_decode(statuses_schema(kind).as_bytes()).unwrap_or_default();
    let default = schema.get_str("default").map(Value::to_php_string);
    for status in parsed
        .iter()
        .filter(|status| Some(*status) != default.as_ref())
    {
        let private = status == b"private" && context.can(&kind.cap("read_private_posts"));
        if !context.can(&kind.cap("edit_posts")) && !private {
            let error = context.forbidden("rest_forbidden_status", "Status is forbidden.");
            return Err(Refusal(Some(error)));
        }
        validate_value_from_schema(&text(status.clone()), &schema, "status")?;
    }
    Ok(list(parsed.into_iter().map(text).collect()))
}

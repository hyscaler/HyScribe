// SPDX-License-Identifier: AGPL-3.0-or-later

//! The arguments `WP_REST_Posts_Controller` registers: the collection's
//! (`get_collection_params()`), a single item's, and a write's.

use hyscribe_phpser::{Value, json_decode};

use super::Kind;
use crate::Context;
use crate::request::{Arg, Sanitize, arg, force_arg, slug, string_arg, typed};
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
    let date = |name| arg(name, "{}", false, Sanitize::NotPorted);
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
    for (_, base) in kind.taxonomies() {
        args.push(typed(base, &limit_schema(true)));
        args.push(typed(exclude_name(base), &limit_schema(false)));
    }
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

/// The arguments of a post's or page's create and update,
/// `get_endpoint_args_for_item_schema()`, none for the other types, whose
/// writes are not ported. `title`, `content` and `excerpt` are neither
/// validated nor sanitized, and `meta` is not ported; `status` is validated by
/// `check_status()` (`write::status_args`), `template` by
/// `check_template()`, which the empty template alone passes unported.
pub(super) fn save(kind: Kind) -> Vec<Arg> {
    const INTEGER: &str = r#"{"type":"integer"}"#;
    const DATE: &str = r#"{"type":["string","null"],"format":"date-time"}"#;
    const OPEN: &str = r#"{"type":"string","enum":["open","closed"]}"#;
    const RAW: &str = r#"{"type":"object","properties":{"raw":{"type":"string"}}}"#;
    if !matches!(kind, Kind::Post | Kind::Page) {
        return Vec::new();
    }
    let value = |name, schema| arg(name, schema, true, Sanitize::Value);
    let given = |name| arg(name, RAW, false, Sanitize::Endpoint);
    let mut args = vec![
        value("date", DATE),
        value("date_gmt", DATE),
        arg("slug", r#"{"type":"string"}"#, true, Sanitize::Site(slug)),
        value(
            "status",
            r#"{"type":"string","enum":["publish","future","draft","pending","private"]}"#,
        ),
        value("password", r#"{"type":"string"}"#),
    ];
    if kind == Kind::Page {
        args.push(value("parent", INTEGER));
    }
    args.extend([
        given("title"),
        given("content"),
        value("author", INTEGER),
        given("excerpt"),
        value("featured_media", INTEGER),
        value("comment_status", OPEN),
        value("ping_status", OPEN),
    ]);
    args.push(match kind {
        Kind::Page => value("menu_order", INTEGER),
        _ => value(
            "format",
            r#"{"type":"string","enum":["standard","aside","chat","gallery","link","image","quote","status","video","audio"]}"#,
        ),
    });
    args.push(arg("meta", RAW, false, Sanitize::NotPorted));
    if kind == Kind::Post {
        args.push(value("sticky", r#"{"type":"boolean"}"#));
    }
    args.push(arg(
        "template",
        r#"{"type":"string"}"#,
        false,
        Sanitize::Value,
    ));
    if kind == Kind::Post {
        let ids = r#"{"type":"array","items":{"type":"integer"}}"#;
        args.extend([value("categories", ids), value("tags", ids)]);
    }
    args
}

/// A post's or page's delete: `force`, false unless given.
pub(super) fn delete() -> Vec<Arg> {
    vec![id(), force_arg()]
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

/// The argument that leaves out the terms of the taxonomy `base`.
pub(super) fn exclude_name(base: &str) -> &'static str {
    match base {
        "categories" => "categories_exclude",
        "tags" => "tags_exclude",
        "wp_pattern_category" => "wp_pattern_category_exclude",
        _ => "menus_exclude",
    }
}

/// `prepare_taxonomy_limit_schema()`'s schema of a taxonomy's terms, with
/// `operator` for those to `include`.
fn limit_schema(include: bool) -> String {
    let operator = if include {
        r#","operator":{"type":"string","enum":["AND","OR"],"default":"OR"}"#
    } else {
        ""
    };
    format!(
        r#"{{"type":["object","array"],"oneOf":[{{"title":"Term ID List","type":"array","items":{{"type":"integer"}}}},{{"title":"Term ID Taxonomy Query","type":"object","properties":{{"terms":{{"type":"array","items":{{"type":"integer"}},"default":[]}},"include_children":{{"type":"boolean","default":false}}{operator}}},"additionalProperties":false}}]}}"#
    )
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

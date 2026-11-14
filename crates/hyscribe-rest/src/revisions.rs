// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_Revisions_Controller` and `WP_REST_Autosaves_Controller` of
//! posts and pages, read only: their revisions, autosaves and single
//! ones. Writes answer 501 but their permission checks give `Allow`.

use hyscribe_model::{OrderBy, Post, PostQuery, StatusFilter};
use hyscribe_phpser::{Key, Value, add_query_arg};
use hyscribe_render::order_by;
use hyscribe_render::permalinks::permalink;
use hyscribe_render::post::admin_title;
use hyscribe_render::rest::Rendering;

use crate::fields::{Properties, fields_for_response, filter_by_context, is_field_included};
use crate::posts::prepare::{PARTS, rfc3339, writes_or_unported};
use crate::posts::{self, Kind, ids, render_request};
use crate::request::{Arg, Params, Sanitize, arg, typed};
use crate::response::{Response, RestError, int, object, paginate, text};
use crate::types::context_arg;
use crate::{Context, Endpoint};

const REVISION: &Properties = &[
    ("author", &["view", "edit", "embed"]),
    ("date", &["view", "edit", "embed"]),
    ("date_gmt", &["view", "edit"]),
    ("guid", &["view", "edit"]),
    ("id", &["view", "edit", "embed"]),
    ("modified", &["view", "edit"]),
    ("modified_gmt", &["view", "edit"]),
    ("parent", &["view", "edit", "embed"]),
    ("slug", &["view", "edit", "embed"]),
    ("title", &["view", "edit", "embed"]),
    ("content", &["view", "edit"]),
    ("excerpt", &["view", "edit", "embed"]),
    ("meta", &["view", "edit"]),
    ("preview_link", &["edit"]),
];

/// The endpoints of the route `regex`, if one of these.
pub(crate) fn endpoints(regex: &str) -> Option<Vec<Endpoint>> {
    let route = regex
        .strip_prefix("/wp/v2/posts/")
        .or_else(|| regex.strip_prefix("/wp/v2/pages/"))?;
    let parent = || typed("parent", r#"{"type":"integer"}"#);
    let id = || typed("id", r#"{"type":"integer"}"#);
    let get = |args, permission, callback| Endpoint {
        args,
        permission,
        callback: Some(callback),
    };
    Some(match route {
        "(?P<parent>[\\d]+)/revisions" => vec![get(collection_args(), items_permission, revisions)],
        "(?P<parent>[\\d]+)/revisions/(?P<id>[\\d]+)" => vec![
            get(
                vec![parent(), id(), context_arg()],
                items_permission,
                revision,
            ),
            Endpoint {
                args: vec![parent(), id()],
                permission: delete_permission,
                callback: None,
            },
        ],
        "(?P<id>[\\d]+)/autosaves" => vec![
            get(
                vec![parent(), context_arg()],
                autosaves_permission,
                autosaves,
            ),
            Endpoint {
                args: vec![parent()],
                permission: create_permission,
                callback: None,
            },
        ],
        "(?P<parent>[\\d]+)/autosaves/(?P<id>[\\d]+)" => vec![get(
            vec![parent(), id(), context_arg()],
            items_permission,
            autosave,
        )],
        _ => return None,
    })
}

/// `get_collection_params()` of the revisions.
fn collection_args() -> Vec<Arg> {
    let ids = r#"{"type":"array","items":{"type":"integer"},"default":[]}"#;
    vec![
        typed("parent", r#"{"type":"integer"}"#),
        context_arg(),
        arg(
            "page",
            r#"{"type":"integer","default":1,"minimum":1}"#,
            true,
            Sanitize::Absint,
        ),
        arg(
            "per_page",
            r#"{"type":"integer","minimum":1,"maximum":100}"#,
            true,
            Sanitize::Absint,
        ),
        arg("search", r#"{"type":"string"}"#, true, Sanitize::TextField),
        typed("exclude", ids),
        typed("include", ids),
        typed("offset", r#"{"type":"integer"}"#),
        typed(
            "order",
            r#"{"type":"string","default":"desc","enum":["asc","desc"]}"#,
        ),
        typed(
            "orderby",
            r#"{"type":"string","default":"date","enum":["date","id","include","relevance","slug","include_slugs","title"]}"#,
        ),
    ]
}

/// `get_parent()`: the post of the route's type with the ID `name` gives.
fn get_parent(context: &Context, params: &Params, name: &str) -> Result<Post, RestError> {
    let id = params.get(name).map_or(0, Value::to_int);
    let found = context.site.post(id.unsigned_abs());
    found
        .filter(|post| post.post_type == Kind::of(params.route).name())
        .ok_or_else(|| {
            RestError::status("rest_post_invalid_parent", "Invalid post parent ID.", 404)
        })
}

/// The parent the user may edit, or why not.
fn editable_parent(
    context: &Context,
    params: &Params,
    name: &str,
    message: &str,
) -> Result<Post, RestError> {
    let parent = get_parent(context, params, name)?;
    if context.can_for("edit_post", parent.id) {
        return Ok(parent);
    }
    Err(context.forbidden("rest_cannot_read", message))
}

/// `get_items_permissions_check()` of the revisions, which a single
/// revision and autosave check too.
fn items_permission(context: &Context, params: &Params) -> Result<(), RestError> {
    let message = "Sorry, you are not allowed to view revisions of this post.";
    editable_parent(context, params, "parent", message).map(|_| ())
}

/// `get_items_permissions_check()` of the autosaves.
fn autosaves_permission(context: &Context, params: &Params) -> Result<(), RestError> {
    let message = "Sorry, you are not allowed to view autosaves of this post.";
    editable_parent(context, params, "id", message).map(|_| ())
}

/// `create_item_permissions_check()` of the autosaves: the parent's
/// update check.
fn create_permission(context: &Context, params: &Params) -> Result<(), RestError> {
    if !params.get("id").is_some_and(|id| !id.is_empty()) {
        return Err(RestError::status(
            "rest_post_invalid_id",
            "Invalid item ID.",
            404,
        ));
    }
    posts::update_permission(context, params, Kind::of(params.route))
}

/// `get_revision()`.
fn get_revision(context: &Context, params: &Params) -> Result<Post, RestError> {
    let id = params.get("id").map_or(0, Value::to_int);
    let found = context.site.post(id.unsigned_abs());
    found
        .filter(|post| post.post_type == "revision")
        .ok_or_else(|| RestError::status("rest_post_invalid_id", "Invalid revision ID.", 404))
}

/// `delete_item_permissions_check()`, which never passes: `map_meta_cap()`
/// lets no one delete a revision.
fn delete_permission(context: &Context, _: &Params) -> Result<(), RestError> {
    let message = "Sorry, you are not allowed to delete this revision.";
    Err(context.forbidden("rest_cannot_delete", message))
}

/// The revisions of `parent` as `get_items()` queries them, every one
/// without `per_page`, and the page asked for.
fn query(params: &Params, parent: &Post) -> PostQuery {
    let orderby = params.text("orderby");
    let per_page = params.get("per_page").map(Value::to_int);
    let page = params.get("page").map_or(1, Value::to_int).unsigned_abs();
    let offset = params.get("offset").map_or(0, Value::to_int).unsigned_abs();
    let include = ids(params, "include");
    let mut query = PostQuery {
        post_types: vec!["revision".into()],
        statuses: vec![StatusFilter {
            status: "inherit".into(),
            author: None,
        }],
        parents: vec![parent.id],
        include: (!include.is_empty()).then_some(include),
        exclude: ids(params, "exclude"),
        limit: u32::MAX,
        ..PostQuery::default()
    };
    // `posts_per_page` of -1 takes every revision, whatever the offset.
    if let Some(per_page) = per_page {
        let per_page = per_page.unsigned_abs();
        let skip = if offset > 0 {
            offset
        } else {
            page.saturating_sub(1).saturating_mul(per_page)
        };
        query.limit = u32::try_from(per_page).unwrap_or(u32::MAX);
        query.offset = u32::try_from(skip).unwrap_or(u32::MAX);
    }
    let by = match orderby.as_slice() {
        b"date" => &b"date ID"[..],
        b"id" => b"ID",
        b"include" => b"post__in",
        b"slug" => b"post_name",
        b"include_slugs" => b"post_name__in",
        by => by,
    };
    let ascending = params.text("order") == b"asc";
    query.order = order_by(&Value::String(by.to_vec()), ascending, &query);
    if let Some(search) = params.get("search").map(Value::to_php_string) {
        posts::searched(&mut query, &search);
        if orderby == b"relevance" {
            query.order.insert(0, (OrderBy::Relevance, false));
        }
    }
    query
}

/// The request's ordering errors, as `get_items()` checks them.
fn order_errors(params: &Params) -> Result<(), RestError> {
    let orderby = params.text("orderby");
    if orderby == b"relevance" && !params.get("search").is_some_and(|s| !s.is_empty()) {
        let message = "You need to define a search term to order by relevance.";
        return Err(RestError::status(
            "rest_no_search_term_defined",
            message,
            400,
        ));
    }
    if orderby == b"include" && ids(params, "include").is_empty() {
        let message = "You need to define an include parameter to order by include.";
        return Err(RestError::status(
            "rest_orderby_include_missing_include",
            message,
            400,
        ));
    }
    Ok(())
}

/// `get_items()` of the revisions.
fn revisions(context: &Context, params: &Params) -> Result<Response, RestError> {
    // The permission check found the parent.
    let parent = get_parent(context, params, "parent").unwrap_or_default();
    order_errors(params)?;
    let query = query(params, &parent);
    let reads = context.site.reads();
    let found = reads.query(&query);
    let total = reads.count_posts(&query);
    let page = params.get("page").map_or(1, Value::to_int).unsigned_abs();
    let offset = params.get("offset").map_or(0, Value::to_int).unsigned_abs();
    let pages = match params.get("per_page") {
        Some(per_page) => total.div_ceil(per_page.to_int().unsigned_abs()),
        None => u64::from(total > 0),
    };
    if total > 0 && offset >= total {
        let message = "The offset number requested is larger than or equal to the number of available revisions.";
        return Err(RestError::status(
            "rest_revision_invalid_offset_number",
            message,
            400,
        ));
    }
    if total > 0 && offset == 0 && page > pages {
        let message = "The page number requested is larger than the number of pages available.";
        return Err(RestError::status(
            "rest_revision_invalid_page_number",
            message,
            400,
        ));
    }
    let mut response = list(context, params, &found, None)?;
    let base = format!(
        "/wp/v2/{}/{}/revisions",
        Kind::of(params.route).base(),
        parent.id
    );
    paginate(
        &mut response,
        (total, pages, page),
        &params.get,
        &context.rest_url(&base),
    );
    Ok(response)
}

/// A list of revisions prepared, empty for a HEAD request, with the
/// preview links of an autosave's `parent`.
fn list(
    context: &Context,
    params: &Params,
    found: &[Post],
    parent: Option<&Post>,
) -> Result<Response, RestError> {
    let request = render_request(context);
    let mut rendering = Rendering::new(context.site.theme(), context.site.reads(), &request);
    let items = found
        .iter()
        .filter(|_| !context.head)
        .map(|revision| Ok(prepare(context, params, revision, parent, &mut rendering)?.into_data()))
        .collect::<Result<Vec<Value>, RestError>>()?;
    Ok(Response::new(crate::response::list(items), 200))
}

/// `get_item()` of a revision.
fn revision(context: &Context, params: &Params) -> Result<Response, RestError> {
    let parent = get_parent(context, params, "parent").unwrap_or_default();
    let revision = get_revision(context, params)?;
    if revision.parent != parent.id {
        let message = format!(
            "The revision does not belong to the specified parent with id of \"{}\"",
            parent.id
        );
        return Err(RestError::status(
            "rest_revision_parent_id_mismatch",
            &message,
            404,
        ));
    }
    single(context, params, &revision, None)
}

/// One revision prepared.
fn single(
    context: &Context,
    params: &Params,
    revision: &Post,
    parent: Option<&Post>,
) -> Result<Response, RestError> {
    let request = render_request(context);
    let mut rendering = Rendering::new(context.site.theme(), context.site.reads(), &request);
    prepare(context, params, revision, parent, &mut rendering)
}

/// `wp_get_post_revisions()` of `parent`, newest first.
fn all_revisions(context: &Context, parent: &Post) -> Vec<Post> {
    let query = PostQuery {
        post_types: vec!["revision".into()],
        statuses: vec![StatusFilter {
            status: "inherit".into(),
            author: None,
        }],
        parents: vec![parent.id],
        order: vec![(OrderBy::Date, false), (OrderBy::Id, false)],
        limit: u32::MAX,
        ..PostQuery::default()
    };
    context.site.reads().query(&query)
}

/// Whether `revision` is an autosave of `parent`, as its name tells.
fn is_autosave(revision: &Post, parent: u64) -> bool {
    let name = format!("{parent}-autosave");
    revision
        .name
        .windows(name.len())
        .any(|window| window == name.as_bytes())
}

/// `get_items()` of the autosaves: every one, by any author.
fn autosaves(context: &Context, params: &Params) -> Result<Response, RestError> {
    let parent = get_parent(context, params, "id").unwrap_or_default();
    let found: Vec<Post> = all_revisions(context, &parent)
        .into_iter()
        .filter(|revision| is_autosave(revision, parent.id))
        .collect();
    list(context, params, &found, Some(&parent))
}

/// `get_item()` of an autosave: the parent's newest, whatever the ID.
fn autosave(context: &Context, params: &Params) -> Result<Response, RestError> {
    let parent = get_parent(context, params, "parent").unwrap_or_default();
    let name = format!("{}-autosave-v1", parent.id);
    let mut found = all_revisions(context, &parent)
        .into_iter()
        .filter(|revision| revision.name == name.as_bytes());
    let Some(autosave) = found.next() else {
        return Err(RestError::status(
            "rest_post_no_autosave",
            "There is no autosave revision for this post.",
            404,
        ));
    };
    single(context, params, &autosave, Some(&parent))
}

/// `prepare_date_response()`: none for an unset GMT date.
fn date(gmt: &str, local: &str) -> Value {
    if gmt == "0000-00-00 00:00:00" {
        Value::Null
    } else {
        rfc3339(local)
    }
}

/// `prepare_item_for_response()` of a revision, with an autosave's
/// `preview_link` when `parent` is given.
fn prepare(
    context: &Context,
    params: &Params,
    revision: &Post,
    parent: Option<&Post>,
    rendering: &mut Rendering,
) -> Result<Response, RestError> {
    if context.head {
        return Ok(Response::new(Value::Array(Vec::new()), 200));
    }
    let fields = fields_for_response(params, REVISION);
    let has = |field: &str| is_field_included(field, &fields);
    let renders = has("content.rendered");
    // Only those who may edit a post see its revisions.
    if renders && writes_or_unported(&revision.content, true) {
        return Err(RestError::not_ported());
    }
    let mut data: Vec<(Key, Value)> = Vec::new();
    let mut put = |name: &str, value: Value| data.push((Key::String(name.into()), value));
    // A part of an object, `raw` or `rendered`, when `_fields` wants it.
    // `_fields` and the context keep `raw` where wanted.
    let parts = |raw: &[u8], rendered: Option<Vec<u8>>| {
        let mut kept = vec![(Key::String(b"raw".to_vec()), text(raw.to_vec()))];
        let rendered = rendered.map(|rendered| (Key::String(b"rendered".to_vec()), text(rendered)));
        kept.extend(rendered);
        Value::Array(kept)
    };
    if has("author") {
        put("author", int(revision.author));
    }
    if has("date") {
        put("date", date(&revision.date_gmt, &revision.date));
    }
    if has("date_gmt") {
        put("date_gmt", date(&revision.date_gmt, &revision.date_gmt));
    }
    // `_fields` keeps `id` where wanted.
    put("id", int(revision.id));
    if has("modified") {
        put("modified", date(&revision.modified_gmt, &revision.modified));
    }
    if has("modified_gmt") {
        put(
            "modified_gmt",
            date(&revision.modified_gmt, &revision.modified_gmt),
        );
    }
    if has("parent") {
        put("parent", int(revision.parent));
    }
    if has("slug") {
        put("slug", text(revision.name.clone()));
    }
    if has("guid") {
        // `rendered` first, then `raw`, both the stored GUID.
        let guid = ["rendered", "raw"]
            .into_iter()
            .filter(|part| has(&format!("guid.{part}")))
            .map(|part| (Key::String(part.into()), text(revision.guid.clone())));
        put("guid", Value::Array(guid.collect()));
    }
    if has("title") {
        let rendered = has("title.rendered").then(|| admin_title(revision));
        put("title", parts(&revision.title, rendered));
    }
    if has("content") {
        let rendered = renders.then(|| rendering.the_content(revision));
        put("content", parts(&revision.content, rendered));
    }
    if has("excerpt") {
        // `the_excerpt` of the excerpt alone, empty without one.
        let rendered = has("excerpt.rendered").then(|| {
            if revision.excerpt.is_empty() {
                Vec::new()
            } else {
                rendering.the_excerpt(revision, 55)
            }
        });
        put("excerpt", parts(&revision.excerpt, rendered));
    }
    if has("meta") {
        let footnotes = context.site.post_meta(revision.id, "footnotes");
        put(
            "meta",
            object([("footnotes", text(footnotes.unwrap_or_default()))]),
        );
    }
    if let Some(parent) = parent.filter(|_| has("preview_link")) {
        put("preview_link", text(preview_link(context, parent)));
    }
    let shown = params.context();
    let data: Vec<(Key, Value)> = filter_by_context(data, REVISION, &shown)
        .into_iter()
        .map(|(name, value)| match value {
            Value::Array(parts) => (name, Value::Array(filter_by_context(parts, PARTS, &shown))),
            value => (name, value),
        })
        .collect();
    let mut response = Response::new(Value::Array(data), 200);
    if has("parent") {
        let route = format!(
            "/wp/v2/{}/{}",
            Kind::of(params.route).base(),
            revision.parent
        );
        response.add_link("parent", context.rest_url(&route));
    }
    Ok(response)
}

/// `get_preview_post_link()` of an autosave's parent.
fn preview_link(context: &Context, parent: &Post) -> Vec<u8> {
    let id = parent.id.to_string();
    let nonce = context.site.nonce(&format!("post_preview_{id}"));
    let link = permalink(context.site.reads(), parent);
    add_query_arg(
        &[
            (b"preview_id", Some(id.as_bytes())),
            (b"preview_nonce", Some(nonce.as_bytes())),
            (b"preview", Some(b"true")),
        ],
        &link,
    )
}

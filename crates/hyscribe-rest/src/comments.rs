// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_Comments_Controller`: `/wp/v2/comments` and its items, read
//! only.

use hyscribe_model::{Comment, CommentOrder, CommentQuery, CommentRow, Post};
use hyscribe_phpser::{Key, Value, add_query_arg};
use hyscribe_render::date::{Zone, format_unix, site_zone, stored_unix};
use hyscribe_render::{avatar_url, front_comment_text, user_name};

use crate::fields::{Properties, fields_for_response, filter_by_context, is_field_included};
use crate::request::{Arg, Params, Sanitize, arg, collection_args};
use crate::response::{Response, RestError, list, object, paginate, text};
use crate::schema::{
    parse_date, parse_list, sanitize_value_from_schema, validate_value_from_schema,
};
use crate::terms::ids;
use crate::types::{context_arg, items_route};
use crate::users::{avatar_urls, rfc3339};
use crate::{Context, Endpoint};

const ALL: &[&str] = &["view", "edit", "embed"];

/// The schema's properties and their contexts; `author_avatar_urls` only
/// while the site shows avatars.
const PROPERTIES: &Properties = &[
    ("id", ALL),
    ("author", ALL),
    ("author_email", &["edit"]),
    ("author_ip", &["edit"]),
    ("author_name", ALL),
    ("author_url", ALL),
    ("author_user_agent", &["edit"]),
    ("content", ALL),
    ("date", ALL),
    ("date_gmt", &["view", "edit"]),
    ("link", ALL),
    ("parent", ALL),
    ("post", &["view", "edit"]),
    ("status", &["view", "edit"]),
    ("type", ALL),
    ("author_avatar_urls", ALL),
    ("meta", &["view", "edit"]),
];

const ROUTE: &str = "/wp/v2/comments";

fn properties(context: &Context) -> Vec<(&'static str, &'static [&'static str])> {
    let avatars = shows_avatars(context);
    PROPERTIES
        .iter()
        .filter(|(name, _)| *name != "author_avatar_urls" || avatars)
        .copied()
        .collect()
}

/// `get_option( 'show_avatars' )`.
pub(crate) fn shows_avatars(context: &Context) -> bool {
    !Value::String(context.option("show_avatars").unwrap_or_default()).is_empty()
}

/// The collection's arguments, `get_collection_params()`.
fn items_args() -> Vec<Arg> {
    let ints = r#"{"type":"array","items":{"type":"integer"}}"#;
    let ints_or_none = r#"{"type":"array","items":{"type":"integer"},"default":[]}"#;
    let date = r#"{"type":"string","format":"date-time"}"#;
    let mut args = collection_args();
    args.extend([
        arg("after", date, false, Sanitize::Schema),
        arg("author", ints, false, Sanitize::Schema),
        arg("author_exclude", ints, false, Sanitize::Schema),
        arg(
            "author_email",
            r#"{"default":null,"format":"email","type":"string"}"#,
            false,
            Sanitize::Schema,
        ),
        arg("before", date, false, Sanitize::Schema),
        arg("exclude", ints_or_none, false, Sanitize::Schema),
        arg("include", ints_or_none, false, Sanitize::Schema),
        arg("offset", r#"{"type":"integer"}"#, false, Sanitize::Schema),
        arg(
            "order",
            r#"{"type":"string","default":"desc","enum":["asc","desc"]}"#,
            false,
            Sanitize::Schema,
        ),
        arg(
            "orderby",
            r#"{"type":"string","default":"date_gmt","enum":["date","date_gmt","id","include","post","parent","type"]}"#,
            false,
            Sanitize::Schema,
        ),
        arg("parent", ints_or_none, false, Sanitize::Schema),
        arg("parent_exclude", ints_or_none, false, Sanitize::Schema),
        arg("post", ints_or_none, false, Sanitize::Schema),
        arg(
            "status",
            r#"{"default":"approve","type":"string"}"#,
            true,
            Sanitize::Key,
        ),
        arg(
            "type",
            r#"{"default":"comment","type":"string"}"#,
            true,
            Sanitize::Key,
        ),
        arg("password", r#"{"type":"string"}"#, false, Sanitize::Schema),
    ]);
    args
}

/// The endpoints of the route `regex`, if ported.
pub(crate) fn endpoints(regex: &str) -> Option<Vec<Endpoint>> {
    let id = || arg("id", r#"{"type":"integer"}"#, false, Sanitize::Schema);
    match regex {
        ROUTE => Some(vec![
            Endpoint {
                args: items_args(),
                permission: items_permission,
                callback: Some(items),
            },
            Endpoint {
                args: Vec::new(),
                permission: create_permission,
                callback: None,
            },
        ]),
        "/wp/v2/comments/(?P<id>[\\d]+)" => {
            let password = arg("password", r#"{"type":"string"}"#, false, Sanitize::Schema);
            let write = |permission| Endpoint {
                args: vec![id()],
                permission,
                callback: None,
            };
            Some(vec![
                Endpoint {
                    args: vec![id(), context_arg(), password],
                    permission: item_permission,
                    callback: Some(item),
                },
                write(edit_permission),
                write(delete_permission),
            ])
        }
        _ => None,
    }
}

fn error(code: &'static str, message: &str, context: &Context) -> RestError {
    RestError::status(code, message, context.authorization_required())
}

/// A parameter as text, empty when not set.
fn param(params: &Params, name: &str) -> Vec<u8> {
    params
        .get(name)
        .map(Value::to_php_string)
        .unwrap_or_default()
}

fn post_ids(params: &Params) -> Vec<Value> {
    parse_list(&params.get("post").cloned().unwrap_or_default())
        .into_iter()
        .map(|(_, id)| id)
        .collect()
}

/// `get_items_permissions_check()`.
fn items_permission(context: &Context, params: &Params) -> Result<(), RestError> {
    let note = param(params, "type") == b"note";
    let edit = params.context() == b"edit";
    let posts = params.get("post").is_some_and(|post| !post.is_empty());
    let protected = ["author", "author_exclude", "author_email", "type", "status"];
    let forbidden = || -> Vec<&str> {
        protected
            .iter()
            .copied()
            .filter(|name| match *name {
                "status" => param(params, name) != b"approve",
                "type" => param(params, name) != b"comment",
                name => params.get(name).is_some_and(|value| !value.is_empty()),
            })
            .collect()
    };
    let not_permitted = |names: Vec<&str>| {
        let message = format!("Query parameter not permitted: {}", names.join(", "));
        error("rest_forbidden_param", &message, context)
    };
    if posts {
        for id in post_ids(params) {
            let post = context
                .site
                .post(u64::try_from(id.to_int()).unwrap_or_default());
            match &post {
                Some(post) if id.to_int() != 0 && !can_read_post(context, params, post) => {
                    let message = "Sorry, you are not allowed to read the post for this comment.";
                    return Err(error("rest_cannot_read_post", message, context));
                }
                _ if id == Value::Int(0) && !context.can("moderate_comments") => {
                    let message = "Sorry, you are not allowed to read comments without a post.";
                    return Err(error("rest_cannot_read", message, context));
                }
                _ => {}
            }
            if let Some(post) = post.filter(|post| note && !supports_notes(&post.post_type)) {
                if context.can_for("edit_post", post.id) {
                    let message = "Sorry, this post type does not support notes.";
                    return Err(RestError::status(
                        "rest_comment_not_supported_post_type",
                        message,
                        403,
                    ));
                }
                return Err(not_permitted(forbidden()));
            }
        }
    }
    if edit && note && posts {
        for id in post_ids(params) {
            if !context.can_for("edit_post", id.to_int().unsigned_abs()) {
                let message = "Sorry, you are not allowed to edit comments.";
                return Err(error("rest_forbidden_context", message, context));
            }
        }
    } else if edit && !context.can("moderate_comments") {
        let message = "Sorry, you are not allowed to edit comments.";
        return Err(error("rest_forbidden_context", message, context));
    }
    let names = forbidden();
    if !context.can("edit_posts") && !names.is_empty() {
        return Err(not_permitted(names));
    }
    Ok(())
}

/// `check_post_type_supports_notes()`: the post type's `editor` support
/// with notes, which no post type shown in REST has on its own.
fn supports_notes(post_type: &str) -> bool {
    crate::types::supports(post_type, "editor")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items
                .iter()
                .any(|(_, item)| item.get_str("notes").is_some_and(|notes| !notes.is_empty()))
        })
}

/// `check_read_post_permission()`.
fn can_read_post(context: &Context, params: &Params, post: &Post) -> bool {
    if !crate::types::registered(&post.post_type) {
        return false;
    }
    let posts = post_ids(params);
    let requested = (params.get("post").is_some_and(|post| !post.is_empty())
        && (!matches!(params.get("post"), Some(Value::Array(_))) || posts.len() == 1))
        || params.get("id").is_some_and(|id| !id.is_empty());
    let unlocked = requested && can_access_password(context, params, post);
    if !post.password.is_empty() && !unlocked {
        return context.can_for("edit_post", post.id);
    }
    // The post type's controller: the patterns' and global styles' own.
    match post.post_type.as_str() {
        "wp_global_styles" => context.can_for("read_post", post.id),
        "wp_block" => context.can_for("read_post", post.id) && readable(context, post),
        _ => readable(context, post),
    }
}

/// `WP_REST_Posts_Controller::can_access_password_content()`.
fn can_access_password(context: &Context, params: &Params, post: &Post) -> bool {
    if post.password.is_empty() {
        return false;
    }
    if params.context() == b"edit" && context.can_for("edit_post", post.id) {
        return true;
    }
    let password = param(params, "password");
    !Value::String(password.clone()).is_empty() && password == post.password
}

/// `WP_REST_Posts_Controller::check_read_permission()`.
fn readable(context: &Context, post: &Post) -> bool {
    if items_route(&post.post_type).is_none() {
        return false;
    }
    // `publish` is the only public status.
    if post.status == "publish" || context.can_for("read_post", post.id) {
        return true;
    }
    // No post has the ID 0, a parent of none.
    let parent = (post.status == "inherit")
        .then(|| context.site.post(post.parent))
        .flatten();
    match parent {
        Some(parent) => readable(context, &parent),
        None => post.status == "inherit",
    }
}

/// `get_comment()`: the comment of the route's ID, and its post there.
fn get_comment(context: &Context, params: &Params) -> Result<CommentRow, RestError> {
    let invalid = || RestError::status("rest_comment_invalid_id", "Invalid comment ID.", 404);
    let id = params.get("id").map_or(0, Value::to_int);
    if id <= 0 {
        return Err(invalid());
    }
    let query = CommentQuery {
        include: vec![id.unsigned_abs()],
        ..CommentQuery::default()
    };
    let row = context
        .site
        .comments(&query)
        .into_iter()
        .next()
        .ok_or_else(invalid)?;
    let post = row.comment.post_id;
    if post != 0 && context.site.post(post).is_none() {
        return Err(RestError::status(
            "rest_post_invalid_id",
            "Invalid post ID.",
            404,
        ));
    }
    Ok(row)
}

/// `check_read_permission()`.
fn can_read(context: &Context, params: &Params, row: &CommentRow) -> bool {
    let comment = &row.comment;
    let post = (comment.comment_type != b"note" && comment.post_id != 0)
        .then(|| context.site.post(comment.post_id))
        .flatten();
    if post.is_some_and(|post| can_read_post(context, params, &post) && row.approved == "1") {
        return true;
    }
    if context.user == 0 {
        return false;
    }
    if comment.post_id == 0 && !context.can("moderate_comments") {
        return false;
    }
    if comment.user_id != 0 && comment.user_id == context.user {
        return true;
    }
    context.can_for("edit_comment", comment.id)
}

/// `get_item_permissions_check()`.
fn item_permission(context: &Context, params: &Params) -> Result<(), RestError> {
    let row = get_comment(context, params)?;
    let comment = &row.comment;
    let may_edit = if comment.comment_type == b"note" {
        context.can_for("edit_comment", comment.id)
    } else {
        context.can("moderate_comments")
    };
    if params.context() == b"edit" && !may_edit {
        let message = "Sorry, you are not allowed to edit comments.";
        return Err(error("rest_forbidden_context", message, context));
    }
    if !can_read(context, params, &row) {
        let message = "Sorry, you are not allowed to read this comment.";
        return Err(error("rest_cannot_read", message, context));
    }
    let post = context.site.post(comment.post_id);
    if post.is_some_and(|post| !can_read_post(context, params, &post)) {
        let message = "Sorry, you are not allowed to read the post for this comment.";
        return Err(error("rest_cannot_read_post", message, context));
    }
    Ok(())
}

/// `get_item()`.
fn item(context: &Context, params: &Params) -> Result<Response, RestError> {
    get_comment(context, params).map(|row| prepare(context, params, &row))
}

/// `check_edit_permission()`.
fn can_edit(context: &Context, comment: &Comment) -> bool {
    context.user != 0
        && (context.can("moderate_comments") || context.can_for("edit_comment", comment.id))
}

/// `update_item_permissions_check()`.
fn edit_permission(context: &Context, params: &Params) -> Result<(), RestError> {
    let row = get_comment(context, params)?;
    let comment = &row.comment;
    if !can_edit(context, comment) {
        let message = "Sorry, you are not allowed to edit this comment.";
        return Err(error("rest_cannot_edit", message, context));
    }
    match params.get("post").map(Value::to_int) {
        Some(post) if post != i64::try_from(comment.post_id).unwrap_or(i64::MAX) => {
            target_permission(context, post, comment.comment_type == b"note")
        }
        _ => Ok(()),
    }
}

/// `delete_item_permissions_check()`.
fn delete_permission(context: &Context, params: &Params) -> Result<(), RestError> {
    let row = get_comment(context, params)?;
    if can_edit(context, &row.comment) {
        return Ok(());
    }
    let message = "Sorry, you are not allowed to delete this comment.";
    Err(error("rest_cannot_delete", message, context))
}

fn no_post() -> RestError {
    let message = "Sorry, you are not allowed to create this comment without a post.";
    RestError::status("rest_comment_invalid_post_id", message, 403)
}

/// `check_target_post_permission()`.
fn target_permission(context: &Context, id: i64, note: bool) -> Result<(), RestError> {
    if id == 0 {
        return Err(no_post());
    }
    let id = u64::try_from(id).unwrap_or_default();
    let may = context.can_for("edit_post", id) || (!note && context.can("moderate_comments"));
    if !may && note {
        let message = "Sorry, you are not allowed to create notes for this post.";
        return Err(error("rest_cannot_create_note", message, context));
    }
    if !may {
        let message = "Sorry, you are not allowed to edit this comment.";
        return Err(error("rest_cannot_edit", message, context));
    }
    let post = context.site.post(id).ok_or_else(no_post)?;
    if !note {
        return Ok(());
    }
    if !supports_notes(&post.post_type) {
        let message = "Sorry, this post type does not support notes.";
        return Err(RestError::status(
            "rest_comment_not_supported_post_type",
            message,
            403,
        ));
    }
    if post.status == "trash" {
        let message = "Sorry, you are not allowed to create a comment on this post.";
        return Err(RestError::status("rest_comment_trash_post", message, 403));
    }
    // PHP checks the post can be read, which `edit_post` on a type with
    // notes grants.
    Ok(())
}

/// `create_item_permissions_check()`, for the `Allow` header.
fn create_permission(context: &Context, params: &Params) -> Result<(), RestError> {
    let note = param(params, "type") == b"note";
    let login = || {
        let message = "Sorry, you must be logged in to comment.";
        RestError::status("rest_comment_login_required", message, 401)
    };
    // Anonymous comments over REST are off without a filter.
    if context.user == 0 {
        return Err(login());
    }
    // The collection's `author` is a list, never the user's ID.
    if params.get("author").is_some() && !context.can("moderate_comments") {
        let message = "Sorry, you are not allowed to edit 'author' for comments.";
        return Err(error("rest_comment_invalid_author", message, context));
    }
    // `$_SERVER['REMOTE_ADDR']` is not read.
    if params.get("author_ip").is_some() && !context.can("moderate_comments") {
        return Err(RestError::not_ported());
    }
    let post = params.get("post").map_or(0, Value::to_int);
    let has_post = params.get("post").is_some_and(|post| !post.is_empty());
    if note && has_post {
        target_permission(context, post, true)?;
    }
    let may_set_status = if note {
        context.can_for("edit_post", post.unsigned_abs())
    } else {
        context.can("moderate_comments")
    };
    if params.get("status").is_some() && !may_set_status {
        let message = "Sorry, you are not allowed to edit 'status' for comments.";
        return Err(error("rest_comment_invalid_status", message, context));
    }
    if !has_post {
        return Err(no_post());
    }
    if note {
        return Ok(());
    }
    let post = context.site.post(post.unsigned_abs()).ok_or_else(no_post)?;
    let unwritable = match post.status.as_str() {
        "draft" => Some("rest_comment_draft_post"),
        "trash" => Some("rest_comment_trash_post"),
        _ => None,
    };
    if let Some(code) = unwritable {
        let message = "Sorry, you are not allowed to create a comment on this post.";
        return Err(RestError::status(code, message, 403));
    }
    if !can_read_post(context, params, &post) {
        let message = "Sorry, you are not allowed to read the post for this comment.";
        return Err(error("rest_cannot_read_post", message, context));
    }
    if post.comment_status != "open" {
        let message = "Sorry, comments are closed for this item.";
        return Err(RestError::status("rest_comment_closed", message, 403));
    }
    // `_close_comments_for_old_post()` reads the clock, which is not ported.
    let set = |name| !Value::String(context.option(name).unwrap_or_default()).is_empty();
    if set("close_comments_for_old_posts")
        && set("close_comments_days_old")
        && post.post_type == "post"
    {
        return Err(RestError::not_ported());
    }
    Ok(())
}

/// A parameter's IDs, `wp_parse_id_list()`, if set.
fn id_list(params: &Params, name: &str) -> Vec<u64> {
    if params.get(name).is_some() {
        ids(params, name)
    } else {
        Vec::new()
    }
}

/// `WP_Date_Query::build_mysql_datetime()` of a date the schema took, in
/// the site's time zone: read in it unless it gives its own.
fn local_time(context: &Context, date: &[u8]) -> String {
    let option = |name| context.option(name).unwrap_or_default();
    let zone = site_zone(&option("timezone_string"), &option("gmt_offset"));
    // The schema took the date, so it parses.
    let unix = parse_date(date).unwrap_or_default();
    let zone_given = |rest: &[u8]| rest.iter().any(|b| matches!(b, b'Z' | b'+' | b'-'));
    let zoned = date.get(19..).is_some_and(zone_given);
    let unix = if zoned {
        unix
    } else {
        stored_unix(&format_unix("Y-m-d H:i:s", unix, Zone::Utc), zone)
    };
    format_unix("Y-m-d H:i:s", unix, zone)
}

/// The query `get_items()` prepares, without its page.
fn prepared(context: &Context, params: &Params) -> CommentQuery {
    let status = param(params, "status");
    let approved = match status.as_slice() {
        b"any" => None,
        b"hold" => Some(vec!["0".into()]),
        b"approve" => Some(vec!["1".into()]),
        b"all" | b"" => Some(vec!["0".into(), "1".into()]),
        other => Some(vec![String::from_utf8_lossy(other).into_owned()]),
    };
    let ty = String::from_utf8_lossy(&param(params, "type")).into_owned();
    let types = match ty.as_str() {
        "" | "all" => Vec::new(),
        "comment" | "comments" => vec![String::new(), "comment".into()],
        "pings" => vec!["pingback".into(), "trackback".into()],
        other => vec![other.into()],
    };
    let excluded_types = if ty == "all" || ty == "note" {
        Vec::new()
    } else {
        vec!["note".into()]
    };
    let order = match param(params, "orderby").as_slice() {
        b"date" => CommentOrder::Date,
        b"id" => CommentOrder::Id,
        b"include" => CommentOrder::Include,
        b"post" => CommentOrder::Post,
        b"parent" => CommentOrder::Parent,
        b"type" => CommentOrder::Type,
        _ => CommentOrder::DateGmt,
    };
    let descending = param(params, "order") != b"asc";
    // A date's ties go by ID in its direction, other columns' by the
    // latest ID first: PHP looks for "ASC" in a clause the wrong way round.
    // An order by ID, or the IDs listed, has no ties to break.
    let tie = match order {
        CommentOrder::Date | CommentOrder::DateGmt => descending,
        _ => true,
    };
    let search = String::from_utf8_lossy(&param(params, "search")).into_owned();
    let email = param(params, "author_email");
    let date = |name| {
        params
            .get(name)
            .map(|date| local_time(context, &date.to_php_string()))
    };
    CommentQuery {
        approved,
        types,
        excluded_types,
        include: id_list(params, "include"),
        exclude: id_list(params, "exclude"),
        parents: id_list(params, "parent"),
        excluded_parents: id_list(params, "parent_exclude"),
        posts: id_list(params, "post"),
        authors: id_list(params, "author"),
        excluded_authors: id_list(params, "author_exclude"),
        author_email: (!email.is_empty()).then_some(email),
        search: (!search.is_empty()).then_some(search),
        after: date("after"),
        before: date("before"),
        // `FIELD()` is written without a direction.
        order: [(order, descending && order != CommentOrder::Include)]
            .into_iter()
            .chain([(CommentOrder::Id, tie)])
            .collect(),
        ..CommentQuery::default()
    }
}

/// `get_items()`.
fn items(context: &Context, params: &Params) -> Result<Response, RestError> {
    let int = |name| params.get(name).map_or(0, Value::to_int);
    let per_page = int("per_page").unsigned_abs().max(1);
    let page = int("page").unsigned_abs();
    let offset = match params.get("offset") {
        Some(offset) if !offset.is_empty() => offset.to_int().unsigned_abs(),
        _ => per_page * page.saturating_sub(1),
    };
    let query = prepared(context, params);
    let total = context.site.count_comments(&query);
    let mut response = if context.head {
        Response::new(Value::Array(Vec::new()), 200)
    } else {
        // `FIELD()` of no IDs is not SQL: PHP's query finds nothing.
        let broken = query.include.is_empty()
            && query
                .order
                .first()
                .is_some_and(|(order, _)| *order == CommentOrder::Include);
        let rows = if broken {
            Vec::new()
        } else {
            context.site.comments(&CommentQuery {
                limit: Some(per_page),
                offset,
                ..query
            })
        };
        let data = rows
            .iter()
            .filter(|row| can_read(context, params, row))
            .map(|row| prepare(context, params, row).into_data())
            .collect();
        Response::new(list(data), 200)
    };
    let pages = total.div_ceil(per_page);
    paginate(
        &mut response,
        (total, pages, page),
        &params.get,
        &context.rest_url(ROUTE),
    );
    Ok(response)
}

/// `rest_get_avatar_urls( $comment )`: `false` for a comment type without
/// avatars.
fn comment_avatars(context: &Context, comment: &Comment) -> Value {
    let avatars = matches!(comment.comment_type.as_slice(), b"" | b"comment" | b"note");
    let user = (comment.user_id != 0)
        .then(|| context.site.user(comment.user_id))
        .flatten();
    let (email, name) = match &user {
        Some(user) => (user.email.clone(), user_name(user)),
        None => (comment.author_email.clone(), comment.author.clone()),
    };
    avatar_urls(|size| {
        if avatars {
            text(avatar_url(
                &|name| context.option(name),
                &email,
                &name,
                size,
            ))
        } else {
            Value::Bool(false)
        }
    })
}

/// `WP_REST_Meta_Fields::get_value()` of the note status: `null` unless a
/// status its schema takes.
fn meta(row: &CommentRow) -> Value {
    // Not stored, it is the schema's default, `''`, as an empty value reads.
    let value = crate::unserialized(&row.note_status);
    let schema = object([
        ("type", text("string")),
        ("enum", list(vec![text("resolved"), text("reopen")])),
    ]);
    let status = validate_value_from_schema(&value, &schema, "")
        .and_then(|()| sanitize_value_from_schema(&value, &schema, ""))
        .unwrap_or(Value::Null);
    object([("_wp_note_status", status)])
}

/// `prepare_status_response()`.
fn status(approved: &str) -> &str {
    match approved {
        "hold" | "0" => "hold",
        "approve" | "1" => "approved",
        other => other,
    }
}

/// `prepare_item_for_response()`.
fn prepare(context: &Context, params: &Params, row: &CommentRow) -> Response {
    if context.head {
        return Response::new(Value::Array(Vec::new()), 200);
    }
    let comment = &row.comment;
    let properties = properties(context);
    let wanted = fields_for_response(params, &properties);
    let has = |name: &str| wanted.iter().any(|field| field == name.as_bytes());
    let int = |n: u64| Value::Int(i64::try_from(n).unwrap_or(i64::MAX));
    let edit = params.context() == b"edit";
    let content = || {
        let home = context.site.home_url();
        let option = |name: &str| context.option(name);
        let rendered = front_comment_text(&option, home.as_bytes(), &comment.content);
        let mut content = vec![(Key::String(b"rendered".to_vec()), text(rendered))];
        if edit {
            content.push((Key::String(b"raw".to_vec()), text(comment.content.clone())));
        }
        Value::Array(content)
    };
    let ty = || {
        text(if comment.comment_type.is_empty() {
            b"comment".to_vec()
        } else {
            comment.comment_type.clone()
        })
    };
    let fields: [(&str, &dyn Fn() -> Value); 17] = [
        ("id", &|| int(comment.id)),
        ("post", &|| int(comment.post_id)),
        ("parent", &|| int(comment.parent)),
        ("author", &|| int(comment.user_id)),
        ("author_name", &|| text(comment.author.clone())),
        ("author_email", &|| text(comment.author_email.clone())),
        ("author_url", &|| text(comment.author_url.clone())),
        ("author_ip", &|| text(comment.author_ip.clone())),
        ("author_user_agent", &|| text(row.agent.clone())),
        ("date", &|| text(rfc3339(&comment.date))),
        ("date_gmt", &|| text(rfc3339(&comment.date_gmt))),
        ("content", &content),
        ("link", &|| text(context.site.comment_link(comment))),
        ("status", &|| text(status(&row.approved))),
        ("type", &ty),
        ("author_avatar_urls", &|| comment_avatars(context, comment)),
        ("meta", &|| meta(row)),
    ];
    let data = fields
        .iter()
        .filter(|(name, _)| has(name))
        .map(|(name, value)| (Key::String(name.as_bytes().to_vec()), value()))
        .collect();
    let data = filter_by_context(data, &properties, &params.context());
    let mut response = Response::new(Value::Array(data), 200);
    if is_field_included("_links", &wanted) || is_field_included("_embedded", &wanted) {
        links(context, &mut response, comment);
    }
    response
}

/// `prepare_links()`.
fn links(context: &Context, response: &mut Response, comment: &Comment) {
    let embeddable = || ("embeddable", Value::Bool(true));
    crate::self_link(context, response, &format!("{ROUTE}/{}", comment.id));
    response.add_link("collection", context.rest_url(ROUTE));
    if comment.user_id != 0 {
        let author = context.rest_url(&format!("/wp/v2/users/{}", comment.user_id));
        response.add_link_with("author", author, vec![embeddable()]);
    }
    let post = (comment.post_id != 0)
        .then(|| context.site.post(comment.post_id))
        .flatten();
    if let Some((post, route)) = post
        .and_then(|post| Some((items_route(&post.post_type)?, post)))
        .map(|(route, post)| (post, route))
    {
        let up = context.rest_url(&format!("{route}/{}", post.id));
        let ty = ("post_type", text(post.post_type.clone()));
        response.add_link_with("up", up, vec![embeddable(), ty]);
    }
    if comment.parent != 0 {
        let parent = context.rest_url(&format!("{ROUTE}/{}", comment.parent));
        response.add_link_with("in-reply-to", parent, vec![embeddable()]);
    }
    let children = CommentQuery {
        approved: Some(vec!["0".into(), "1".into()]),
        parents: vec![comment.id],
        ..CommentQuery::default()
    };
    if context.site.count_comments(&children) > 0 {
        let id = comment.id.to_string();
        let mut args: Vec<(&[u8], Option<&[u8]>)> = vec![(b"parent", Some(id.as_bytes()))];
        if comment.comment_type == b"note" {
            args.extend([
                (&b"type"[..], Some(&b"note"[..])),
                (b"status", Some(b"all")),
            ]);
        }
        let href = add_query_arg(&args, context.rest_url(ROUTE).as_bytes());
        let href = String::from_utf8_lossy(&href).into_owned();
        response.add_link_with("children", href, vec![embeddable()]);
    }
}

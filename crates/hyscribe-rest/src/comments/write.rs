// SPDX-License-Identifier: AGPL-3.0-or-later

//! The comments' writes (MVP 4.3): `create_item()`, `update_item()` and
//! `delete_item()` of `WP_REST_Comments_Controller`. A request whose checks
//! pass gives the [`Plan`] `hyscribe-http` writes with
//! `wp_allow_comment()` and `wp_insert_comment()`, `wp_update_comment()`,
//! the status functions, `wp_trash_comment()` or `wp_delete_comment()`.
//! Notes, `author_ip` and `meta` are not ported; a visitor's create is
//! refused, as `rest_allow_anonymous_comments` refuses it by default.

use hyscribe_phpser::{Value, lookup, php_trim};

use super::{ROUTE, get_comment, prepare};
use crate::Context;
use crate::posts::write::{Outcome, Plan, Write, Writing, date_with_gmt, invalid};
use crate::request::{Arg, Params, Sanitize, arg, force_arg, typed};
use crate::response::{Response, RestError, text};
use crate::schema::validate_value_from_schema;

/// What a create or update writes, unslashed: `$prepared_comment` where the
/// request gives it, and the status to give the comment after.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CommentSave {
    /// The comment to update, 0 to create one.
    pub id: u64,
    /// `comment_post_ID`.
    pub post_id: Option<u64>,
    /// `comment_parent`.
    pub parent: Option<u64>,
    /// `user_id`.
    pub user_id: Option<u64>,
    /// `comment_author`.
    pub author: Option<Vec<u8>>,
    /// `comment_author_email`.
    pub author_email: Option<Vec<u8>>,
    /// `comment_author_url`.
    pub author_url: Option<Vec<u8>>,
    /// `comment_author_IP`: the request's address.
    pub author_ip: Vec<u8>,
    /// `comment_agent`.
    pub agent: Option<Vec<u8>>,
    /// `comment_content`, trimmed.
    pub content: Option<Vec<u8>>,
    /// `comment_date` and `comment_date_gmt`; a new comment's are now
    /// where none is given.
    pub date: Option<(String, String)>,
    /// `handle_status_param()` of this status after the write.
    pub status: Option<Vec<u8>>,
    /// Whether the comment's user has `moderate_comments`, which approves
    /// a new comment.
    pub moderate_comments: bool,
    /// Whether the current user may moderate or manage options, which
    /// skips the flood check.
    pub skip_flood: bool,
    /// Whether `wp_update_comment()` filters the content through kses.
    pub kses: bool,
}

/// `wp_get_comment_fields_max_lengths()` of the core tables.
const MAX_LENGTHS: [(&str, usize); 4] = [
    ("comment_author_column_length", 245),
    ("comment_author_email_column_length", 100),
    ("comment_author_url_column_length", 200),
    ("comment_content_column_length", 65525),
];

/// The ported write of the route `regex`'s endpoint `index`, if any.
pub(crate) fn writing(regex: &str, index: usize) -> Option<Writing> {
    let item = "/wp/v2/comments/(?P<id>[\\d]+)";
    let (unported, plan): (fn(&Params) -> bool, _) = match (regex, index) {
        (ROUTE, 1) => (|params| params.text("type") == b"note", plan_create as _),
        (_, 1) if regex == item => (|_| false, plan_update as _),
        (_, 2) if regex == item => (|_| false, plan_delete as _),
        _ => return None,
    };
    Some(Writing {
        unported,
        ..Writing::plain(plan, finish)
    })
}

/// `get_endpoint_args_for_item_schema()` of a create (with defaults) or an
/// update; `author_ip`, whose format is not ported, and `meta` are not.
pub(super) fn save_args(create: bool) -> Vec<Arg> {
    let string = r#"{"type":"string"}"#;
    let date = r#"{"type":"string","format":"date-time"}"#;
    let id = if create {
        r#"{"type":"integer","default":0}"#
    } else {
        r#"{"type":"integer"}"#
    };
    let value = |name, schema| arg(name, schema, true, Sanitize::Value);
    vec![
        value("author", r#"{"type":"integer"}"#),
        arg("author_email", string, false, Sanitize::Site(author_email)),
        arg("author_ip", string, false, Sanitize::NotPorted),
        arg("author_name", string, true, Sanitize::TextField),
        value("author_url", r#"{"type":"string","format":"uri"}"#),
        arg("author_user_agent", string, true, Sanitize::TextField),
        arg("content", r#"{"type":"object"}"#, false, Sanitize::Endpoint),
        value("date", date),
        value("date_gmt", date),
        value("parent", id),
        value("post", id),
        arg("status", string, true, Sanitize::Key),
        arg("meta", r#"{"type":"object"}"#, false, Sanitize::NotPorted),
    ]
}

/// A comment's delete: `force`, false unless given, and the post's
/// password.
pub(super) fn delete_args() -> Vec<Arg> {
    vec![
        typed("id", r#"{"type":"integer"}"#),
        force_arg(),
        typed("password", r#"{"type":"string"}"#),
    ]
}

/// `check_comment_author_email()`: an empty address as given, any other
/// checked as an email address.
fn author_email(_: &Context, email: &Value) -> crate::schema::Checked<Value> {
    let email = text(email.to_php_string());
    if !email.is_empty() {
        let schema = hyscribe_phpser::json_decode(br#"{"type":"string","format":"email"}"#);
        validate_value_from_schema(&email, &schema.unwrap_or_default(), "author_email")?;
    }
    Ok(email)
}

/// `prepare_item_for_database()`, with the request's address and user
/// agent.
fn prepared(context: &Context, params: &Params) -> Result<CommentSave, RestError> {
    let content = match params.get("content") {
        Some(Value::String(content)) => Some(content),
        Some(Value::Array(parts)) => match lookup(parts, "raw") {
            Some(Value::String(raw)) => Some(raw),
            _ => None,
        },
        _ => None,
    };
    let id = |name| params.get(name).map(|id| id.to_int().unsigned_abs());
    let given = |name| params.get(name).map(Value::to_php_string);
    let filled = |name| given(name).filter(|text| !Value::String(text.clone()).is_empty());
    let user = match params.get("author") {
        Some(author) => {
            let user = u64::try_from(author.to_int()).ok();
            let user = user.and_then(|id| context.site.user(id));
            let message = "Invalid comment author ID.";
            Some(user.ok_or_else(|| invalid("rest_comment_author_invalid", message))?)
        }
        None => None,
    };
    // `rest_is_ip_address()` of `$_SERVER['REMOTE_ADDR']`.
    let address = std::str::from_utf8(context.remote_addr).ok();
    let valid = address.is_some_and(|address| address.parse::<std::net::IpAddr>().is_ok());
    let header = context.user_agent.map(<[u8]>::to_vec);
    let header = header.filter(|agent| !Value::String(agent.clone()).is_empty());
    let zone = context.zone();
    Ok(CommentSave {
        content: content.map(|content| php_trim(content).to_vec()),
        post_id: id("post"),
        parent: id("parent"),
        user_id: user.as_ref().map(|user| user.id),
        author: given("author_name").or_else(|| user.as_ref().map(|u| u.display_name.clone())),
        author_email: given("author_email").or_else(|| user.as_ref().map(|u| u.email.clone())),
        author_url: given("author_url").or_else(|| user.as_ref().map(|u| u.url.clone())),
        author_ip: match valid {
            true => context.remote_addr.to_vec(),
            false => b"127.0.0.1".to_vec(),
        },
        agent: filled("author_user_agent").or(header),
        date: match (filled("date"), filled("date_gmt")) {
            (Some(date), _) => Some(date_with_gmt(&date, false, zone)),
            (None, Some(date)) => Some(date_with_gmt(&date, true, zone)),
            (None, None) => None,
        },
        status: given("status"),
        ..CommentSave::default()
    })
}

/// `check_is_comment_content_allowed()`: content, where given, is not
/// empty once trimmed.
fn content_allowed(save: &CommentSave) -> Result<(), RestError> {
    match save.content.as_deref() {
        Some(b"") => Err(invalid(
            "rest_comment_content_invalid",
            "Invalid comment content.",
        )),
        _ => Ok(()),
    }
}

/// `wp_check_comment_data_max_lengths()`, by bytes.
fn max_lengths(save: &CommentSave) -> Result<(), RestError> {
    let fields = [
        &save.author,
        &save.author_email,
        &save.author_url,
        &save.content,
    ];
    for ((code, max), value) in MAX_LENGTHS.into_iter().zip(fields) {
        if value.as_ref().is_some_and(|value| value.len() > max) {
            return Err(invalid(
                code,
                "Comment field exceeds maximum length allowed.",
            ));
        }
    }
    Ok(())
}

/// `create_item()` up to `wp_allow_comment()`, which is written with it.
fn plan_create(context: &Context, params: &Params) -> Result<Plan, RestError> {
    if params.get("id").is_some_and(|id| !id.is_empty()) {
        return Err(invalid(
            "rest_comment_exists",
            "Cannot create existing comment.",
        ));
    }
    let kind = params.text("type");
    if !Value::String(kind.clone()).is_empty() && kind != b"comment" {
        let message = "Cannot create a comment with that type.";
        return Err(invalid("rest_invalid_comment_type", message));
    }
    // Its type, none or `comment`, is stored as `comment`.
    let mut save = prepared(context, params)?;
    save.content = Some(save.content.unwrap_or_default());
    content_allowed(&save)?;
    let empty = |field: &Option<Vec<u8>>| {
        field
            .as_ref()
            .is_none_or(|field| Value::String(field.clone()).is_empty())
    };
    // `user_id` is set only for an author who exists, never 0.
    let missing = save.user_id.is_none()
        && empty(&save.author)
        && empty(&save.author_email)
        && empty(&save.author_url);
    if missing && let Some(user) = context.site.user(context.user) {
        save.user_id = Some(user.id);
        save.author = Some(user.display_name);
        save.author_email = Some(user.email);
        save.author_url = Some(user.url);
    }
    let required = context.option("require_name_email").unwrap_or_default();
    if !Value::String(required).is_empty() && (empty(&save.author) || empty(&save.author_email)) {
        let message = "Creating a comment requires valid author name and email values.";
        return Err(invalid("rest_comment_author_data_required", message));
    }
    max_lengths(&save)?;
    let user = save.user_id.unwrap_or_default();
    save.moderate_comments = user != 0 && context.site.can(user, "moderate_comments", &[]);
    save.skip_flood = context.can("manage_options") || context.can("moderate_comments");
    Ok(Plan::new(
        context,
        Write::Comment(Box::new(save)),
        Value::Null,
    ))
}

/// `update_item()` up to `wp_update_comment()`.
fn plan_update(context: &Context, params: &Params) -> Result<Plan, RestError> {
    // The permission check found it.
    let row = get_comment(context, params).unwrap_or_default();
    let comment = &row.comment;
    let kind = match comment.comment_type.as_slice() {
        b"" => b"comment".to_vec(),
        kind => kind.to_vec(),
    };
    if params
        .get("type")
        .is_some_and(|given| given.to_php_string() != kind)
    {
        let message = "Sorry, you are not allowed to change the comment type.";
        return Err(RestError::status("rest_comment_invalid_type", message, 404));
    }
    let mut save = prepared(context, params)?;
    save.id = comment.id;
    content_allowed(&save)?;
    max_lengths(&save)?;
    // `wp_update_comment()` filters the content unless the user and the
    // comment's author may post unfiltered HTML.
    let author = save.user_id.unwrap_or(comment.user_id);
    let unfiltered = [
        context.can("unfiltered_html"),
        context.site.can(author, "unfiltered_html", &[]),
    ];
    save.kses = unfiltered.contains(&false);
    Ok(Plan::new(
        context,
        Write::Comment(Box::new(save)),
        Value::Null,
    ))
}

/// `delete_item()` up to its write: deleted with `force`, else to the
/// trash, which keeps it for `EMPTY_TRASH_DAYS`.
fn plan_delete(context: &Context, params: &Params) -> Result<Plan, RestError> {
    // The permission check found it.
    let row = get_comment(context, params).unwrap_or_default();
    let mut edit = params.clone();
    edit.set("context", text("edit"));
    if params.get("force").is_some_and(|force| !force.is_empty()) {
        let previous = prepare(context, &edit, &row).data;
        return Ok(Plan::new(
            context,
            Write::DeleteComment(row.comment.id),
            previous,
        ));
    }
    if !context.site.uses_trash() {
        let message = "The comment does not support trashing. Set 'force=true' to delete.";
        return Err(RestError::status("rest_trash_not_supported", message, 501));
    }
    if row.approved == "trash" {
        let message = "The comment has already been trashed.";
        return Err(RestError::status("rest_already_trashed", message, 410));
    }
    Ok(Plan::new(
        context,
        Write::TrashComment(row.comment.id),
        Value::Null,
    ))
}

/// The answer once written: the comment, a new one with `201`; a deleted
/// comment's answer as it was; or PHP's error where it refused.
fn finish(
    context: &Context,
    params: &mut Params,
    plan: Plan,
    outcome: Outcome,
) -> Result<Response, RestError> {
    let id = match outcome {
        Outcome::Written(id) => id,
        Outcome::Refused("comment_duplicate") => {
            let message =
                "Duplicate comment detected; it looks as though you&#8217;ve already said that!";
            return Err(RestError::status("comment_duplicate", message, 409));
        }
        Outcome::Refused("comment_flood") => {
            let message = "You are posting comments too quickly. Slow down.";
            return Err(invalid("comment_flood", message));
        }
        Outcome::Refused("rest_comment_failed_edit") => {
            let message = "Updating comment failed.";
            return Err(RestError::status("rest_comment_failed_edit", message, 500));
        }
        _ => {
            let message = "The comment cannot be deleted.";
            return Err(RestError::status("rest_cannot_delete", message, 500));
        }
    };
    if let Write::DeleteComment(_) = plan.write {
        return Ok(plan.deleted());
    }
    let created = matches!(&plan.write, Write::Comment(save) if save.id == 0);
    let edit = !created || context.can("moderate_comments");
    params.set("context", text(if edit { "edit" } else { "view" }));
    let found = super::find(context, id).unwrap_or_default();
    let mut response = prepare(context, params, &found);
    if created {
        response.status = 201;
        response.header("Location", &context.rest_url(&format!("{ROUTE}/{id}")));
    }
    Ok(response)
}

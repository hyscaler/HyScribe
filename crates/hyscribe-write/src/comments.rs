// SPDX-License-Identifier: AGPL-3.0-or-later

//! Comments, from WordPress 7.1.3's `wp-includes/comment.php`:
//! `wp_insert_comment()`, `wp_new_comment()` without its request,
//! `wp_update_comment()`, the status changes (`wp_set_comment_status()`,
//! trash, spam and their undoing), `wp_delete_comment()` and comment counts.
//! Each writes through the action's [`Write`]; what PHP reads from the
//! request or the clock is an argument. Comments go to the trash before
//! they are deleted while `EMPTY_TRASH_DAYS` is not 0.

mod allow;
mod filter;

use hyscribe_db::{CommentFields, DbError, Meta, Write};
use hyscribe_phpser::{Value, is_empty_string};
use hyscribe_render::date::{Zone, format_unix, stored_unix};

use crate::options::{delete_metadata, delete_object_meta};
use crate::steps::{Steps, step};
use crate::{MYSQL, wp_timezone};

pub use allow::Refused;
pub use filter::{CommentCookie, sanitize_comment_cookie};

/// What the comment functions take from outside the database.
#[derive(Clone, Copy, Debug)]
pub struct Context<'a> {
    /// The time of the request, in Unix seconds.
    pub now: i64,
    /// `home_url()`, whose host makes a link internal.
    pub home: &'a [u8],
    /// `EMPTY_TRASH_DAYS`: comments are deleted without the trash at 0.
    pub empty_trash_days: u32,
}

/// The capabilities of the user who writes a comment, who is also its
/// `user_id` (or a visitor, `user_id` 0).
#[derive(Clone, Copy, Debug, Default)]
pub struct Commenter {
    /// `moderate_comments`: approved at once and never throttled.
    pub moderate_comments: bool,
    /// `manage_options`: never throttled.
    pub manage_options: bool,
    /// `unfiltered_html`: content saved without kses.
    pub unfiltered_html: bool,
}

/// A comment to insert, unslashed: `wp_insert_comment()`'s `$commentdata`.
#[derive(Clone, Debug, Default)]
pub struct NewComment {
    /// `comment_post_ID`.
    pub post_id: u64,
    /// `comment_author`.
    pub author: Vec<u8>,
    /// `comment_author_email`.
    pub author_email: Vec<u8>,
    /// `comment_author_url`.
    pub author_url: Vec<u8>,
    /// `comment_author_IP`, from the request.
    pub author_ip: Vec<u8>,
    /// `comment_date`; the site's time now when unset.
    pub date: Option<String>,
    /// `comment_date_gmt`; from `comment_date` when unset.
    pub date_gmt: Option<String>,
    /// `comment_content`.
    pub content: Vec<u8>,
    /// `comment_karma`.
    pub karma: i64,
    /// `comment_approved`, `1` when unset; `wp_new_comment()` sets its own.
    pub approved: Option<String>,
    /// `comment_agent`, the request's user agent.
    pub agent: Vec<u8>,
    /// `comment_type`, `comment` when empty.
    pub comment_type: String,
    /// `comment_parent`.
    pub parent: u64,
    /// `user_id`.
    pub user_id: u64,
}

/// `wp_update_comment()`'s `$commentarr`, unslashed: the fields to change.
/// `comment_date_gmt` always follows `comment_date`.
#[derive(Clone, Debug, Default)]
pub struct CommentEdit {
    /// `comment_ID`.
    pub id: u64,
    /// `comment_post_ID`.
    pub post_id: Option<u64>,
    /// `comment_author`.
    pub author: Option<Vec<u8>>,
    /// `comment_author_email`.
    pub author_email: Option<Vec<u8>>,
    /// `comment_author_url`.
    pub author_url: Option<Vec<u8>>,
    /// `comment_author_IP`.
    pub author_ip: Option<Vec<u8>>,
    /// `comment_date`.
    pub date: Option<String>,
    /// `comment_content`.
    pub content: Option<Vec<u8>>,
    /// `comment_karma`.
    pub karma: Option<i64>,
    /// `comment_approved`, with `hold` and `approve` for `0` and `1`.
    pub approved: Option<String>,
    /// `comment_agent`.
    pub agent: Option<Vec<u8>>,
    /// `comment_type`.
    pub comment_type: Option<String>,
    /// `comment_parent`.
    pub parent: Option<u64>,
    /// `user_id`.
    pub user_id: Option<u64>,
}

const TRASH_STATUS: &str = "_wp_trash_meta_status";
const TRASH_TIME: &str = "_wp_trash_meta_time";

/// `get_option( $name )`, empty when it is not set.
async fn option(steps: &mut Steps<'_, '_, DbError>, name: &str) -> Vec<u8> {
    step!(steps, option_value(name)).unwrap_or_default()
}

/// `wp_timezone()`.
async fn zone(steps: &mut Steps<'_, '_, DbError>) -> Zone {
    let zone = steps.run(async |w| wp_timezone(w).await.map(Some));
    // Read as UTC once a statement failed, and never written.
    zone.await.unwrap_or(Zone::Utc)
}

/// `get_gmt_from_date()` of a stored date in the site's `zone`.
fn gmt_from_date(date: &str, zone: Zone) -> String {
    format_unix(MYSQL, stored_unix(date, zone), Zone::Utc)
}

/// Whether `use_balanceTags` asks `balanceTags()` to balance.
async fn balance_tags(steps: &mut Steps<'_, '_, DbError>) -> bool {
    Value::String(option(steps, "use_balanceTags").await).to_int() == 1
}

/// `wp_insert_comment()`: the new comment's ID. An approved comment
/// recounts its post's comments.
#[cfg(test)]
pub(crate) async fn insert_comment(
    write: &mut Write<'_>,
    context: Context<'_>,
    comment: &NewComment,
) -> Result<u64, DbError> {
    let mut steps = Steps::new(write);
    let id = insert(&mut steps, context, comment).await;
    steps.finish(Ok(id))
}

async fn insert(
    steps: &mut Steps<'_, '_, DbError>,
    context: Context<'_>,
    comment: &NewComment,
) -> u64 {
    let zone = zone(steps).await;
    let date = (comment.date.clone()).unwrap_or_else(|| format_unix(MYSQL, context.now, zone));
    let date_gmt = (comment.date_gmt.clone()).unwrap_or_else(|| gmt_from_date(&date, zone));
    let comment_type = match comment.comment_type.as_str() {
        "" | "0" => "comment",
        kind => kind,
    };
    let fields = CommentFields {
        post_id: comment.post_id,
        author: comment.author.clone(),
        author_email: comment.author_email.clone(),
        author_url: comment.author_url.clone(),
        author_ip: comment.author_ip.clone(),
        date,
        date_gmt,
        content: comment.content.clone(),
        karma: comment.karma,
        approved: comment.approved.clone().unwrap_or_else(|| "1".into()),
        agent: comment.agent.clone(),
        comment_type: comment_type.into(),
        parent: comment.parent,
        user_id: comment.user_id,
    };
    let id = step!(steps, insert_comment(&fields));
    if Value::String(fields.approved.into_bytes()).to_int() == 1 {
        update_count(steps, fields.post_id).await;
    }
    id
}

/// `wp_new_comment()` with `$wp_error`, from the point where the request's
/// IP address and user agent are in `comment`: the parent kept only if it
/// is approved or held, the approval decided, the fields filtered, then
/// inserted. A duplicate or a flood is refused and writes nothing.
pub async fn new_comment(
    write: &mut Write<'_>,
    context: Context<'_>,
    mut comment: NewComment,
    commenter: Commenter,
) -> Result<Result<u64, Refused>, DbError> {
    let mut steps = Steps::new(write);
    let parent = match comment.parent {
        0 => None,
        parent => step!(steps, comment(parent)),
    };
    if !parent.is_some_and(|parent| matches!(parent.approved.as_str(), "0" | "1")) {
        comment.parent = 0;
    }
    comment
        .author_ip
        .retain(|b| b.is_ascii_hexdigit() || b":., ".contains(b));
    comment.agent.truncate(254);
    let zone = zone(&mut steps).await;
    let set = |date: Option<String>| date.filter(|date| !is_empty_string(date.as_bytes()));
    let date = set(comment.date.take()).unwrap_or_else(|| format_unix(MYSQL, context.now, zone));
    let date_gmt =
        set(comment.date_gmt.take()).unwrap_or_else(|| format_unix(MYSQL, context.now, Zone::Utc));
    comment.date = Some(date);
    comment.date_gmt = Some(date_gmt);
    // An empty `comment_type` is `comment` from here on in PHP, which only
    // trackbacks and pingbacks tell apart; `insert()` names it.
    let approved = allow::allow_comment(&mut steps, context, &comment, commenter).await;
    let approved = match approved {
        Ok(approved) => approved,
        Err(refused) => return steps.finish(Ok(Err(refused))),
    };
    let balance = balance_tags(&mut steps).await;
    filter::Fields {
        author: &mut comment.author,
        email: &mut comment.author_email,
        url: &mut comment.author_url,
        content: &mut comment.content,
    }
    .filter(!commenter.unfiltered_html, context.home, balance);
    let approved = match approved.as_str() {
        "trash" | "spam" => approved,
        _ => allow::check_comment_data(&mut steps, context, &comment, commenter).await,
    };
    comment.approved = Some(approved);
    let id = insert(&mut steps, context, &comment).await;
    steps.finish(Ok(Ok(id)))
}

/// The REST API's create: `wp_allow_comment()`, then `wp_insert_comment(
/// wp_filter_comment() )`, without `wp_new_comment()`'s other checks.
pub async fn allow_and_insert_comment(
    write: &mut Write<'_>,
    context: Context<'_>,
    mut comment: NewComment,
    commenter: Commenter,
) -> Result<Result<u64, Refused>, DbError> {
    let mut steps = Steps::new(write);
    let approved = allow::allow_comment(&mut steps, context, &comment, commenter).await;
    let approved = match approved {
        Ok(approved) => approved,
        Err(refused) => return steps.finish(Ok(Err(refused))),
    };
    comment.approved = Some(approved);
    let id = filtered_insert(&mut steps, context, comment, commenter.unfiltered_html).await;
    steps.finish(Ok(Ok(id)))
}

/// The REST API's create of a note: approved with no `wp_allow_comment()`,
/// then `wp_insert_comment( wp_filter_comment() )`.
pub async fn insert_note(
    write: &mut Write<'_>,
    context: Context<'_>,
    mut comment: NewComment,
    unfiltered_html: bool,
) -> Result<u64, DbError> {
    let mut steps = Steps::new(write);
    comment.comment_type = "note".into();
    comment.approved = Some("1".into());
    let id = filtered_insert(&mut steps, context, comment, unfiltered_html).await;
    steps.finish(Ok(id))
}

/// `wp_insert_comment( wp_filter_comment( $comment ) )`.
async fn filtered_insert(
    steps: &mut Steps<'_, '_, DbError>,
    context: Context<'_>,
    mut comment: NewComment,
    unfiltered_html: bool,
) -> u64 {
    let balance = balance_tags(steps).await;
    filter::Fields {
        author: &mut comment.author,
        email: &mut comment.author_email,
        url: &mut comment.author_url,
        content: &mut comment.content,
    }
    .filter(!unfiltered_html, context.home, balance);
    insert(steps, context, &comment).await
}

/// `wp_update_comment()`: `None` when there is no such comment or no such
/// new post, else whether the row changed. Every field is filtered again,
/// through kses when `kses` (the current user or the comment's author may
/// not post unfiltered HTML).
pub async fn update_comment(
    write: &mut Write<'_>,
    context: Context<'_>,
    edit: &CommentEdit,
    kses: bool,
) -> Result<Option<bool>, DbError> {
    let mut steps = Steps::new(write);
    let changed = update(&mut steps, context, edit, kses).await;
    steps.finish(Ok(changed))
}

async fn update(
    steps: &mut Steps<'_, '_, DbError>,
    context: Context<'_>,
    edit: &CommentEdit,
    kses: bool,
) -> Option<bool> {
    let old = step!(steps, comment(edit.id))?;
    let post = edit.post_id.filter(|&post| post != 0);
    if let Some(post) = post
        && step!(steps, post_author(post)).is_none()
    {
        return None;
    }
    let mut fields = old.clone();
    for (new, old) in [
        (&edit.author, &mut fields.author),
        (&edit.author_email, &mut fields.author_email),
        (&edit.author_url, &mut fields.author_url),
        (&edit.author_ip, &mut fields.author_ip),
        (&edit.content, &mut fields.content),
        (&edit.agent, &mut fields.agent),
    ] {
        if let Some(new) = new {
            old.clone_from(new);
        }
    }
    fields.post_id = edit.post_id.unwrap_or(fields.post_id);
    fields.karma = edit.karma.unwrap_or(fields.karma);
    fields.parent = edit.parent.unwrap_or(fields.parent);
    fields.user_id = edit.user_id.unwrap_or(fields.user_id);
    fields.date = edit.date.clone().unwrap_or(fields.date);
    fields.comment_type = edit.comment_type.clone().unwrap_or(fields.comment_type);
    fields.approved = match edit.approved.as_deref() {
        Some("hold") => "0".into(),
        Some("approve") => "1".into(),
        status => status.map_or(fields.approved, Into::into),
    };
    let balance = balance_tags(steps).await;
    filter::Fields {
        author: &mut fields.author,
        email: &mut fields.author_email,
        url: &mut fields.author_url,
        content: &mut fields.content,
    }
    .filter(kses, context.home, balance);
    // `comment_save_pre`.
    let content = crate::convert_invalid_entities(&fields.content);
    fields.content = filter::balance_tags(&content, balance);
    fields.date_gmt = gmt_from_date(&fields.date, zone(steps).await);
    step!(steps, update_comment(edit.id, &fields));
    update_count(steps, fields.post_id).await;
    Some(fields != old)
}

/// `wp_set_comment_status()` to `hold`, `approve`, `spam` or `trash` (or
/// `0` and `1`): false for any other status, a missing comment, or one
/// already in that status, which PHP's database reports as unchanged.
pub async fn set_comment_status(
    write: &mut Write<'_>,
    id: u64,
    status: &str,
) -> Result<bool, DbError> {
    let mut steps = Steps::new(write);
    let changed = set_status(&mut steps, id, status).await;
    steps.finish(Ok(changed))
}

async fn set_status(steps: &mut Steps<'_, '_, DbError>, id: u64, status: &str) -> bool {
    let status = match status {
        "hold" | "0" => "0",
        "approve" | "1" => "1",
        "spam" | "trash" => status,
        _ => return false,
    };
    let Some(comment) = step!(steps, comment(id)) else {
        return false;
    };
    if comment.approved == status {
        return false;
    }
    step!(steps, set_comment_status(id, status));
    update_count(steps, comment.post_id).await;
    true
}

/// `wp_trash_comment()`: the comment to the trash with its status and time
/// kept in meta; a top level note takes its approved and held replying
/// notes with it.
pub async fn trash_comment(
    write: &mut Write<'_>,
    context: Context<'_>,
    id: u64,
) -> Result<bool, DbError> {
    let mut steps = Steps::new(write);
    let trashed = trash(&mut steps, context, id).await;
    steps.finish(Ok(trashed))
}

async fn trash(steps: &mut Steps<'_, '_, DbError>, context: Context<'_>, id: u64) -> bool {
    // Without the Trash it deletes; a note's replies are moved up first, so
    // its query for them after finds none.
    if context.empty_trash_days == 0 {
        return Box::pin(delete(steps, context, id, true)).await;
    }
    let Some(comment) = put_away(steps, context, id, "trash").await else {
        return false;
    };
    if comment.comment_type != "note" || comment.parent != 0 {
        return true;
    }
    // The replies are approved or held, so each goes to the trash.
    for child in step!(steps, child_note_ids(id)) {
        put_away(steps, context, child, "trash").await;
    }
    true
}

/// `wp_spam_comment()`: the comment marked as spam, its status and time
/// kept in meta.
pub async fn spam_comment(
    write: &mut Write<'_>,
    context: Context<'_>,
    id: u64,
) -> Result<bool, DbError> {
    let mut steps = Steps::new(write);
    let spammed = put_away(&mut steps, context, id, "spam").await.is_some();
    steps.finish(Ok(spammed))
}

/// The comment `id` given `status`, the status it had and the time kept in
/// `_wp_trash_meta_*`: the comment as it was, if it changed.
async fn put_away(
    steps: &mut Steps<'_, '_, DbError>,
    context: Context<'_>,
    id: u64,
    status: &str,
) -> Option<CommentFields> {
    let comment = step!(steps, comment(id))?;
    if !set_status(steps, id, status).await {
        return None;
    }
    delete_meta(steps, id, TRASH_STATUS).await;
    delete_meta(steps, id, TRASH_TIME).await;
    let (old, now) = (comment.approved.as_bytes(), context.now.to_string());
    step!(steps, add_meta(Meta::Comment, id, TRASH_STATUS, Some(old)));
    step!(
        steps,
        add_meta(Meta::Comment, id, TRASH_TIME, Some(now.as_bytes()))
    );
    Some(comment)
}

/// `wp_untrash_comment()` and `wp_unspam_comment()`, which differ only in
/// their hooks: the status kept in meta back, held if none, and the meta
/// deleted.
pub async fn restore_comment(write: &mut Write<'_>, id: u64) -> Result<bool, DbError> {
    let mut steps = Steps::new(write);
    let kept = step!(steps, meta_value(Meta::Comment, id, TRASH_STATUS));
    let status = kept.filter(|status| !is_empty_string(status));
    let status = String::from_utf8_lossy(status.as_deref().unwrap_or(b"0")).into_owned();
    let restored = set_status(&mut steps, id, &status).await;
    if restored {
        delete_meta(&mut steps, id, TRASH_TIME).await;
        delete_meta(&mut steps, id, TRASH_STATUS).await;
    }
    steps.finish(Ok(restored))
}

/// `delete_comment_meta( $id, $key )`.
async fn delete_meta(steps: &mut Steps<'_, '_, DbError>, id: u64, key: &str) {
    let any = &Value::String(Vec::new());
    let deleted =
        async |w: &mut Write<'_>| delete_metadata(w, Meta::Comment, id, key, any, false).await;
    steps.run(deleted).await;
}

/// `wp_delete_comment()`: a comment not in the trash or spam goes to the
/// trash unless `force`; else its replies move up to its parent, its meta
/// and the comment are deleted, and an approved one recounts its post.
pub async fn delete_comment(
    write: &mut Write<'_>,
    context: Context<'_>,
    id: u64,
    force: bool,
) -> Result<bool, DbError> {
    let mut steps = Steps::new(write);
    let deleted = delete(&mut steps, context, id, force).await;
    steps.finish(Ok(deleted))
}

async fn delete(
    steps: &mut Steps<'_, '_, DbError>,
    context: Context<'_>,
    id: u64,
    force: bool,
) -> bool {
    let Some(comment) = step!(steps, comment(id)) else {
        return false;
    };
    let to_trash = !force && context.empty_trash_days != 0;
    if to_trash && !matches!(comment.approved.as_str(), "trash" | "spam") {
        return trash(steps, context, id).await;
    }
    step!(steps, reparent_comments(id, comment.parent));
    steps
        .run(async |w| delete_object_meta(w, Meta::Comment, id).await)
        .await;
    step!(steps, delete_comment(id));
    if comment.approved == "1" {
        update_count(steps, comment.post_id).await;
    }
    true
}

/// `wp_update_comment_count_now()`: the post's `comment_count` set to its
/// approved comments, notes left out, counted again (ADR 0012). Nothing for
/// post 0 or a missing post.
async fn update_count(steps: &mut Steps<'_, '_, DbError>, post: u64) {
    let count = step!(steps, approved_comment_count(post));
    step!(steps, set_comment_count(post, count));
}

#[cfg(test)]
mod tests;

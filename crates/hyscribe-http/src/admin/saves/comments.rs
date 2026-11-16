// SPDX-License-Identifier: AGPL-3.0-or-later

//! The comments' moderation: `edit-comments.php`'s bulk actions,
//! `comment.php`'s row actions and the list's Ajax actions, from
//! `wp-admin/includes/ajax-actions.php`. WordPress mails nothing here.

use hyscribe_admin::actions::{Asked, action, post_int};
use hyscribe_admin::posts::Answer;
use hyscribe_db::{DbError, Write};
use hyscribe_kses::esc_url_raw;
use hyscribe_phpser::{Value, absint, add_query_arg, parse_str, php_trim, remove_query_arg};
use hyscribe_render::number_format;
use hyscribe_write::Error;
use hyscribe_write::comments::{
    Commenter, Context, NewComment, delete_comment, new_comment, restore_comment,
    set_comment_status, spam_comment, trash_comment,
};

use super::super::Admin;
use super::{Acting, Ajax, Handler, Saving, Stop, read, referer, text};

mod edit;

/// A moderation, as the bulk actions name it.
#[derive(Clone, Copy, PartialEq)]
enum Moderation {
    Approve,
    Unapprove,
    Spam,
    Unspam,
    Trash,
    Untrash,
    Delete,
}

use Moderation::{Approve, Delete, Spam, Trash, Unapprove, Unspam, Untrash};

impl Moderation {
    const ALL: [Moderation; 7] = [Approve, Unapprove, Spam, Unspam, Trash, Untrash, Delete];

    /// The bulk action's name, which is also the redirect's count.
    fn name(self) -> &'static str {
        match self {
            Approve => "approve",
            Unapprove => "unapprove",
            Spam => "spam",
            Unspam => "unspam",
            Trash => "trash",
            Untrash => "untrash",
            Delete => "delete",
        }
    }

    /// The count `edit-comments.php` sends back.
    fn count(self) -> &'static str {
        match self {
            Approve => "approved",
            Unapprove => "unapproved",
            Spam => "spammed",
            Unspam => "unspammed",
            Trash => "trashed",
            Untrash => "untrashed",
            Delete => "deleted",
        }
    }

    /// The moderation's write of the comment `id`, whether it changed.
    async fn apply(
        self,
        write: &mut Write<'_>,
        asked: &Asked<'_>,
        id: u64,
    ) -> Result<bool, DbError> {
        let context = Context {
            now: asked.now,
            home: asked.home.as_bytes(),
        };
        match self {
            Approve => set_comment_status(write, id, "approve").await,
            Unapprove => set_comment_status(write, id, "hold").await,
            Spam => spam_comment(write, context, id).await,
            Unspam | Untrash => restore_comment(write, id).await,
            Trash => trash_comment(write, context, id).await,
            Delete => delete_comment(write, context, id, false).await,
        }
    }
}

/// The redirect counts both lists clear first.
const COUNTS: [&[u8]; 8] = [
    b"trashed",
    b"untrashed",
    b"deleted",
    b"spammed",
    b"unspammed",
    b"approved",
    b"unapproved",
    b"ids",
];

/// `current_user_can( 'edit_comment', $id )`.
fn can_edit(site: &dyn Acting, id: &Value) -> bool {
    read(|| site.current_user_can("edit_comment", std::slice::from_ref(id)))
}

/// `edit-comments.php`'s bulk actions, Empty Spam and Empty Trash: each
/// comment the user may edit moderated, then the list sent back with the
/// counts, or the referer when no comment is named.
pub(super) fn bulk<'a>(
    write: &'a mut Write<'_>,
    asked: &'a Asked<'a>,
    site: &'a dyn Acting,
) -> Saving<'a> {
    Box::pin(async move {
        let mut doaction = action("edit-comments", asked);
        let pagenum = asked.request("paged").map_or(0, absint).max(1);
        let referer = referer(asked);
        let ids: Vec<Value> = if doaction == b"delete_all"
            && asked
                .request("pagegen_timestamp")
                .is_some_and(|time| !time.is_empty())
        {
            // `$wpdb->prepare()` of an array is not a query.
            let given = |name| match asked.request(name) {
                Some(Value::Array(_)) => Err(Error::Unported("an array to empty the trash with")),
                given => Ok(text(&given.map(Value::to_php_string).unwrap_or_default())),
            };
            let (status, time) = (given("comment_status")?, given("pagegen_timestamp")?);
            doaction = b"delete".to_vec();
            let ids = write.comment_ids_before(&status, &time).await?;
            ids.into_iter()
                .map(|id| Value::Int(id.cast_signed()))
                .collect()
        } else if let Some(given) = asked.request("delete_comments") {
            doaction = (asked.request("action").map(Value::to_php_string)).unwrap_or_default();
            given
                .as_array()
                .unwrap_or_default()
                .iter()
                .map(|(_, id)| id.clone())
                .collect()
        } else if let Some(ids) = asked.request("ids") {
            // `explode()` of an array is a fatal error.
            let ids = ids.as_bytes().ok_or(Error::Unported("an array as ids"))?;
            let ids = ids.split(|&b| b == b',');
            ids.map(|id| Value::Int(absint(&Value::String(id.to_vec())).cast_signed()))
                .collect()
        } else if let Some(referer) = referer {
            return Ok(Answer::Redirect(text(&referer)));
        } else {
            Vec::new()
        };
        let moderation = Moderation::ALL
            .into_iter()
            .find(|m| m.name().as_bytes() == doaction);
        let mut done = 0;
        for id in &ids {
            if !can_edit(site, id) {
                continue;
            }
            if let Some(moderation) = moderation {
                moderation
                    .apply(write, asked, id.to_int().unsigned_abs())
                    .await?;
                done += 1;
            }
        }
        let back = referer.unwrap_or_else(|| asked.uri.to_vec());
        let back = remove_query_arg(&COUNTS, &back);
        let mut to = add_query_arg(&[(b"paged", Some(pagenum.to_string().as_bytes()))], &back);
        if let Some(moderation) = moderation.filter(|_| done > 0) {
            let done = done.to_string();
            to = add_query_arg(
                &[(moderation.count().as_bytes(), Some(done.as_bytes()))],
                &to,
            );
            if matches!(moderation, Spam | Trash) {
                let ids: Vec<Vec<u8>> = ids.iter().map(Value::to_php_string).collect();
                to = add_query_arg(&[(b"ids", Some(&ids.join(&b',')))], &to);
            }
        }
        // `wp_safe_redirect()`: the referer is valid, the request's URI local.
        Ok(Answer::Redirect(text(&to)))
    })
}

/// `comment.php`'s handler of `action`, a row action's moderation.
pub(super) fn moderation(action: &[u8]) -> Option<Handler> {
    row_moderation(action).map(|_| moderate as Handler)
}

/// The moderation of `comment.php`'s row action `name`: `approvecomment`
/// and so on.
fn row_moderation(name: &[u8]) -> Option<Moderation> {
    let named = |m: &Moderation| [m.name().as_bytes(), b"comment"].concat() == name;
    Moderation::ALL.into_iter().find(named)
}

/// `comment.php`'s moderation of the comment `c`, sent back to the referer
/// or the comments list with its count.
fn moderate<'a>(
    write: &'a mut Write<'_>,
    asked: &'a Asked<'a>,
    site: &'a dyn Acting,
) -> Saving<'a> {
    Box::pin(async move {
        let moderation = row_moderation(&action("comment", asked)).unwrap_or(Delete);
        let id = asked.request("c").map_or(0, absint);
        let comment = write.comment(id).await?;
        let comment = comment.filter(|_| can_edit(site, &Value::Int(id.cast_signed())));
        let Some(comment) = comment else {
            return Err(Error::Unported("comment_footer_die()'s page").into());
        };
        let noredir = asked.request("noredir").is_some();
        let referer = referer(asked);
        // Not back to `comment.php`'s pages.
        let referer = referer.filter(|referer| {
            let text = text(referer);
            !text.contains("comment.php") && !text.contains("hs-admin/comment")
        });
        let original = match asked.request("_wp_original_http_referer") {
            Some(Value::Array(_)) => {
                return Err(Error::Unported("an array as _wp_original_http_referer").into());
            }
            Some(given) if !given.is_empty() => {
                Some(read(|| site.validate_redirect(&given.to_php_string())))
            }
            _ => None,
        };
        let back = match (referer, original.filter(|o| !o.is_empty())) {
            (Some(referer), _) if !noredir => referer,
            (_, Some(original)) if !noredir => original,
            _ if matches!(moderation, Approve | Unapprove) => {
                let path = format!("edit-comments.php?p={}", comment.post_id);
                read(|| site.admin_url(&path)).into_bytes()
            }
            _ => read(|| site.admin_url("edit-comments.php")).into_bytes(),
        };
        let back = remove_query_arg(&COUNTS, &back);
        moderation.apply(write, asked, id).await?;
        let id = id.to_string();
        let mut args: Vec<(&[u8], Option<&[u8]>)> =
            vec![(moderation.count().as_bytes(), Some(b"1"))];
        if matches!(moderation, Spam | Trash) {
            args.push((b"ids", Some(id.as_bytes())));
        }
        Ok(Answer::Redirect(text(&add_query_arg(&args, &back))))
    })
}

/// `wp_get_comment_status()`, `None` for false.
fn comment_status(approved: &str) -> Option<&'static str> {
    Some(hyscribe_admin::dashboard::comment_status(approved)).filter(|status| !status.is_empty())
}

/// `wp_die( time() )`.
fn now(asked: &Asked) -> Ajax {
    Ajax::Text(asked.now.to_string())
}

/// The list's Approve and Unapprove (`wp_ajax_dim_comment()`): approved
/// when held or spam, else held.
pub(super) fn dim<'a>(
    write: &'a mut Write<'_>,
    asked: &'a Asked<'a>,
    site: &'a dyn Acting,
) -> Saving<'a, Ajax> {
    Box::pin(async move {
        let id = post_int(asked, "id");
        let comment = match u64::try_from(id) {
            Ok(id) => write.comment(id).await?,
            Err(_) => None,
        };
        let Some(comment) = comment else {
            let error = format!(
                "<wp_error code='invalid_comment'><![CDATA[Comment {id} does not exist]]></wp_error>"
            );
            return Ok(Ajax::Xml(ajax_response(
                asked,
                ("comment", 0, "1"),
                &error,
                &[],
            )));
        };
        let current = comment_status(&comment.approved);
        let new = asked.post("new").and_then(Value::as_bytes);
        if new.is_some() && new == current.map(str::as_bytes) {
            return Ok(now(asked));
        }
        let id = id.unsigned_abs();
        let status = match current {
            Some("unapproved" | "spam") => "approve",
            _ => "hold",
        };
        set_comment_status(write, id, status).await?;
        Ok(delete_response(write, asked, site, id, -1).await?)
    })
}

/// The list's Trash, Spam, Delete Permanently and their undoing
/// (`wp_ajax_delete_comment()`): `time()` for a comment missing or in the
/// status already.
pub(super) fn delete<'a>(
    write: &'a mut Write<'_>,
    asked: &'a Asked<'a>,
    site: &'a dyn Acting,
) -> Saving<'a, Ajax> {
    Box::pin(async move {
        let id = post_int(asked, "id");
        let comment = match u64::try_from(id) {
            Ok(id) => write.comment(id).await?,
            Err(_) => None,
        };
        let Some(comment) = comment else {
            return Ok(now(asked));
        };
        let id = id.unsigned_abs();
        let status = comment_status(&comment.approved);
        let asks = |name| asked.post(name).and_then(Value::as_bytes) == Some(b"1");
        // Undone, out of the view it was in.
        let back = |view: &[u8]| {
            let given = asked.post("comment_status").and_then(Value::as_bytes);
            if given == Some(view) { -1 } else { 1 }
        };
        let (done, delta) = if asks("trash") {
            if status == Some("trash") {
                return Ok(now(asked));
            }
            (Trash, -1)
        } else if asks("untrash") {
            if status != Some("trash") {
                return Ok(now(asked));
            }
            (Untrash, back(b"trash"))
        } else if asks("spam") {
            if status == Some("spam") {
                return Ok(now(asked));
            }
            (Spam, -1)
        } else if asks("unspam") {
            if status != Some("spam") {
                return Ok(now(asked));
            }
            (Unspam, back(b"spam"))
        } else if asks("delete") {
            (Delete, -1)
        } else {
            return Ok(Ajax::Text("-1".into()));
        };
        if done.apply(write, asked, id).await? {
            Ok(delete_response(write, asked, site, id, delta).await?)
        } else {
            Ok(Ajax::Text("0".into()))
        }
    })
}

/// `_wp_ajax_delete_comment_response()`: the comment's status and the
/// counts after it, and the list's total when the list sends it, counted
/// again each time (where PHP does at random, 1 in `_per_page` times).
async fn delete_response(
    write: &mut Write<'_>,
    asked: &Asked<'_>,
    site: &dyn Acting,
    id: u64,
    delta: i64,
) -> Result<Ajax, Error> {
    let url = match asked.post("_url") {
        // `sanitize_url()` of an array is a fatal error.
        Some(Value::Array(_)) => return Err(Error::Unported("an array as _url")),
        Some(url) => esc_url_raw(&url.to_php_string()),
        None => Vec::new(),
    };
    let [total, per_page, page] =
        ["_total", "_per_page", "_page"].map(|name| post_int(asked, name));
    let comment = write.comment(id).await?;
    let status = comment
        .as_ref()
        .map_or("", |c| c.approved.as_str())
        .to_string();
    let post = comment
        .as_ref()
        .map_or(String::new(), |c| c.post_id.to_string());
    let paged = total != 0 && per_page != 0 && page != 0 && !url.is_empty();
    let mut supplemental = vec![("status", status.clone()), ("postId", post)];
    if paged {
        let mut total = total.saturating_add(delta).max(0);
        if total % per_page != 0 && per_page.is_negative() {
            // `mt_rand( 1, $per_page )` is a fatal error.
            return Err(Error::Unported("a negative _per_page"));
        }
        // Always counted again: PHP's `1 === mt_rand( 1, $per_page )` outcome.
        let query = url.splitn(2, |&b| b == b'?').nth(1).unwrap_or_default();
        let query = query.split(|&b| b == b'#').next().unwrap_or_default();
        let vars = parse_str(query);
        let var = |name| vars.get_str(name).filter(|v| !v.is_empty());
        if var("comment_type").is_none() {
            let post = var("p").map_or(0, Value::to_int);
            let counts = write
                .comment_counts(post.try_into().unwrap_or(u64::MAX))
                .await?;
            let view = var("comment_status").map_or(b"all".to_vec(), Value::to_php_string);
            if let Some(count) = count_of(&counts, &view) {
                total = count.cast_signed();
            }
        }
        // `(int) ceil( $total / $per_page )`, in floats as PHP divides.
        let pages = (total as f64 / per_page as f64).ceil() as i64;
        let items = if total == 1 { "item" } else { "items" };
        supplemental.extend([
            (
                "total_items_i18n",
                format!("{} {items}", number_format(total)),
            ),
            ("total_pages", pages.to_string()),
            ("total_pages_i18n", number_format(pages)),
            ("total", total.to_string()),
        ]);
    }
    let [approved, moderated, ..] = write.comment_counts(0).await?;
    supplemental.push(("time", asked.now.to_string()));
    let mut counts = counts_text(approved, moderated);
    if paged {
        // The list's total stands for the comments' count.
        counts.remove(1);
    } else {
        let link = match Value::String(status.into_bytes()).to_int() {
            1 => read(|| site.comment_url(id)),
            _ => String::new(),
        };
        counts.push(("comment_link", link));
    }
    supplemental.extend(counts);
    let data = "<response_data><![CDATA[]]></response_data>";
    Ok(Ajax::Xml(ajax_response(
        asked,
        ("comment", id, "1"),
        data,
        &supplemental,
    )))
}

/// `wp_count_comments()`'s count named `view`, if it has one.
pub(super) fn count_of(counts: &[u64; 5], view: &[u8]) -> Option<u64> {
    let [approved, moderated, spam, trash, post_trashed] = *counts;
    Some(match view {
        b"approved" => approved,
        b"moderated" => moderated,
        b"spam" => spam,
        b"trash" => trash,
        b"post-trashed" => post_trashed,
        b"all" => approved + moderated,
        b"total_comments" => approved + moderated + spam,
        _ => return None,
    })
}

/// `WP_Ajax_Response::send()` of one comment response: `response` as the
/// data, then `supplemental`.
fn ajax_response(
    asked: &Asked,
    (what, id, position): (&str, u64, &str),
    response: &str,
    supplemental: &[(&str, String)],
) -> String {
    let action = asked
        .post("action")
        .map(Value::to_php_string)
        .unwrap_or_default();
    let supplemental: String = supplemental
        .iter()
        .map(|(name, value)| format!("<{name}><![CDATA[{value}]]></{name}>"))
        .collect();
    format!(
        "<?xml version='1.0' encoding='UTF-8' standalone='yes'?><wp_ajax><response action='{}_{id}'><{what} id='{id}' position='{position}'>{response}<supplemental>{supplemental}</supplemental></{what}></response></wp_ajax>",
        text(&action)
    )
}

/// `comment.php`'s `editedcomment`, the edit screen's Update: back to the
/// screen it came from, else the post's comments, at the comment.
pub(super) fn edited<'a>(
    write: &'a mut Write<'_>,
    asked: &'a Asked<'a>,
    site: &'a dyn Acting,
) -> Saving<'a> {
    Box::pin(async move {
        let referred = match asked.post("referredby") {
            Some(Value::Array(_)) => return Err(Error::Unported("an array as referredby").into()),
            referred => referred.filter(|r| !r.is_empty()).map(Value::to_php_string),
        };
        if let Err(message) = edit::edit_comment(write, asked, site, None).await? {
            return Err(Stop::Died(Answer::Died(message)));
        }
        let id = asked.post("comment_ID").map_or(0, absint);
        let post = asked.post("comment_post_ID").map_or(0, absint);
        let list = hyscribe_admin::menu::link(&format!("edit-comments.php?p={post}"));
        let back = referred.unwrap_or_else(|| list.into_bytes());
        Ok(Answer::Redirect(format!("{}#comment-{id}", text(&back))))
    })
}

/// A comment's row sent once committed, as `WP_Ajax_Response` adds it.
#[derive(Debug, PartialEq)]
pub(in crate::admin) struct Row {
    /// The response's `what`.
    what: &'static str,
    /// The comment.
    id: u64,
    /// `position`, sanitized.
    position: String,
    /// The `supplemental` fields.
    supplemental: Vec<(&'static str, String)>,
    /// The dashboard's Activity row rather than the list's.
    dashboard: bool,
}

impl Row {
    /// The XML with the comments list's row of the comment as it is now.
    pub(in crate::admin) fn sent(&self, admin: &Admin, asked: &Asked) -> String {
        let post = asked.request("p").map_or(0, absint);
        let row = list_row(admin, self.id, post, self.dashboard);
        let data = format!("<response_data><![CDATA[{row}]]></response_data>");
        let response = (self.what, self.id, self.position.as_str());
        ajax_response(asked, response, &data, &self.supplemental)
    }
}

/// `position`: the `(int)` of the request's, else `-1`, as
/// `WP_Ajax_Response::add()` keeps it.
fn position(asked: &Asked) -> String {
    match post_int(asked, "position") {
        0 => "-1".into(),
        n => n.to_string(),
    }
}

/// The list's Quick Edit (`wp_ajax_edit_comment()`): the comment saved,
/// then its row.
pub(super) fn quick_edit<'a>(
    write: &'a mut Write<'_>,
    asked: &'a Asked<'a>,
    site: &'a dyn Acting,
) -> Saving<'a, Ajax> {
    Box::pin(async move {
        if asked.post("content").and_then(Value::as_bytes) == Some(b"") {
            return Ok(Ajax::Text("Please type your comment text.".into()));
        }
        // The post screen's list (`WP_Post_Comments_List_Table`) is not ported.
        if asked.post("checkbox").and_then(Value::as_bytes) != Some(b"1") {
            return Err(Error::Unported("the post screen's comments").into());
        }
        let status = match asked.post("status") {
            Some(Value::Array(_)) => return Err(Error::Unported("an array as status").into()),
            status => status.map(Value::to_php_string),
        };
        if let Err(message) = edit::edit_comment(write, asked, site, status).await? {
            return Err(Stop::Died(Ajax::Text(message.into())));
        }
        let id = post_int(asked, "comment_ID").unsigned_abs();
        Ok(Ajax::CommentRow(Row {
            what: "edit_comment",
            id,
            position: position(asked),
            supplemental: Vec::new(),
            dashboard: false,
        }))
    })
}

/// The comments list's row of the comment `id` as it is now, or the
/// dashboard's.
fn list_row(admin: &Admin, id: u64, post: u64, dashboard: bool) -> String {
    use hyscribe_admin::comments::CommentEditSite;
    // The posts the checks read are older than the action.
    admin.posts.borrow_mut().clear();
    admin
        .comment_to_edit(id)
        .map_or_else(String::new, |row| match dashboard {
            true => hyscribe_admin::dashboard::ajax_comment_row(admin, &row),
            false => hyscribe_admin::comments::ajax_row(admin, &row, post),
        })
}

/// `trim()` of the `$_POST` field `name`, empty when not sent; `None` for
/// an array, PHP's fatal error.
fn trimmed(asked: &Asked, name: &str) -> Option<Vec<u8>> {
    match asked.post(name) {
        Some(Value::Array(_)) => None,
        given => Some(php_trim(&given.map(Value::to_php_string).unwrap_or_default()).to_vec()),
    }
}

/// The list's and the dashboard's Reply (`wp_ajax_replyto_comment()`): a
/// comment by the user on the post, approving a held parent if asked,
/// then its row and the counts.
pub(super) fn reply<'a>(
    write: &'a mut Write<'_>,
    asked: &'a Asked<'a>,
    site: &'a dyn Acting,
) -> Saving<'a, Ajax> {
    Box::pin(async move {
        let post_id = post_int(asked, "comment_post_ID").unsigned_abs();
        // The checks' answer, for a post deleted since.
        let Some(post) = write.post(post_id).await? else {
            return Ok(Ajax::Text("-1".into()));
        };
        match post.status.as_str() {
            "" => return Ok(Ajax::Text("1".into())),
            "draft" | "pending" | "trash" => {
                let message = "You cannot reply to a comment on a draft post.";
                return Ok(Ajax::Text(message.into()));
            }
            _ => {}
        }
        let mode = asked.request("mode").and_then(Value::as_bytes);
        // The post screen's list (`WP_Post_Comments_List_Table`) is not ported.
        if mode == Some(b"single") {
            return Err(Error::Unported("the post screen's comments").into());
        }
        let Some((_, user)) = write.user(asked.user).await? else {
            return Ok(Ajax::Text(
                "Sorry, you must be logged in to reply to a comment.".into(),
            ));
        };
        let (Some(content), Some(kind)) =
            (trimmed(asked, "content"), trimmed(asked, "comment_type"))
        else {
            return Err(Error::Unported("an array to trim").into());
        };
        if content.is_empty() {
            return Ok(Ajax::Text("Please type your comment text.".into()));
        }
        let kind = asked
            .post("comment_type")
            .map_or(b"comment".to_vec(), |_| kind);
        let parent = asked.post("comment_ID").map_or(0, absint);
        // A held parent on the post approved with the reply.
        let mut approved_parent = None;
        if asked.post("approve_parent").is_some_and(|a| !a.is_empty())
            && let Some(found) = write.comment(parent).await?
            && found.approved == "0"
            && found.post_id == post_id
        {
            if !can_edit(site, &Value::Int(parent.cast_signed())) {
                return Ok(Ajax::Text("-1".into()));
            }
            let approved = set_comment_status(write, parent, "approve").await?;
            approved_parent = approved.then_some(found.post_id);
        }
        let comment = NewComment {
            post_id,
            author: user.display_name,
            author_email: user.email,
            author_url: user.url,
            author_ip: asked.ip.to_vec(),
            content,
            agent: asked.agent.to_vec(),
            comment_type: text(&kind),
            parent,
            user_id: asked.user,
            ..NewComment::default()
        };
        // Unfiltered for a user who may post unfiltered HTML only with the
        // form's own nonce, PHP's `wp_create_nonce()` compared as it is now.
        let given = asked.post("_wp_unfiltered_html_comment");
        let given = given.map(Value::to_php_string).unwrap_or_default();
        let commenter = read(|| {
            let own = site.create_nonce("unfiltered-html-comment").into_bytes() == given;
            Commenter {
                moderate_comments: site.current_user_can("moderate_comments", &[]),
                manage_options: site.current_user_can("manage_options", &[]),
                unfiltered_html: site.current_user_can("unfiltered_html", &[]) & own,
            }
        });
        let context = Context {
            now: asked.now,
            home: asked.home.as_bytes(),
        };
        // `wp_die()` of the refusal's message, without its status.
        let id = new_comment(write, context, comment, commenter).await?;
        let id = id.map_err(|refused| {
            let (_, message) = crate::comment_post::submit::refusal(refused);
            Stop::Died(Ajax::Text(message.into()))
        })?;
        let [approved, moderated, ..] = write.comment_counts(0).await?;
        let mut supplemental = counts_text(approved, moderated);
        if let Some(parent_post) = approved_parent {
            supplemental.push(("parent_approved", parent.to_string()));
            supplemental.push(("parent_post_id", parent_post.to_string()));
        }
        Ok(Ajax::CommentRow(Row {
            what: "comment",
            id,
            position: position(asked),
            supplemental,
            dashboard: mode == Some(b"dashboard"),
        }))
    })
}

/// The supplemental counts of a reply: `in_moderation` and their texts.
fn counts_text(approved: u64, moderated: u64) -> Vec<(&'static str, String)> {
    let comments = if approved == 1 { "Comment" } else { "Comments" };
    let pending = if moderated == 1 {
        "Comment"
    } else {
        "Comments"
    };
    vec![
        ("in_moderation", moderated.to_string()),
        (
            "i18n_comments_text",
            format!("{} {comments}", number_format(approved)),
        ),
        (
            "i18n_moderation_text",
            format!("{} {pending} in moderation", number_format(moderated)),
        ),
    ]
}

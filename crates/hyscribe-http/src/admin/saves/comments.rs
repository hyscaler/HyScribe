// SPDX-License-Identifier: AGPL-3.0-or-later

//! The comments' moderation: `edit-comments.php`'s bulk actions,
//! `comment.php`'s row actions and the list's Ajax actions, from
//! `wp-admin/includes/ajax-actions.php`. WordPress mails nothing here.

use hyscribe_admin::actions::{Asked, action, post_int};
use hyscribe_admin::posts::Answer;
use hyscribe_db::{DbError, Write};
use hyscribe_kses::esc_url_raw;
use hyscribe_phpser::{Value, absint, add_query_arg, parse_str, remove_query_arg};
use hyscribe_render::number_format;
use hyscribe_write::Error;
use hyscribe_write::comments::{
    Context, delete_comment, restore_comment, set_comment_status, spam_comment, trash_comment,
};

use super::{Acting, Ajax, Handler, Saving, read, referer, text};

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
    Some(match approved {
        "1" => "approved",
        "0" => "unapproved",
        "spam" => "spam",
        "trash" => "trash",
        _ => return None,
    })
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
            return Ok(ajax_response(asked, "0", &error, &[]));
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
    let counts = write.comment_counts(0).await?;
    let [approved, moderated, ..] = counts;
    supplemental.push(("time", asked.now.to_string()));
    supplemental.push(("in_moderation", moderated.to_string()));
    let comments = if approved == 1 { "Comment" } else { "Comments" };
    let pending = if moderated == 1 {
        "Comment"
    } else {
        "Comments"
    };
    let moderation = format!("{} {pending} in moderation", number_format(moderated));
    if !paged {
        let link = match Value::String(status.into_bytes()).to_int() {
            1 => read(|| site.comment_url(id)),
            _ => String::new(),
        };
        supplemental.push((
            "i18n_comments_text",
            format!("{} {comments}", number_format(approved)),
        ));
        supplemental.push(("i18n_moderation_text", moderation));
        supplemental.push(("comment_link", link));
    } else {
        supplemental.push(("i18n_moderation_text", moderation));
    }
    let data = "<response_data><![CDATA[]]></response_data>";
    Ok(ajax_response(asked, &id.to_string(), data, &supplemental))
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
fn ajax_response(asked: &Asked, id: &str, response: &str, supplemental: &[(&str, String)]) -> Ajax {
    let action = asked
        .post("action")
        .map(Value::to_php_string)
        .unwrap_or_default();
    let supplemental: String = supplemental
        .iter()
        .map(|(name, value)| format!("<{name}><![CDATA[{value}]]></{name}>"))
        .collect();
    Ajax::Xml(format!(
        "<?xml version='1.0' encoding='UTF-8' standalone='yes'?><wp_ajax><response action='{}_{id}'><comment id='{id}' position='1'>{response}<supplemental>{supplemental}</supplemental></comment></response></wp_ajax>",
        text(&action)
    ))
}

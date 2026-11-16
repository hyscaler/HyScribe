// SPDX-License-Identifier: AGPL-3.0-or-later

//! `edit_comment()` from `wp-admin/includes/comment.php`: the comment of
//! `$_POST['comment_ID']` saved from the edit screen or Quick Edit.

use hyscribe_admin::actions::Asked;
use hyscribe_db::Write;
use hyscribe_phpser::Value;
use hyscribe_write::Error;
use hyscribe_write::comments::{CommentEdit, Context, update_comment};

use super::super::{Acting, read, text};
use super::comment_status;

/// The `$_POST` field `name`, a string; an array PHP's filters cannot take.
fn field(asked: &Asked, name: &str) -> Result<Option<Vec<u8>>, Error> {
    match asked.post(name) {
        Some(Value::Array(_)) => Err(Error::Unported("an array as a comment's field")),
        given => Ok(given.map(Value::to_php_string)),
    }
}

/// The field `name`, else `old`, as `edit_comment()` copies `newcomment_*`.
fn renamed(asked: &Asked, new: &str, old: &str) -> Result<Option<Vec<u8>>, Error> {
    Ok(field(asked, new)?.or(field(asked, old)?))
}

/// A number column as given; text MySQL would turn into a number is not
/// ported.
fn number(asked: &Asked, name: &str) -> Result<Option<i64>, Error> {
    let Some(given) = field(asked, name)? else {
        return Ok(None);
    };
    let text = String::from_utf8_lossy(&given);
    let parsed = text.parse::<i64>().ok().filter(|n| n.to_string() == text);
    parsed
        .map(Some)
        .ok_or(Error::Unported("a number column as text"))
}

fn unsigned(asked: &Asked, name: &str) -> Result<Option<u64>, Error> {
    let given = number(asked, name)?;
    given
        .map(|n| u64::try_from(n).map_err(|_| Error::Unported("a negative ID")))
        .transpose()
}

/// `edit_comment()` with `wp_update_comment( $_POST, true )`: `Err` with
/// the `WP_Error`'s message PHP dies with, as the screen or Ajax sends it.
pub(super) async fn edit_comment(
    write: &mut Write<'_>,
    asked: &Asked<'_>,
    site: &dyn Acting,
    status: Option<Vec<u8>>,
) -> Result<Result<(), &'static str>, Error> {
    if asked.post("comment_meta").is_some() {
        return Err(Error::Unported("comment meta from the request"));
    }
    let id = asked.post("comment_ID").map_or(0, Value::to_int);
    // `$_POST['status']` from Quick Edit, else the edit screen's radio.
    let approved = match status {
        Some(status) => Some(status),
        None => renamed(asked, "comment_status", "comment_approved")?,
    };
    let comment = match u64::try_from(id) {
        Ok(id) => write.comment(id).await?,
        Err(_) => None,
    };
    let parent = asked.post("comment_parent").map(Value::to_int);
    if let (Some(parent), Some(found)) = (parent, &comment)
        && let Some(invalid) = parent_error(write, (id, found), parent, approved.as_deref()).await?
    {
        return Ok(Err(invalid));
    }
    let Some(comment) = comment else {
        return Ok(Err("Invalid comment ID."));
    };
    let id = id.unsigned_abs();
    let text = |bytes: Vec<u8>| text(&bytes);
    let edit = CommentEdit {
        id,
        post_id: unsigned(asked, "comment_post_ID")?,
        author: renamed(asked, "newcomment_author", "comment_author")?,
        author_email: renamed(asked, "newcomment_author_email", "comment_author_email")?,
        author_url: renamed(asked, "newcomment_author_url", "comment_author_url")?,
        author_ip: field(asked, "comment_author_IP")?,
        date: edited_date(asked)?.map_or_else(
            || field(asked, "comment_date").map(|d| d.map(text)),
            |d| Ok(Some(d)),
        )?,
        content: renamed(asked, "content", "comment_content")?,
        karma: number(asked, "comment_karma")?,
        approved: approved.map(text),
        agent: field(asked, "comment_agent")?,
        comment_type: field(asked, "comment_type")?.map(text),
        parent: parent.map(i64::unsigned_abs),
        user_id: unsigned(asked, "user_id")?,
    };
    let context = Context {
        now: asked.now,
        home: asked.home.as_bytes(),
        empty_trash_days: site.constants().empty_trash_days,
    };
    // Through kses unless both the user and the comment's author may post
    // unfiltered HTML.
    let kses = read(|| {
        let theirs = site.user_can(comment.user_id, "unfiltered_html");
        !(site.current_user_can("unfiltered_html", &[]) & theirs)
    });
    match update_comment(write, context, &edit, kses).await? {
        Some(_) => Ok(Ok(())),
        None => Ok(Err("Invalid post ID.")),
    }
}

/// `edit_comment()`'s date from the edit screen's fields once one differs
/// from its hidden copy. PHP clamps a day, hour, minute or second out of
/// range, then MySQL refuses or reads what is not a whole valid date: such
/// a date is not ported.
fn edited_date(asked: &Asked) -> Result<Option<String>, Error> {
    let units = ["aa", "mm", "jj", "hh", "mn"];
    let changed = units.iter().any(|unit| {
        let hidden = asked.post(&format!("hidden_{unit}"));
        hidden.is_some_and(|h| !h.is_empty()) && hidden != asked.post(unit)
    });
    let edit = asked.post("edit_date").is_some_and(|e| !e.is_empty());
    if !changed && !edit {
        return Ok(None);
    }
    let fields = [
        ("aa", 4),
        ("mm", 2),
        ("jj", 2),
        ("hh", 2),
        ("mn", 2),
        ("ss", 2),
    ];
    let mut parts = [0; 6];
    for (part, (name, width)) in parts.iter_mut().zip(fields) {
        let given = field(asked, name)?.unwrap_or_default();
        let number = (given.len() == width && given.iter().all(u8::is_ascii_digit))
            .then(|| String::from_utf8_lossy(&given).parse::<u32>().ok())
            .flatten();
        *part = number.ok_or(Error::Unported("a date part MySQL would not read"))?;
    }
    let [year, month, day, hour, minute, second] = parts;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    let valid = (1..=12).contains(&month)
        && (1..=days).contains(&day)
        && hour < 24
        && minute < 60
        && second < 60;
    match valid {
        true => Ok(Some(format!(
            "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}"
        ))),
        false => Err(Error::Unported("a date out of range")),
    }
}

/// `edit_comment()`'s checks of a new parent: `None` when it may be the
/// comment's parent, else the error's message.
async fn parent_error(
    write: &mut Write<'_>,
    (id, comment): (i64, &hyscribe_db::CommentFields),
    parent: i64,
    approved: Option<&[u8]>,
) -> Result<Option<&'static str>, Error> {
    if parent == 0 || i64::try_from(comment.parent) == Ok(parent) {
        return Ok(None);
    }
    let threaded = write.option_value("thread_comments").await?;
    if threaded.is_none_or(|t| Value::String(t).is_empty()) {
        return Ok(Some(
            "The comment parent cannot be changed because threaded comments are disabled.",
        ));
    }
    if parent == id {
        return Ok(Some("A comment cannot be a reply to itself."));
    }
    let found = match u64::try_from(parent) {
        Ok(parent) => write.comment(parent).await?,
        Err(_) => None,
    };
    let invalid = Ok(Some("Invalid parent comment."));
    let Some(found) = found else {
        return invalid;
    };
    let status = comment_status(&found.approved);
    let approved = approved.map_or(comment.approved.as_bytes(), |a| a);
    if found.post_id != comment.post_id
        || found.comment_type != comment.comment_type
        || matches!(status, Some("spam" | "trash"))
        || matches!(approved, b"1" | b"approve") && status != Some("approved")
    {
        return invalid;
    }
    // The new parent's ancestors, which must not include the comment.
    let mut depth = 1;
    let mut seen = Vec::new();
    let mut ancestor = found;
    let mut at = parent.unsigned_abs();
    while ancestor.parent != 0 && !seen.contains(&at) {
        if i64::try_from(ancestor.parent) == Ok(id) {
            return Ok(Some(
                "A comment cannot be a reply to one of its own replies.",
            ));
        }
        seen.push(at);
        depth += 1;
        at = ancestor.parent;
        let Some(next) = write.comment(at).await? else {
            break;
        };
        ancestor = next;
    }
    let most = write.option_value("thread_comments_depth").await?;
    let most = Value::String(most.unwrap_or_default()).to_int();
    if most != 0 {
        // The comment's replies move with it: the subtree's height, one
        // level at a time while it can still fit.
        let (mut height, mut level) = (1, vec![id.unsigned_abs()]);
        while !level.is_empty() && depth + height <= most {
            level = write.comment_child_ids(&level).await?;
            if !level.is_empty() {
                height += 1;
            }
        }
        if depth + height > most {
            return Ok(Some(
                "The comment cannot be moved there because it or its replies would exceed the maximum threading depth.",
            ));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests;

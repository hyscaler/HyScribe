// SPDX-License-Identifier: AGPL-3.0-or-later

//! The media library's writes (MVP 6.4): the grid's and the media modal's
//! `save-attachment`, `save-attachment-compat` and `delete-post`, and
//! `upload.php`'s bulk actions. An attachment's files are deleted once its
//! rows are committed.

use hyscribe_admin::actions::{Asked, media_action};
use hyscribe_admin::posts::Answer;
use hyscribe_db::{Meta, Write};
use hyscribe_phpser::{Value, absint, add_query_arg, addslashes, remove_query_arg, stripslashes};
use hyscribe_render::formatting::sanitize_text_field;
use hyscribe_write::Error;
use hyscribe_write::caps::Caps;
use hyscribe_write::options::{delete_metadata, get_metadata, update_metadata};
use hyscribe_write::posts::{
    AttachmentDeleted, Context, PostArgs, wp_delete_attachment, wp_delete_post, wp_trash_post,
    wp_untrash_post, wp_update_post,
};

use super::posts::{context, mentions};
use super::{Acting, Ajax, Saving, Stop, referer, text};

/// The attachment the request names, which its checks found.
async fn existing(write: &mut Write<'_>, id: u64) -> Result<hyscribe_db::PostFields, Error> {
    let post = write.post(id).await?;
    post.ok_or(Error::Unported("a post gone since its checks"))
}

/// The action's context for attachments.
async fn attachments_context(
    write: &mut Write<'_>,
    asked: &Asked<'_>,
    site: &dyn Acting,
) -> Result<Context, Error> {
    let caps = Caps::load(write, asked.user, None, site.constants()).await?;
    Ok(context(asked, site, (&caps, "attachment")))
}

/// `wp_delete_attachment( $id, $force )`, its files deleted once the rows
/// are: whether it was trashed or deleted.
pub(super) async fn delete_attachment(
    write: &mut Write<'_>,
    (ctx, site): (&Context, &dyn Acting),
    id: u64,
    force: bool,
) -> Result<bool, Error> {
    let deleted = wp_delete_attachment(write, ctx, id, force).await?;
    if let Some(AttachmentDeleted::Deleted(files)) = &deleted
        && !site.delete_files(files.clone())
    {
        return Err(Error::Unported("deleting files without an uploads folder"));
    }
    Ok(deleted.is_some())
}

/// `wp_update_post( $post )` of `get_post( $id, ARRAY_A )`, which
/// `wp_insert_post()` unslashes as if it had been slashed, with `change`.
async fn update_attachment(
    write: &mut Write<'_>,
    ctx: &Context,
    id: u64,
    change: &dyn Fn(&mut PostArgs),
) -> Result<(), Error> {
    wp_update_post(write, ctx, id, &|args| {
        for field in [
            &mut args.title,
            &mut args.content,
            &mut args.excerpt,
            &mut args.content_filtered,
            &mut args.password,
            &mut args.to_ping,
            &mut args.pinged,
        ] {
            *field = stripslashes(field);
        }
        change(args);
    })
    .await?;
    Ok(())
}

/// A change of `changes` as text, `None` when not sent; a list is not
/// ported.
fn change(changes: Option<&Value>, key: &str) -> Result<Option<Vec<u8>>, Error> {
    match changes.and_then(|changes| changes.get_str(key)) {
        None => Ok(None),
        Some(Value::Array(_)) => Err(Error::Unported("a list of an attachment's field")),
        Some(value) => Ok(Some(value.to_php_string())),
    }
}

/// `wp_attachment_is( 'audio', $post )`: a song by its type, with a file.
async fn is_audio(
    write: &mut Write<'_>,
    (id, post): (u64, &hyscribe_db::PostFields),
) -> Result<bool, Error> {
    let file = write
        .meta_value(Meta::Post, id, "_wp_attached_file")
        .await?;
    if post.mime_type == "import" {
        return Err(Error::Unported("an imported attachment's type"));
    }
    Ok(file.is_some_and(|file| !file.is_empty()) && post.mime_type.starts_with("audio/"))
}

/// The song's artist and album of `changes`, as `wp_update_attachment_metadata()`
/// stores them.
async fn save_tags(write: &mut Write<'_>, id: u64, changes: Option<&Value>) -> Result<(), Error> {
    let stored = get_metadata(write, Meta::Post, id, "_wp_attachment_metadata").await?;
    let stored = stored.into_iter().next();
    let (mut changed, mut data) = match stored {
        Some(Value::Array(data)) => (false, data),
        _ => (true, Vec::new()),
    };
    for key in ["artist", "album"] {
        let Some(tag) = change(changes, key)? else {
            continue;
        };
        changed = true;
        let tag = Value::String(sanitize_text_field(&tag));
        let key = hyscribe_phpser::Key::String(key.into());
        match data.iter_mut().find(|(existing, _)| *existing == key) {
            Some((_, existing)) => *existing = tag,
            None => data.push((key, tag)),
        }
    }
    if !changed {
        return Ok(());
    }
    let key = "_wp_attachment_metadata";
    if data.is_empty() {
        let any = Value::String(Vec::new());
        delete_metadata(write, Meta::Post, id, key, &any, false).await?;
    } else {
        let any = Value::String(Vec::new());
        update_metadata(write, Meta::Post, id, key, &Value::Array(data), &any).await?;
    }
    Ok(())
}

/// `wp_ajax_save_attachment()`, past its checks.
pub(super) fn save_attachment<'a>(
    write: &'a mut Write<'_>,
    asked: &'a Asked<'a>,
    site: &'a dyn Acting,
) -> Saving<'a, Ajax> {
    Box::pin(async move {
        let id = asked.request("id").map_or(0, absint);
        let post = existing(write, id).await?;
        let changes = asked.request("changes");
        let ctx = attachments_context(write, asked, site).await?;
        if let Some(alt) = change(changes, "alt")? {
            let key = "_wp_attachment_image_alt";
            let stored = get_metadata(write, Meta::Post, id, key).await?;
            if stored.first().and_then(Value::as_bytes).unwrap_or_default() != alt {
                let alt = hyscribe_kses::wp_strip_all_tags(&alt, true);
                let alt = Value::String(addslashes(&alt));
                let any = Value::String(Vec::new());
                update_metadata(write, Meta::Post, id, key, &alt, &any).await?;
            }
        }
        if is_audio(write, (id, &post)).await? {
            save_tags(write, id, changes).await?;
        }
        let media_trash = site.constants().media_trash;
        let status = change(changes, "status")?.filter(|_| media_trash);
        if status.as_deref() == Some(b"trash") {
            delete_attachment(write, (&ctx, site), id, false).await?;
            return Ok(Ajax::Json("{\"success\":true}".into()));
        }
        let parent = change(changes, "parent")?.map(|parent| Value::String(parent).to_int());
        let parent = parent.map(u64::try_from).transpose();
        let parent = parent.map_err(|_| Error::Unported("a negative parent"))?;
        let texts = ["title", "caption", "description"].map(|key| change(changes, key));
        let [title, caption, description] = texts;
        let (title, caption, description) = (title?, caption?, description?);
        update_attachment(write, &ctx, id, &|args| {
            args.parent = parent.unwrap_or(args.parent);
            for (field, given) in [
                (&mut args.title, &title),
                (&mut args.excerpt, &caption),
                (&mut args.content, &description),
            ] {
                if let Some(given) = given {
                    field.clone_from(given);
                }
            }
            if let Some(status) = &status {
                args.status = String::from_utf8_lossy(status).into_owned();
            }
        })
        .await?;
        Ok(Ajax::Json("{\"success\":true}".into()))
    })
}

/// `wp_ajax_save_attachment_compat()`, past its checks: the attachment
/// saved as it is, with no fields of a plugin's to take.
pub(super) fn save_attachment_compat<'a>(
    write: &'a mut Write<'_>,
    asked: &'a Asked<'a>,
    site: &'a dyn Acting,
) -> Saving<'a, Ajax> {
    Box::pin(async move {
        let id = asked.request("id").map_or(0, absint);
        let ctx = attachments_context(write, asked, site).await?;
        update_attachment(write, &ctx, id, &|_| {}).await?;
        Ok(Ajax::Attachment(id))
    })
}

/// `wp_ajax_delete_post()`, past its checks: `1` when the post is deleted,
/// or trashed, else `0`.
pub(super) fn delete_post<'a>(
    write: &'a mut Write<'_>,
    asked: &'a Asked<'a>,
    site: &'a dyn Acting,
) -> Saving<'a, Ajax> {
    Box::pin(async move {
        let id = asked.post("id").map_or(0, Value::to_int).unsigned_abs();
        let post = existing(write, id).await?;
        let caps = Caps::load(write, asked.user, None, site.constants()).await?;
        let ctx = context(asked, site, (&caps, &post.post_type));
        let done = match post.post_type.as_str() {
            "attachment" => delete_attachment(write, (&ctx, site), id, false).await?,
            _ => wp_delete_post(write, &ctx, id, false).await?,
        };
        Ok(Ajax::Text(if done { "1" } else { "0" }.into()))
    })
}

/// `upload.php`'s IDs of the request: the Trash's, `media` or `ids`.
async fn bulk_ids(
    write: &mut Write<'_>,
    asked: &Asked<'_>,
    doaction: &[u8],
) -> Result<Vec<i64>, Stop> {
    if doaction == b"delete_all" {
        let ids = write.status_post_ids("attachment", "trash").await?;
        return Ok(ids.into_iter().map(u64::cast_signed).collect());
    }
    let ids = match (asked.request("media"), asked.request("ids")) {
        (Some(media), _) => hyscribe_admin::actions::items(media),
        (None, Some(Value::String(ids))) => ids
            .split(|&b| b == b',')
            .map(|id| Value::String(id.to_vec()))
            .collect(),
        (None, None) => Vec::new(),
        _ => return Err(Error::Unported("IDs PHP cannot read").into()),
    };
    Ok(ids.iter().map(Value::to_int).collect())
}

/// `upload.php`'s bulk actions in list mode.
pub(super) fn bulk<'a>(
    write: &'a mut Write<'_>,
    asked: &'a Asked<'a>,
    site: &'a dyn Acting,
) -> Saving<'a> {
    Box::pin(async move {
        let doaction = media_action(asked).map(|action| action.to_php_string());
        let doaction = doaction.unwrap_or_default();
        let ids = bulk_ids(write, asked, &doaction).await?;
        let mut location = b"upload.php".to_vec();
        if let Some(referer) = referer(asked).filter(|referer| mentions(referer, "upload.php")) {
            let notices: [&[u8]; 6] = [
                b"trashed",
                b"untrashed",
                b"deleted",
                b"message",
                b"ids",
                b"posted",
            ];
            location = remove_query_arg(&notices, &referer);
        }
        let ctx = attachments_context(write, asked, site).await?;
        let (denied, failed, done): (&str, &str, &[u8]) = match &doaction[..] {
            b"attach" | b"detach" => return Err(Error::Unported("attaching media").into()),
            _ if ids.is_empty() => return Ok(Answer::Redirect(text(&location))),
            b"trash" => (
                "Sorry, you are not allowed to move this item to the Trash.",
                "Error in moving the item to Trash.",
                b"trashed",
            ),
            b"untrash" => (
                "Sorry, you are not allowed to restore this item from the Trash.",
                "Error in restoring the item from Trash.",
                b"untrashed",
            ),
            b"delete" | b"delete_all" => (
                "Sorry, you are not allowed to delete this item.",
                "Error in deleting the attachment.",
                b"deleted",
            ),
            // `handle_bulk_actions-upload`, which nothing hooks.
            _ => return Ok(Answer::Redirect(text(&location))),
        };
        for &id in &ids {
            let caps = Caps::load(write, asked.user, u64::try_from(id).ok(), ctx.constants).await?;
            if !caps.can("delete_post", &[Value::Int(id)]) {
                return Err(Stop::Died(Answer::Died(denied)));
            }
            let id = id.unsigned_abs();
            let written = match done {
                b"trashed" => wp_trash_post(write, &ctx, id).await?,
                b"untrashed" => wp_untrash_post(write, &ctx, id, false).await?,
                _ => delete_attachment(write, (&ctx, site), id, false).await?,
            };
            if !written {
                return Err(Stop::Died(Answer::Died(failed)));
            }
        }
        let count = ids.len().to_string();
        let mut args: Vec<(&[u8], Option<&[u8]>)> = vec![(done, Some(count.as_bytes()))];
        let listed: Vec<Vec<u8>> = ids.iter().map(|id| id.to_string().into_bytes()).collect();
        let listed = listed.join(&b","[..]);
        if done == b"trashed" {
            args.push((b"ids", Some(&listed)));
        }
        Ok(Answer::Redirect(text(&add_query_arg(&args, &location))))
    })
}

#[cfg(test)]
mod tests;

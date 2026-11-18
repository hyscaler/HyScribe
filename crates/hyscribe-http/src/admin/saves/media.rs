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

use super::posts::{Data, context, mentions};
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

/// `wp_update_attachment_metadata()` of the song's `tags`, sanitized and set
/// in the metadata stored; `always` for a caller that updates it even when
/// no tag is given.
async fn save_tags(
    write: &mut Write<'_>,
    id: u64,
    (tags, always): (&[(&str, Vec<u8>)], bool),
) -> Result<(), Error> {
    let stored = get_metadata(write, Meta::Post, id, "_wp_attachment_metadata").await?;
    let stored = stored.into_iter().next();
    let (stored, mut data) = match stored {
        Some(Value::Array(data)) => (true, data),
        _ => (false, Vec::new()),
    };
    if stored && !always && tags.is_empty() {
        return Ok(());
    }
    for (key, tag) in tags {
        let tag = Value::String(sanitize_text_field(tag));
        let key = hyscribe_phpser::Key::String(key.as_bytes().to_vec());
        match data.iter_mut().find(|(existing, _)| *existing == key) {
            Some((_, existing)) => *existing = tag,
            None => data.push((key, tag)),
        }
    }
    let key = "_wp_attachment_metadata";
    let any = Value::String(Vec::new());
    if data.is_empty() {
        delete_metadata(write, Meta::Post, id, key, &any, false).await?;
    } else {
        update_metadata(write, Meta::Post, id, key, &Value::Array(data), &any).await?;
    }
    Ok(())
}

/// `update_post_meta( $id, '_wp_attachment_image_alt', wp_slash( $alt ) )` of
/// the unslashed `alt`, tags stripped, when it is not the one stored.
async fn save_alt(write: &mut Write<'_>, id: u64, alt: &[u8]) -> Result<(), Error> {
    let key = "_wp_attachment_image_alt";
    let stored = get_metadata(write, Meta::Post, id, key).await?;
    if stored.first().and_then(Value::as_bytes).unwrap_or_default() != alt {
        let alt = hyscribe_kses::wp_strip_all_tags(alt, true);
        let alt = Value::String(addslashes(&alt));
        let any = Value::String(Vec::new());
        update_metadata(write, Meta::Post, id, key, &alt, &any).await?;
    }
    Ok(())
}

/// `edit_post()`'s steps for an attachment, from the `data` of the edit
/// screen: a song's or a video's tags, then the image's alternative text.
pub(super) async fn edit_fields(
    write: &mut Write<'_>,
    (id, post): (u64, &hyscribe_db::PostFields),
    data: &Data,
) -> Result<(), Error> {
    if post.mime_type.starts_with("audio/") || post.mime_type.starts_with("video/") {
        let mut tags = Vec::new();
        for key in ["artist", "album"] {
            match data.get(&format!("id3_{key}")) {
                Some(Value::Array(_)) => return Err(Error::Unported("a list of a tag")),
                Some(tag) => tags.push((key, tag.to_php_string())),
                None => {}
            }
        }
        save_tags(write, id, (&tags, true)).await?;
    }
    if let Some(alt) = data.get("_wp_attachment_image_alt") {
        save_alt(write, id, &alt.to_php_string()).await?;
    }
    Ok(())
}

/// `wp_insert_post()`'s featured image of an attachment saved: a song's or
/// video's, removed with `-1`, or set to the image it has already.
async fn save_thumbnail(
    write: &mut Write<'_>,
    (id, post): (u64, &hyscribe_db::PostFields),
    thumbnail: &Value,
) -> Result<(), Error> {
    let key = "_thumbnail_id";
    let file = write
        .meta_value(Meta::Post, id, "_wp_attached_file")
        .await?;
    let played = post.mime_type.starts_with("audio/") || post.mime_type.starts_with("video/");
    if !played || file.is_none_or(|file| file.is_empty()) {
        return Err(Error::Unported(
            "a featured image of a file but a song or video",
        ));
    }
    let stored = write.meta_value(Meta::Post, id, key).await?;
    let wanted = thumbnail.to_int();
    if wanted == -1 {
        let any = Value::String(Vec::new());
        delete_metadata(write, Meta::Post, id, key, &any, false).await?;
        return Ok(());
    }
    let image = u64::try_from(wanted)
        .ok()
        .filter(|_| stored.as_deref() == Some(wanted.to_string().as_bytes()));
    let image = match image {
        Some(image) => write.post(image).await?,
        None => None,
    };
    match image {
        Some(image) if image.mime_type.starts_with("image/") => Ok(()),
        _ => Err(Error::Unported("a featured image to set")),
    }
}

/// What `wp_update_post()` and `edit_post()` do for an attachment once it
/// is updated: its featured image from `data`, then `_fix_attachment_links( $id )`,
/// which acts on a published, scheduled or private attachment whose
/// description links to attachment pages by ID.
pub(super) async fn edit_done(write: &mut Write<'_>, id: u64, data: &Data) -> Result<(), Error> {
    let post = existing(write, id).await?;
    if let Some(thumbnail) = data.get("_thumbnail_id") {
        save_thumbnail(write, (id, &post), thumbnail).await?;
    }
    let links = post
        .content
        .windows(15)
        .any(|window| window == b"?attachment_id=");
    match ["publish", "future", "private"].contains(&post.status.as_str()) && links {
        true => Err(Error::Unported("links to attachment pages")),
        false => Ok(()),
    }
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
            save_alt(write, id, &alt).await?;
        }
        if is_audio(write, (id, &post)).await? {
            let mut tags = Vec::new();
            for key in ["artist", "album"] {
                tags.extend(change(changes, key)?.map(|tag| (key, tag)));
            }
            save_tags(write, id, (&tags, false)).await?;
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

/// `upload.php` as the bulk actions send the user back: the referer without
/// the `notices` when it is the list's, else the list.
fn upload_location(asked: &Asked<'_>, notices: &[&[u8]]) -> Vec<u8> {
    match referer(asked).filter(|referer| mentions(referer, "upload.php")) {
        Some(referer) => remove_query_arg(notices, &referer),
        None => hyscribe_admin::menu::link("upload.php").into_bytes(),
    }
}

/// `wp_media_attach_action()`: where it sends the user once it attached the
/// media to the post of `found_post_id`, or detached it (`detach`), none when
/// it did nothing.
async fn attach(
    write: &mut Write<'_>,
    asked: &Asked<'_>,
    site: &dyn Acting,
    detach: bool,
) -> Result<Option<String>, Stop> {
    let name = if detach {
        "parent_post_id"
    } else {
        "found_post_id"
    };
    let Some(parent) = asked.request(name).filter(|parent| !parent.is_empty()) else {
        return Ok(None);
    };
    if matches!(parent, Value::Array(_)) {
        return Err(Error::Unported("a list for a parent").into());
    }
    let can = async |write: &mut Write<'_>, id: i64| -> Result<bool, Error> {
        let user = asked.user;
        let caps = Caps::load(write, user, u64::try_from(id).ok(), site.constants()).await?;
        Ok(caps.can("edit_post", &[Value::Int(id)]))
    };
    let parent = parent.to_int();
    if !can(write, parent).await? {
        let message = "Sorry, you are not allowed to edit this post.";
        return Err(Stop::Died(Answer::Died(message)));
    }
    let mut ids = Vec::new();
    let media = asked.request("media");
    for id in media
        .map(hyscribe_admin::actions::items)
        .unwrap_or_default()
    {
        let id = id.to_int();
        if can(write, id).await? {
            ids.push(id.unsigned_abs());
        }
    }
    if ids.is_empty() {
        return Ok(None);
    }
    let parent = if detach { 0 } else { parent.unsigned_abs() };
    let changed = write.set_attachments_parent(&ids, parent).await?;
    let location = upload_location(asked, &[b"attached", b"detach"]);
    let key: &[u8] = if detach { b"detach" } else { b"attached" };
    let changed = changed.to_string();
    let args: [(&[u8], Option<&[u8]>); 1] = [(key, Some(changed.as_bytes()))];
    Ok(Some(text(&add_query_arg(&args, &location))))
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
        let notices: [&[u8]; 6] = [
            b"trashed",
            b"untrashed",
            b"deleted",
            b"message",
            b"ids",
            b"posted",
        ];
        let location = upload_location(asked, &notices);
        let ctx = attachments_context(write, asked, site).await?;
        let (denied, failed, done): (&str, &str, &[u8]) = match &doaction[..] {
            b"attach" | b"detach" => {
                let detach = &doaction[..] == b"detach";
                let attached = attach(write, asked, site, detach).await?;
                return Ok(Answer::Redirect(
                    attached.unwrap_or_else(|| text(&location)),
                ));
            }
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

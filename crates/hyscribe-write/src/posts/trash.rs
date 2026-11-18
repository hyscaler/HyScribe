// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_trash_post()`, `wp_untrash_post()` and `wp_delete_post()`, with the
//! comments, meta, terms and options that go with a post.

use hyscribe_db::Write;
use hyscribe_phpser::{Key, Value, unserialize};

use crate::options::{
    Meta, add_metadata, delete_metadata, delete_object_meta, get_metadata, update_option,
};

use super::{Context, Error, Kind, Result, hooks, site, wp_update_post};
use crate::cron;

const TRASH_STATUS: &str = "_wp_trash_meta_status";
const TRASH_TIME: &str = "_wp_trash_meta_time";
const COMMENTS_STATUS: &str = "_wp_trash_meta_comments_status";

/// `delete_post_meta( $id, $key )`.
async fn delete_post_meta(write: &mut Write<'_>, id: u64, key: &str) -> Result<()> {
    let any = Value::String(Vec::new());
    delete_metadata(write, Meta::Post, id, key, &any, false).await?;
    Ok(())
}

/// The `sticky_posts` option's IDs, `intval()`ed and each once, if an array.
async fn stickies(write: &mut Write<'_>) -> Result<Option<Vec<i64>>> {
    let stored = write
        .option_value("sticky_posts")
        .await?
        .unwrap_or_default();
    let Some(Value::Array(stickies)) = unserialize(&stored) else {
        return Ok(None);
    };
    let mut ids: Vec<i64> = Vec::new();
    for (_, sticky) in stickies {
        let sticky = sticky.to_int();
        if !ids.contains(&sticky) {
            ids.push(sticky);
        }
    }
    Ok(Some(ids))
}

/// Stores `ids` as the `sticky_posts` option.
async fn set_stickies(write: &mut Write<'_>, ids: Vec<i64>) -> Result<()> {
    let list = ids
        .into_iter()
        .zip(0..)
        .map(|(sticky, n)| (Key::Int(n), Value::Int(sticky)))
        .collect();
    update_option(write, "sticky_posts", &Value::Array(list), None).await?;
    Ok(())
}

/// `stick_post( $id )`.
pub async fn stick_post(write: &mut Write<'_>, id: u64) -> Result<()> {
    let mut ids = stickies(write).await?.unwrap_or_default();
    let id = i64::try_from(id).unwrap_or(i64::MAX);
    if ids.contains(&id) {
        return Ok(());
    }
    ids.push(id);
    set_stickies(write, ids).await
}

/// `unstick_post( $id )`.
pub async fn unstick_post(write: &mut Write<'_>, id: u64) -> Result<()> {
    let Some(mut ids) = stickies(write).await? else {
        return Ok(());
    };
    let Some(at) = ids
        .iter()
        .position(|&sticky| u64::try_from(sticky) == Ok(id))
    else {
        return Ok(());
    };
    ids.remove(at);
    set_stickies(write, ids).await
}

/// `_reset_front_page_settings_for_post( $id )`.
async fn reset_front_page(write: &mut Write<'_>, id: u64, kind: Kind) -> Result<()> {
    if kind == Kind::Page {
        if site::option_int(write, "page_on_front").await? == id {
            let posts = Value::String(b"posts".to_vec());
            update_option(write, "show_on_front", &posts, None).await?;
            update_option(write, "page_on_front", &Value::Int(0), None).await?;
        }
        if site::option_int(write, "page_for_posts").await? == id {
            update_option(write, "page_for_posts", &Value::Int(0), None).await?;
        }
    }
    unstick_post(write, id).await
}

/// `wp_trash_post( $id )`: whether the post went to the trash, or with
/// `EMPTY_TRASH_DAYS` at 0 was deleted.
pub async fn wp_trash_post(write: &mut Write<'_>, ctx: &Context, id: u64) -> Result<bool> {
    if ctx.constants.empty_trash_days == 0 {
        return Box::pin(wp_delete_post(write, ctx, id, true)).await;
    }
    let Some(post) = write.post(id).await? else {
        return Ok(false);
    };
    if post.status == "trash" {
        return Ok(false);
    }
    reset_front_page(write, id, Kind::of(&post.post_type)?).await?;
    let status = Value::String(post.status.into_bytes());
    add_metadata(write, Meta::Post, id, TRASH_STATUS, &status, false).await?;
    let time = Value::Int(ctx.now);
    add_metadata(write, Meta::Post, id, TRASH_TIME, &time, false).await?;
    wp_update_post(write, ctx, id, &|args| args.status = "trash".into()).await?;
    // wp_trash_post_comments().
    let comments = write.post_comments(id).await?;
    if !comments.is_empty() {
        let statuses = comments
            .into_iter()
            .map(|(comment, approved)| {
                let comment = i64::try_from(comment).unwrap_or(i64::MAX);
                (Key::Int(comment), Value::String(approved.into_bytes()))
            })
            .collect();
        let statuses = Value::Array(statuses);
        add_metadata(write, Meta::Post, id, COMMENTS_STATUS, &statuses, false).await?;
        write.trash_post_comments(id).await?;
    }
    Ok(true)
}

/// `wp_untrash_post( $id )`: whether the post left the trash, as a draft,
/// or with the status it was trashed from when `previous`, as the
/// trashed notice's Undo asks (`wp_untrash_post_set_previous_status()`).
pub async fn wp_untrash_post(
    write: &mut Write<'_>,
    ctx: &Context,
    id: u64,
    previous: bool,
) -> Result<bool> {
    let Some(post) = write.post(id).await? else {
        return Ok(false);
    };
    if post.status != "trash" {
        return Ok(false);
    }
    let status = if previous {
        let stored = get_metadata(write, Meta::Post, id, TRASH_STATUS).await?;
        let status = stored.first().map(Value::to_php_string);
        String::from_utf8_lossy(&status.unwrap_or_default()).into_owned()
    } else {
        "draft".to_string()
    };
    delete_post_meta(write, id, TRASH_STATUS).await?;
    delete_post_meta(write, id, TRASH_TIME).await?;
    wp_update_post(write, ctx, id, &|args| args.status.clone_from(&status)).await?;
    // wp_untrash_post_comments().
    let stored = get_metadata(write, Meta::Post, id, COMMENTS_STATUS).await?;
    let statuses = stored.into_iter().next().unwrap_or_default();
    if statuses.is_empty() {
        return Ok(true);
    }
    let mut groups: Vec<(Vec<u8>, Vec<u64>)> = Vec::new();
    for (comment, status) in statuses.as_array().unwrap_or_default() {
        let comment = Value::String(comment.to_php_string()).to_int();
        let comment = u64::try_from(comment).unwrap_or(0);
        let status = match status.to_php_string() {
            status if status == b"post-trashed" => b"0".to_vec(),
            status => status,
        };
        match groups.iter_mut().find(|(group, _)| *group == status) {
            Some((_, comments)) => comments.push(comment),
            None => groups.push((status, vec![comment])),
        }
    }
    for (status, comments) in groups {
        let status = String::from_utf8_lossy(&status).into_owned();
        write.set_comments_status(&comments, &status).await?;
    }
    delete_post_meta(write, id, COMMENTS_STATUS).await?;
    Ok(true)
}

/// `wp_delete_post( $id, true )` of one post of `kind`: its revisions,
/// which the caller deletes next.
async fn delete_one(
    write: &mut Write<'_>,
    ctx: &Context,
    (id, kind): (u64, Kind),
) -> Result<Vec<u64>> {
    let post = super::existing(write, id).await?;
    // before_delete_post.
    reset_front_page(write, id, kind).await?;
    if kind == Kind::Page && site::option_int(write, "wp_page_for_privacy_policy").await? == id {
        update_option(write, "wp_page_for_privacy_policy", &Value::Int(0), None).await?;
    }
    delete_post_meta(write, id, TRASH_STATUS).await?;
    delete_post_meta(write, id, TRASH_TIME).await?;
    site::delete_object_terms(write, id, kind.taxonomies()).await?;
    if kind.hierarchical() {
        write
            .reparent_posts(id, &post.post_type, post.parent)
            .await?;
    }
    let revisions = write.child_ids(id, "revision").await?;
    write.reparent_posts(id, "attachment", post.parent).await?;
    let comments = crate::comments::Context {
        now: ctx.now,
        home: &ctx.home,
        empty_trash_days: ctx.constants.empty_trash_days,
    };
    for comment in write.post_comment_ids(id).await? {
        crate::comments::delete_comment(write, comments, comment, true).await?;
    }
    delete_object_meta(write, Meta::Post, id).await?;
    // block_core_calendar_update_has_published_post_on_delete().
    if post.status == "publish" && kind == Kind::Post {
        hooks::update_has_published_posts(write).await?;
    }
    write.delete_post(id).await?;
    cron::wp_clear_scheduled_hook(write, "publish_future_post", id).await?;
    Ok(revisions)
}

/// `wp_delete_post( $id, $force )`: whether the post was deleted, or
/// trashed when not forced.
pub async fn wp_delete_post(
    write: &mut Write<'_>,
    ctx: &Context,
    id: u64,
    force: bool,
) -> Result<bool> {
    let Some(post) = write.post(id).await? else {
        return Ok(false);
    };
    let kind = Kind::of(&post.post_type)?;
    // An attachment's files go with it: see `wp_delete_attachment()`.
    if kind == Kind::Attachment {
        return Err(Error::Unported("an attachment through wp_delete_post()"));
    }
    let to_trash = !force && ctx.constants.empty_trash_days != 0;
    if to_trash && matches!(kind, Kind::Post | Kind::Page) && post.status != "trash" {
        return wp_trash_post(write, ctx, id).await;
    }
    // _wp_delete_post_menu_item() deletes the classic menu items to it.
    let menu_items = write
        .meta_ids_of_key_valued(
            Meta::Post,
            "_menu_item_object_id",
            id.to_string().as_bytes(),
        )
        .await?;
    if !menu_items.is_empty() {
        return Err(Error::Unported("a post in a classic menu"));
    }
    // Revisions go through wp_delete_post() too, after their parent's
    // terms and children; their order changes no row.
    let mut queue = vec![(id, kind)];
    while let Some((next, kind)) = queue.pop() {
        let revisions = delete_one(write, ctx, (next, kind)).await?;
        queue.extend(revisions.into_iter().map(|id| (id, Kind::Revision)));
    }
    Ok(true)
}

/// What `wp_delete_attachment()` did.
#[derive(Debug, PartialEq, Eq)]
pub enum AttachmentDeleted {
    /// It went to the trash (`MEDIA_TRASH`).
    Trashed,
    /// Its rows were deleted. The files `wp_delete_attachment_files()`
    /// deletes once they are, each with the folder it must be within, as
    /// paths under the uploads folder.
    Deleted(Vec<(Vec<u8>, Vec<u8>)>),
}

/// `dirname()` of a relative path: `.` for a bare name.
fn dirname(path: &[u8]) -> Vec<u8> {
    match path.iter().rposition(|&b| b == b'/') {
        Some(at) => path.get(..at).unwrap_or_default().to_vec(),
        None if path.is_empty() => Vec::new(),
        None => b".".to_vec(),
    }
}

/// `path_join( $base, $path )` of a relative `path`.
fn path_join(base: &[u8], path: &[u8]) -> Vec<u8> {
    let base = base.strip_suffix(b"/").unwrap_or(base);
    [base, b"/", path].concat()
}

/// The files of an attachment at `file` with metadata `meta` and backup
/// sizes `backups`, as `wp_delete_attachment_files()` deletes them.
fn attachment_files(file: &[u8], meta: &Value, backups: &Value) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
    // A file stored by its absolute path, or WordPress 2.6's `thumb`.
    if file.first() == Some(&b'/') || meta.get_str("thumb").is_some_and(|t| !t.is_empty()) {
        return Err(Error::Unported("an attachment's file by its absolute path"));
    }
    let name = hyscribe_phpser::wp_basename(file);
    let folder = dirname(file);
    let beside =
        |other: &Value| hyscribe_render::template::replace(file, &name, &other.to_php_string());
    let mut files = Vec::new();
    let sizes = meta.get_str("sizes").and_then(Value::as_array);
    for (_, size) in sizes.unwrap_or_default() {
        let sized = size.get_str("file").cloned().unwrap_or_default();
        files.push((beside(&sized), folder.clone()));
    }
    for key in [
        "original_image",
        "source_image",
        "animated_video",
        "animated_video_poster",
    ] {
        // Only `original_image` is taken when it is not a string.
        let other = meta.get_str(key).filter(|other| {
            !other.is_empty() && (key == "original_image" || other.as_bytes().is_some())
        });
        files.extend(other.map(|other| (beside(other), folder.clone())));
    }
    if let Some(backups) = backups.as_array() {
        let stored = meta.get_str("file").map(Value::to_php_string);
        let folder = dirname(&stored.unwrap_or_default());
        for (_, backup) in backups {
            let backup = backup.get_str("file").map(Value::to_php_string);
            let backup = path_join(&folder, &backup.unwrap_or_default());
            files.push((backup, folder.clone()));
        }
    }
    files.push((file.to_vec(), Vec::new()));
    Ok(files)
}

/// Whether `_delete_attachment_theme_mod()` finds nothing to remove: the
/// attachment is not a logo, and no header or background image is set.
async fn theme_mods_kept(write: &mut Write<'_>, id: u64) -> Result<bool> {
    let stylesheet = write.option_value("stylesheet").await?.unwrap_or_default();
    let name = format!("theme_mods_{}", String::from_utf8_lossy(&stylesheet));
    let mods = write.option_value(&name).await?.unwrap_or_default();
    let mods = unserialize(&mods).unwrap_or_default();
    let set = |key| mods.get_str(key).filter(|value| !value.is_empty());
    let logo = set("custom_logo").is_some_and(|logo| logo.to_int() == id.cast_signed());
    let images = set("header_image").is_some() || set("background_image").is_some();
    Ok(!logo && !images && site::option_int(write, "site_logo").await? != id)
}

/// `wp_delete_attachment( $id, $force )`: what it did, or `None` where PHP
/// returns false, for a post that is missing or not an attachment.
pub async fn wp_delete_attachment(
    write: &mut Write<'_>,
    ctx: &Context,
    id: u64,
    force: bool,
) -> Result<Option<AttachmentDeleted>> {
    let post = write.post(id).await?;
    let Some(post) = post.filter(|post| post.post_type == "attachment") else {
        return Ok(None);
    };
    let trash = ctx.constants.empty_trash_days != 0 && ctx.constants.media_trash;
    if !force && trash && post.status != "trash" {
        let trashed = wp_trash_post(write, ctx, id).await?;
        return Ok(trashed.then_some(AttachmentDeleted::Trashed));
    }
    delete_post_meta(write, id, TRASH_STATUS).await?;
    delete_post_meta(write, id, TRASH_TIME).await?;
    let mut single = Vec::new();
    for key in [
        "_wp_attachment_metadata",
        "_wp_attachment_backup_sizes",
        "_wp_attached_file",
    ] {
        let values = get_metadata(write, Meta::Post, id, key).await?;
        single.push(values.into_iter().next().unwrap_or_default());
    }
    let mut single = single.into_iter();
    let (meta, backups) = (single.next(), single.next());
    let file = single.next().unwrap_or_default().to_php_string();
    let (meta, backups) = (meta.unwrap_or_default(), backups.unwrap_or_default());
    let files = attachment_files(&file, &meta, &backups)?;
    // delete_attachment: _delete_attachment_theme_mod().
    if !theme_mods_kept(write, id).await? {
        return Err(Error::Unported("an attachment the theme or site logo uses"));
    }
    site::delete_object_terms(write, id, &["category", "post_tag"]).await?;
    let thumbnail = Value::String(id.to_string().into_bytes());
    delete_metadata(write, Meta::Post, 0, "_thumbnail_id", &thumbnail, true).await?;
    let comments = crate::comments::Context {
        now: ctx.now,
        home: &ctx.home,
        empty_trash_days: ctx.constants.empty_trash_days,
    };
    for comment in write.post_comment_ids(id).await? {
        crate::comments::delete_comment(write, comments, comment, true).await?;
    }
    delete_object_meta(write, Meta::Post, id).await?;
    write.delete_post(id).await?;
    Ok(Some(AttachmentDeleted::Deleted(files)))
}

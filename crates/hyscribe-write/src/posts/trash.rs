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

/// `wp_trash_post( $id )`: whether the post went to the trash.
pub async fn wp_trash_post(write: &mut Write<'_>, ctx: &Context, id: u64) -> Result<bool> {
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
    let to_trash = !force && crate::EMPTY_TRASH_DAYS != 0;
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

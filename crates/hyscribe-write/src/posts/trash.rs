// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_trash_post()`, `wp_untrash_post()` and `wp_delete_post()`, with the
//! comments, meta, terms and options that go with a post.

use hyscribe_db::{Meta, Write};
use hyscribe_phpser::{Key, Value, encode, unserialize};

use super::{Context, Kind, PostError, Result, cron, hooks, site, wp_update_post};

/// `unstick_post( $id )`.
async fn unstick(write: &mut Write<'_>, id: u64) -> Result<()> {
    let stored = write.option("sticky_posts").await?.unwrap_or_default();
    let Some(Value::Array(stickies)) = unserialize(&stored) else {
        return Ok(());
    };
    let mut ids: Vec<i64> = Vec::new();
    for (_, sticky) in stickies {
        let sticky = sticky.to_int();
        if !ids.contains(&sticky) {
            ids.push(sticky);
        }
    }
    let Some(at) = ids
        .iter()
        .position(|&sticky| u64::try_from(sticky) == Ok(id))
    else {
        return Ok(());
    };
    ids.remove(at);
    let list = ids
        .into_iter()
        .zip(0..)
        .map(|(sticky, n)| (Key::Int(n), Value::Int(sticky)))
        .collect();
    site::update_option(write, "sticky_posts", &encode(&Value::Array(list)), None).await
}

/// `_reset_front_page_settings_for_post( $id )`.
async fn reset_front_page(write: &mut Write<'_>, id: u64, kind: Kind) -> Result<()> {
    if kind == Kind::Page {
        if site::option_int(write, "page_on_front").await? == id {
            site::update_option(write, "show_on_front", b"posts", None).await?;
            site::update_option(write, "page_on_front", b"0", None).await?;
        }
        if site::option_int(write, "page_for_posts").await? == id {
            site::update_option(write, "page_for_posts", b"0", None).await?;
        }
    }
    unstick(write, id).await
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
    let time = ctx.now.to_string();
    write
        .add_meta(
            Meta::Post,
            id,
            "_wp_trash_meta_status",
            post.status.as_bytes(),
        )
        .await?;
    write
        .add_meta(Meta::Post, id, "_wp_trash_meta_time", time.as_bytes())
        .await?;
    wp_update_post(write, ctx, id, &|args| args.status = "trash".into()).await?;
    // wp_trash_post_comments().
    let comments = write.post_comments(id).await?;
    if !comments.is_empty() {
        let statuses = comments
            .into_iter()
            .map(|(comment, approved, _)| {
                let comment = i64::try_from(comment).unwrap_or(i64::MAX);
                (Key::Int(comment), Value::String(approved.into_bytes()))
            })
            .collect();
        let statuses = encode(&Value::Array(statuses));
        write
            .add_meta(Meta::Post, id, "_wp_trash_meta_comments_status", &statuses)
            .await?;
        write.trash_post_comments(id).await?;
    }
    Ok(true)
}

/// `wp_untrash_post( $id )`: whether the post left the trash, as a draft.
pub async fn wp_untrash_post(write: &mut Write<'_>, ctx: &Context, id: u64) -> Result<bool> {
    let Some(post) = write.post(id).await? else {
        return Ok(false);
    };
    if post.status != "trash" {
        return Ok(false);
    }
    site::delete_post_meta(write, id, "_wp_trash_meta_status").await?;
    site::delete_post_meta(write, id, "_wp_trash_meta_time").await?;
    wp_update_post(write, ctx, id, &|args| args.status = "draft".into()).await?;
    // wp_untrash_post_comments().
    let key = "_wp_trash_meta_comments_status";
    let stored = write.post_meta_values(id, key).await?;
    let statuses = stored.first().and_then(|value| unserialize(value));
    let statuses = statuses.unwrap_or_default();
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
    site::delete_post_meta(write, id, key).await?;
    Ok(true)
}

/// `wp_delete_comment( $id, true )` of each comment of `post`, newest
/// first.
async fn delete_comments(write: &mut Write<'_>, post: u64) -> Result<()> {
    let mut comments = write.post_comments(post).await?;
    while let Some((comment, _, parent)) = comments.pop() {
        write.reparent_comments(comment, parent).await?;
        for (_, _, other) in &mut comments {
            if *other == comment {
                *other = parent;
            }
        }
        let meta = write.object_meta_ids(Meta::Comment, comment).await?;
        write.delete_meta(Meta::Comment, &meta).await?;
        write.delete_comment(comment).await?;
    }
    Ok(())
}

/// `wp_delete_post( $id, true )` of one post of `kind`: its revisions,
/// which the caller deletes next.
async fn delete_one(write: &mut Write<'_>, id: u64, kind: Kind) -> Result<Vec<u64>> {
    let post = super::existing(write, id).await?;
    // before_delete_post.
    reset_front_page(write, id, kind).await?;
    if kind == Kind::Page && site::option_int(write, "wp_page_for_privacy_policy").await? == id {
        site::update_option(write, "wp_page_for_privacy_policy", b"0", None).await?;
    }
    site::delete_post_meta(write, id, "_wp_trash_meta_status").await?;
    site::delete_post_meta(write, id, "_wp_trash_meta_time").await?;
    for taxonomy in kind.taxonomies() {
        site::remove_object_terms(write, id, taxonomy).await?;
    }
    if kind.hierarchical() {
        write
            .reparent_posts(id, &post.post_type, post.parent)
            .await?;
    }
    let revisions = write.child_ids(id, "revision").await?;
    write.reparent_posts(id, "attachment", post.parent).await?;
    delete_comments(write, id).await?;
    let meta = write.object_meta_ids(Meta::Post, id).await?;
    write.delete_meta(Meta::Post, &meta).await?;
    // block_core_calendar_update_has_published_post_on_delete().
    if post.status == "publish" && kind == Kind::Post {
        hooks::update_has_published_posts(write).await?;
    }
    write.delete_post(id).await?;
    cron::clear(write, "publish_future_post", id).await?;
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
    if !force && kind != Kind::Revision && post.status != "trash" {
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
        return Err(PostError::Unported("a post in a classic menu"));
    }
    // Revisions go through wp_delete_post() too, after their parent's
    // terms and children; their order changes no row.
    let mut queue = vec![(id, kind)];
    while let Some((next, kind)) = queue.pop() {
        let revisions = delete_one(write, next, kind).await?;
        queue.extend(revisions.into_iter().map(|id| (id, Kind::Revision)));
    }
    Ok(true)
}

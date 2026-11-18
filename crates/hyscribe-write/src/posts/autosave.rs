// SPDX-License-Identifier: AGPL-3.0-or-later

//! Autosaves, as the REST API's `WP_REST_Autosaves_Controller::create_item()`
//! makes them while `DOING_AUTOSAVE`: the author's own draft that no one
//! else holds is updated without a revision, any other post gets the user's
//! autosave revision, written only when it changes something.

use hyscribe_db::Write;
use hyscribe_render::formatting::normalize_whitespace;

use super::{Context, Error, PostArgs, Result, existing, hooks, insert, update_args};
use crate::options::{Meta, add_metadata, get_metadata};

/// The revisioned fields an autosave request gives: `post_title`,
/// `post_content` and `post_excerpt`, where it gives them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Revisioned {
    /// `post_title`.
    pub title: Option<Vec<u8>>,
    /// `post_content`.
    pub content: Option<Vec<u8>>,
    /// `post_excerpt`.
    pub excerpt: Option<Vec<u8>>,
}

/// A stored post's field.
type Field = fn(&hyscribe_db::PostFields) -> &Vec<u8>;

impl Revisioned {
    fn fields(&self) -> [(&Option<Vec<u8>>, Field); 3] {
        [
            (&self.title, |post| &post.title),
            (&self.content, |post| &post.content),
            (&self.excerpt, |post| &post.excerpt),
        ]
    }

    /// The fields given, on `args`.
    fn set(&self, args: &mut PostArgs) {
        let targets = [&mut args.title, &mut args.content, &mut args.excerpt];
        for ((value, _), target) in self.fields().into_iter().zip(targets) {
            if let Some(value) = value {
                target.clone_from(value);
            }
        }
    }
}

/// `wp_check_post_lock( $id )`: whether another existing user holds the
/// post's `_edit_lock` taken within the last 150 seconds.
async fn locked(write: &mut Write<'_>, ctx: &Context, id: u64) -> Result<bool> {
    let first = async |write: &mut Write<'_>, key| {
        let values = get_metadata(write, Meta::Post, id, key).await?;
        Ok::<_, Error>(
            values
                .into_iter()
                .next()
                .unwrap_or_default()
                .to_php_string(),
        )
    };
    let lock = first(write, "_edit_lock").await?;
    if !super::truthy(&lock) {
        return Ok(false);
    }
    let mut parts = lock.splitn(3, |&b| b == b':');
    let time = parts.next().unwrap_or_default().to_vec();
    let user = match parts.next() {
        Some(user) => user.to_vec(),
        None => first(write, "_edit_last").await?,
    };
    // `get_userdata()` takes a numeric ID only.
    let user = std::str::from_utf8(&user)
        .ok()
        .and_then(|u| u.parse::<u64>().ok());
    let Some(user) = user else {
        return Ok(false);
    };
    if write.user(user).await?.is_none() || !super::truthy(&time) {
        return Ok(false);
    }
    let time = std::str::from_utf8(&time)
        .ok()
        .and_then(|t| t.parse::<i64>().ok());
    let time = time.ok_or(Error::Unported("a post lock time that is not a number"))?;
    Ok(time > ctx.now - 150 && ctx.user != user)
}

/// `create_item()`'s write for the post `id`: the post itself, updated by
/// `change` without a revision, for its author's draft no one else holds;
/// else the user's autosave (`create_post_autosave()`) with the `fields`
/// given. The ID of what was written, or of the autosave left as it was.
pub async fn wp_create_post_autosave(
    write: &mut Write<'_>,
    ctx: &Context,
    id: u64,
    fields: &Revisioned,
    change: &dyn Fn(&mut PostArgs),
) -> Result<u64> {
    let post = existing(write, id).await?;
    let draft = matches!(post.status.as_str(), "draft" | "auto-draft");
    if draft && post.author == ctx.user && !locked(write, ctx, id).await? {
        let args = update_args(write, ctx, id, change).await?;
        // `wp_save_post_revision()` does nothing while `DOING_AUTOSAVE`.
        return insert(write, ctx, args, None).await.map(|(id, ..)| id);
    }
    let different = fields.fields().iter().any(|(value, of)| {
        value
            .as_ref()
            .is_some_and(|value| normalize_whitespace(value) != normalize_whitespace(of(&post)))
    });
    let name = format!("{id}-autosave-v1").into_bytes();
    let mut old = None;
    for revision in write.revision_ids(id).await? {
        let fields = existing(write, revision).await?;
        if fields.name == name && fields.author == ctx.user {
            old = Some(revision);
            break;
        }
    }
    let mut args = PostArgs {
        parent: id,
        status: "inherit".into(),
        post_type: "revision".into(),
        name: Some(name),
        ..PostArgs::default()
    };
    match old {
        Some(old) if !different => Ok(old),
        Some(old) => {
            let change = |kept: &mut PostArgs| {
                fields.set(kept);
                kept.parent = args.parent;
                kept.status.clone_from(&args.status);
                kept.name.clone_from(&args.name);
                kept.author = Some(ctx.user);
                kept.date = String::new();
                kept.date_gmt = String::new();
            };
            let args = update_args(write, ctx, old, &change).await?;
            insert(write, ctx, args, None).await.map(|(id, ..)| id)
        }
        None => {
            fields.set(&mut args);
            let (revision, ..) = insert(write, ctx, args, None).await?;
            // `_wp_put_post_revision`: wp_save_revisioned_meta_fields().
            for value in get_metadata(write, Meta::Post, id, "footnotes").await? {
                let value = hooks::slash(value);
                add_metadata(write, Meta::Post, revision, "footnotes", &value, false).await?;
            }
            Ok(revision)
        }
    }
}

#[cfg(test)]
mod tests;

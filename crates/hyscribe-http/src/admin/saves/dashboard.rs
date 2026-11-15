// SPDX-License-Identifier: AGPL-3.0-or-later

//! The dashboard's writes: the Quick Draft form's auto-draft and
//! its save (`post.php`'s `post-quickdraft-save`), and the welcome panel's
//! state (`?welcome=` and `update-welcome-panel`).

use hyscribe_admin::actions::{Asked, quickdraft_field};
use hyscribe_admin::posts::Answer;
use hyscribe_db::Write;
use hyscribe_phpser::{Value, lookup, stripslashes};
use hyscribe_write::Error;
use hyscribe_write::options::{Meta, update_metadata};
use hyscribe_write::posts::{Context, get_default_post_to_edit, wp_update_post};

use super::screen::{update_user_meta, update_user_option};
use super::{Acting, Saving, text};

/// The posts' context of `user` on the site at `home`, now; their
/// capability to publish plays no part in the drafts written here.
pub(in crate::admin) fn context(user: u64, home: &str) -> Context {
    let host = home.split_once("://").map_or(home, |(_, rest)| rest);
    Context {
        user,
        can_publish: false,
        home: home.as_bytes().to_vec(),
        upload_host: host
            .split('/')
            .next()
            .unwrap_or_default()
            .as_bytes()
            .to_vec(),
        now: crate::now(),
    }
}

/// `wp_dashboard_quick_press()`'s auto-draft for a user with none to
/// reuse: written, and kept in their `dashboard_quick_press_last_post_id`.
pub(in crate::admin) async fn quick_press_draft(
    write: &mut Write<'_>,
    ctx: &Context,
) -> Result<u64, Error> {
    let id = get_default_post_to_edit(write, ctx).await?;
    let option = "dashboard_quick_press_last_post_id";
    update_user_option(write, ctx.user, option, &Value::Int(id.cast_signed())).await?;
    Ok(id)
}

/// The screen options' `?welcome=`: whether the welcome panel shows.
pub(in crate::admin) async fn welcome(
    write: &mut Write<'_>,
    user: u64,
    shown: bool,
) -> Result<(), Error> {
    let shown = Value::Int(i64::from(shown));
    Ok(update_user_meta(write, user, "show_welcome_panel", &shown).await?)
}

/// `str_replace( array( "\r\n", "\r", "\n" ), '<br />', $text )`.
fn breaks(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut bytes = text.iter().peekable();
    while let Some(&byte) = bytes.next() {
        match byte {
            b'\r' => {
                bytes.next_if_eq(&&b'\n');
                out.extend_from_slice(b"<br />");
            }
            b'\n' => out.extend_from_slice(b"<br />"),
            byte => out.push(byte),
        }
    }
    out
}

/// An option's value as text, empty when unset.
async fn option_text(write: &mut Write<'_>, name: &str) -> Result<String, Error> {
    Ok(text(&write.option_value(name).await?.unwrap_or_default()))
}

/// `post-quickdraft-save` once checked: `edit_post()` of the form's
/// auto-draft, its content wrapped in a paragraph block. The form that
/// follows is a page view of its own.
pub(super) fn quickdraft_save<'a>(
    write: &'a mut Write<'_>,
    asked: &'a Asked<'a>,
    _: &'a dyn Acting,
) -> Saving<'a> {
    Box::pin(async move {
        let id = lookup(asked.post, "post_ID").map_or(0, Value::to_int);
        let id = u64::try_from(id).unwrap_or_default();
        let post = write.post(id).await?;
        // The form only ever names an auto-draft.
        if post.is_none_or(|post| post.status != "auto-draft") {
            return Err(
                Error::Unported("a Quick Draft of a post that is not an auto-draft").into(),
            );
        }
        // The route answers a form with an array before it saves.
        let trimmed = quickdraft_field(asked, "content").unwrap_or_default();
        let marked = trimmed.windows(21).any(|w| w == b"<!-- wp:paragraph -->");
        let content = if Value::String(trimmed.clone()).is_empty() || marked {
            lookup(asked.post, "content").map(Value::to_php_string)
        } else {
            let wrapped = [
                &b"<!-- wp:paragraph -->"[..],
                &breaks(&trimmed),
                b"<!-- /wp:paragraph -->",
            ];
            Some(stripslashes(&wrapped.concat()))
        };
        let title = lookup(asked.post, "post_title").map(Value::to_php_string);
        let comments = option_text(write, "default_comment_status").await?;
        let pings = option_text(write, "default_ping_status").await?;
        let user = Value::Int(asked.user.cast_signed());
        update_metadata(write, Meta::Post, id, "_edit_last", &user, &Value::Null).await?;
        let ctx = context(asked.user, asked.home);
        wp_update_post(write, &ctx, id, &|args| {
            args.author = Some(asked.user);
            args.status = "draft".into();
            args.comment_status.clone_from(&comments);
            args.ping_status.clone_from(&pings);
            args.title = title.clone().unwrap_or_else(|| args.title.clone());
            args.content = content.clone().unwrap_or_else(|| args.content.clone());
        })
        .await?;
        // `wp_set_post_lock()`.
        let lock = Value::String(format!("{}:{}", ctx.now, asked.user).into_bytes());
        update_metadata(write, Meta::Post, id, "_edit_lock", &lock, &Value::Null).await?;
        Ok(Answer::Page(String::new()))
    })
}

/// `wp_ajax_update_welcome_panel()`.
pub(super) fn update_welcome_panel<'a>(
    write: &'a mut Write<'_>,
    asked: &'a Asked<'a>,
    _: &'a dyn Acting,
) -> Saving<'a> {
    Box::pin(async move {
        let visible = lookup(asked.post, "visible").is_some_and(|v| !v.is_empty());
        welcome(write, asked.user, visible).await?;
        Ok(Answer::Page("1".into()))
    })
}

#[cfg(test)]
mod tests;

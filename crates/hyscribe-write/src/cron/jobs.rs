// SPDX-License-Identifier: AGPL-3.0-or-later

//! The core jobs HyScribe runs for their cron events, each as its hook's
//! callback does in PHP with no user, at the action's time. The update
//! checks (`wp_version_check`, `wp_update_plugins`, `wp_update_themes`)
//! ask wordpress.org, so they are not run.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use hyscribe_db::Write;
use hyscribe_phpser::Value;
use hyscribe_render::date::{Zone, format_unix, stored_unix};

use super::{Result, clear, schedule};
use crate::comments::{self, delete_comment};
use crate::options::{Meta, delete_metadata};
use crate::posts::{Context, wp_delete_post, wp_publish_post, wp_update_post};
use crate::{EMPTY_TRASH_DAYS, MYSQL, wp_timezone};

const DAY: i64 = 86_400;

/// `check_and_publish_future_post( $post )`: a scheduled post published
/// once its time has come, or its event moved to its time if early.
pub async fn check_and_publish_future_post(
    write: &mut Write<'_>,
    ctx: &Context,
    id: u64,
) -> Result<()> {
    let Some(post) = write.post(id).await? else {
        return Ok(());
    };
    if post.status != "future" {
        return Ok(());
    }
    let time = stored_unix(&post.date_gmt, Zone::Utc);
    if time > ctx.now {
        clear(write, "publish_future_post", id).await?;
        return schedule(write, (ctx.now, time), "publish_future_post", Some(id)).await;
    }
    wp_publish_post(write, ctx, id).await
}

/// `wp_scheduled_delete()`: the posts and comments in the trash for longer
/// than `EMPTY_TRASH_DAYS` deleted, and the trash meta of those since taken
/// out of it dropped.
pub async fn wp_scheduled_delete(write: &mut Write<'_>, ctx: &Context) -> Result<()> {
    let before = ctx.now - DAY * EMPTY_TRASH_DAYS as i64;
    let any = Value::String(Vec::new());
    for id in write.posts_trashed_before(before).await? {
        match write.post(id).await? {
            Some(post) if post.status == "trash" => {
                wp_delete_post(write, ctx, id, false).await?;
            }
            _ => {
                for key in ["_wp_trash_meta_status", "_wp_trash_meta_time"] {
                    delete_metadata(write, Meta::Post, id, key, &any, false).await?;
                }
            }
        }
    }
    let context = comments::Context {
        now: ctx.now,
        home: &ctx.home,
    };
    for id in write.comments_trashed_before(before).await? {
        match write.comment(id).await? {
            Some(comment) if comment.approved == "trash" => {
                delete_comment(write, context, id, false).await?;
            }
            _ => {
                for key in ["_wp_trash_meta_time", "_wp_trash_meta_status"] {
                    delete_metadata(write, Meta::Comment, id, key, &any, false).await?;
                }
            }
        }
    }
    Ok(())
}

/// `delete_expired_transients()` with no persistent object cache.
pub async fn delete_expired_transients(write: &mut Write<'_>, now: i64) -> Result<()> {
    write.delete_expired_transients(now).await?;
    Ok(())
}

/// `wp_delete_auto_drafts()`: auto-drafts older than seven days deleted.
pub async fn wp_delete_auto_drafts(write: &mut Write<'_>, ctx: &Context) -> Result<()> {
    for id in write.old_auto_draft_ids(ctx.now).await? {
        wp_delete_post(write, ctx, id, true).await?;
    }
    Ok(())
}

/// `_wp_personal_data_cleanup_requests()`: the pending privacy requests
/// unchanged for the day a confirmation key lasts marked failed.
pub async fn wp_privacy_personal_data_cleanup_requests(
    write: &mut Write<'_>,
    ctx: &Context,
) -> Result<()> {
    let before = format_unix(MYSQL, ctx.now - DAY, wp_timezone(write).await?);
    for id in write.pending_requests_before(&before).await? {
        let fail = |args: &mut crate::posts::PostArgs| {
            args.status = "request-failed".into();
            args.password = Vec::new();
        };
        wp_update_post(write, ctx, id, &fail).await?;
    }
    Ok(())
}

/// `wp_privacy_delete_old_export_files()`: the files under the uploads
/// folder's `wp-personal-data-exports` older than three days deleted,
/// its `index.php` kept. Symbolic links are neither followed nor deleted,
/// so nothing outside the folder is reached; a file that cannot be read or
/// deleted is skipped, as PHP warns and goes on.
pub fn wp_privacy_delete_old_export_files(uploads: &Path, now: i64) {
    let exports = uploads.join("wp-personal-data-exports");
    for file in list_files(&exports, 100, &["index.php"]) {
        let modified = std::fs::symlink_metadata(&file).and_then(|meta| meta.modified());
        let since = modified
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok());
        if since.is_some_and(|since| now - since.as_secs() as i64 > 3 * DAY) {
            // PHP's unlink() only warns when it fails.
            let _ = std::fs::remove_file(file);
        }
    }
}

/// `list_files( $folder, $levels, $exclusions )`'s files: those under
/// `folder` down `levels` folders, hidden ones and the top's `exclusions` left out.
fn list_files(folder: &Path, levels: u32, exclusions: &[&str]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Some(levels) = levels.checked_sub(1) else {
        return files;
    };
    let entries = std::fs::read_dir(folder).into_iter().flatten().flatten();
    for entry in entries {
        let name = entry.file_name();
        let name = name.as_encoded_bytes();
        if name.starts_with(b".") || exclusions.iter().any(|e| e.as_bytes() == name) {
            continue;
        }
        let kind = entry.file_type().ok();
        if kind.is_some_and(|kind| kind.is_dir()) {
            files.extend(list_files(&entry.path(), levels, &[]));
        } else if kind.is_some_and(|kind| kind.is_file()) {
            files.push(entry.path());
        }
    }
    files
}

#[cfg(test)]
pub(crate) mod tests;

// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp-cron.php` as the service runs it (ADR 0017): the due events of the
//! hooks HyScribe runs, behind the `doing_cron` transient PHP takes, so a
//! PHP WordPress on the same database never runs the list at the same time.

use std::path::Path;

use hyscribe_config::Constants;
use hyscribe_db::{DbError, Write};
use hyscribe_phpser::Value;

use super::{Job, Result, due, run};
use crate::options::{delete_transient, get_option, set_transient};
use crate::posts::Context;

/// `WP_CRON_LOCK_TIMEOUT`: how long a lock holds, in seconds.
const LOCK_TIMEOUT: f64 = 60.0;

/// What a run did.
#[derive(Debug)]
pub enum Ran {
    /// No event of a hook HyScribe runs was due, so no lock was taken.
    Idle,
    /// Another process holds the lock.
    Locked,
    /// The events run, each hook with its job's outcome; `stolen` when
    /// another process took the lock, which stops the run as in PHP.
    Done {
        /// Each event's hook and outcome, in order.
        events: Vec<(Job, Result<()>)>,
        /// Whether the lock was taken from it.
        stolen: bool,
    },
}

/// PHP's `microtime( true )`: seconds and microseconds.
pub fn microtime() -> f64 {
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    since.map_or(0.0, |since| {
        since.as_secs() as f64 + f64::from(since.subsec_micros()) / 1e6
    })
}

/// `sprintf( '%.22F', $time )`, the lock's value.
fn lock_value(time: f64) -> Vec<u8> {
    format!("{time:.22}").into_bytes()
}

/// `_get_cron_lock()`: the stored lock, read past any cache.
async fn cron_lock(write: &mut Write<'_>) -> Result<Option<Vec<u8>>> {
    Ok(write.option_value("_transient_doing_cron").await?)
}

/// The posts' context of a job at `now`, with no user, the site's
/// addresses read in its action.
async fn context(write: &mut Write<'_>, constants: Constants, now: i64) -> Result<Context> {
    let home = get_option(write, "home").await?.to_php_string();
    let site = get_option(write, "siteurl").await?.to_php_string();
    Ok(Context {
        user: 0,
        can_publish: false,
        home,
        upload_host: hyscribe_render::link::host(&site),
        unfiltered_html: false,
        constants,
        now,
    })
}

/// One run of `wp-cron.php` at the times `clock` gives, each step an
/// action `begin` starts: the due events of HyScribe's hooks behind the
/// lock. A job that fails keeps its event and joins `failed`, whose hooks
/// are not run again; `uploads` holds the personal data exports.
pub async fn wp_cron<'w>(
    begin: impl AsyncFn() -> std::result::Result<Write<'w>, DbError>,
    (constants, uploads): (Constants, &Path),
    clock: &dyn Fn() -> f64,
    failed: &mut Vec<Job>,
) -> Result<Ran> {
    let gmt = clock();
    let events = due(&mut begin().await?, gmt.floor() as i64).await?;
    let events: Vec<_> = events
        .into_iter()
        .filter(|event| !failed.contains(&event.job))
        .collect();
    if events.is_empty() {
        return Ok(Ran::Idle);
    }
    let mut write = begin().await?;
    let held = cron_lock(&mut write).await?;
    // `(float) get_transient()`, none when over ten minutes ahead (`spawn_cron()`).
    let held = Value::String(held.unwrap_or_default()).to_float();
    let held = if held > gmt + 600.0 { 0.0 } else { held };
    if held + LOCK_TIMEOUT > gmt {
        return Ok(Ran::Locked);
    }
    let ours = lock_value(gmt);
    let value = Value::String(ours.clone());
    set_transient(&mut write, "doing_cron", &value, 0).await?;
    let ours = Some(ours);
    write.commit().await?;
    let mut done = Vec::new();
    for event in events {
        // An earlier event of the hook failed in this run.
        if failed.contains(&event.job) {
            continue;
        }
        let mut write = begin().await?;
        let ctx = context(&mut write, constants, clock().floor() as i64).await?;
        let result = match run(&mut write, &ctx, uploads, &event).await {
            Ok(()) => write.commit().await.map_err(Into::into),
            Err(err) => Err(err),
        };
        if result.is_err() {
            failed.push(event.job);
        }
        done.push((event.job, result));
        if cron_lock(&mut begin().await?).await? != ours {
            return Ok(Ran::Done {
                events: done,
                stolen: true,
            });
        }
    }
    let mut write = begin().await?;
    if cron_lock(&mut write).await? == ours {
        delete_transient(&mut write, "doing_cron").await?;
    }
    write.commit().await?;
    Ok(Ran::Done {
        events: done,
        stolen: false,
    })
}

#[cfg(test)]
mod tests;

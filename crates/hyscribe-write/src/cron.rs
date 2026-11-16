// SPDX-License-Identifier: AGPL-3.0-or-later

//! WP-Cron over the `cron` option as PHP stores it: the events posts
//! schedule and clear (`wp_schedule_single_event()`, `wp_schedule_event()`,
//! `wp_clear_scheduled_hook()` and `wp_next_scheduled()`), and `wp-cron.php`'s
//! run of a due event of a hook HyScribe runs ([`Job`]): rescheduled when
//! recurring, unscheduled, and its job done, in one action (ADR 0015).
//! Events of any other hook stay in the option, untouched, for PHP WordPress.

use std::path::Path;

use hyscribe_db::Write;
use hyscribe_phpser::{Key, Value, encode, md5, strnatcasecmp, unserialize};

use crate::Error;
use crate::options::{Autoload, update_option};
use crate::posts::Context;

mod jobs;

pub use jobs::{
    check_and_publish_future_post, delete_expired_transients, wp_delete_auto_drafts,
    wp_privacy_delete_old_export_files, wp_privacy_personal_data_cleanup_requests,
    wp_scheduled_delete,
};

type Result<T> = std::result::Result<T, Error>;

/// The events by timestamp, without the `version` entry.
type Crons = Vec<(Key, Value)>;

/// The hooks HyScribe runs, each a core job. Every other event, the update
/// checks and plugins' hooks among them, is left for PHP WordPress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Job {
    /// `publish_future_post`: [`check_and_publish_future_post`].
    PublishFuturePost,
    /// `wp_scheduled_delete`: [`wp_scheduled_delete`].
    ScheduledDelete,
    /// `delete_expired_transients`: [`delete_expired_transients`].
    DeleteExpiredTransients,
    /// `wp_scheduled_auto_draft_delete`: [`wp_delete_auto_drafts`].
    AutoDraftDelete,
    /// `wp_privacy_delete_old_export_files`: [`wp_privacy_delete_old_export_files`].
    DeleteOldExportFiles,
    /// `wp_privacy_personal_data_cleanup_requests`: [`wp_privacy_personal_data_cleanup_requests`].
    CleanupRequests,
}

impl Job {
    const ALL: [Job; 6] = [
        Job::PublishFuturePost,
        Job::ScheduledDelete,
        Job::DeleteExpiredTransients,
        Job::AutoDraftDelete,
        Job::DeleteOldExportFiles,
        Job::CleanupRequests,
    ];

    /// The job of the hook `hook`, if HyScribe runs it.
    fn of(hook: &Key) -> Option<Job> {
        let hook = hook.to_php_string();
        Job::ALL
            .into_iter()
            .find(|job| hook == job.hook().as_bytes())
    }

    /// The hook's name.
    pub fn hook(self) -> &'static str {
        match self {
            Job::PublishFuturePost => "publish_future_post",
            Job::ScheduledDelete => "wp_scheduled_delete",
            Job::DeleteExpiredTransients => "delete_expired_transients",
            Job::AutoDraftDelete => "wp_scheduled_auto_draft_delete",
            Job::DeleteOldExportFiles => "wp_privacy_delete_old_export_files",
            Job::CleanupRequests => "wp_privacy_personal_data_cleanup_requests",
        }
    }
}

/// `wp_get_schedules()`' core recurrences and their intervals.
const SCHEDULES: [(&str, i64); 4] = [
    ("hourly", 3_600),
    ("twicedaily", 43_200),
    ("daily", 86_400),
    ("weekly", 604_800),
];

/// A due event of a hook HyScribe runs, as `wp-cron.php` reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    /// Its timestamp.
    pub at: i64,
    /// Its hook's job.
    pub job: Job,
    /// Its arguments, an array.
    pub args: Value,
    /// The recurrence and interval of a recurring event.
    pub recurrence: Option<(&'static str, i64)>,
}

/// An event's arguments: none, or one post ID.
fn args(post: Option<u64>) -> Value {
    let id = post.map(|id| {
        (
            Key::Int(0),
            Value::Int(i64::try_from(id).unwrap_or(i64::MAX)),
        )
    });
    Value::Array(id.into_iter().collect())
}

/// `md5( serialize( $args ) )`, an event's key under its hook.
fn key(args: &Value) -> Key {
    Key::String(md5(&encode(args)).into_bytes())
}

fn name(text: &str) -> Key {
    Key::String(text.as_bytes().to_vec())
}

/// `isset( $cron[ $hook ][ $key ] )` of a timestamp's hooks.
fn has(hooks: &[(Key, Value)], hook: &str, key: &Key) -> bool {
    let hook = name(hook);
    let events = hooks.iter().rev().find(|(name, _)| *name == hook);
    let event = events.and_then(|(_, events)| events.get(key));
    event.is_some_and(|event| *event != Value::Null)
}

/// [`has`] of a timestamp's entry, which may not be an array.
fn has_value(cron: &Value, hook: &str, key: &Key) -> bool {
    cron.as_array().is_some_and(|hooks| has(hooks, hook, key))
}

/// `$entries[ $key ] = f( $entries[ $key ] )` for an array entry, which
/// keeps its place, or comes last when new.
fn update(entries: &mut Vec<(Key, Value)>, key: Key, f: impl FnOnce(Crons) -> Crons) {
    match entries.iter_mut().find(|(k, _)| *k == key) {
        Some((_, value)) => {
            let inner = match std::mem::take(value) {
                Value::Array(inner) => inner,
                _ => Vec::new(),
            };
            *value = Value::Array(f(inner));
        }
        None => entries.push((key, Value::Array(f(Vec::new())))),
    }
}

/// `wp_schedule_single_event()` on `crons` at `now`: nothing for a
/// duplicate within ten minutes.
fn add(crons: &mut Crons, (now, at): (i64, i64), hook: &str, post: Option<u64>) {
    let args = args(post);
    let key = key(&args);
    let min = if at < now + 600 { 0 } else { at - 600 };
    let max = at.max(now) + 600;
    for (stamp, cron) in crons.iter() {
        let stamp = Value::String(stamp.to_php_string()).to_int();
        if stamp < min {
            continue;
        }
        if stamp > max {
            break;
        }
        if has_value(cron, hook, &key) {
            return;
        }
    }
    let event = vec![(name("schedule"), Value::Bool(false)), (name("args"), args)];
    put(crons, at, hook, key, event);
}

/// `$crons[ $at ][ $hook ][ $key ] = $event`, the timestamps sorted.
fn put(crons: &mut Crons, at: i64, hook: &str, key: Key, event: Crons) {
    update(crons, Key::Int(at), |mut hooks| {
        update(&mut hooks, name(hook), |mut events| {
            update(&mut events, key, |_| event);
            events
        });
        hooks
    });
    crons.sort_by(|(a, _), (b, _)| strnatcasecmp(&a.to_php_string(), &b.to_php_string()));
}

/// `wp_unschedule_event()` of each event of `hook` with `key`, dropping
/// what it leaves empty: whether there was one.
fn remove(crons: &mut Crons, hook: &str, key: &Key) -> bool {
    let mut removed = false;
    crons.retain_mut(|(_, cron)| match cron {
        Value::Array(hooks) if has(hooks, hook, key) => {
            removed = true;
            hooks.retain_mut(|(at, events)| match events {
                Value::Array(list) if *at == name(hook) => {
                    list.retain(|(k, _)| k != key);
                    !list.is_empty()
                }
                _ => true,
            });
            !hooks.is_empty()
        }
        _ => true,
    });
    removed
}

/// `_get_cron_array()`.
async fn crons(write: &mut Write<'_>) -> Result<Crons> {
    let stored = write.option_value("cron").await?.unwrap_or_default();
    let Some(Value::Array(mut crons)) = unserialize(&stored) else {
        return Ok(Vec::new());
    };
    let version = name("version");
    if !crons.iter().any(|(key, _)| *key == version) {
        return Err(Error::Unported("a cron array older than version 2"));
    }
    crons.retain(|(key, _)| *key != version);
    Ok(crons)
}

/// `_set_cron_array( $crons )`.
async fn set_crons(write: &mut Write<'_>, mut crons: Crons) -> Result<()> {
    crons.push((name("version"), Value::Int(2)));
    update_option(write, "cron", &Value::Array(crons), Some(Autoload::On)).await?;
    Ok(())
}

/// `wp_next_scheduled( $hook )` for an event without arguments: whether
/// one is scheduled.
pub(crate) async fn scheduled(write: &mut Write<'_>, hook: &str) -> Result<bool> {
    let key = key(&args(None));
    let crons = crons(write).await?;
    Ok(crons.iter().any(|(_, cron)| has_value(cron, hook, &key)))
}

/// `wp_schedule_single_event( $at, $hook, $args )` at `now`.
pub(crate) async fn schedule(
    write: &mut Write<'_>,
    times: (i64, i64),
    hook: &str,
    post: Option<u64>,
) -> Result<()> {
    let mut crons = crons(write).await?;
    // A duplicate leaves the option as it is, which writes nothing.
    add(&mut crons, times, hook, post);
    set_crons(write, crons).await
}

/// `wp_schedule_event( $at, $recurrence, $hook, $args )` on `crons`.
fn put_recurring(crons: &mut Crons, at: i64, hook: &str, args: Value, every: (&str, i64)) {
    let key = key(&args);
    let event = vec![
        (name("schedule"), Value::String(every.0.as_bytes().to_vec())),
        (name("args"), args),
        (name("interval"), Value::Int(every.1)),
    ];
    put(crons, at, hook, key, event);
}

/// `wp_schedule_event( $now, 'daily', $hook )` of an event without arguments.
pub(crate) async fn schedule_daily(write: &mut Write<'_>, now: i64, hook: &str) -> Result<()> {
    let mut crons = crons(write).await?;
    put_recurring(&mut crons, now, hook, args(None), ("daily", 86_400));
    set_crons(write, crons).await
}

/// `wp_clear_scheduled_hook( $hook, array( $post ) )`.
pub(crate) async fn clear(write: &mut Write<'_>, hook: &str, post: u64) -> Result<()> {
    let mut crons = crons(write).await?;
    if remove(&mut crons, hook, &key(&args(Some(post)))) {
        set_crons(write, crons).await?;
    }
    Ok(())
}

/// `wp_reschedule_event()` of a recurring `event` at `now`: again at the
/// first of its interval's steps from its time that is after `now`.
fn reschedule(crons: &mut Crons, now: i64, event: &Event) {
    let Some((recurrence, interval)) = event.recurrence else {
        return;
    };
    let at = if event.at >= now {
        now + interval
    } else {
        now + (interval - (now - event.at) % interval)
    };
    let hook = event.job.hook();
    put_recurring(crons, at, hook, event.args.clone(), (recurrence, interval));
}

/// `wp_unschedule_event()` of `event`: out of its timestamp, dropping what
/// that leaves empty.
fn unschedule(crons: &mut Crons, event: &Event) {
    let (stamp, hook, key) = (Key::Int(event.at), name(event.job.hook()), key(&event.args));
    crons.retain_mut(|(at, hooks)| {
        if *at == stamp
            && let Value::Array(hooks) = hooks
        {
            hooks.retain_mut(|(name, events)| {
                if *name == hook
                    && let Value::Array(events) = events
                {
                    events.retain(|(k, _)| *k != key);
                }
                *name != hook || !events.is_empty()
            });
        }
        *at != stamp || !hooks.is_empty()
    });
}

/// `wp_get_ready_cron_jobs()` at `now` as `wp-cron.php` goes through them:
/// the due events of the hooks HyScribe runs, in PHP's order. An event of
/// a recurrence a plugin adds, or with no arguments array, is left for PHP.
pub async fn due(write: &mut Write<'_>, now: i64) -> Result<Vec<Event>> {
    let mut due = Vec::new();
    for (stamp, hooks) in crons(write).await? {
        // A timestamp is an integer key; others sort after them.
        let Key::Int(at) = stamp else { break };
        if at > now {
            break;
        }
        for (hook, events) in hooks.as_array().unwrap_or_default() {
            let Some(job) = Job::of(hook) else { continue };
            for (_, event) in events.as_array().unwrap_or_default() {
                let recurrence = match event.get_str("schedule").filter(|s| !s.is_empty()) {
                    Some(schedule) => {
                        let schedule = schedule.to_php_string();
                        let known = SCHEDULES
                            .iter()
                            .find(|(name, _)| name.as_bytes() == schedule);
                        let Some(&every) = known else { continue };
                        Some(every)
                    }
                    None => None,
                };
                if let Some(args) = event
                    .get_str("args")
                    .filter(|args| args.as_array().is_some())
                {
                    let args = args.clone();
                    due.push(Event {
                        at,
                        job,
                        args,
                        recurrence,
                    });
                }
            }
        }
    }
    Ok(due)
}

/// `wp-cron.php`'s run of `event` at `ctx.now`, with no user: rescheduled
/// when recurring, unscheduled, then its job, all in the one action, so
/// a job that fails leaves its event as it was. `uploads` is the uploads
/// folder, which holds the personal data exports.
pub async fn run(
    write: &mut Write<'_>,
    ctx: &Context,
    uploads: &Path,
    event: &Event,
) -> Result<()> {
    let mut crons = crons(write).await?;
    reschedule(&mut crons, ctx.now, event);
    unschedule(&mut crons, event);
    set_crons(write, crons).await?;
    match event.job {
        Job::PublishFuturePost => {
            // do_action_ref_array() passes the arguments' values in order.
            let first = event.args.as_array().and_then(<[_]>::first);
            let post = first.map_or(0, |(_, post)| post.to_int());
            let post = u64::try_from(post).unwrap_or(0);
            check_and_publish_future_post(write, ctx, post).await
        }
        Job::ScheduledDelete => wp_scheduled_delete(write, ctx).await,
        Job::DeleteExpiredTransients => delete_expired_transients(write, ctx.now).await,
        Job::AutoDraftDelete => wp_delete_auto_drafts(write, ctx).await,
        Job::DeleteOldExportFiles => {
            wp_privacy_delete_old_export_files(uploads, ctx.now);
            Ok(())
        }
        Job::CleanupRequests => wp_privacy_personal_data_cleanup_requests(write, ctx).await,
    }
}

#[cfg(test)]
mod tests;

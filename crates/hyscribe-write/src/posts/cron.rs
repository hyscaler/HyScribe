// SPDX-License-Identifier: AGPL-3.0-or-later

//! The cron events posts schedule and clear: `wp_schedule_single_event()`,
//! `wp_clear_scheduled_hook()` and `wp_next_scheduled()` over the `cron`
//! option, for events with no arguments or with one post ID.

use hyscribe_db::Write;
use hyscribe_phpser::{Key, Value, encode, md5, strnatcasecmp, unserialize};

use super::{Error, Result};
use crate::options::{Autoload, update_option};

/// The events by timestamp, without the `version` entry.
type Crons = Vec<(Key, Value)>;

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
pub(super) async fn scheduled(write: &mut Write<'_>, hook: &str) -> Result<bool> {
    let key = key(&args(None));
    let crons = crons(write).await?;
    Ok(crons.iter().any(|(_, cron)| has_value(cron, hook, &key)))
}

/// `wp_schedule_single_event( $at, $hook, $args )` at `now`.
pub(super) async fn schedule(
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

/// `wp_schedule_event( $now, 'daily', $hook )` of an event without arguments.
pub(super) async fn schedule_daily(write: &mut Write<'_>, now: i64, hook: &str) -> Result<()> {
    let mut crons = crons(write).await?;
    let args = args(None);
    let event = vec![
        (name("schedule"), Value::String(b"daily".to_vec())),
        (name("args"), args.clone()),
        (name("interval"), Value::Int(86_400)),
    ];
    put(&mut crons, now, hook, key(&args), event);
    set_crons(write, crons).await
}

/// `wp_clear_scheduled_hook( $hook, array( $post ) )`.
pub(super) async fn clear(write: &mut Write<'_>, hook: &str, post: u64) -> Result<()> {
    let mut crons = crons(write).await?;
    if remove(&mut crons, hook, &key(&args(Some(post)))) {
        set_crons(write, crons).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;

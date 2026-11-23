// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use hyscribe_db::Connection;

use crate::cron::jobs::tests::{cron, missed, transients, trash};
use crate::posts::tests::{act, check};
use crate::tests::{ONE_AT_A_TIME, admin, fails_at_each_step, rolled_back, text, writer};

const NOW: i64 = 1_000_000;

fn event(args: Value) -> Value {
    Value::Array(vec![
        (name("schedule"), Value::Bool(false)),
        (name("args"), args),
    ])
}

/// A timestamp's entry with one event of each `(hook, args)`.
fn at(stamp: Key, events: &[(&str, Option<u64>)]) -> (Key, Value) {
    let hooks = events
        .iter()
        .map(|&(hook, post)| {
            let args = args(post);
            (name(hook), Value::Array(vec![(key(&args), event(args))]))
        })
        .collect();
    (stamp, Value::Array(hooks))
}

/// Whether [`add`] scheduled the event.
fn added(crons: &mut Crons, times: (i64, i64), hook: &str, post: Option<u64>) -> bool {
    let before = crons.clone();
    add(crons, times, hook, post);
    *crons != before
}

#[test]
fn events_join_their_timestamp_and_the_list_stays_sorted() {
    let mut crons = vec![
        at(Key::Int(NOW + 5000), &[("other", None)]),
        at(Key::Int(NOW + 9000), &[("other", None)]),
    ];
    assert!(added(&mut crons, (NOW, NOW + 5000), "do_pings", None));
    assert!(added(
        &mut crons,
        (NOW, NOW + 7000),
        "publish_future_post",
        Some(7)
    ));
    let expected = vec![
        at(Key::Int(NOW + 5000), &[("other", None), ("do_pings", None)]),
        at(Key::Int(NOW + 7000), &[("publish_future_post", Some(7))]),
        at(Key::Int(NOW + 9000), &[("other", None)]),
    ];
    assert_eq!(crons, expected);
    // A timestamp whose entry is not an array gets one.
    let mut broken = vec![(Key::Int(NOW + 5000), Value::String(b"broken".to_vec()))];
    assert!(added(&mut broken, (NOW, NOW + 5000), "do_pings", None));
    assert_eq!(broken, [at(Key::Int(NOW + 5000), &[("do_pings", None)])]);
}

#[test]
fn an_event_due_within_ten_minutes_of_the_same_is_a_duplicate() {
    let mut crons = vec![
        at(Key::String(b"x".to_vec()), &[]),
        (Key::Int(NOW - 100), Value::String(b"broken".to_vec())),
        at(Key::Int(NOW + 100), &[("do_pings", None)]),
    ];
    let before = crons.clone();
    assert!(!added(&mut crons, (NOW, NOW), "do_pings", None));
    assert!(!added(&mut crons, (NOW, NOW - 300), "do_pings", None));
    assert_eq!(crons, before);
    // Ten minutes and a second apart, and after a later timestamp.
    let mut later = vec![at(Key::Int(NOW + 2000), &[("do_pings", None)])];
    assert!(added(&mut later, (NOW, NOW + 2601), "do_pings", None));
    assert!(added(&mut later, (NOW, NOW + 1399), "do_pings", None));
    assert!(!added(&mut later, (NOW, NOW + 1400), "do_pings", None));
    assert!(!added(&mut later, (NOW, NOW + 2600), "do_pings", None));
    let mut none = vec![at(Key::Int(NOW + 601), &[("do_pings", None)])];
    assert!(added(&mut none, (NOW, NOW), "do_pings", None));
    let mut past = vec![at(Key::Int(NOW + 600), &[("do_pings", None)])];
    assert!(!added(&mut past, (NOW, NOW - 5000), "do_pings", None));
    let mut wide = vec![at(Key::Int(NOW - 5000), &[("do_pings", None)])];
    assert!(!added(&mut wide, (NOW, NOW + 599), "do_pings", None));
}

#[test]
fn removing_an_event_drops_what_it_leaves_empty() {
    let mut crons = vec![
        at(
            Key::Int(1),
            &[("publish_future_post", Some(7)), ("other", None)],
        ),
        at(Key::Int(2), &[("publish_future_post", Some(7))]),
        at(Key::Int(3), &[("publish_future_post", Some(8))]),
        (Key::Int(4), Value::String(b"broken".to_vec())),
    ];
    let mut both = at(Key::Int(5), &[("publish_future_post", Some(7))]);
    if let Value::Array(hooks) = &mut both.1
        && let Some((_, Value::Array(events))) = hooks.first_mut()
    {
        let args = args(Some(8));
        events.push((key(&args), event(args)));
    }
    crons.push(both);
    assert!(remove(
        &mut crons,
        None,
        "publish_future_post",
        &key(&args(Some(7)))
    ));
    let expected = vec![
        at(Key::Int(1), &[("other", None)]),
        at(Key::Int(3), &[("publish_future_post", Some(8))]),
        (Key::Int(4), Value::String(b"broken".to_vec())),
        at(Key::Int(5), &[("publish_future_post", Some(8))]),
    ];
    assert_eq!(crons, expected);
    assert!(!remove(
        &mut crons,
        None,
        "publish_future_post",
        &key(&args(Some(7)))
    ));
}

#[test]
fn keys_are_those_of_php_arguments() {
    let Key::String(none) = key(&args(None)) else {
        panic!("a string key");
    };
    assert_eq!(none, b"40cd750bba9870f18aada2478b24840a");
    let Key::String(post) = key(&args(Some(900))) else {
        panic!("a string key");
    };
    assert_eq!(post, b"f3dd847b0fa5d985c704d7471d9bdbf1");
}

#[test]
fn the_duplicate_window_has_php_bounds() {
    // Due within ten minutes, the window starts at 0; past that, ten minutes back.
    let mut near = vec![at(Key::Int(NOW - 1), &[("do_pings", None)])];
    assert!(added(&mut near, (NOW, NOW + 600), "do_pings", None));
    let mut edge = vec![at(Key::Int(NOW + 4400), &[("do_pings", None)])];
    assert!(!added(&mut edge, (NOW, NOW + 5000), "do_pings", None));
}

#[test]
fn removing_leaves_other_hooks_with_the_same_arguments() {
    let mut crons = vec![at(
        Key::Int(1),
        &[("publish_future_post", Some(7)), ("other", Some(7))],
    )];
    assert!(remove(
        &mut crons,
        None,
        "publish_future_post",
        &key(&args(Some(7)))
    ));
    assert_eq!(crons, [at(Key::Int(1), &[("other", Some(7))])]);
}

/// A recurring event of `job` at `at`, without arguments.
fn recurring(job: Job, at: i64, every: (&'static str, i64)) -> Event {
    let (args, recurrence) = (args(None), Some((every.0.to_string(), every.1)));
    Event {
        at,
        job,
        args,
        recurrence,
    }
}

#[test]
fn a_recurring_event_moves_to_its_next_step_after_now() {
    let mut crons = Vec::new();
    // Due before now: the step after now; due at or after now: an interval on.
    reschedule(
        &mut crons,
        NOW,
        &recurring(Job::ScheduledDelete, NOW - 100, ("hourly", 3_600)),
    );
    reschedule(
        &mut crons,
        NOW,
        &recurring(Job::AutoDraftDelete, NOW, ("daily", 86_400)),
    );
    reschedule(
        &mut crons,
        NOW,
        &recurring(Job::CleanupRequests, NOW + 5, ("daily", 86_400)),
    );
    let single = Event {
        recurrence: None,
        ..recurring(Job::ScheduledDelete, NOW, ("daily", 1))
    };
    reschedule(&mut crons, NOW, &single);
    let stamps: Vec<Key> = crons.iter().map(|(stamp, _)| stamp.clone()).collect();
    assert_eq!(stamps, [Key::Int(NOW + 3_500), Key::Int(NOW + 86_400)]);
    let event = crons[0]
        .1
        .get_path(&["wp_scheduled_delete", "40cd750bba9870f18aada2478b24840a"]);
    let expected = Value::Array(vec![
        (name("schedule"), Value::String(b"hourly".to_vec())),
        (name("args"), Value::Array(Vec::new())),
        (name("interval"), Value::Int(3_600)),
    ]);
    assert_eq!(event, Some(&expected));
    let daily = crons[1].1.as_array().unwrap();
    assert_eq!(daily[0].0, name("wp_scheduled_auto_draft_delete"));
    assert_eq!(
        daily[1].0,
        name("wp_privacy_personal_data_cleanup_requests")
    );
}

/// [`unschedule`] of `event`.
fn unschedule_event(crons: &mut Crons, event: &Event) {
    remove(crons, Some(event.at), event.job.hook(), &key(&event.args));
}

#[test]
fn unscheduling_takes_the_event_from_its_timestamp_only() {
    let mut crons = vec![
        at(
            Key::Int(1),
            &[("wp_scheduled_delete", None), ("other", None)],
        ),
        at(Key::Int(2), &[("wp_scheduled_delete", None)]),
        (Key::Int(3), Value::String(b"broken".to_vec())),
    ];
    // Other empty entries, which PHP only drops at the event's own place.
    crons.push(at(Key::Int(4), &[]));
    let extra = [
        (name("wp_scheduled_delete"), Value::String(b"x".to_vec())),
        (name("empty"), Value::Array(Vec::new())),
    ];
    if let Value::Array(hooks) = &mut crons[0].1 {
        hooks.extend(extra.clone());
    }
    let event = |at| recurring(Job::ScheduledDelete, at, ("daily", 86_400));
    unschedule_event(&mut crons, &event(1));
    unschedule_event(&mut crons, &event(3));
    let mut expected = vec![
        at(Key::Int(1), &[("other", None)]),
        at(Key::Int(2), &[("wp_scheduled_delete", None)]),
        (Key::Int(3), Value::String(b"broken".to_vec())),
        at(Key::Int(4), &[]),
    ];
    if let Value::Array(hooks) = &mut expected[0].1 {
        hooks.extend(extra);
    }
    assert_eq!(crons, expected);
    // A timestamp left empty goes, and another key of the hook stays.
    unschedule_event(&mut crons, &event(2));
    assert_eq!(crons.len(), 3);
    let mut two = vec![at(Key::Int(2), &[("publish_future_post", Some(7))])];
    let post = Event {
        args: args(Some(8)),
        recurrence: None,
        ..event(2)
    };
    let post = Event {
        job: Job::PublishFuturePost,
        ..post
    };
    unschedule_event(&mut two, &post);
    assert_eq!(two, [at(Key::Int(2), &[("publish_future_post", Some(7))])]);
}

#[test]
fn each_job_has_its_hook() {
    for job in Job::ALL {
        assert_eq!(Job::of(&name(job.hook())), Some(job));
    }
    assert_eq!(Job::of(&name("wp_version_check")), None);
}

/// An event's entry as PHP stores it.
fn stored(schedule: Value, args: Value) -> Value {
    Value::Array(vec![(name("schedule"), schedule), (name("args"), args)])
}

/// The hooks of a timestamp.
fn hooks(entries: Vec<(&str, Value)>) -> Value {
    let entries = entries
        .into_iter()
        .map(|(hook, events)| (name(hook), events));
    Value::Array(entries.collect())
}

/// One entry under the key of no arguments.
fn one(event: Value) -> Value {
    Value::Array(vec![(key(&args(None)), event)])
}

#[tokio::test]
async fn due_events_are_those_of_hooks_hyscribe_runs_in_php_s_order() {
    let daily = || text("daily");
    let none = || Value::Array(Vec::new());
    let crons = vec![
        (
            Key::Int(NOW - 10),
            hooks(vec![
                ("wp_version_check", one(stored(daily(), none()))),
                ("wp_scheduled_delete", one(stored(daily(), none()))),
                ("delete_expired_transients", Value::Int(1)),
                ("wp_scheduled_auto_draft_delete", one(Value::Int(1))),
                // A plugin's recurrence, or no arguments array: left for PHP.
                (
                    "wp_privacy_delete_old_export_files",
                    one(stored(text("often"), none())),
                ),
                (
                    "wp_privacy_personal_data_cleanup_requests",
                    one(stored(daily(), Value::Int(1))),
                ),
            ]),
        ),
        (Key::Int(NOW - 5), Value::Int(1)),
        (
            Key::Int(NOW),
            hooks(vec![(
                "publish_future_post",
                one(stored(text(""), args(Some(7)))),
            )]),
        ),
        (
            Key::Int(NOW + 1),
            hooks(vec![("wp_scheduled_delete", one(stored(daily(), none())))]),
        ),
    ];
    let mut later = crons.clone();
    later.insert(2, (Key::String(b"x".to_vec()), Value::Int(1)));
    let publish = Event {
        args: args(Some(7)),
        recurrence: None,
        ..recurring(Job::PublishFuturePost, NOW, ("", 0))
    };
    let expected = [
        recurring(Job::ScheduledDelete, NOW - 10, ("daily", 86_400)),
        publish,
    ];
    rolled_back(async |w| {
        _set_cron_array(w, crons).await.unwrap();
        assert_eq!(wp_get_ready_cron_jobs(w, NOW).await.unwrap(), expected);
        // A key that is not a timestamp ends the due ones.
        _set_cron_array(w, later).await.unwrap();
        assert_eq!(wp_get_ready_cron_jobs(w, NOW).await.unwrap(), expected[..1]);
        w.fail_after(0);
        assert!(matches!(
            wp_get_ready_cron_jobs(w, NOW).await,
            Err(Error::Db(_))
        ));
    })
    .await;
}

/// Where the tests' uploads would be: a folder that does not exist.
fn no_uploads() -> std::path::PathBuf {
    std::env::temp_dir().join("hyscribe-no-uploads")
}

#[tokio::test]
async fn the_loop_runs_hyscribe_s_hooks_and_leaves_the_rest_as_php_does() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = &writer().await;
    // One second for both, as PHP's run had: the loop's events are due at the setup's time.
    let now = crate::now();
    let setup = async |conn: &mut Connection| {
        let id = missed(writer, conn).await?;
        trash(writer, conn).await?;
        transients(writer, conn).await?;
        act(writer, conn, &Context { now, ..admin() }, async |w, ctx| {
            let mut crons = Vec::new();
            for (hook, every) in [
                ("wp_scheduled_delete", ("daily", 86_400)),
                ("wp_version_check", ("twicedaily", 43_200)),
                ("delete_expired_transients", ("daily", 86_400)),
                ("oracle_plugin_hook", ("hourly", 3_600)),
                ("wp_privacy_delete_old_export_files", ("hourly", 3_600)),
                (
                    "wp_privacy_personal_data_cleanup_requests",
                    ("daily", 86_400),
                ),
            ] {
                put_recurring(&mut crons, ctx.now, hook, args(None), every);
            }
            let args = args(Some(id));
            let single = vec![
                (name("schedule"), Value::Bool(false)),
                (name("args"), args.clone()),
            ];
            put(
                &mut crons,
                ctx.now,
                "publish_future_post",
                key(&args),
                single,
            );
            _set_cron_array(w, crons).await.map(drop)
        })
        .await
    };
    let action = async |conn: &mut Connection, ()| {
        act(writer, conn, &Context { now, ..cron() }, async |w, ctx| {
            for event in wp_get_ready_cron_jobs(w, ctx.now).await? {
                run(w, ctx, &no_uploads(), &event).await?;
            }
            Ok(())
        })
        .await
    };
    check("cron-loop", setup, action).await;
}

#[tokio::test]
async fn running_an_event_stops_at_a_database_error() {
    let ctx = cron();
    fails_at_each_step(
        async |w| {
            let mut crons = Vec::new();
            for job in Job::ALL {
                put_recurring(
                    &mut crons,
                    ctx.now,
                    job.hook(),
                    args(None),
                    ("daily", 86_400),
                );
            }
            _set_cron_array(w, crons).await.unwrap();
            wp_get_ready_cron_jobs(w, ctx.now).await.unwrap()
        },
        async |w, due| {
            for event in &due {
                run(w, &ctx, &no_uploads(), event).await?;
            }
            Ok(())
        },
    )
    .await;
}

#[test]
fn schedules_are_core_s_after_the_filter_s() {
    let named = |list: &[(&str, i64)]| -> Vec<Schedule> {
        list.iter().map(|(n, i)| (n.to_string(), *i)).collect()
    };
    let core = [
        ("hourly", 3_600),
        ("twicedaily", 43_200),
        ("daily", 86_400),
        ("weekly", 604_800),
    ];
    assert_eq!(wp_get_schedules(), named(&core));
    let added = named(&[("often", 60), ("daily", 5)]);
    assert_eq!(cron_schedules(added.clone()), added);
    let expected = [
        ("often", 60),
        ("daily", 86_400),
        ("hourly", 3_600),
        ("twicedaily", 43_200),
        ("weekly", 604_800),
    ];
    assert_eq!(with_core_schedules(added), named(&expected));
}

#[tokio::test]
async fn events_are_scheduled_found_and_unscheduled_by_name() {
    let none = Value::Array(Vec::new());
    let post = args(Some(7));
    rolled_back(async |w| {
        _set_cron_array(w, Vec::new()).await.unwrap();
        // A timestamp below 1, or an unknown recurrence, schedules nothing.
        for (at, every) in [(0, "daily"), (NOW, "often")] {
            assert!(!wp_schedule_event(w, at, every, "h", &none).await.unwrap());
        }
        assert_eq!(wp_next_scheduled(w, "h", &none).await.unwrap(), None);
        assert!(
            wp_schedule_event(w, NOW + 9, "hourly", "h", &post)
                .await
                .unwrap()
        );
        assert!(
            wp_schedule_event(w, NOW, "weekly", "h", &post)
                .await
                .unwrap()
        );
        // The same event again changes nothing.
        assert!(
            !wp_schedule_event(w, NOW, "weekly", "h", &post)
                .await
                .unwrap()
        );
        assert_eq!(wp_next_scheduled(w, "h", &post).await.unwrap(), Some(NOW));
        assert_eq!(wp_next_scheduled(w, "h", &none).await.unwrap(), None);
        let event = _get_cron_array(w).await.unwrap()[0]
            .1
            .get_path(&["h"])
            .cloned();
        let stored = Value::Array(vec![(
            key(&post),
            Value::Array(vec![
                (name("schedule"), text("weekly")),
                (name("args"), post.clone()),
                (name("interval"), Value::Int(604_800)),
            ]),
        )]);
        assert_eq!(event, Some(stored));
        assert!(!wp_unschedule_event(w, 0, "h", &post).await.unwrap());
        assert!(wp_unschedule_event(w, NOW, "h", &post).await.unwrap());
        assert!(!wp_unschedule_event(w, NOW, "h", &post).await.unwrap());
        assert_eq!(
            wp_next_scheduled(w, "h", &post).await.unwrap(),
            Some(NOW + 9)
        );
        // An event at timestamp 0 is none, as `! $next` reads it.
        _set_cron_array(w, vec![at(Key::Int(0), &[("h", None)])])
            .await
            .unwrap();
        assert_eq!(wp_next_scheduled(w, "h", &none).await.unwrap(), None);
        w.fail_after(0);
        for result in [
            wp_schedule_event(w, NOW, "daily", "h", &none).await,
            wp_unschedule_event(w, NOW, "h", &none).await,
        ] {
            assert!(matches!(result, Err(Error::Db(_))));
        }
        let next = wp_next_scheduled(w, "h", &none).await;
        assert!(matches!(next, Err(Error::Db(_))));
    })
    .await;
}

/// The events of `hooks` cleared, as `wp_clear_scheduled_hook( $hook )` does.
async fn cleared(write: &mut Write<'_>, hooks: &[&str]) -> Result<()> {
    let mut crons = _get_cron_array(write).await?;
    for hook in hooks {
        remove(&mut crons, None, hook, &key(&args(None)));
    }
    _set_cron_array(write, crons).await.map(drop)
}

/// What a page view of the admin schedules, by hook.
const VIEW_EVENTS: [&str; 4] = [
    "wp_privacy_delete_old_export_files",
    "wp_privacy_personal_data_cleanup_requests",
    "wp_scheduled_delete",
    "delete_expired_transients",
];

#[tokio::test]
async fn an_admin_page_view_schedules_its_events_as_php_does() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = &writer().await;
    let none = Value::Array(Vec::new());
    let view = async |conn: &mut Connection, ()| {
        act(writer, conn, &admin(), async |w, ctx| {
            schedule_admin_events(w, ctx.now).await
        })
        .await
    };
    check(
        "cron-schedule-events",
        async |conn| {
            act(writer, conn, &admin(), async |w, _| {
                cleared(w, &VIEW_EVENTS).await
            })
            .await
        },
        view,
    )
    .await;
    // One event scheduled for later keeps its time; the others are scheduled now.
    check(
        "cron-schedule-events-some",
        async |conn| {
            act(writer, conn, &admin(), async |w, ctx| {
                cleared(w, &VIEW_EVENTS).await?;
                let at = ctx.now + 3_600;
                wp_schedule_event(w, at, "daily", "delete_expired_transients", &none).await?;
                Ok(())
            })
            .await
        },
        view,
    )
    .await;
    // With all four scheduled, a view writes nothing.
    check(
        "cron-schedule-events-present",
        async |conn| {
            act(writer, conn, &admin(), async |w, ctx| {
                cleared(w, &VIEW_EVENTS).await?;
                schedule_admin_events(w, ctx.now).await
            })
            .await
        },
        view,
    )
    .await;
}

#[tokio::test]
async fn scheduling_an_admin_page_view_s_events_stops_at_a_database_error() {
    fails_at_each_step(
        async |w| cleared(w, &VIEW_EVENTS).await.unwrap(),
        async |w, ()| schedule_admin_events(w, NOW).await,
    )
    .await;
}

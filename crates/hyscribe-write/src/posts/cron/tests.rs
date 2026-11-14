// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

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

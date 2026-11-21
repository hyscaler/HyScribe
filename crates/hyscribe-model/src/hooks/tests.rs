// SPDX-License-Identifier: AGPL-3.0-or-later

use std::sync::Mutex;

use super::*;

/// Answers each filter with `answer`, and logs what it was handed.
struct Fixed {
    answer: Value,
    seen: Mutex<Vec<(String, Value, Vec<Value>)>>,
}

impl Dispatch for Fixed {
    fn attached(&self, hook: &str) -> bool {
        hook != "unattached"
    }

    fn filter(&self, hook: &str, value: Value, args: &[Value]) -> Value {
        self.seen
            .lock()
            .map(|mut seen| seen.push((hook.into(), value, args.to_vec())))
            .ok();
        self.answer.clone()
    }

    fn action(&self, hook: &str, args: &[Value]) {
        self.filter(hook, Value::Null, args);
    }
}

fn fixed(answer: Value) -> (Arc<Fixed>, Arc<dyn Dispatch>) {
    let fixed = Arc::new(Fixed {
        answer,
        seen: Mutex::new(Vec::new()),
    });
    let dispatch: Arc<dyn Dispatch> = fixed.clone();
    (fixed, dispatch)
}

fn seen(fixed: &Fixed) -> Vec<(String, Value, Vec<Value>)> {
    fixed
        .seen
        .lock()
        .map(|seen| seen.clone())
        .unwrap_or_default()
}

fn text(s: &str) -> Value {
    Value::String(s.as_bytes().to_vec())
}

fn texts(all: &[&str]) -> Value {
    Value::Array(
        all.iter()
            .zip(0..)
            .map(|(s, n)| (Key::Int(n), text(s)))
            .collect(),
    )
}

/// What a filter on `value` gives back with `answer` attached.
fn filtered<T: 'static>(value: T, answer: Value) -> T {
    let (_, dispatch) = fixed(answer);
    scoped(&dispatch, || apply_filters("hook", value, &[]))
}

#[test]
fn plain_calls_without_a_dispatcher() {
    assert_eq!(apply_filters("hook", 5_i64, &[]), 5);
    do_action("hook", &[&1_i64]);
    let (fixed, dispatch) = fixed(Value::Int(9));
    assert_eq!(
        scoped(&dispatch, || apply_filters("unattached", 5_i64, &[])),
        5
    );
    scoped(&dispatch, || do_action("unattached", &[]));
    assert_eq!(seen(&fixed), Vec::new());
}

#[test]
fn values_read_back_as_their_type() {
    assert_eq!(filtered(text("a"), Value::Int(1)), Value::Int(1));
    assert!(filtered(false, Value::Bool(true)));
    assert!(!filtered(false, Value::Int(1)));
    assert_eq!(filtered(1_i64, Value::Int(-2)), -2);
    assert_eq!(filtered(1_i64, text("2")), 1);
    assert_eq!(filtered(1_i32, Value::Int(2)), 2);
    assert_eq!(filtered(1_i32, Value::Int(i64::MAX)), 1);
    assert_eq!(filtered(1_u64, Value::Int(2)), 2);
    assert_eq!(filtered(1_u64, Value::Int(-2)), 1);
    assert_eq!(filtered(1.5_f64, Value::Float(Float::from_f64(2.5))), 2.5);
    assert_eq!(filtered(1.5_f64, Value::Int(3)), 3.0);
    assert_eq!(filtered(1.5_f64, Value::Null), 1.5);
    assert_eq!(filtered(String::from("a"), text("b")), "b");
    assert_eq!(filtered(String::from("a"), Value::String(vec![0xff])), "a");
    assert_eq!(filtered(String::from("a"), Value::Null), "a");
    assert_eq!(
        filtered(b"a".to_vec(), Value::String(vec![0xff])),
        vec![0xff]
    );
    assert_eq!(filtered(b"a".to_vec(), Value::Int(1)), b"a");
    let names = vec![String::from("a")];
    assert_eq!(filtered(names.clone(), texts(&["b", "c"])), ["b", "c"]);
    assert_eq!(
        filtered(
            names.clone(),
            Value::Array(vec![(Key::Int(0), Value::Int(1))])
        ),
        names
    );
    let bad = Value::Array(vec![(Key::Int(0), Value::String(vec![0xff]))]);
    assert_eq!(filtered(names.clone(), bad.clone()), names);
    assert_eq!(filtered(names.clone(), text("b")), names);
    let raw = vec![b"a".to_vec()];
    assert_eq!(filtered(raw.clone(), bad), [vec![0xff]]);
    assert_eq!(filtered(raw.clone(), Value::Null), raw);
}

#[test]
fn what_does_not_read_back_does_not_cross() {
    let (fixed, dispatch) = fixed(Value::Int(9));
    let unread = scoped(&dispatch, || {
        (
            apply_filters("hook", 3_usize, &[]),
            apply_filters("hook", "static", &[]),
            apply_filters("hook", u64::MAX, &[]),
            apply_filters("hook", (1, 2), &[]),
            apply_filters("hook", 1_i64, &[&(1, 2)]),
        )
    });
    assert_eq!(unread, (3, "static", u64::MAX, (1, 2), 1));
    assert_eq!(seen(&fixed), Vec::new());
}

#[test]
fn arguments_cross_as_php_holds_them() {
    let (fixed, dispatch) = fixed(Value::Null);
    scoped(&dispatch, || {
        let args: [&dyn Any; 12] = [
            &text("v"),
            &true,
            &-1_i64,
            &2_i32,
            &3_u64,
            &4_usize,
            &0.5_f64,
            &String::from("s"),
            &b"b".to_vec(),
            &"st",
            &vec![String::from("x")],
            &vec![b"y".to_vec()],
        ];
        do_action("action", &args);
    });
    let expected = vec![
        text("v"),
        Value::Bool(true),
        Value::Int(-1),
        Value::Int(2),
        Value::Int(3),
        Value::Int(4),
        Value::Float(Float::from_f64(0.5)),
        text("s"),
        text("b"),
        text("st"),
        texts(&["x"]),
        texts(&["y"]),
    ];
    assert_eq!(
        seen(&fixed),
        [("action".to_string(), Value::Null, expected)]
    );
    scoped(&dispatch, || do_action("action", &[&u64::MAX]));
    scoped(&dispatch, || do_action("action", &[&usize::MAX]));
    assert_eq!(seen(&fixed).len(), 1);
}

#[test]
fn filters_hand_their_value_and_arguments() {
    let (fixed, dispatch) = fixed(Value::Int(7));
    let value = scoped(&dispatch, || {
        apply_filters("expires", 86_400_i64, &[&1_u64])
    });
    assert_eq!(value, 7);
    let expected = [(
        "expires".to_string(),
        Value::Int(86_400),
        vec![Value::Int(1)],
    )];
    assert_eq!(seen(&fixed), expected);
}

#[test]
fn scopes_nest_and_restore() {
    let (outer, outer_dispatch) = fixed(Value::Int(1));
    let (inner, inner_dispatch) = fixed(Value::Int(2));
    let values = scoped(&outer_dispatch, || {
        let inside = scoped(&inner_dispatch, || apply_filters("hook", 0_i64, &[]));
        (inside, apply_filters("hook", 0_i64, &[]))
    });
    assert_eq!(values, (2, 1));
    assert_eq!((seen(&outer).len(), seen(&inner).len()), (1, 1));
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        scoped::<()>(&inner_dispatch, || {
            std::panic::resume_unwind(Box::new("in scope"))
        })
    }));
    assert!(panicked.is_err());
    assert_eq!(apply_filters("hook", 0_i64, &[]), 0);
}

#[test]
fn work_carried_to_another_thread_keeps_its_dispatcher() {
    let (fixed, dispatch) = fixed(Value::Int(3));
    let carried = scoped(&dispatch, || carry(|| apply_filters("hook", 0_i64, &[])));
    let plain = carry(|| apply_filters("hook", 0_i64, &[]));
    let threads = std::thread::spawn(move || (carried(), plain()));
    assert_eq!(threads.join().ok(), Some((3, 0)));
    assert_eq!(seen(&fixed).len(), 1);
}

/// Pending once, then ready with what the filter gave at each poll.
struct Twice(Vec<i64>);

impl Future for Twice {
    type Output = Vec<i64>;

    fn poll(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Vec<i64>> {
        self.0.push(apply_filters("hook", 0_i64, &[]));
        if self.0.len() < 2 {
            return Poll::Pending;
        }
        Poll::Ready(self.0.clone())
    }
}

#[test]
fn a_future_within_a_dispatcher_has_it_at_each_poll_only() {
    let (_, dispatch) = fixed(Value::Int(4));
    let mut cx = Context::from_waker(std::task::Waker::noop());
    let mut future = within(Some(dispatch), Twice(Vec::new()));
    assert_eq!(Pin::new(&mut future).poll(&mut cx), Poll::Pending);
    assert_eq!(apply_filters("hook", 0_i64, &[]), 0);
    let polled = Pin::new(&mut future).poll(&mut cx);
    assert_eq!(polled, Poll::Ready(vec![4, 4]));
    let mut plain = within(None, Twice(Vec::new()));
    assert_eq!(Pin::new(&mut plain).poll(&mut cx), Poll::Pending);
    let polled = Pin::new(&mut plain).poll(&mut cx);
    assert_eq!(polled, Poll::Ready(vec![0, 0]));
}

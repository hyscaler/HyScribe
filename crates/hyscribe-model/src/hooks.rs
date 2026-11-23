// SPDX-License-Identifier: AGPL-3.0-or-later

//! WordPress's hooks under their names and argument order (docs/EXTENSIONS.md).
//! With no dispatcher on the thread they are plain calls, so a filter gives
//! its value back. Within [`scoped`], a hook an extension attached to crosses
//! to the extension runtime (ADR 0022) as PHP values: booleans, integers,
//! floats, strings, PHP values and lists of strings. A hook whose value or
//! arguments are of another type does not cross yet, and its value stays
//! as it was, as does a value whose answer does not read back as its type.
//! What an answer gives goes through the same checks as the value it
//! replaced. The embedder enters the scope for each request: [`within`] for
//! its future, wherever it is polled, and [`carry`] for work it hands to a
//! blocking thread. An action that prints runs core's callbacks among the
//! extensions' by priority, and an extension's callback prints the string
//! it answers there (ADR 0023's 2026-11-23 amendment).

use std::any::Any;
use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use hyscribe_phpser::{Float, Key, Value};

/// What runs the callbacks extensions attached: the runtime's request.
pub trait Dispatch: Send + Sync {
    /// Whether any extension has a callback on `hook`.
    fn attached(&self, hook: &str) -> bool;
    /// `apply_filters( $hook, $value, ...$args )` over the extensions' callbacks.
    fn filter(&self, hook: &str, value: Value, args: &[Value]) -> Value;
    /// `do_action( $hook, ...$args )` over the extensions' callbacks.
    fn action(&self, hook: &str, args: &[Value]);
    /// [`Dispatch::action`] of an action that prints: before the callbacks
    /// of each priority, `core( Some( $priority ) )` prints core's up to it,
    /// and `core( None )` the rest after the last; what all print, in order.
    fn printing(
        &self,
        hook: &str,
        args: &[Value],
        core: &mut dyn FnMut(Option<i32>) -> Vec<u8>,
    ) -> Vec<u8> {
        self.action(hook, args);
        core(None)
    }
}

/// A callback core adds to an action that prints, at its priority: what it
/// prints when it runs.
pub type Core<'a> = (i32, Box<dyn FnOnce() -> Vec<u8> + 'a>);

thread_local! {
    static DISPATCH: RefCell<Option<Arc<dyn Dispatch>>> = const { RefCell::new(None) };
}

/// Puts the previous dispatcher back, also when the scope panics.
struct Restore(Option<Arc<dyn Dispatch>>);

impl Drop for Restore {
    fn drop(&mut self) {
        let previous = self.0.take();
        DISPATCH.with(|dispatch| dispatch.replace(previous));
    }
}

/// Runs `run` with `dispatch` answering the hooks fired on this thread.
pub fn scoped<R>(dispatch: &Arc<dyn Dispatch>, run: impl FnOnce() -> R) -> R {
    let previous = DISPATCH.with(|current| current.replace(Some(dispatch.clone())));
    let _restore = Restore(previous);
    run()
}

/// The dispatcher on this thread, if any.
fn current() -> Option<Arc<dyn Dispatch>> {
    DISPATCH.with(|dispatch| dispatch.borrow().clone())
}

/// The dispatcher on this thread, when an extension attached to `hook`.
fn attached(hook: &str) -> Option<Arc<dyn Dispatch>> {
    current().filter(|dispatch| dispatch.attached(hook))
}

/// Runs `run` with `dispatch`, or as plain calls without one.
fn maybe<R>(dispatch: Option<&Arc<dyn Dispatch>>, run: impl FnOnce() -> R) -> R {
    match dispatch {
        Some(dispatch) => scoped(dispatch, run),
        None => run(),
    }
}

/// `run`, to be run on another thread with the dispatcher of this one.
pub fn carry<R>(run: impl FnOnce() -> R + Send) -> impl FnOnce() -> R + Send {
    let dispatch = current();
    move || maybe(dispatch.as_ref(), run)
}

/// A future answering the hooks it fires with its dispatcher, set on the
/// thread for each poll only, so tasks sharing a thread keep their own.
pub struct Within<F> {
    dispatch: Option<Arc<dyn Dispatch>>,
    future: Pin<Box<F>>,
}

/// `future` with `dispatch` answering its hooks, or as plain calls.
pub fn within<F: Future>(dispatch: Option<Arc<dyn Dispatch>>, future: F) -> Within<F> {
    Within {
        dispatch,
        future: Box::pin(future),
    }
}

impl<F: Future> Future for Within<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        let this = self.get_mut();
        maybe(this.dispatch.as_ref(), || this.future.as_mut().poll(cx))
    }
}

fn bytes(text: &[u8]) -> Value {
    Value::String(text.to_vec())
}

fn list<T>(items: &[T], each: impl Fn(&T) -> Value) -> Value {
    let items = items.iter().zip(0..);
    Value::Array(items.map(|(item, n)| (Key::Int(n), each(item))).collect())
}

/// A hook's value or argument as PHP holds it, none for a type that does
/// not cross.
fn to_php(value: &dyn Any) -> Option<Value> {
    if let Some(value) = value.downcast_ref::<Value>() {
        Some(value.clone())
    } else if let Some(b) = value.downcast_ref::<bool>() {
        Some(Value::Bool(*b))
    } else if let Some(n) = value.downcast_ref::<i64>() {
        Some(Value::Int(*n))
    } else if let Some(n) = value.downcast_ref::<i32>() {
        Some(Value::Int(i64::from(*n)))
    } else if let Some(n) = value.downcast_ref::<u64>() {
        i64::try_from(*n).ok().map(Value::Int)
    } else if let Some(n) = value.downcast_ref::<usize>() {
        i64::try_from(*n).ok().map(Value::Int)
    } else if let Some(f) = value.downcast_ref::<f64>() {
        Some(Value::Float(Float::from_f64(*f)))
    } else if let Some(text) = value.downcast_ref::<String>() {
        Some(bytes(text.as_bytes()))
    } else if let Some(text) = value.downcast_ref::<Vec<u8>>() {
        Some(bytes(text))
    } else if let Some(text) = value.downcast_ref::<&'static str>() {
        Some(bytes(text.as_bytes()))
    } else if let Some(texts) = value.downcast_ref::<Vec<String>>() {
        Some(list(texts, |text| bytes(text.as_bytes())))
    } else {
        let texts = value.downcast_ref::<Vec<Vec<u8>>>();
        texts.map(|texts| list(texts, |text| bytes(text)))
    }
}

/// The text of a string, for a string.
fn text(value: &Value) -> Option<&[u8]> {
    match value {
        Value::String(text) => Some(text),
        _ => None,
    }
}

/// An integer, for an integer.
fn int(value: &Value) -> Option<i64> {
    match value {
        Value::Int(n) => Some(*n),
        _ => None,
    }
}

/// The strings of a list of strings.
fn texts(value: &Value) -> Option<Vec<Vec<u8>>> {
    let items = value.as_array()?.iter();
    items
        .map(|(_, item)| text(item).map(<[u8]>::to_vec))
        .collect()
}

fn utf8(text: Vec<u8>) -> Option<String> {
    String::from_utf8(text).ok()
}

fn put<T>(slot: &mut T, new: Option<T>) {
    if let Some(new) = new {
        *slot = new;
    }
}

/// Whether a filter's answer can be read back as the type of `value`;
/// `usize` and `&str` cross as arguments only.
fn readable(value: &dyn Any) -> bool {
    !(value.is::<usize>() || value.is::<&'static str>())
}

/// Takes `answer` as the new `value` where it reads back as its type, and
/// leaves `value` as it was otherwise.
fn read_back(value: &mut dyn Any, answer: Value) {
    if let Some(slot) = value.downcast_mut::<Value>() {
        *slot = answer;
    } else if let Some(slot) = value.downcast_mut::<bool>() {
        let b = match answer {
            Value::Bool(b) => Some(b),
            _ => None,
        };
        put(slot, b);
    } else if let Some(slot) = value.downcast_mut::<i64>() {
        put(slot, int(&answer));
    } else if let Some(slot) = value.downcast_mut::<i32>() {
        put(slot, int(&answer).and_then(|n| i32::try_from(n).ok()));
    } else if let Some(slot) = value.downcast_mut::<u64>() {
        put(slot, int(&answer).and_then(|n| u64::try_from(n).ok()));
    } else if let Some(slot) = value.downcast_mut::<f64>() {
        let f = match answer {
            Value::Float(f) => Some(f.to_f64()),
            Value::Int(n) => Some(n as f64),
            _ => None,
        };
        put(slot, f);
    } else if let Some(slot) = value.downcast_mut::<String>() {
        put(slot, text(&answer).map(<[u8]>::to_vec).and_then(utf8));
    } else if let Some(slot) = value.downcast_mut::<Vec<u8>>() {
        put(slot, text(&answer).map(<[u8]>::to_vec));
    } else if let Some(slot) = value.downcast_mut::<Vec<String>>() {
        let all = texts(&answer).map(|all| all.into_iter().map(utf8).collect());
        put(slot, all.flatten());
    } else {
        // The last type `to_php` and `readable` let through.
        let slot = value.downcast_mut::<Vec<Vec<u8>>>();
        if let (Some(slot), Some(new)) = (slot, texts(&answer)) {
            *slot = new;
        }
    }
}

/// `apply_filters( $hook, $value, ...$args )`.
pub fn apply_filters<T: 'static>(hook: &str, mut value: T, args: &[&dyn Any]) -> T {
    let Some(dispatch) = attached(hook) else {
        return value;
    };
    let given = args
        .iter()
        .map(|arg| to_php(*arg))
        .collect::<Option<Vec<_>>>();
    let php = to_php(&value).filter(|_| readable(&value));
    if let (Some(php), Some(given)) = (php, given) {
        read_back(&mut value, dispatch.filter(hook, php, &given));
    }
    value
}

/// The actions fired, by name, for tests to read.
#[cfg(feature = "hook-log")]
pub static FIRED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// `do_action( $hook, ...$args )`.
pub fn do_action(hook: &str, args: &[&dyn Any]) {
    #[cfg(feature = "hook-log")]
    FIRED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(hook.to_string());
    let given = args
        .iter()
        .map(|arg| to_php(*arg))
        .collect::<Option<Vec<_>>>();
    if let (Some(dispatch), Some(given)) = (attached(hook), given) {
        dispatch.action(hook, &given);
    }
}

/// `do_action( $hook, ...$args )` of an action that prints, core having
/// added `core` in that order: the extensions' callbacks run among them by
/// priority, after core's of the same priority; what all print, in order.
pub fn do_action_printing(hook: &str, args: &[&dyn Any], mut core: Vec<Core<'_>>) -> Vec<u8> {
    #[cfg(feature = "hook-log")]
    FIRED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(hook.to_string());
    core.sort_by_key(|(priority, _)| *priority);
    let mut core = core.into_iter().peekable();
    let mut run = |upto: Option<i32>| {
        let mut printed = Vec::new();
        while let Some((_, print)) = core.next_if(|(at, _)| upto.is_none_or(|upto| *at <= upto)) {
            printed.extend(print());
        }
        printed
    };
    let given = args
        .iter()
        .map(|arg| to_php(*arg))
        .collect::<Option<Vec<_>>>();
    match (attached(hook), given) {
        (Some(dispatch), Some(given)) => dispatch.printing(hook, &given, &mut run),
        _ => run(None),
    }
}

#[cfg(test)]
mod tests;

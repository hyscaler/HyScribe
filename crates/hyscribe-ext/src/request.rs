// SPDX-License-Identifier: AGPL-3.0-or-later

//! One request's view of the active extensions: a fresh instance per
//! extension, made the first time a hook reaches it and dropped with the
//! request; each call bounded by its deadline and the extension's budget.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use hyscribe_phpser::Value;
use wasmtime::Store;
use wasmtime::component::Func;

use crate::extension::{Answered, Args, Done};
use crate::faults::{Counted, Kind};
use crate::host::{Phase, State, begin, classify};
use crate::value::{Crossing, Lifted, Lowered, lift, lower_all};
use crate::{Extension, Extensions, Fault, Hook, Host, Site};

/// A fault in a request, for the log and the Plugins screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    /// The extension's slug.
    pub extension: String,
    /// The hook, shortcode, route or lifecycle call it happened on.
    pub hook: String,
    /// The callback, when one ran.
    pub callback: Option<u32>,
    /// What went wrong.
    pub fault: Fault,
}

/// What an extension's REST route answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rest {
    /// The callback's answer, once the permission callback allowed it.
    Answered(Value),
    /// The permission callback's answer, anything but `true`.
    Forbidden(Value),
    /// The extension did not answer: a fault, or no such route.
    Failed,
}

/// An extension's place in the request.
#[derive(Default)]
struct Slot {
    instance: Option<(Store<State>, Func)>,
    spent: Duration,
    stopped: bool,
}

/// A call's answer, or what stopped it.
type Outcome = Result<Result<Lifted, String>, (Kind, Fault)>;

/// What a callback was registered as, to honour its removal.
#[derive(Clone, Copy)]
pub(crate) enum Registered<'a> {
    Hook(&'a Hook),
    Shortcode(&'a str),
    Other,
}

/// How a chain's answers carry: `apply_filters()` takes each callback's
/// answer, `do_action()` none. A step of a chain an extension fired takes
/// the answers of callbacks added as filters, as the host cannot tell
/// which of the two the extension runs.
#[derive(Clone, Copy)]
pub(crate) enum Fired {
    Filter,
    Action,
    Step,
}

/// The request's state, which the stores reach through a weak reference.
pub(crate) struct Shared {
    engine: wasmtime::Engine,
    pub(crate) extensions: Arc<Extensions>,
    site: Arc<dyn Site>,
    me: Weak<Shared>,
    slots: Vec<Mutex<Slot>>,
    current: Mutex<Vec<String>>,
    did: Mutex<BTreeMap<String, u32>>,
    reports: Mutex<Vec<Report>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The active extensions in one request.
pub struct Request(pub(crate) Arc<Shared>);

impl Request {
    /// A request on `site`; no instance is made until a hook reaches one.
    pub fn new(host: &Host, extensions: &Arc<Extensions>, site: Arc<dyn Site>) -> Request {
        let slots = extensions.list.iter().map(|_| Mutex::default()).collect();
        Request(Arc::new_cyclic(|me| Shared {
            engine: host.engine.clone(),
            extensions: extensions.clone(),
            site,
            me: me.clone(),
            slots,
            current: Mutex::default(),
            did: Mutex::default(),
            reports: Mutex::default(),
        }))
    }

    /// The place of the extension `slug` in the request's set.
    pub fn index(&self, slug: &str) -> Option<usize> {
        let list = self.0.extensions.list.iter();
        list.into_iter()
            .position(|extension| *extension.slug == *slug)
    }

    /// Whether any extension has a callback on `hook`: the host's own
    /// lookup, no crossing.
    pub fn attached(&self, hook: &str) -> bool {
        self.0.extensions.chains.contains_key(hook)
    }

    /// `apply_filters( $hook, $value, ...$args )` over the extensions'
    /// callbacks. A callback that faults leaves the value as it was.
    pub fn filter(&self, hook: &str, value: Value, args: &[Value]) -> Value {
        let mut all = vec![value];
        all.extend_from_slice(args);
        self.0
            .fire(hook, self.0.chain(hook), &mut all, Fired::Filter);
        all.swap_remove(0)
    }

    /// `do_action( $hook, ...$args )` over the extensions' callbacks.
    pub fn action(&self, hook: &str, args: &[Value]) {
        *lock(&self.0.did).entry(hook.to_string()).or_default() += 1;
        self.0
            .fire(hook, self.0.chain(hook), &mut args.to_vec(), Fired::Action);
    }

    /// `do_shortcode_tag()` for a tag an extension added, the last to add
    /// it answering: `( $atts, $content, $tag )`.
    pub fn shortcode(&self, tag: &str, atts: Value, content: Value) -> Option<Value> {
        let list = self.0.extensions.list.iter().enumerate().rev();
        let (ext, callback) = list.into_iter().find_map(|(ext, extension)| {
            let added = extension.registered.shortcodes.iter().rev();
            added
                .into_iter()
                .find(|(added, _)| added == tag)
                .map(|&(_, callback)| (ext, callback))
        })?;
        let args = [atts, content, Value::String(tag.into())];
        self.0
            .call(ext, callback, tag, &args, Registered::Shortcode(tag))
    }

    /// A block's `render_callback`: `( $attributes, $content, $block )`.
    pub fn block(&self, ext: usize, block: usize, args: [Value; 3]) -> Option<Value> {
        let extension = self.0.extensions.list.get(ext)?;
        let registered = extension.registered.blocks.get(block)?;
        let render = registered.render?;
        self.0
            .call(ext, render, &registered.path, &args, Registered::Other)
    }

    /// A REST route of an extension: its permission callback first, and
    /// its callback only when that answered `true`.
    pub fn rest(&self, ext: usize, route: usize, request: Value) -> Rest {
        let found = self.0.extensions.list.get(ext);
        let Some(route) = found.and_then(|extension| extension.registered.routes.get(route)) else {
            return Rest::Failed;
        };
        let name = format!("{}{}", route.namespace, route.path);
        let args = [request];
        match self
            .0
            .call(ext, route.permission, &name, &args, Registered::Other)
        {
            Some(Value::Bool(true)) => {
                let answer = self
                    .0
                    .call(ext, route.callback, &name, &args, Registered::Other);
                answer.map_or(Rest::Failed, Rest::Answered)
            }
            Some(refused) => Rest::Forbidden(refused),
            None => Rest::Failed,
        }
    }

    /// The extension's `activate` export, in an instance of its own.
    pub fn activate(&self, ext: usize) -> Result<(), Fault> {
        self.0.lifecycle(ext, true)
    }

    /// The extension's `deactivate` export, in an instance of its own.
    pub fn deactivate(&self, ext: usize) -> Result<(), Fault> {
        self.0.lifecycle(ext, false)
    }

    /// The faults of the request so far.
    pub fn reports(&self) -> Vec<Report> {
        lock(&self.0.reports).clone()
    }
}

/// The subsystems' hook points reach the extensions through the request,
/// within `hyscribe_model::hooks::scoped`.
impl hyscribe_model::hooks::Dispatch for Request {
    fn attached(&self, hook: &str) -> bool {
        Request::attached(self, hook)
    }

    fn filter(&self, hook: &str, value: Value, args: &[Value]) -> Value {
        Request::filter(self, hook, value, args)
    }

    fn action(&self, hook: &str, args: &[Value]) {
        Request::action(self, hook, args);
    }
}

impl Shared {
    /// `hook`'s callbacks across the extensions, in the order they run.
    pub(crate) fn chain(&self, hook: &str) -> &[(usize, usize)] {
        self.extensions.chains.get(hook).map_or(&[], Vec::as_slice)
    }

    /// Runs `entries` of `hook`'s chain over `args`; a callback's answer
    /// becomes the first argument of the next as `fired` says.
    pub(crate) fn fire(
        &self,
        hook: &str,
        entries: &[(usize, usize)],
        args: &mut [Value],
        fired: Fired,
    ) {
        lock(&self.current).push(hook.to_string());
        let found = entries.iter().filter_map(|&entry| {
            let registered = self.extensions.hook(entry);
            registered.map(|registered| (entry.0, registered))
        });
        for (ext, registered) in found {
            let accepted = args.len().min(registered.accepted_args as usize);
            let given = args.get(..accepted).unwrap_or_default();
            let answer = self.call(
                ext,
                registered.callback,
                hook,
                given,
                Registered::Hook(registered),
            );
            let replaces = match fired {
                Fired::Filter => true,
                Fired::Action => false,
                Fired::Step => !registered.action,
            };
            if let (Some(answer), true, Some(first)) = (answer, replaces, args.first_mut()) {
                *first = answer;
            }
        }
        lock(&self.current).pop();
    }

    /// The hook the request is running, innermost first.
    pub(crate) fn current(&self) -> Option<String> {
        lock(&self.current).last().cloned()
    }

    /// `did_action( $hook )`.
    pub(crate) fn did_action(&self, hook: &str) -> u32 {
        lock(&self.did).get(hook).copied().unwrap_or_default()
    }

    /// Calls one callback of an extension. None when it was not called (it
    /// is running, stopped, disabled, removed it, or an argument cannot
    /// cross) or did not answer (a fault, its own error).
    pub(crate) fn call(
        &self,
        ext: usize,
        callback: u32,
        hook: &str,
        args: &[Value],
        registered: Registered<'_>,
    ) -> Option<Value> {
        // A slot held is the extension running: it is never entered twice.
        let slot = self.slots.get(ext).and_then(|slot| slot.try_lock().ok());
        let (Some(extension), Some(mut slot)) = (self.extensions.list.get(ext), slot) else {
            return None;
        };
        if slot.stopped || extension.disabled() || removed(&slot, registered) {
            return None;
        }
        let Some(crossing) = lower_all(args) else {
            self.report(
                extension,
                hook,
                Some(callback),
                Fault::Value("an argument does not cross".into()),
            );
            return None;
        };
        let lowered: Vec<Lowered<'_>> = crossing.iter().map(Crossing::lowered).collect();
        let budget = extension.limits.request().saturating_sub(slot.spent);
        let budget = budget.min(extension.limits.call());
        let (answer, used) = self.run(extension, ext, &mut slot, budget, (callback, &lowered));
        slot.spent += used;
        let fault = match answer {
            Ok(Ok(answer)) => match lift(answer) {
                Some(value) => return Some(value),
                None => Fault::Value("its answer does not decode".into()),
            },
            Ok(Err(why)) => Fault::Guest(why),
            Err((kind, fault)) => {
                slot.stopped = true;
                let at = self.site.now();
                let counted = Counted {
                    at,
                    kind,
                    hook: hook.to_string(),
                };
                if let Some(unrecorded) = extension.count(counted) {
                    self.report(extension, hook, Some(callback), unrecorded);
                }
                fault
            }
        };
        self.report(extension, hook, Some(callback), fault);
        None
    }

    /// The call itself, in the slot's instance, made first if need be, and
    /// the time it ran outside host calls.
    fn run(
        &self,
        extension: &Extension,
        ext: usize,
        slot: &mut Slot,
        budget: Duration,
        args: Args<'_>,
    ) -> (Outcome, Duration) {
        let (mut store, func) = match slot.instance.take() {
            Some((mut store, func)) => {
                begin(&mut store, budget);
                (store, Ok(func))
            }
            None => {
                let (mut store, instance) =
                    extension.instantiate(&self.engine, self.live(ext), budget);
                let invoke = instance.and_then(|instance| {
                    let export = extension.export(None);
                    let invoke = instance.get_typed_func::<Args<'_>, Answered>(&mut store, export);
                    invoke.map(|invoke| *invoke.func())
                });
                (store, invoke)
            }
        };
        let answer = func.and_then(|func| {
            let invoke = func.typed::<Args<'_>, Answered>(&store);
            let answer = invoke.and_then(|invoke| invoke.call(&mut store, args));
            answer.map(|(answer,)| (func, answer))
        });
        store.data_mut().chains.clear();
        let used = store.data().used();
        match answer {
            Ok((func, answer)) => {
                slot.instance = Some((store, func));
                (Ok(answer), used)
            }
            Err(e) => (Err(classify(&e, store.data())), used),
        }
    }

    /// The phase of a store running for extension `ext` in this request.
    fn live(&self, ext: usize) -> Phase {
        Phase::Live {
            site: self.site.clone(),
            shared: self.me.clone(),
            me: ext,
        }
    }

    fn lifecycle(&self, ext: usize, activate: bool) -> Result<(), Fault> {
        let extension = self
            .extensions
            .list
            .get(ext)
            .ok_or(Fault::Refused("no such extension".into()))?;
        if extension.disabled() {
            return Err(Fault::Disabled);
        }
        let budget = extension.limits.call();
        let (mut store, instance) = extension.instantiate(&self.engine, self.live(ext), budget);
        let answer = instance
            .and_then(|instance| {
                instance.get_typed_func::<(), Done>(&mut store, extension.export(Some(activate)))
            })
            .and_then(|lifecycle| lifecycle.call(&mut store, ()));
        let name = if activate { "activate" } else { "deactivate" };
        let fault = match answer {
            Ok((Ok(()),)) => return Ok(()),
            Ok((Err(why),)) => Fault::Guest(why),
            Err(e) => {
                let (kind, fault) = classify(&e, store.data());
                let at = self.site.now();
                if let Some(unrecorded) = extension.count(Counted {
                    at,
                    kind,
                    hook: name.into(),
                }) {
                    self.report(extension, name, None, unrecorded);
                }
                fault
            }
        };
        self.report(extension, name, None, fault.clone());
        Err(fault)
    }

    fn report(&self, extension: &Extension, hook: &str, callback: Option<u32>, fault: Fault) {
        lock(&self.reports).push(Report {
            extension: extension.slug.to_string(),
            hook: hook.to_string(),
            callback,
            fault,
        });
    }
}

/// Whether the extension removed this callback in the request.
fn removed(slot: &Slot, registered: Registered<'_>) -> bool {
    let Some((store, _)) = &slot.instance else {
        return false;
    };
    let state = store.data();
    match registered {
        Registered::Hook(hook) => {
            let key = (hook.hook.clone(), hook.callback, hook.priority);
            state.removed.contains(&key)
        }
        Registered::Shortcode(tag) => state.removed_shortcodes.contains(tag),
        Registered::Other => false,
    }
}

#[cfg(test)]
mod tests;

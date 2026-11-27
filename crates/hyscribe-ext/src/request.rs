// SPDX-License-Identifier: AGPL-3.0-or-later

//! One request's view of the active extensions: a fresh instance per
//! extension, made the first time a hook reaches it and dropped with the
//! request; each call bounded by its deadline and the extension's budget.
//! A theme has a second instance that renders its templates (ADR 0023),
//! as a component cannot be entered twice and a template's host calls fire
//! hooks the theme's callbacks answer; both share its budget.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use hyscribe_phpser::Value;
use wasmtime::Store;
use wasmtime::component::{ComponentExportIndex, ComponentNamedList, Func, Lift, Lower};

use crate::extension::{Answered, Args, Done, Rendered};
use crate::faults::{Counted, Kind};
use crate::host::{Phase, State, begin, classify};
use crate::value::{Crossing, Lowered, lift, lower_all};
use crate::{
    AdminPage, Extension, Extensions, Fault, Hook, Host, Route, Setting, Site, TemplateFile,
};

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

/// An instance made in the request, and the export it calls.
type Instance = Option<(Store<State>, Func)>;

/// What an extension spent of its budget in the request, and whether a
/// fault stopped it, across its instances.
#[derive(Clone, Copy, Default)]
struct Ledger {
    spent: Duration,
    stopped: bool,
}

/// A call's answer, or what stopped it.
type Outcome<R> = Result<R, (Kind, Fault)>;

/// What a callback was registered as, to honour its removal.
#[derive(Clone, Copy)]
pub(crate) enum Registered<'a> {
    Hook(&'a Hook),
    Shortcode(&'a str),
    Other,
}

/// How a chain's answers carry: `apply_filters()` takes each callback's
/// answer, `do_action()` none, and an action that prints prints the
/// strings answered. A step of a chain an extension fired takes the
/// answers of callbacks added as filters, as the host cannot tell which of
/// the two the extension runs.
#[derive(Clone, Copy)]
pub(crate) enum Fired {
    Filter,
    Action,
    Print,
    Step,
}

/// The request's state, which the stores reach through a weak reference.
pub(crate) struct Shared {
    engine: wasmtime::Engine,
    pub(crate) extensions: Arc<Extensions>,
    site: Arc<dyn Site>,
    me: Weak<Shared>,
    /// Each extension's instance for its callbacks.
    slots: Vec<Mutex<Instance>>,
    /// A theme's instance for `render`.
    renders: Vec<Mutex<Instance>>,
    ledgers: Vec<Mutex<Ledger>>,
    current: Mutex<Vec<String>>,
    did: Mutex<BTreeMap<String, u32>>,
    reports: Mutex<Vec<Report>>,
    /// `wp_unique_id()`'s count.
    unique: Mutex<u64>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The active extensions in one request.
pub struct Request(pub(crate) Arc<Shared>);

impl Request {
    /// A request on `site`; no instance is made until a hook reaches one.
    pub fn new(host: &Host, extensions: &Arc<Extensions>, site: Arc<dyn Site>) -> Request {
        let each = || extensions.list.iter().map(|_| Mutex::default()).collect();
        Request(Arc::new_cyclic(|me| Shared {
            engine: host.engine.clone(),
            extensions: extensions.clone(),
            site,
            me: me.clone(),
            slots: each(),
            renders: each(),
            ledgers: extensions.list.iter().map(|_| Mutex::default()).collect(),
            current: Mutex::default(),
            did: Mutex::default(),
            reports: Mutex::default(),
            unique: Mutex::default(),
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

    /// [`Request::action`] of an action that prints: before the callbacks
    /// of each priority, `core( Some( $priority ) )` prints core's up to
    /// it, then `core( None )` the rest; a callback prints the string it
    /// answers. What all print, in order.
    pub fn printing(
        &self,
        hook: &str,
        args: &[Value],
        core: &mut dyn FnMut(Option<i32>) -> Vec<u8>,
    ) -> Vec<u8> {
        *lock(&self.0.did).entry(hook.to_string()).or_default() += 1;
        let at = |entry: &(usize, usize)| self.0.extensions.hook(*entry).map(|h| h.priority);
        let mut printed = Vec::new();
        for entries in self.0.chain(hook).chunk_by(|a, b| at(a) == at(b)) {
            let priority = entries.first().and_then(at);
            printed.extend(
                priority
                    .map(|priority| core(Some(priority)))
                    .unwrap_or_default(),
            );
            let mut given = args.to_vec();
            printed.extend(self.0.fire(hook, entries, &mut given, Fired::Print));
        }
        printed.extend(core(None));
        printed
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

    /// The REST routes of the request's extensions, but a disabled one's:
    /// each with its extension's place and its own, in load order, then
    /// in the order registered.
    pub fn routes(&self) -> Vec<((usize, usize), Route)> {
        let list = self.0.extensions.list.iter().enumerate();
        let running = list.filter(|(_, extension)| !extension.disabled());
        let routes = running.flat_map(|(ext, extension)| {
            let each = extension.routes().iter().enumerate();
            each.map(move |(n, route)| ((ext, n), route.clone()))
        });
        routes.collect()
    }

    /// The callback `callback` of the REST route `route` of extension
    /// `ext` over `args`, if the route has it: its permission callback,
    /// its callback or an argument's. None when it did not answer.
    pub fn route(
        &self,
        (ext, route): (usize, usize),
        callback: u32,
        args: &[Value],
    ) -> Option<Value> {
        let extension = self.0.extensions.list.get(ext)?;
        let route = extension.routes().get(route).filter(|r| r.owns(callback))?;
        let name = route.full();
        self.0.call(ext, callback, &name, args, Registered::Other)
    }

    /// The active theme's template files, when a theme component is active.
    pub fn templates(&self) -> Option<&[TemplateFile]> {
        let theme = self.0.theme().map(|(_, extension, _)| extension);
        theme.map(Extension::templates)
    }

    /// The active theme's `render` of one of its template files, in its
    /// instance for rendering (ADR 0023). A fault stops the theme for the
    /// rest of the request, as in a callback.
    pub fn render(&self, file: &str) -> Result<Vec<u8>, Fault> {
        self.0.render(file)
    }

    /// The extension's `activate` export, in an instance of its own.
    pub fn activate(&self, ext: usize) -> Result<(), Fault> {
        self.0.lifecycle(ext, true)
    }

    /// The extension's `deactivate` export, in an instance of its own.
    pub fn deactivate(&self, ext: usize) -> Result<(), Fault> {
        self.0.lifecycle(ext, false)
    }

    /// The active extensions' admin pages (SDK 0.4): each with its
    /// extension's place, in load order, then in the order added.
    pub fn pages(&self) -> Vec<(usize, AdminPage)> {
        let list = self.0.extensions.list.iter().enumerate();
        let pages = list.flat_map(|(ext, extension)| {
            extension
                .pages()
                .iter()
                .map(move |page| (ext, page.clone()))
        });
        pages.collect()
    }

    /// What the admin page `slug` of extension `ext` prints where PHP runs
    /// `do_action( $page_hook )`: its callback's answer, as an action that
    /// prints prints it; nothing on a fault.
    pub fn page(&self, ext: usize, slug: &str) -> Vec<u8> {
        let extension = self.0.extensions.list.get(ext);
        let page =
            extension.and_then(|extension| extension.pages().iter().find(|page| page.slug == slug));
        let Some(page) = page else {
            return Vec::new();
        };
        let args = [Value::String(Vec::new())];
        match self
            .0
            .call(ext, page.callback, slug, &args, Registered::Other)
        {
            Some(Value::String(printed)) => printed,
            _ => Vec::new(),
        }
    }

    /// The active extensions' settings (`register_setting()`), in load
    /// order, then in the order registered.
    pub fn settings(&self) -> Vec<Setting> {
        let list = self.0.extensions.list.iter();
        let settings = list.flat_map(|extension| extension.settings().iter());
        settings.cloned().collect()
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

    fn printing(
        &self,
        hook: &str,
        args: &[Value],
        core: &mut dyn FnMut(Option<i32>) -> Vec<u8>,
    ) -> Vec<u8> {
        Request::printing(self, hook, args, core)
    }
}

impl Shared {
    /// `hook`'s callbacks across the extensions, in the order they run.
    pub(crate) fn chain(&self, hook: &str) -> &[(usize, usize)] {
        self.extensions.chains.get(hook).map_or(&[], Vec::as_slice)
    }

    /// Runs `entries` of `hook`'s chain over `args`; a callback's answer
    /// becomes the first argument of the next as `fired` says. What the
    /// callbacks of an action that prints print.
    pub(crate) fn fire(
        &self,
        hook: &str,
        entries: &[(usize, usize)],
        args: &mut [Value],
        fired: Fired,
    ) -> Vec<u8> {
        let mut printed = Vec::new();
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
                Fired::Print => {
                    if let Some(Value::String(text)) = &answer {
                        printed.extend_from_slice(text);
                    }
                    false
                }
                Fired::Step => !registered.action,
            };
            if let (Some(answer), true, Some(first)) = (answer, replaces, args.first_mut()) {
                *first = answer;
            }
        }
        lock(&self.current).pop();
        printed
    }

    /// The hook the request is running, innermost first.
    pub(crate) fn current(&self) -> Option<String> {
        lock(&self.current).last().cloned()
    }

    /// `wp_unique_id( $prefix )`: the prefix and the request's next number.
    pub(crate) fn unique_id(&self, prefix: &str) -> String {
        let mut count = lock(&self.unique);
        *count += 1;
        format!("{prefix}{count}")
    }

    /// `did_action( $hook )`.
    pub(crate) fn did_action(&self, hook: &str) -> u32 {
        lock(&self.did).get(hook).copied().unwrap_or_default()
    }

    /// The extension's ledger, as it stands.
    fn ledger(&self, ext: usize) -> Ledger {
        self.ledgers
            .get(ext)
            .map(|ledger| *lock(ledger))
            .unwrap_or_default()
    }

    /// What is left for a call of `extension`, as its ledger stands.
    fn budget(extension: &Extension, ledger: Ledger) -> Duration {
        let left = extension.limits.request().saturating_sub(ledger.spent);
        left.min(extension.limits.call())
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
        let ledger = self.ledger(ext);
        if ledger.stopped || extension.disabled() || removed(&slot, registered) {
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
        let budget = Shared::budget(extension, ledger);
        let export = extension.export(None);
        let ran = self.run::<Args<'_>, Answered>(
            (extension, ext),
            &mut slot,
            (budget, export),
            (callback, &lowered),
        );
        let answer = self.settle((extension, ext), hook, Some(callback), ran.map(|(a,)| a));
        let fault = match answer.map(lift) {
            Ok(Some(value)) => return Some(value),
            Ok(None) => Fault::Value("its answer does not decode".into()),
            Err(_) => return None,
        };
        self.report(extension, hook, Some(callback), fault);
        None
    }

    /// The active theme: its place, itself and its `render`.
    fn theme(&self) -> Option<(usize, &Extension, &ComponentExportIndex)> {
        let list = self.extensions.list.iter().enumerate();
        list.into_iter().find_map(|(ext, extension)| {
            extension.render().map(|render| (ext, &**extension, render))
        })
    }

    fn render(&self, file: &str) -> Result<Vec<u8>, Fault> {
        let (ext, extension, export) = self.theme().ok_or(Fault::Refused("no theme".into()))?;
        let slot = self.renders.get(ext).and_then(|slot| slot.try_lock().ok());
        let mut slot = slot.ok_or(Fault::Refused("the theme is rendering".into()))?;
        let ledger = self.ledger(ext);
        if extension.disabled() {
            return Err(Fault::Disabled);
        }
        if ledger.stopped {
            return Err(Fault::Refused("stopped in this request".into()));
        }
        let budget = Shared::budget(extension, ledger);
        let ran =
            self.run::<(&str,), Rendered>((extension, ext), &mut slot, (budget, export), (file,));
        self.settle((extension, ext), file, None, ran.map(|(a,)| a))
    }

    /// The call itself, in `slot`'s instance, made first if need be, its
    /// time outside host calls added to the extension's ledger.
    fn run<P, R>(
        &self,
        (extension, ext): (&Extension, usize),
        slot: &mut Instance,
        (budget, export): (Duration, &ComponentExportIndex),
        args: P,
    ) -> Outcome<R>
    where
        P: ComponentNamedList + Lower,
        R: ComponentNamedList + Lift,
    {
        let (mut store, func) = match slot.take() {
            Some((mut store, func)) => {
                begin(&mut store, budget);
                (store, Ok(func))
            }
            None => {
                let (mut store, instance) =
                    extension.instantiate(&self.engine, self.live(ext), budget);
                let func = instance.and_then(|instance| {
                    let typed = instance.get_typed_func::<P, R>(&mut store, export);
                    typed.map(|typed| *typed.func())
                });
                (store, func)
            }
        };
        let answer = func.and_then(|func| {
            let typed = func.typed::<P, R>(&store);
            let answer = typed.and_then(|typed| typed.call(&mut store, args));
            answer.map(|answer| (func, answer))
        });
        store.data_mut().chains.clear();
        let used = store.data().used();
        let ledger = self.ledgers.get(ext).into_iter();
        ledger.for_each(|ledger| lock(ledger).spent += used);
        match answer {
            Ok((func, answer)) => {
                *slot = Some((store, func));
                Ok(answer)
            }
            Err(e) => Err(classify(&e, store.data())),
        }
    }

    /// A call's outcome: a trap, timeout or memory fault stops the
    /// extension for the request and counts; its own error is reported.
    fn settle<R>(
        &self,
        (extension, ext): (&Extension, usize),
        hook: &str,
        callback: Option<u32>,
        answer: Outcome<Result<R, String>>,
    ) -> Result<R, Fault> {
        let fault = match answer {
            Ok(Ok(answer)) => return Ok(answer),
            Ok(Err(why)) => Fault::Guest(why),
            Err((kind, fault)) => {
                let ledger = self.ledgers.get(ext).into_iter();
                ledger.for_each(|ledger| lock(ledger).stopped = true);
                let counted = Counted {
                    at: self.site.now(),
                    kind,
                    hook: hook.to_string(),
                };
                if let Some(unrecorded) = extension.count(counted) {
                    self.report(extension, hook, callback, unrecorded);
                }
                fault
            }
        };
        self.report(extension, hook, callback, fault.clone());
        Err(fault)
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
fn removed(slot: &Instance, registered: Registered<'_>) -> bool {
    let Some((store, _)) = slot else {
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

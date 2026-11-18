// SPDX-License-Identifier: AGPL-3.0-or-later

//! The extension host (ADR 0020), as its spike: loads a WebAssembly
//! component of the SDK's `spike` world, checks every hook it adds or
//! schedules and every option it touches against its grants, and runs its
//! filters in a fresh instance per request, each call bounded in time
//! (epochs) and memory. A callback that traps, runs
//! over or grows past its memory ends the extension's part of the request
//! and keeps the value as it was; enough faults disable the extension.

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use wasmtime::component::{
    Component, ComponentExportIndex, ComponentType, Func, InstancePre, Lift, Linker, Lower,
};
use wasmtime::{Config, Engine, ResourceLimiter, Store, StoreContextMut, Trap};

/// How often the epoch advances: a call's deadline is counted in these.
pub const TICK: Duration = Duration::from_millis(1);

const GUEST: &str = "hyscribe:sdk/guest@0.1.0";

/// A PHP value crossing a hook.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// `null`.
    Null,
    /// A boolean.
    Boolean(bool),
    /// An integer.
    Int(i64),
    /// A float.
    Float(f64),
    /// A string.
    Text(String),
    /// An array or object, as JSON.
    Json(String),
}

/// `types.value-kind`.
#[derive(ComponentType, Lift, Lower, Clone, Copy)]
#[component(enum)]
#[repr(u8)]
enum Kind {
    #[component(name = "null")]
    Null,
    #[component(name = "boolean")]
    Boolean,
    #[component(name = "int")]
    Int,
    #[component(name = "float")]
    Float,
    #[component(name = "text")]
    Text,
    #[component(name = "json")]
    Json,
}

/// `types.value` as the host lowers it, its text borrowed.
type Lowered<'a> = (Kind, i64, f64, &'a str);

/// `types.value` as the host lifts it.
type Lifted = (Kind, i64, f64, String);

impl Value {
    fn lowered(&self) -> Lowered<'_> {
        match self {
            Self::Null => (Kind::Null, 0, 0.0, ""),
            Self::Boolean(b) => (Kind::Boolean, i64::from(*b), 0.0, ""),
            Self::Int(n) => (Kind::Int, *n, 0.0, ""),
            Self::Float(f) => (Kind::Float, 0, *f, ""),
            Self::Text(text) => (Kind::Text, 0, 0.0, text),
            Self::Json(json) => (Kind::Json, 0, 0.0, json),
        }
    }

    fn lift((kind, int, float, text): Lifted) -> Self {
        match kind {
            Kind::Null => Self::Null,
            Kind::Boolean => Self::Boolean(int != 0),
            Kind::Int => Self::Int(int),
            Kind::Float => Self::Float(float),
            Kind::Text => Self::Text(text),
            Kind::Json => Self::Json(text),
        }
    }
}

/// `guest.invoke`'s arguments and answer.
type Args<'a> = (u32, &'a [Lowered<'a>]);
type Answered = (Result<Lifted, String>,);

/// Why the host refused a call (`types.sdk-error`).
#[derive(ComponentType, Lower)]
#[component(variant)]
pub enum SdkError {
    /// The manifest does not grant this.
    #[component(name = "denied")]
    Denied(String),
    /// An argument the host does not accept.
    #[component(name = "invalid")]
    Invalid(String),
    /// The host could not do it.
    #[component(name = "failed")]
    Failed(String),
}

/// An action or a filter (`hooks.hook-kind`).
#[derive(ComponentType, Lift, PartialEq)]
#[component(enum)]
#[repr(u8)]
pub enum HookKind {
    /// `add_action()`.
    #[component(name = "action")]
    Action,
    /// `add_filter()`.
    #[component(name = "filter")]
    Filter,
}

/// One callback on one hook, as `hooks.add` gave it.
struct Registration {
    hook: String,
    kind: HookKind,
    callback: u32,
    priority: i32,
}

/// A `wp_schedule_event()` of the spike's stand-in cron.
struct Event {
    timestamp: i64,
    hook: String,
    args: Vec<Value>,
}

/// What the manifest grants: for the spike, hooks and options by name.
#[derive(Clone, Debug, Default)]
pub struct Grants {
    /// Action and filter names, cron hooks included.
    pub hooks: BTreeSet<String>,
    /// Option names.
    pub options: BTreeSet<String>,
}

/// The bounds every extension runs in.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Linear memory per instance, in bytes.
    pub memory: usize,
    /// A call's deadline, in ticks.
    pub ticks: u64,
    /// Traps, timeouts and memory faults before the extension is disabled.
    pub faults: u32,
    /// The Wasm stack of a call, in bytes.
    pub stack: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            memory: 16 << 20,
            ticks: 50,
            faults: 3,
            stack: 512 << 10,
        }
    }
}

/// What went wrong in an extension.
#[derive(Clone, Debug, PartialEq)]
pub enum Fault {
    /// Not loaded: not a component of the SDK's world, or its `init` failed.
    Refused(String),
    /// A trap other than the two below.
    Trap(String),
    /// A call ran past its deadline.
    Timeout,
    /// The instance asked for more memory than its limit.
    Memory,
    /// The callback answered an error; the instance carries on.
    Guest(String),
    /// Disabled after too many faults.
    Disabled,
}

struct State {
    grants: Arc<Grants>,
    initialising: bool,
    added: Vec<Registration>,
    // Stands in for `wp_options` in the spike: per instance, so per request.
    options: BTreeMap<String, Value>,
    events: Vec<Event>,
    memory: usize,
    over: bool,
}

impl State {
    fn add(&mut self, registration: Registration) -> Result<(), SdkError> {
        if !self.initialising {
            return Err(SdkError::Denied("hooks are added in init only".into()));
        }
        self.hook(&registration.hook)?;
        self.added.push(registration);
        Ok(())
    }

    fn hook(&self, name: &str) -> Result<(), SdkError> {
        if self.grants.hooks.contains(name) {
            Ok(())
        } else {
            Err(SdkError::Denied(format!("{name} is not in the manifest")))
        }
    }

    fn events(&mut self, hook: &str) -> Result<&mut Vec<Event>, SdkError> {
        self.hook(hook).map(|()| &mut self.events)
    }

    fn options(&mut self, name: &str) -> Result<&mut BTreeMap<String, Value>, SdkError> {
        if self.grants.options.contains(name) {
            Ok(&mut self.options)
        } else {
            Err(SdkError::Denied(format!(
                "option {name} is not in the manifest"
            )))
        }
    }
}

impl ResourceLimiter for State {
    fn memory_growing(
        &mut self,
        _: usize,
        desired: usize,
        _: Option<usize>,
    ) -> wasmtime::Result<bool> {
        self.over = desired > self.memory;
        Ok(!self.over)
    }

    fn table_growing(
        &mut self,
        _: usize,
        desired: usize,
        _: Option<usize>,
    ) -> wasmtime::Result<bool> {
        Ok(desired <= 10_000)
    }
}

/// The engine, the SDK's host functions and the epoch ticker.
pub struct Host {
    engine: Engine,
    linker: Linker<State>,
    limits: Limits,
    stop: Arc<AtomicBool>,
    ticker: Option<JoinHandle<()>>,
}

impl Host {
    /// Makes the engine (Cranelift, epoch interruption) and starts its ticker.
    pub fn new(limits: Limits) -> wasmtime::Result<Self> {
        let mut config = Config::new();
        config.epoch_interruption(true).max_wasm_stack(limits.stack);
        let engine = Engine::new(&config)?;
        let mut linker = Linker::new(&engine);
        linker
            .instance("hyscribe:sdk/hooks@0.1.0")
            .and_then(|mut hooks| hooks.func_wrap("add", add))
            .and_then(|()| linker.instance("hyscribe:sdk/options@0.1.0"))
            .and_then(|mut options| {
                options
                    .func_wrap("get", get_option)
                    .and_then(|()| options.func_wrap("update", update_option))
                    .and_then(|()| options.func_wrap("add", add_option))
                    .and_then(|()| options.func_wrap("delete", delete_option))
            })
            .and_then(|()| linker.instance("hyscribe:sdk/cron@0.1.0"))
            .and_then(|mut cron| {
                cron.func_wrap("schedule", schedule)
                    .and_then(|()| cron.func_wrap("unschedule", unschedule))
                    .and_then(|()| cron.func_wrap("next", next_scheduled))
            })
            .map(|()| {
                let stop = Arc::new(AtomicBool::new(false));
                let ticker = {
                    let (engine, stop) = (engine.clone(), stop.clone());
                    thread::spawn(move || {
                        while !stop.load(Ordering::Relaxed) {
                            thread::sleep(TICK);
                            engine.increment_epoch();
                        }
                    })
                };
                Self {
                    engine,
                    linker,
                    limits,
                    stop,
                    ticker: Some(ticker),
                }
            })
    }

    fn store(&self, grants: &Arc<Grants>, initialising: bool) -> Store<State> {
        let state = State {
            grants: grants.clone(),
            initialising,
            added: Vec::new(),
            options: BTreeMap::new(),
            events: Vec::new(),
            memory: self.limits.memory,
            over: false,
        };
        let mut store = Store::new(&self.engine, state);
        store.limiter(|state| state);
        store.set_epoch_deadline(self.limits.ticks);
        store
    }

    /// Compiles a component, links it against the SDK only (no WASI) and
    /// runs its `init` once, keeping the hooks it adds.
    pub fn load(&self, wasm: &[u8], grants: Grants) -> Result<Extension, Fault> {
        let (component, pre) = Component::new(&self.engine, wasm)
            .and_then(|component| {
                self.linker
                    .instantiate_pre(&component)
                    .map(|pre| (component, pre))
            })
            .map_err(refused)?;
        let (init, invoke) = component
            .get_export_index(None, GUEST)
            .and_then(|guest| {
                let export = |name| component.get_export_index(Some(&guest), name);
                export("init").zip(export("invoke"))
            })
            .ok_or(Fault::Refused(format!("no {GUEST}")))?;
        let grants = Arc::new(grants);
        let mut store = self.store(&grants, true);
        let answer = pre
            .instantiate(&mut store)
            .and_then(|instance| {
                instance
                    .get_typed_func::<Args<'_>, Answered>(&mut store, &invoke)
                    .and_then(|_| {
                        instance.get_typed_func::<(), (Result<(), String>,)>(&mut store, &init)
                    })
            })
            .and_then(|init| init.call(&mut store, ()));
        match answer {
            Ok((Ok(()),)) => {}
            Ok((Err(why),)) => return Err(Fault::Refused(why)),
            Err(e) => return Err(classify(&e, &store)),
        }
        let mut hooks = std::mem::take(&mut store.data_mut().added);
        hooks.sort_by_key(|hook| hook.priority);
        Ok(Extension {
            pre,
            invoke,
            grants,
            hooks,
            limits: self.limits,
            faults: AtomicU32::new(0),
        })
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.ticker.take().map(JoinHandle::join);
    }
}

type Answer<T> = wasmtime::Result<(Result<T, SdkError>,)>;

fn add(
    mut cx: StoreContextMut<'_, State>,
    (hook, kind, callback, priority, _): (String, HookKind, u32, i32, u32),
) -> Answer<()> {
    let registration = Registration {
        hook,
        kind,
        callback,
        priority,
    };
    Ok((cx.data_mut().add(registration),))
}

fn get_option(mut cx: StoreContextMut<'_, State>, (name,): (String,)) -> Answer<Option<Lifted>> {
    let owned = |(kind, int, float, text): Lowered<'_>| (kind, int, float, text.to_owned());
    Ok((cx
        .data_mut()
        .options(&name)
        .map(|options| options.get(&name).map(|value| owned(value.lowered()))),))
}

fn update_option(
    mut cx: StoreContextMut<'_, State>,
    (name, value, _): (String, Lifted, Option<bool>),
) -> Answer<bool> {
    let (options, value) = (cx.data_mut().options(&name), Value::lift(value));
    Ok((options.map(|options| options.insert(name, value.clone()) != Some(value)),))
}

fn add_option(
    mut cx: StoreContextMut<'_, State>,
    (name, value, _): (String, Lifted, Option<bool>),
) -> Answer<bool> {
    let options = cx.data_mut().options(&name);
    Ok((options.map(|options| match options.entry(name) {
        Entry::Vacant(entry) => {
            entry.insert(Value::lift(value));
            true
        }
        Entry::Occupied(_) => false,
    }),))
}

fn delete_option(mut cx: StoreContextMut<'_, State>, (name,): (String,)) -> Answer<bool> {
    Ok((cx
        .data_mut()
        .options(&name)
        .map(|options| options.remove(&name).is_some()),))
}

fn schedule(
    mut cx: StoreContextMut<'_, State>,
    (timestamp, _, hook, args): (i64, Option<String>, String, Vec<Lifted>),
) -> Answer<()> {
    let events = cx.data_mut().events(&hook);
    Ok((events.map(|events| {
        events.push(Event {
            timestamp,
            hook,
            args: args.into_iter().map(Value::lift).collect(),
        })
    }),))
}

fn unschedule(
    mut cx: StoreContextMut<'_, State>,
    (hook, args): (String, Vec<Lifted>),
) -> Answer<u32> {
    let args: Vec<Value> = args.into_iter().map(Value::lift).collect();
    Ok((cx.data_mut().events(&hook).map(|events| {
        let before = events.len();
        events.retain(|event| event.hook != hook || event.args != args);
        u32::try_from(before - events.len()).unwrap_or(u32::MAX)
    }),))
}

fn next_scheduled(
    mut cx: StoreContextMut<'_, State>,
    (hook, args): (String, Vec<Lifted>),
) -> wasmtime::Result<(Option<i64>,)> {
    let args: Vec<Value> = args.into_iter().map(Value::lift).collect();
    let events = cx.data_mut().events(&hook).ok();
    Ok((events.and_then(|events| {
        let matching = events
            .iter()
            .filter(|event| event.hook == hook && event.args == args);
        matching.map(|event| event.timestamp).min()
    }),))
}

fn refused(e: wasmtime::Error) -> Fault {
    Fault::Refused(format!("{e:#}"))
}

fn classify(e: &wasmtime::Error, store: &Store<State>) -> Fault {
    if store.data().over {
        Fault::Memory
    } else if e.downcast_ref::<Trap>() == Some(&Trap::Interrupt) {
        Fault::Timeout
    } else {
        Fault::Trap(format!("{e:#}"))
    }
}

/// A loaded extension: its linked component and the hooks its `init` added.
pub struct Extension {
    pre: InstancePre<State>,
    invoke: ComponentExportIndex,
    grants: Arc<Grants>,
    hooks: Vec<Registration>,
    limits: Limits,
    faults: AtomicU32,
}

impl Extension {
    /// Whether it has faulted often enough to be disabled.
    pub fn disabled(&self) -> bool {
        self.faults.load(Ordering::Relaxed) >= self.limits.faults
    }

    /// A fresh instance for one request, sharing nothing with any other.
    pub fn start(&self, host: &Host) -> Result<Running<'_>, Fault> {
        if self.disabled() {
            return Err(Fault::Disabled);
        }
        let mut store = host.store(&self.grants, false);
        self.pre
            .instantiate(&mut store)
            .and_then(|instance| {
                instance.get_typed_func::<Args<'_>, Answered>(&mut store, &self.invoke)
            })
            .map(|invoke| Running {
                extension: self,
                invoke: *invoke.func(),
                store,
                fault: None,
            })
            .map_err(refused)
    }

    fn fault(&self, fault: Fault) -> Fault {
        if !matches!(fault, Fault::Guest(_)) {
            self.faults.fetch_add(1, Ordering::Relaxed);
        }
        fault
    }
}

/// An extension's instance for one request.
pub struct Running<'a> {
    extension: &'a Extension,
    store: Store<State>,
    invoke: Func,
    fault: Option<Fault>,
}

impl Running<'_> {
    /// `apply_filters( $hook, $value )` over the extension's own callbacks,
    /// by priority. After a trap, timeout or memory fault the value stays
    /// as it was and the instance runs nothing more.
    pub fn filter(&mut self, hook: &str, mut value: Value) -> Value {
        let extension = self.extension;
        let callbacks = extension
            .hooks
            .iter()
            .filter(|r| r.kind == HookKind::Filter && r.hook == hook);
        for registration in callbacks {
            if self
                .fault
                .as_ref()
                .is_some_and(|fault| !matches!(fault, Fault::Guest(_)))
            {
                break;
            }
            match self.call(registration.callback, &value) {
                Ok(filtered) => value = filtered,
                Err(fault) => self.fault = Some(extension.fault(fault)),
            }
        }
        value
    }

    fn call(&mut self, callback: u32, value: &Value) -> Result<Value, Fault> {
        let ticks = self.extension.limits.ticks;
        let answer = self
            .invoke
            .typed::<Args<'_>, Answered>(&self.store)
            .and_then(|invoke| {
                self.store.set_epoch_deadline(ticks);
                invoke.call(&mut self.store, (callback, &[value.lowered()]))
            });
        match answer {
            Ok((Ok(filtered),)) => Ok(Value::lift(filtered)),
            Ok((Err(why),)) => Err(Fault::Guest(why)),
            Err(e) => Err(classify(&e, &self.store)),
        }
    }

    /// The last fault of this request, for the log and the admin's report.
    pub fn fault(&self) -> Option<&Fault> {
        self.fault.as_ref()
    }
}

#[cfg(test)]
mod tests;

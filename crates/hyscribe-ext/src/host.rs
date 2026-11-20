// SPDX-License-Identifier: AGPL-3.0-or-later

//! The engine, the SDK's host functions, the epoch ticker and the state
//! each store carries.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use wasmtime::component::Linker;
use wasmtime::{Config, Engine, ResourceLimiter, Store, Trap};

use crate::extension::Registrations;
use crate::faults::Kind;
use crate::request::Shared;
use crate::{Fault, Grants, Site};

/// How often the epoch advances: a call's deadline is counted in these.
pub const TICK: Duration = Duration::from_millis(1);

/// The Wasm stack of a call, wasmtime's default, set here so the limits
/// hold it in one place.
const STACK: usize = 512 << 10;

/// The most elements a table may grow to.
const TABLE: usize = 10_000;

/// The engine (Cranelift, epoch interruption, no WASI), the SDK's host
/// functions linked once, and the thread that advances the epoch.
pub struct Host {
    pub(crate) engine: Engine,
    pub(crate) linker: Linker<State>,
    pub(crate) stop: Arc<AtomicBool>,
    ticker: Option<JoinHandle<()>>,
}

impl Host {
    /// Makes the engine and starts its ticker.
    pub fn new() -> Result<Host, Fault> {
        let mut config = Config::new();
        config.epoch_interruption(true).max_wasm_stack(STACK);
        let linked = Engine::new(&config).and_then(|engine| {
            let mut linker = Linker::new(&engine);
            crate::calls::link(&mut linker).map(|()| (engine, linker))
        });
        let host = linked.map(|(engine, linker)| {
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
            Host {
                engine,
                linker,
                stop,
                ticker: Some(ticker),
            }
        });
        host.map_err(refused)
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.ticker.take().map(JoinHandle::join);
    }
}

/// Where a store's host calls stand: registering in `init`, or running in
/// a request (activation included) with its site.
pub(crate) enum Phase {
    Init,
    Live {
        site: Arc<dyn Site>,
        shared: Weak<Shared>,
        me: usize,
    },
}

/// A chain `hooks.chain` handed out: its token, its hook and its steps.
pub(crate) struct Chain {
    pub(crate) token: u64,
    pub(crate) hook: String,
    pub(crate) steps: Vec<Vec<(usize, usize)>>,
}

/// What each store carries: the extension's grants, its phase, its memory
/// limit and what its calls left for the rest of the request.
pub(crate) struct State {
    pub(crate) grants: Arc<Grants>,
    pub(crate) slug: Arc<str>,
    pub(crate) phase: Phase,
    /// What `init` registered.
    pub(crate) registered: Registrations,
    memory: usize,
    pub(crate) over: bool,
    /// The chains handed out in the current call, valid for it only.
    pub(crate) chains: Vec<Chain>,
    pub(crate) next_token: u64,
    /// Its own callbacks it removed for this request: hook, callback, priority.
    pub(crate) removed: BTreeSet<(String, u32, i32)>,
    /// Its own shortcodes it removed for this request.
    pub(crate) removed_shortcodes: BTreeSet<String>,
}

impl State {
    /// A store for an extension in `phase`, bounded by `memory` bytes and
    /// interrupted `ticks` from now.
    pub(crate) fn store(
        engine: &Engine,
        (grants, slug): (&Arc<Grants>, &Arc<str>),
        phase: Phase,
        memory: usize,
        ticks: u64,
    ) -> Store<State> {
        let state = State {
            grants: grants.clone(),
            slug: slug.clone(),
            phase,
            registered: Registrations::default(),
            memory,
            over: false,
            chains: Vec::new(),
            next_token: 0,
            removed: BTreeSet::new(),
            removed_shortcodes: BTreeSet::new(),
        };
        let mut store = Store::new(engine, state);
        store.limiter(|state| state);
        store.set_epoch_deadline(ticks);
        store
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
        Ok(desired <= TABLE)
    }
}

pub(crate) fn refused(e: wasmtime::Error) -> Fault {
    Fault::Refused(format!("{e:#}"))
}

/// What stopped a call: the memory limit, the deadline, or a trap.
pub(crate) fn classify(e: &wasmtime::Error, state: &State) -> (Kind, Fault) {
    if state.over {
        (Kind::Memory, Fault::Memory)
    } else if e.downcast_ref::<Trap>() == Some(&Trap::Interrupt) {
        (Kind::Timeout, Fault::Timeout)
    } else {
        (Kind::Trap, Fault::Trap(format!("{e:#}")))
    }
}

/// A deadline in ticks: whole milliseconds, as the ticker counts them.
pub(crate) fn ticks(time: Duration) -> u64 {
    u64::try_from(time.as_millis()).unwrap_or(u64::MAX)
}

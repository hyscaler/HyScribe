// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The SDK's test host (ADR 0023): a port's own Rust code built natively
//! and tested against the SDK without wasm, so coverage and mutation
//! testing reach it. Built natively only.
//!
//! [`Host`] gives the thread a fresh site in memory: options, meta,
//! transients, roles, posts, theme mods and the rest are [`State`]'s fields,
//! which a test fills and reads. The SDK's calls answer from it as the host
//! does, registrations taken in [`Host::init`] only. A call named in
//! [`State::fails`] answers `SdkError::Failed`, for a port's error paths;
//! grants are not checked here but by the host, in the golden suites.
//!
//! ```ignore
//! let host = hyscribe_sdk::test::Host::<MyPlugin>::new();
//! host.init().unwrap();
//! host.state(|state| state.options.insert("blogname".into(), "Site".into()));
//! assert_eq!(host.filter("the_content", "<p>x</p>".into(), &[]), ...);
//! ```

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::marker::PhantomData;

use crate::assets::{Asset, AssetKind};
use crate::customize::CustomizeKind;
use crate::mail::Message;
use crate::query::{Found, Post};
use crate::sys::hooks::Link;
use crate::sys::rest::Route;
use crate::theme::TemplateFile;
use crate::{ObjectKind, Plugin, Theme, Value};

/// Another callback on a hook: core's, or another extension's.
pub type Other = Rc<dyn Fn(Vec<Value>) -> Value>;

/// A callback the plugin added with `hooks::on_filter` or `on_action`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hook {
    /// The hook's name.
    pub hook: String,
    /// Added as an action.
    pub action: bool,
    /// Its number.
    pub callback: u32,
    /// `$priority`.
    pub priority: i32,
    /// `$accepted_args`.
    pub accepted_args: u32,
}

/// What a hook's entry runs.
#[derive(Clone)]
pub(crate) enum Added {
    Mine {
        action: bool,
        callback: u32,
        accepted_args: u32,
    },
    Other(Other),
}

/// An entry of a hook, in the order added.
#[derive(Clone)]
struct Entry {
    hook: String,
    priority: i32,
    added: Added,
}

/// `wp_schedule_event()`'s event.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    /// `$timestamp`.
    pub timestamp: i64,
    /// `$recurrence`, none for a single event.
    pub recurrence: Option<String>,
    /// `$hook`.
    pub hook: String,
    /// `$args`.
    pub args: Vec<Value>,
}

/// A request `http::send` made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sent {
    /// The method.
    pub method: String,
    /// The URL.
    pub url: String,
    /// The headers.
    pub headers: Vec<(String, String)>,
    /// The body.
    pub body: Vec<u8>,
    /// The timeout, in seconds.
    pub timeout: u32,
}

/// What `customize::add` added.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Customized {
    /// A panel, section, setting, control or partial.
    pub kind: CustomizeKind,
    /// Its id.
    pub id: String,
    /// Its arguments, as JSON.
    pub args: String,
    /// Its callbacks by role.
    pub callbacks: Vec<(String, u32)>,
}

/// An HTTP answer: status, headers and body.
pub type Response = (u16, Vec<(String, Vec<u8>)>, Vec<u8>);

/// The site in memory a port's code runs on.
#[derive(Default)]
pub struct State {
    /// Registering, as in `init`: registrations are taken, other calls refused.
    pub registering: bool,
    /// Calls that answer `SdkError::Failed`, by interface and function as
    /// the WIT names them (`options.get`, `theme-setup.get-mod`).
    pub fails: BTreeSet<&'static str>,
    entries: Vec<Entry>,
    removed: BTreeSet<(String, u32, i32)>,
    chains: Vec<(u64, String, Vec<Other>)>,
    /// The hooks running, innermost last.
    pub current: Vec<String>,
    /// `did_action()`'s counts.
    pub did: BTreeMap<String, u32>,
    /// Options by name.
    pub options: BTreeMap<String, Value>,
    /// Meta values by object kind, id and key.
    pub meta: BTreeMap<(ObjectKind, u64, String), Vec<Value>>,
    /// The last meta id handed out.
    pub meta_id: u64,
    /// `register_meta()`: kind, then subtype, key and arguments.
    pub meta_keys: Vec<(ObjectKind, [String; 3])>,
    /// `register_post_type()`: name and arguments.
    pub post_types: Vec<(String, String)>,
    /// `register_taxonomy()`: name, object types and arguments.
    pub taxonomies: Vec<(String, Vec<String>, String)>,
    /// The posts `get_post()` and the loop read.
    pub posts: Vec<Post>,
    /// What every query answers.
    pub found: Found,
    /// Each query's variables, as JSON.
    pub queries: Vec<String>,
    /// `register_rest_route()`.
    pub routes: Vec<Route>,
    /// `add_shortcode()`: tag and callback.
    pub shortcodes: Vec<(String, u32)>,
    /// `register_block_type()`: folder and render callback.
    pub blocks: Vec<(String, Option<u32>)>,
    /// Registered scripts and styles.
    pub assets: Vec<Asset>,
    /// Enqueued handles.
    pub enqueued: Vec<(AssetKind, String)>,
    /// Inline scripts and styles: handle, data and whether before.
    pub inline: Vec<(AssetKind, String, String, bool)>,
    /// Scheduled events.
    pub events: Vec<Event>,
    /// Mail sent.
    pub mails: Vec<Message>,
    /// `get_current_user_id()`.
    pub user: u64,
    /// The current user's capabilities.
    pub caps: BTreeSet<String>,
    /// Transients by site flag and name: value and expiration.
    pub transients: BTreeMap<(bool, String), (Value, i64)>,
    /// Roles: display name and capabilities.
    pub roles: BTreeMap<String, (String, Vec<(String, bool)>)>,
    /// What each URL answers; one not here fails.
    pub responses: BTreeMap<String, Response>,
    /// Requests made.
    pub sent: Vec<Sent>,
    /// The theme's template files.
    pub templates: Vec<TemplateFile>,
    /// How many of `posts` the loop set up.
    pub looped: usize,
    /// `in_the_loop()`.
    pub in_loop: bool,
    /// How many times `wp_reset_postdata()` ran.
    pub resets: u32,
    /// The conditional tags that are true.
    pub conditionals: BTreeSet<String>,
    /// What each printing template tag prints; one not here is refused.
    pub printed: BTreeMap<String, Vec<u8>>,
    /// What each template tag answering a value answers.
    pub values: BTreeMap<String, Value>,
    /// Theme features and their arguments.
    pub supports: Vec<(String, Vec<Value>)>,
    /// Menu locations and descriptions.
    pub nav_menus: Vec<(String, String)>,
    /// Each `register_sidebar()`'s arguments.
    pub sidebars: Vec<String>,
    /// Image sizes: name, width, height and crop.
    pub image_sizes: Vec<(String, u32, u32, Value)>,
    /// `set_post_thumbnail_size()`.
    pub thumbnail: Option<(u32, u32, Value)>,
    /// Editor styles.
    pub editor_styles: Vec<String>,
    /// `$content_width`.
    pub content_width: Option<u32>,
    /// Block styles: block and arguments.
    pub block_styles: Vec<(String, String)>,
    /// Block patterns: name and arguments.
    pub block_patterns: Vec<(String, String)>,
    /// Theme mods.
    pub mods: BTreeMap<String, Value>,
    /// What `$wp_customize` holds.
    pub customized: Vec<Customized>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::default();
}

/// Runs `run` over the thread's state; a callback it hands out runs after.
pub(crate) fn with<R>(run: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|state| run(&mut state.borrow_mut()))
}

/// Runs `run` as the hook `hook` running.
pub(crate) fn fired<R>(hook: &str, run: impl FnOnce() -> R) -> R {
    with(|state| state.current.push(hook.into()));
    let answer = run();
    with(|state| state.current.pop());
    answer
}

impl State {
    /// The plugin's own callbacks, in the order added.
    pub fn hooks(&self) -> Vec<Hook> {
        let mine = self.entries.iter().filter_map(|entry| match entry.added {
            Added::Mine {
                action,
                callback,
                accepted_args,
            } => Some(Hook {
                hook: entry.hook.clone(),
                action,
                callback,
                priority: entry.priority,
                accepted_args,
            }),
            Added::Other(_) => None,
        });
        mine.collect()
    }

    pub(crate) fn add(&mut self, hook: &str, priority: i32, added: Added) {
        self.entries.push(Entry {
            hook: hook.into(),
            priority,
            added,
        });
    }

    /// `remove_filter()` of the plugin's own callback: gone in `init`, and
    /// skipped for the rest of the test otherwise.
    pub(crate) fn remove(&mut self, hook: &str, callback: u32, priority: i32) -> bool {
        let matches = |entry: &Entry| {
            let mine = matches!(entry.added, Added::Mine { callback: c, .. } if c == callback);
            mine && entry.hook == hook && entry.priority == priority
        };
        if self.registering {
            let before = self.entries.len();
            self.entries.retain(|entry| !matches(entry));
            return self.entries.len() != before;
        }
        let own = self.entries.iter().any(matches);
        own && self.removed.insert((hook.into(), callback, priority))
    }

    /// `hook`'s entries by priority, then in the order added, without the
    /// callbacks the plugin removed.
    fn entries(&self, hook: &str) -> Vec<Entry> {
        let mut found: Vec<Entry> = self
            .entries
            .iter()
            .filter(|entry| entry.hook == hook)
            .filter(|entry| match entry.added {
                Added::Mine { callback, .. } => {
                    let key = (entry.hook.clone(), callback, entry.priority);
                    !self.removed.contains(&key)
                }
                Added::Other(_) => true,
            })
            .cloned()
            .collect();
        found.sort_by_key(|entry| entry.priority);
        found
    }

    pub(crate) fn chain(&mut self, hook: &str) -> (u64, Vec<Link>) {
        let (mut links, mut steps) = (Vec::new(), Vec::new());
        for entry in self.entries(hook) {
            match entry.added {
                Added::Mine { callback, .. } => links.push(Link::Mine(callback)),
                Added::Other(other) => {
                    links.push(Link::Others(u32::try_from(steps.len()).unwrap_or(u32::MAX)));
                    steps.push(other);
                }
            }
        }
        let token = self.chains.len() as u64;
        self.chains.push((token, hook.into(), steps));
        (token, links)
    }

    pub(crate) fn step(&self, token: u64, step: u32) -> Option<(String, Other)> {
        let (_, hook, steps) = self.chains.iter().find(|(held, _, _)| *held == token)?;
        let other = steps.get(step as usize)?;
        Some((hook.clone(), other.clone()))
    }
}

/// A test host for the plugin or theme `P` on this thread, with a fresh
/// state that it drops again when it goes.
pub struct Host<P> {
    plugin: PhantomData<P>,
}

impl<P: Plugin> Default for Host<P> {
    fn default() -> Host<P> {
        Host::new()
    }
}

impl<P> Drop for Host<P> {
    fn drop(&mut self) {
        with(|state| *state = State::default());
    }
}

impl<P: Plugin> Host<P> {
    /// A fresh site on this thread.
    pub fn new() -> Host<P> {
        with(|state| *state = State::default());
        Host {
            plugin: PhantomData,
        }
    }

    /// Runs `run` over the site, to fill it or read what the code did.
    pub fn state<R>(&self, run: impl FnOnce(&mut State) -> R) -> R {
        with(run)
    }

    /// The plugin's `init`, registering.
    pub fn init(&self) -> Result<(), String> {
        with(|state| state.registering = true);
        let done = P::init();
        with(|state| state.registering = false);
        done
    }

    /// The plugin's `activate`.
    pub fn activate(&self) -> Result<(), String> {
        P::activate()
    }

    /// The plugin's `deactivate`.
    pub fn deactivate(&self) -> Result<(), String> {
        P::deactivate()
    }

    /// Adds core's or another extension's callback to a hook.
    pub fn add(&self, hook: &str, priority: i32, other: impl Fn(Vec<Value>) -> Value + 'static) {
        with(|state| state.add(hook, priority, Added::Other(Rc::new(other))));
    }

    /// Runs `hook`'s callbacks over `args` as core fires them; a filter
    /// takes each answer as the next first argument, and an action prints
    /// the strings answered.
    fn fire(&self, hook: &str, mut args: Vec<Value>, filter: bool) -> (Vec<Value>, String) {
        let entries = with(|state| state.entries(hook));
        let mut printed = String::new();
        fired(hook, || {
            for entry in entries {
                let answer = match entry.added {
                    Added::Mine {
                        callback,
                        accepted_args,
                        ..
                    } => {
                        let given = args.iter().take(accepted_args as usize).cloned();
                        P::invoke(callback, given.collect()).ok()
                    }
                    Added::Other(other) => Some(other(args.clone())),
                };
                match (answer, filter, args.first_mut()) {
                    (Some(answer), true, Some(first)) => *first = answer,
                    (Some(Value::Text(text)), false, _) => printed.push_str(&text),
                    _ => {}
                }
            }
        });
        (args, printed)
    }

    /// `apply_filters( $hook, $value, ...$args )` as core runs it.
    pub fn filter(&self, hook: &str, value: Value, args: &[Value]) -> Value {
        let mut all = Vec::with_capacity(args.len() + 1);
        all.push(value);
        all.extend_from_slice(args);
        self.fire(hook, all, true).0.swap_remove(0)
    }

    /// `do_action( $hook, ...$args )` as core runs it, and what its
    /// callbacks print where it is an action that prints (`wp_head`,
    /// `wp_footer`, `wp_body_open`): the strings they answer.
    pub fn action(&self, hook: &str, args: &[Value]) -> String {
        with(|state| *state.did.entry(hook.into()).or_default() += 1);
        self.fire(hook, args.to_vec(), false).1
    }
}

impl<T: Theme> Host<T> {
    /// Renders one of the theme's template files, as the host calls it.
    pub fn render(&self, file: &str) -> Result<Vec<u8>, String> {
        T::render(file)
    }
}

#[cfg(test)]
mod tests;

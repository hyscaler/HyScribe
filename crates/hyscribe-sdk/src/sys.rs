// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The SDK's interfaces built natively: the same functions and types the
//! bindings give a component, answered by the test host's state on this
//! thread (`crate::test`). Registrations are taken while it registers, as
//! in `init`, and refused otherwise; every other call is refused while it
//! registers, as the host refuses them in `init`.

use alloc::format;
use alloc::vec::Vec;

use crate::Value;
use crate::test::{State, with};
use types::{SdkError, Value as Wire};

/// Writes a wire type's `Debug` as the bindings write it: `Name::Case`.
macro_rules! wire_debug {
    ($name:ident { $($case:ident),* $(,)? }) => {
        impl core::fmt::Debug for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.write_str(match self {
                    $($name::$case => concat!(stringify!($name), "::", stringify!($case)),)*
                })
            }
        }
    };
}

pub mod types {
    //! `types`.

    use alloc::string::String;

    /// `value-kind`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum ValueKind {
        /// `null`.
        Null,
        /// `boolean`.
        Boolean,
        /// `int`.
        Int,
        /// `float`.
        Float,
        /// `text`.
        Text,
        /// `json`.
        Json,
    }

    /// `value`.
    pub type Value = (ValueKind, i64, f64, String);

    /// `sdk-error`.
    #[derive(Clone, PartialEq, Eq)]
    pub enum SdkError {
        /// The manifest does not grant this.
        Denied(String),
        /// An argument the host does not accept.
        Invalid(String),
        /// The host could not do it.
        Failed(String),
    }

    impl core::fmt::Debug for SdkError {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            let (case, why) = match self {
                SdkError::Denied(why) => ("SdkError::Denied", why),
                SdkError::Invalid(why) => ("SdkError::Invalid", why),
                SdkError::Failed(why) => ("SdkError::Failed", why),
            };
            f.debug_tuple(case).field(why).finish()
        }
    }

    /// `object-kind`.
    #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub enum ObjectKind {
        /// `wp_postmeta`.
        Post,
        /// `wp_usermeta`.
        User,
        /// `wp_termmeta`.
        Term,
        /// `wp_commentmeta`.
        Comment,
    }

    wire_debug!(ObjectKind {
        Post,
        User,
        Term,
        Comment
    });
}

fn values(wire: &[Wire]) -> Vec<Value> {
    wire.iter().cloned().map(Value::__from_wit).collect()
}

fn value(wire: &Wire) -> Value {
    Value::__from_wit(wire.clone())
}

/// What the test host answers a call it was told to fail.
fn failing(state: &State, call: &str) -> Result<(), SdkError> {
    if state.fails.contains(call) {
        Err(SdkError::Failed(format!("{call} failed in the test host")))
    } else {
        Ok(())
    }
}

/// A call made in a request: refused while registering, as in `init`.
fn live<T>(call: &str, run: impl FnOnce(&mut State) -> T) -> Result<T, SdkError> {
    with(|state| {
        if state.registering {
            return Err(SdkError::Failed("runs in a request, not in init".into()));
        }
        failing(state, call).map(|()| run(state))
    })
}

/// A registration: taken while registering only.
fn registering(call: &str, run: impl FnOnce(&mut State)) -> Result<(), SdkError> {
    with(|state| {
        if !state.registering {
            return Err(SdkError::Denied("registered in init only".into()));
        }
        failing(state, call).map(|()| run(state))
    })
}

pub mod hooks {
    //! `hooks`.

    use alloc::string::String;
    use alloc::vec::Vec;

    use super::types::{SdkError, Value as Wire};
    use super::{live, registering, values};
    use crate::test::{Added, with};

    /// `hook-kind`.
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub enum HookKind {
        /// `add_action()`.
        Action,
        /// `add_filter()`.
        Filter,
    }

    wire_debug!(HookKind { Action, Filter });

    /// `link`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Link {
        /// The plugin's own callback.
        Mine(u32),
        /// Another callback, a step the host runs.
        Others(u32),
    }

    pub fn add(
        hook: &str,
        kind: HookKind,
        callback: u32,
        priority: i32,
        accepted_args: u32,
    ) -> Result<(), SdkError> {
        registering("hooks.add", |state| {
            state.add(
                hook,
                priority,
                Added::Mine {
                    action: kind == HookKind::Action,
                    callback,
                    accepted_args,
                },
            );
        })
    }

    pub fn remove(hook: &str, callback: u32, priority: i32) -> bool {
        with(|state| state.remove(hook, callback, priority))
    }

    pub fn chain(hook: &str) -> Result<(u64, Vec<Link>), SdkError> {
        live("hooks.chain", |state| state.chain(hook))
    }

    pub fn run_step(token: u64, step: u32, args: &[Wire]) -> Result<Wire, SdkError> {
        let found = live("hooks.run-step", |state| state.step(token, step))?;
        let (hook, callback) = found.ok_or(SdkError::Invalid("no such step".into()))?;
        let answer = crate::test::fired(&hook, || callback(values(args)));
        Ok(answer.__to_wit())
    }

    pub fn current() -> Option<String> {
        with(|state| state.current.last().cloned())
    }

    pub fn did_action(hook: &str) -> u32 {
        with(|state| state.did.get(hook).copied().unwrap_or_default())
    }
}

pub mod options {
    //! `options`.

    use super::types::{SdkError, Value as Wire};
    use super::{live, value};

    pub fn get(name: &str) -> Result<Option<Wire>, SdkError> {
        live("options.get", |state| {
            state.options.get(name).map(crate::Value::__to_wit)
        })
    }

    pub fn update(name: &str, value: &Wire, _: Option<bool>) -> Result<bool, SdkError> {
        live("options.update", |state| {
            let value = super::value(value);
            state.options.insert(name.into(), value.clone()) != Some(value)
        })
    }

    pub fn add(name: &str, given: &Wire, _: Option<bool>) -> Result<bool, SdkError> {
        live("options.add", |state| {
            let added = !state.options.contains_key(name);
            if added {
                state.options.insert(name.into(), value(given));
            }
            added
        })
    }

    pub fn delete(name: &str) -> Result<bool, SdkError> {
        live("options.delete", |state| {
            state.options.remove(name).is_some()
        })
    }
}

pub mod meta {
    //! `meta`.

    use alloc::string::String;
    use alloc::vec::Vec;

    use super::types::{ObjectKind, SdkError, Value as Wire};
    use super::{live, registering, value};

    pub fn get(kind: ObjectKind, id: u64, key: &str) -> Result<Vec<Wire>, SdkError> {
        live("meta.get", |state| {
            let held = state.meta.get(&(kind, id, key.into()));
            held.into_iter()
                .flatten()
                .map(crate::Value::__to_wit)
                .collect()
        })
    }

    pub fn add(
        kind: ObjectKind,
        id: u64,
        key: &str,
        given: &Wire,
        unique: bool,
    ) -> Result<Option<u64>, SdkError> {
        live("meta.add", |state| {
            let held = state.meta.entry((kind, id, key.into())).or_default();
            if unique && !held.is_empty() {
                return None;
            }
            held.push(value(given));
            state.meta_id += 1;
            Some(state.meta_id)
        })
    }

    pub fn update(
        kind: ObjectKind,
        id: u64,
        key: &str,
        given: &Wire,
        previous: Option<&Wire>,
    ) -> Result<bool, SdkError> {
        live("meta.update", |state| {
            let (given, previous) = (value(given), previous.map(value));
            let held = state.meta.entry((kind, id, key.into())).or_default();
            if held.is_empty() {
                held.push(given);
                return true;
            }
            let mut changed = false;
            for each in held.iter_mut() {
                if previous.as_ref().is_none_or(|previous| previous == each) && *each != given {
                    *each = given.clone();
                    changed = true;
                }
            }
            changed
        })
    }

    pub fn delete(
        kind: ObjectKind,
        id: u64,
        key: &str,
        given: Option<&Wire>,
    ) -> Result<bool, SdkError> {
        live("meta.delete", |state| {
            let given = given.map(value);
            let held = state.meta.entry((kind, id, key.into())).or_default();
            let before = held.len();
            held.retain(|each| given.as_ref().is_some_and(|given| given != each));
            held.len() != before
        })
    }

    pub fn register(
        kind: ObjectKind,
        subtype: &str,
        key: &str,
        args: &str,
    ) -> Result<(), SdkError> {
        registering("meta.register", |state| {
            let names = [subtype, key, args].map(String::from);
            state.meta_keys.push((kind, names));
        })
    }
}

pub mod content {
    //! `content`.

    use alloc::string::String;
    use alloc::vec::Vec;

    use super::registering;
    use super::types::SdkError;

    pub fn register_post_type(name: &str, args: &str) -> Result<(), SdkError> {
        registering("content.register-post-type", |state| {
            state.post_types.push((name.into(), args.into()));
        })
    }

    pub fn register_taxonomy(
        name: &str,
        object_types: &[String],
        args: &str,
    ) -> Result<(), SdkError> {
        registering("content.register-taxonomy", |state| {
            let object_types: Vec<String> = object_types.to_vec();
            state
                .taxonomies
                .push((name.into(), object_types, args.into()));
        })
    }
}

pub mod query {
    //! `query`.

    use alloc::string::String;
    use alloc::vec::Vec;

    use super::live;
    use super::types::SdkError;

    /// `post`.
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    #[allow(missing_docs)]
    pub struct Post {
        pub id: u64,
        pub post_type: String,
        pub status: String,
        pub author: u64,
        pub parent: u64,
        pub menu_order: i32,
        pub name: String,
        pub title: String,
        pub content: String,
        pub excerpt: String,
        pub date_gmt: String,
        pub modified_gmt: String,
    }

    /// `found`.
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    #[allow(missing_docs)]
    pub struct Found {
        pub posts: Vec<Post>,
        pub total: u64,
        pub max_pages: u32,
    }

    pub fn posts(args: &str) -> Result<Found, SdkError> {
        live("query.posts", |state| {
            state.queries.push(args.into());
            state.found.clone()
        })
    }

    pub fn get_post(id: u64) -> Result<Option<Post>, SdkError> {
        live("query.get-post", |state| {
            state.posts.iter().find(|post| post.id == id).cloned()
        })
    }
}

pub mod rest {
    //! `rest`.

    use alloc::string::String;
    use alloc::vec::Vec;

    use super::registering;
    use super::types::SdkError;

    /// `route`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    #[allow(missing_docs)]
    pub struct Route {
        pub namespace: String,
        pub path: String,
        pub methods: Vec<String>,
        pub callback: u32,
        pub permission: u32,
        pub args: String,
    }

    pub fn register(route: &Route) -> Result<(), SdkError> {
        registering("rest.register", |state| state.routes.push(route.clone()))
    }
}

pub mod shortcodes {
    //! `shortcodes`.

    use super::registering;
    use super::types::SdkError;
    use crate::test::with;

    pub fn add(tag: &str, callback: u32) -> Result<(), SdkError> {
        registering("shortcodes.add", |state| {
            state.shortcodes.push((tag.into(), callback));
        })
    }

    pub fn remove(tag: &str) -> bool {
        with(|state| {
            let before = state.shortcodes.len();
            state.shortcodes.retain(|(added, _)| added != tag);
            state.shortcodes.len() != before
        })
    }
}

pub mod blocks {
    //! `blocks`.

    use super::registering;
    use super::types::SdkError;

    pub fn register(path: &str, render: Option<u32>) -> Result<(), SdkError> {
        registering("blocks.register", |state| {
            state.blocks.push((path.into(), render));
        })
    }
}

pub mod assets {
    //! `assets`.

    use alloc::string::String;
    use alloc::vec::Vec;

    use super::live;
    use super::types::SdkError;

    /// `asset-kind`.
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub enum AssetKind {
        /// A script.
        Script,
        /// A style.
        Style,
    }

    wire_debug!(AssetKind { Script, Style });

    /// `asset`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    #[allow(missing_docs)]
    pub struct Asset {
        pub kind: AssetKind,
        pub handle: String,
        pub src: String,
        pub deps: Vec<String>,
        pub version: Option<String>,
        pub placement: String,
    }

    pub fn register(asset: &Asset) -> Result<(), SdkError> {
        live("assets.register", |state| state.assets.push(asset.clone()))
    }

    pub fn enqueue(kind: AssetKind, handle: &str) -> Result<(), SdkError> {
        live("assets.enqueue", |state| {
            state.enqueued.push((kind, handle.into()));
        })
    }

    pub fn add_inline(
        kind: AssetKind,
        handle: &str,
        data: &str,
        before: bool,
    ) -> Result<(), SdkError> {
        live("assets.add-inline", |state| {
            state
                .inline
                .push((kind, handle.into(), data.into(), before));
        })
    }
}

pub mod cron {
    //! `cron`.

    use super::types::{SdkError, Value as Wire};
    use super::{live, values};
    use crate::test::Event;

    pub fn schedule(
        timestamp: i64,
        recurrence: Option<&str>,
        hook: &str,
        args: &[Wire],
    ) -> Result<(), SdkError> {
        live("cron.schedule", |state| {
            state.events.push(Event {
                timestamp,
                recurrence: recurrence.map(Into::into),
                hook: hook.into(),
                args: values(args),
            });
        })
    }

    pub fn unschedule(hook: &str, args: &[Wire]) -> Result<u32, SdkError> {
        live("cron.unschedule", |state| {
            let args = values(args);
            let before = state.events.len();
            state
                .events
                .retain(|event| event.hook != hook || event.args != args);
            u32::try_from(before - state.events.len()).unwrap_or(u32::MAX)
        })
    }

    pub fn next(hook: &str, args: &[Wire]) -> Option<i64> {
        let args = values(args);
        let found = live("cron.next", |state| {
            let matching = state.events.iter();
            let matching = matching.filter(|event| event.hook == hook && event.args == args);
            matching.map(|event| event.timestamp).min()
        });
        found.ok().flatten()
    }
}

pub mod mail {
    //! `mail`.

    use alloc::string::String;
    use alloc::vec::Vec;

    use super::live;
    use super::types::SdkError;

    /// `message`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    #[allow(missing_docs)]
    pub struct Message {
        pub to: Vec<String>,
        pub subject: String,
        pub body: String,
        pub headers: Vec<String>,
        pub attachments: Vec<String>,
    }

    pub fn send(message: &Message) -> Result<bool, SdkError> {
        live("mail.send", |state| {
            state.mails.push(message.clone());
            true
        })
    }
}

pub mod auth {
    //! `auth`.

    use alloc::format;
    use alloc::string::String;

    use super::live;
    use super::types::Value as Wire;

    pub fn current_user() -> u64 {
        live("auth.current-user", |state| state.user).unwrap_or_default()
    }

    pub fn user_can(capability: &str, _: &[Wire]) -> bool {
        live("auth.user-can", |state| state.caps.contains(capability)).unwrap_or_default()
    }

    pub fn create_nonce(action: &str) -> String {
        live("auth.create-nonce", |_| format!("nonce-{action}")).unwrap_or_default()
    }

    pub fn verify_nonce(nonce: &str, action: &str) -> u8 {
        let made = create_nonce(action);
        u8::from(!made.is_empty() && made == nonce)
    }
}

pub mod i18n {
    //! `i18n`: the text as given, as with no translation loaded.

    use alloc::string::String;

    pub fn translate(text: &str, _: Option<&str>) -> String {
        text.into()
    }

    pub fn translate_plural(single: &str, plural: &str, number: u64, _: Option<&str>) -> String {
        if number == 1 { single } else { plural }.into()
    }
}

pub mod transients {
    //! `transients`.

    use super::types::{SdkError, Value as Wire};
    use super::{live, value};

    pub fn get(name: &str, site: bool) -> Result<Option<Wire>, SdkError> {
        live("transients.get", |state| {
            let held = state.transients.get(&(site, name.into()));
            held.map(|(value, _)| value.__to_wit())
        })
    }

    pub fn set(name: &str, given: &Wire, expiration: i64, site: bool) -> Result<bool, SdkError> {
        live("transients.set", |state| {
            let held = (value(given), expiration);
            state.transients.insert((site, name.into()), held);
            true
        })
    }

    pub fn delete(name: &str, site: bool) -> Result<bool, SdkError> {
        live("transients.delete", |state| {
            state.transients.remove(&(site, name.into())).is_some()
        })
    }
}

pub mod roles {
    //! `roles`.

    use alloc::string::String;
    use alloc::vec::Vec;

    use super::live;
    use super::types::SdkError;

    pub fn get(role: &str) -> Result<Option<Vec<(String, bool)>>, SdkError> {
        live("roles.get", |state| {
            state.roles.get(role).map(|(_, caps)| caps.clone())
        })
    }

    pub fn add(role: &str, display_name: &str, caps: &[(String, bool)]) -> Result<bool, SdkError> {
        live("roles.add", |state| {
            let added = !state.roles.contains_key(role);
            if added {
                state
                    .roles
                    .insert(role.into(), (display_name.into(), caps.to_vec()));
            }
            added
        })
    }

    pub fn remove(role: &str) -> Result<bool, SdkError> {
        live("roles.remove", |state| state.roles.remove(role).is_some())
    }

    /// A role's capability given, denied, or taken away without a grant.
    fn cap(role: &str, cap: &str, grant: Option<bool>) -> Result<bool, SdkError> {
        live("roles.cap", |state| {
            let Some((_, caps)) = state.roles.get_mut(role) else {
                return false;
            };
            caps.retain(|(held, _)| held != cap);
            caps.extend(grant.map(|grant| (String::from(cap), grant)));
            true
        })
    }

    pub fn add_cap(role: &str, name: &str, grant: bool) -> Result<bool, SdkError> {
        cap(role, name, Some(grant))
    }

    pub fn remove_cap(role: &str, name: &str) -> Result<bool, SdkError> {
        cap(role, name, None)
    }
}

pub mod http {
    //! `http`.

    use alloc::format;
    use alloc::string::String;

    use super::live;
    use super::types::SdkError;
    use crate::test::{Response, Sent};

    pub fn send(
        method: &str,
        url: &str,
        headers: &[(String, String)],
        body: &[u8],
        timeout: u32,
    ) -> Result<Response, SdkError> {
        let answer = live("http.send", |state| {
            state.sent.push(Sent {
                method: method.into(),
                url: url.into(),
                headers: headers.to_vec(),
                body: body.to_vec(),
                timeout,
            });
            state.responses.get(url).cloned()
        })?;
        answer.ok_or(SdkError::Failed(format!("no answer for {url}")))
    }
}

pub mod template {
    //! `template`.

    use alloc::format;
    use alloc::string::String;
    use alloc::vec::Vec;

    use super::live;
    use super::types::{SdkError, Value as Wire};

    /// `part-kind`.
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub enum PartKind {
        /// `get_header()`.
        Header,
        /// `get_footer()`.
        Footer,
        /// `get_sidebar()`.
        Sidebar,
        /// `get_template_part()`.
        TemplatePart,
    }

    wire_debug!(PartKind {
        Header,
        Footer,
        Sidebar,
        TemplatePart
    });

    pub fn have_posts() -> bool {
        live("template.have-posts", |state| {
            let more = state.looped < state.posts.len();
            if !more {
                state.in_loop = false;
            }
            more
        })
        .unwrap_or_default()
    }

    pub fn the_post() {
        let _ = live("template.the-post", |state| {
            state.in_loop = true;
            state.looped += 1;
        });
    }

    pub fn rewind_posts() {
        let _ = live("template.rewind-posts", |state| state.looped = 0);
    }

    pub fn reset_postdata() {
        let _ = live("template.reset-postdata", |state| state.resets += 1);
    }

    pub fn in_the_loop() -> bool {
        live("template.in-the-loop", |state| state.in_loop).unwrap_or_default()
    }

    pub fn conditional(name: &str, _: &[Wire]) -> Result<bool, SdkError> {
        let held = live("template.conditional", |state| {
            let served = !state.refused.contains(name);
            served.then(|| state.conditionals.contains(name))
        })?;
        held.ok_or_else(|| unserved(name))
    }

    /// A tag the test did not give an answer for.
    fn unserved(name: &str) -> SdkError {
        SdkError::Invalid(format!("{name} is not served by the test host"))
    }

    /// The answer held for the tag `name` with `args`, as `name(args)`
    /// with the arguments' debug form, or else for `name`.
    fn held<T: Clone>(
        answers: &alloc::collections::BTreeMap<String, T>,
        name: &str,
        args: &[Wire],
    ) -> Option<T> {
        let given = format!("{name}{:?}", super::values(args));
        answers.get(&given).or_else(|| answers.get(name)).cloned()
    }

    pub fn print(name: &str, args: &[Wire]) -> Result<Vec<u8>, SdkError> {
        let printed = live("template.print", |state| held(&state.printed, name, args))?;
        printed.ok_or_else(|| unserved(name))
    }

    pub fn get(name: &str, args: &[Wire]) -> Result<Wire, SdkError> {
        let value = live("template.get", |state| held(&state.values, name, args))?;
        value
            .map(|value| value.__to_wit())
            .ok_or_else(|| unserved(name))
    }

    /// `locate_template()` over the theme's files: `{slug}-{name}.php`,
    /// then `{slug}.php`.
    pub fn part(_: PartKind, slug: &str, name: Option<&str>, _: &[Wire]) -> Option<String> {
        let named = name.map(|name| format!("{slug}-{name}.php"));
        let candidates = named.into_iter().chain([format!("{slug}.php")]);
        let found = live("template.part", |state| {
            let listed = |file: &String| state.templates.iter().any(|t| t.file == *file);
            candidates.into_iter().find(listed)
        });
        found.ok().flatten()
    }
}

pub mod theme_setup {
    //! `theme-setup`.

    use alloc::format;
    use alloc::string::String;
    use alloc::vec::Vec;

    use super::types::{SdkError, Value as Wire};
    use super::{live, registering, value, values};

    /// `template-file`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    #[allow(missing_docs)]
    pub struct TemplateFile {
        pub file: String,
        pub name: Option<String>,
        pub post_types: Vec<String>,
    }

    pub fn templates(files: &[TemplateFile]) -> Result<(), SdkError> {
        registering("theme-setup.templates", |state| {
            state.templates.extend_from_slice(files);
        })
    }

    pub fn add_support(feature: &str, args: &[Wire]) -> Result<(), SdkError> {
        live("theme-setup.add-support", |state| {
            state.supports.push((feature.into(), values(args)));
        })
    }

    pub fn remove_support(feature: &str) -> bool {
        live("theme-setup.remove-support", |state| {
            let before = state.supports.len();
            state.supports.retain(|(held, _)| held != feature);
            state.supports.len() != before
        })
        .unwrap_or_default()
    }

    pub fn register_nav_menus(locations: &[(String, String)]) -> Result<(), SdkError> {
        live("theme-setup.register-nav-menus", |state| {
            state.nav_menus.extend_from_slice(locations);
        })
    }

    /// `register_sidebar()`: `sidebar-N` as PHP numbers a sidebar given no
    /// id; the test host does not read the arguments.
    pub fn register_sidebar(args: &str) -> Result<String, SdkError> {
        live("theme-setup.register-sidebar", |state| {
            state.sidebars.push(args.into());
            format!("sidebar-{}", state.sidebars.len())
        })
    }

    pub fn add_image_size(
        name: &str,
        width: u32,
        height: u32,
        crop: &Wire,
    ) -> Result<(), SdkError> {
        live("theme-setup.add-image-size", |state| {
            state
                .image_sizes
                .push((name.into(), width, height, value(crop)));
        })
    }

    pub fn set_post_thumbnail_size(width: u32, height: u32, crop: &Wire) -> Result<(), SdkError> {
        live("theme-setup.set-post-thumbnail-size", |state| {
            state.thumbnail = Some((width, height, value(crop)));
        })
    }

    pub fn add_editor_style(files: &[String]) -> Result<(), SdkError> {
        live("theme-setup.add-editor-style", |state| {
            state.editor_styles.extend_from_slice(files);
        })
    }

    pub fn set_content_width(width: u32) -> Result<(), SdkError> {
        live("theme-setup.set-content-width", |state| {
            state.content_width = Some(width);
        })
    }

    pub fn register_block_style(block: &str, args: &str) -> Result<(), SdkError> {
        live("theme-setup.register-block-style", |state| {
            state.block_styles.push((block.into(), args.into()));
        })
    }

    pub fn register_block_pattern(name: &str, args: &str) -> Result<(), SdkError> {
        live("theme-setup.register-block-pattern", |state| {
            state.block_patterns.push((name.into(), args.into()));
        })
    }

    pub fn get_mod(name: &str, default: Option<&Wire>) -> Result<Option<Wire>, SdkError> {
        live("theme-setup.get-mod", |state| {
            let held = state.mods.get(name).map(crate::Value::__to_wit);
            held.or_else(|| default.cloned())
        })
    }

    pub fn set_mod(name: &str, given: &Wire) -> Result<bool, SdkError> {
        live("theme-setup.set-mod", |state| {
            state.mods.insert(name.into(), value(given));
            true
        })
    }

    pub fn remove_mod(name: &str) -> Result<bool, SdkError> {
        live("theme-setup.remove-mod", |state| {
            state.mods.remove(name).is_some()
        })
    }
}

pub mod customize {
    //! `customize`.

    use alloc::string::String;

    use super::live;
    use super::types::SdkError;
    use crate::test::Customized;

    /// `customize-kind`.
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub enum CustomizeKind {
        /// `add_panel()`.
        Panel,
        /// `add_section()`.
        Section,
        /// `add_setting()`.
        Setting,
        /// `add_control()`.
        Control,
        /// `add_partial()`.
        Partial,
    }

    wire_debug!(CustomizeKind {
        Panel,
        Section,
        Setting,
        Control,
        Partial
    });

    pub fn add(
        kind: CustomizeKind,
        id: &str,
        args: &str,
        callbacks: &[(String, u32)],
    ) -> Result<(), SdkError> {
        live("customize.add", |state| {
            state.customized.push(Customized {
                kind,
                id: id.into(),
                args: args.into(),
                callbacks: callbacks.to_vec(),
            });
        })
    }

    pub fn remove(kind: CustomizeKind, id: &str) -> bool {
        live("customize.remove", |state| {
            let before = state.customized.len();
            let same = |each: &Customized| each.kind == kind && each.id == id;
            state.customized.retain(|each| !same(each));
            state.customized.len() != before
        })
        .unwrap_or_default()
    }
}

pub mod admin {
    //! `admin`.

    use alloc::string::String;
    use alloc::vec::Vec;

    use super::types::SdkError;
    use super::{live, registering};
    use crate::test::{Field, Section, Setting, with};

    /// `page`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    #[allow(missing_docs)]
    pub struct Page {
        pub parent: Option<String>,
        pub page_title: String,
        pub menu_title: String,
        pub capability: String,
        pub slug: String,
        pub callback: u32,
        pub icon: String,
        pub position: Option<i32>,
    }

    /// `piece`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum Piece {
        /// `markup`.
        Markup(Vec<u8>),
        /// `call`.
        Call((u32, String)),
    }

    pub fn add_page(page: &Page) -> Result<(), SdkError> {
        registering("admin.add-page", |state| state.pages.push(page.clone()))
    }

    pub fn register_setting(
        group: &str,
        option: &str,
        args: &str,
        sanitize: Option<u32>,
    ) -> Result<(), SdkError> {
        registering("admin.register-setting", |state| {
            state.settings.push(Setting {
                group: group.into(),
                option: option.into(),
                args: args.into(),
                sanitize,
            });
        })
    }

    pub fn add_section(
        id: &str,
        title: &str,
        callback: Option<u32>,
        page: &str,
    ) -> Result<(), SdkError> {
        registering("admin.add-section", |state| {
            state.sections.push(Section {
                id: id.into(),
                title: title.into(),
                callback,
                page: page.into(),
            });
        })
    }

    pub fn add_field(
        id: &str,
        title: &str,
        callback: u32,
        page: &str,
        section: &str,
        args: &str,
    ) -> Result<(), SdkError> {
        registering("admin.add-field", |state| {
            state.fields.push(Field {
                id: id.into(),
                title: title.into(),
                callback,
                page: page.into(),
                section: section.into(),
                args: args.into(),
            });
        })
    }

    pub fn sections(page: &str) -> Vec<Piece> {
        with(|state| state.pieces.get(page).cloned().unwrap_or_default())
    }

    pub fn settings_fields(group: &str) -> Result<Vec<u8>, SdkError> {
        live("admin.settings-fields", |state| {
            let named = alloc::format!("settings_fields[{group}]");
            state.printed.get(&named).cloned().unwrap_or_default()
        })
    }
}

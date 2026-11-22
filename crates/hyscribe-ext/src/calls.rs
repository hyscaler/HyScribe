// SPDX-License-Identifier: AGPL-3.0-or-later

//! The SDK's host functions (`hyscribe:sdk@0.3.0`, ADR 0022 and 0023). Each checks
//! its grant in the store's state before it touches anything, and answers
//! `sdk-error.denied` otherwise; registrations are taken in `init` only,
//! and everything else in a request only.

use std::sync::Arc;
use std::time::Instant;

use hyscribe_phpser::{Value, json_decode, lookup};
use wasmtime::component::types::ComponentFunc;
use wasmtime::component::{ComponentType, Lift, Linker, Lower, Val};
use wasmtime::{AsContextMut, StoreContextMut};

use crate::extension::{Block, Hook, MetaKey, Registrations, Route, Taxonomy, TemplateFile};
use crate::grants::relative;
use crate::host::{Chain, Phase, State};
use crate::request::{Fired, Shared};
use crate::value::{Lifted, lift, lift_all, lower};
use crate::{
    Asset, AssetKind, Caps, CustomizeKind, Found, Incoming, Message, ObjectKind, Outgoing, Part,
    Post, Site, ThemeCall,
};

/// Why the host refused or failed a call (`types.sdk-error`).
#[derive(ComponentType, Lower, Debug, PartialEq, Eq)]
#[component(variant)]
pub(crate) enum SdkError {
    #[component(name = "denied")]
    Denied(String),
    #[component(name = "invalid")]
    Invalid(String),
    #[component(name = "failed")]
    Failed(String),
}

/// `hooks.hook-kind`.
#[derive(ComponentType, Lift, PartialEq, Eq)]
#[component(enum)]
#[repr(u8)]
enum HookKind {
    #[component(name = "action")]
    Action,
    #[component(name = "filter")]
    #[allow(dead_code, reason = "lifted from the guest, never built by the host")]
    Filter,
}

/// `hooks.link`: the caller's own callback, or a run of others' as a step.
#[derive(ComponentType, Lower, Debug, PartialEq, Eq)]
#[component(variant)]
enum Link {
    #[component(name = "mine")]
    Mine(u32),
    #[component(name = "others")]
    Others(u32),
}

/// `rest.route`.
struct RouteIn {
    namespace: String,
    path: String,
    methods: Vec<String>,
    callback: u32,
    permission: u32,
    args: String,
}

// The records cross as `Val`s, whose shape the linker has checked: field
// by field, in the WIT's order.

/// A record's fields, by position.
fn fields(value: Option<&Val>) -> &[(String, Val)] {
    match value {
        Some(Val::Record(fields)) => fields,
        _ => &[],
    }
}

fn field(fields: &[(String, Val)], n: usize) -> Option<&Val> {
    fields.get(n).map(|(_, value)| value)
}

fn text(value: Option<&Val>) -> String {
    match value {
        Some(Val::String(text)) => text.clone(),
        _ => String::new(),
    }
}

/// A list's items.
fn items(value: Option<&Val>) -> &[Val] {
    match value {
        Some(Val::List(items)) => items,
        _ => &[],
    }
}

fn texts(value: Option<&Val>) -> Vec<String> {
    items(value).iter().map(|item| text(Some(item))).collect()
}

fn id(value: Option<&Val>) -> u64 {
    match value {
        Some(Val::U64(id)) => *id,
        _ => 0,
    }
}

fn number(value: Option<&Val>) -> u32 {
    match value {
        Some(Val::U32(n)) => *n,
        _ => 0,
    }
}

fn route_in(params: &[Val]) -> RouteIn {
    let fields = fields(params.first());
    RouteIn {
        namespace: text(field(fields, 0)),
        path: text(field(fields, 1)),
        methods: texts(field(fields, 2)),
        callback: number(field(fields, 3)),
        permission: number(field(fields, 4)),
        args: text(field(fields, 5)),
    }
}

fn message(params: &[Val]) -> Message {
    let fields = fields(params.first());
    Message {
        to: texts(field(fields, 0)),
        subject: text(field(fields, 1)),
        body: text(field(fields, 2)),
        headers: texts(field(fields, 3)),
        attachments: texts(field(fields, 4)),
    }
}

fn asset(params: &[Val]) -> Asset {
    let fields = fields(params.first());
    let kind = match field(fields, 0) {
        Some(Val::Enum(kind)) if kind == "style" => AssetKind::Style,
        _ => AssetKind::Script,
    };
    let version = match field(fields, 4) {
        Some(Val::Option(Some(version))) => Some(text(Some(version))),
        _ => None,
    };
    Asset {
        kind,
        handle: text(field(fields, 1)),
        src: text(field(fields, 2)),
        deps: texts(field(fields, 3)),
        version,
        placement: text(field(fields, 5)),
    }
}

/// `theme-setup.templates`' list of `template-file` records.
fn template_files(params: &[Val]) -> Vec<TemplateFile> {
    let file = |item: &Val| {
        let fields = fields(Some(item));
        let name = match field(fields, 1) {
            Some(Val::Option(Some(name))) => Some(text(Some(name))),
            _ => None,
        };
        TemplateFile {
            file: text(field(fields, 0)),
            name,
            post_types: texts(field(fields, 2)),
        }
    };
    items(params.first()).iter().map(file).collect()
}

fn post_val(post: Post) -> Val {
    let text = |name: &str, text: String| (name.to_string(), Val::String(text));
    Val::Record(vec![
        ("id".into(), Val::U64(post.id)),
        text("post-type", post.post_type),
        text("status", post.status),
        ("author".into(), Val::U64(post.author)),
        ("parent".into(), Val::U64(post.parent)),
        ("menu-order".into(), Val::S32(post.menu_order)),
        text("name", post.name),
        text("title", post.title),
        text("content", post.content),
        text("excerpt", post.excerpt),
        text("date-gmt", post.date_gmt),
        text("modified-gmt", post.modified_gmt),
    ])
}

fn found_val(found: Found) -> Val {
    Val::Record(vec![
        (
            "posts".into(),
            Val::List(found.posts.into_iter().map(post_val).collect()),
        ),
        ("total".into(), Val::U64(found.total)),
        ("max-pages".into(), Val::U32(found.max_pages)),
    ])
}

/// `result<T, sdk-error>` as a `Val`, none for `result<_, sdk-error>`.
fn result_val(result: Result<Option<Val>, SdkError>) -> Val {
    let error = |e| {
        let (case, why) = match e {
            SdkError::Denied(why) => ("denied", why),
            SdkError::Invalid(why) => ("invalid", why),
            SdkError::Failed(why) => ("failed", why),
        };
        Some(Box::new(Val::Variant(
            case.into(),
            Some(Box::new(Val::String(why))),
        )))
    };
    Val::Result(result.map(|value| value.map(Box::new)).map_err(error))
}

/// A host function of `Val`s: its one result from the store and its
/// params, its time counted as the host's.
fn dynamic(
    answer: impl Fn(&mut State, &[Val]) -> Val + Send + Sync + 'static,
) -> impl Fn(Cx<'_>, ComponentFunc, &[Val], &mut [Val]) -> wasmtime::Result<()> + Send + Sync + 'static
{
    move |mut cx, _, params, results| {
        let started = Instant::now();
        let value = answer(cx.data_mut(), params);
        let slots = results.iter_mut().zip([value]);
        slots.for_each(|(result, value)| *result = value);
        cx.data_mut().host += started.elapsed();
        Ok(())
    }
}

/// A typed host function whose time counts as the host's, not the call's.
fn timed<P, R>(
    call: impl Fn(Cx<'_>, P) -> wasmtime::Result<R> + Send + Sync + 'static,
) -> impl Fn(Cx<'_>, P) -> wasmtime::Result<R> + Send + Sync + 'static {
    move |mut cx, params| {
        let started = Instant::now();
        let answer = call(cx.as_context_mut(), params);
        cx.data_mut().host += started.elapsed();
        answer
    }
}

type Cx<'a> = StoreContextMut<'a, State>;
type Answer<T> = wasmtime::Result<(Result<T, SdkError>,)>;

fn answer<T>(result: Result<T, SdkError>) -> Answer<T> {
    Ok((result,))
}

fn denied(why: String) -> SdkError {
    SdkError::Denied(why)
}

fn failed(why: String) -> SdkError {
    SdkError::Failed(why)
}

/// A store's request: its site, its shared state, the extension's place.
type Live<'a> = (&'a Arc<dyn Site>, Arc<Shared>, usize);

const IN_INIT: &str = "registered in init only";
const IN_REQUEST: &str = "runs in a request, not in init";
const METHODS: [&str; 5] = ["GET", "POST", "PUT", "PATCH", "DELETE"];

/// The headers the client sets itself, which an extension's request does not.
const OWN_HEADERS: [&str; 11] = [
    "accept-encoding",
    "connection",
    "content-length",
    "expect",
    "host",
    "keep-alive",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

impl State {
    /// The request this store runs in: its site, its shared state and the
    /// extension's place in it.
    fn live(&self) -> Result<Live<'_>, SdkError> {
        match &self.phase {
            Phase::Live { site, shared, me } => shared
                .upgrade()
                .map(|shared| (site, shared, *me))
                .ok_or(SdkError::Failed("the request ended".into())),
            Phase::Init => Err(SdkError::Failed(IN_REQUEST.into())),
        }
    }

    fn site(&self) -> Result<&Arc<dyn Site>, SdkError> {
        self.live().map(|(site, _, _)| site)
    }

    fn registering(&mut self) -> Result<&mut Registrations, SdkError> {
        match self.phase {
            Phase::Init => Ok(&mut self.registered),
            Phase::Live { .. } => Err(SdkError::Denied(IN_INIT.into())),
        }
    }

    /// Whether the extension has a post type to read: granted, or public.
    fn readable(&self, site: &Arc<dyn Site>, post_type: &str) -> Result<(), SdkError> {
        if site.public_post_type(post_type) {
            Ok(())
        } else {
            self.grants.post_type(post_type).map_err(denied)
        }
    }
}

fn json(text: &str) -> Result<Value, SdkError> {
    json_decode(text.as_bytes()).ok_or(SdkError::Invalid("not JSON".into()))
}

fn lifted(value: Lifted) -> Result<Value, SdkError> {
    lift(value).ok_or(SdkError::Invalid("a value that does not decode".into()))
}

fn lifted_all(values: Vec<Lifted>) -> Result<Vec<Value>, SdkError> {
    lift_all(values).ok_or(SdkError::Invalid("a value that does not decode".into()))
}

fn crossed(value: &Value) -> Result<Lifted, SdkError> {
    let crossing = lower(value).ok_or(SdkError::Failed("a value that does not cross".into()))?;
    let (kind, int, float, text) = crossing.lowered();
    Ok((kind, int, float, text.to_owned()))
}

fn add(
    state: &mut State,
    (hook, kind, callback, priority, accepted_args): (String, HookKind, u32, i32, u32),
) -> Result<(), SdkError> {
    let action = kind == HookKind::Action;
    state.grants.hook(&hook, action).map_err(denied)?;
    state.registering()?.hooks.push(Hook {
        hook,
        action,
        callback,
        priority,
        accepted_args,
    });
    Ok(())
}

fn remove(state: &mut State, (hook, callback, priority): (String, u32, i32)) -> bool {
    let matches = |added: &Hook| {
        added.hook == hook && added.callback == callback && added.priority == priority
    };
    if let Ok(registered) = state.registering() {
        let before = registered.hooks.len();
        registered.hooks.retain(|added| !matches(added));
        return registered.hooks.len() != before;
    }
    let own = state.live().is_ok_and(|(_, shared, me)| {
        let extension = shared.extensions.list.get(me);
        extension.is_some_and(|extension| extension.registered.hooks.iter().any(matches))
    });
    own && state.removed.insert((hook, callback, priority))
}

fn chain(state: &mut State, hook: String) -> Result<(u64, Vec<Link>), SdkError> {
    if state.grants.is_sensitive(&hook) {
        state.grants.hook(&hook, true).map_err(denied)?;
    }
    let (_, shared, me) = state.live()?;
    let (mut links, mut steps) = (Vec::new(), Vec::<Vec<(usize, usize)>>::new());
    let entries = shared.chain(&hook).iter();
    let found =
        entries.filter_map(|&entry| shared.extensions.hook(entry).map(|hook| (entry, hook)));
    for (entry, registered) in found {
        if entry.0 == me {
            let key = (hook.clone(), registered.callback, registered.priority);
            if !state.removed.contains(&key) {
                links.push(Link::Mine(registered.callback));
            }
            continue;
        }
        match (links.last(), steps.last_mut()) {
            (Some(Link::Others(_)), Some(step)) => step.push(entry),
            _ => {
                links.push(Link::Others(u32::try_from(steps.len()).unwrap_or(u32::MAX)));
                steps.push(vec![entry]);
            }
        }
    }
    state.next_token += 1;
    let token = state.next_token;
    state.chains.push(Chain { token, hook, steps });
    Ok((token, links))
}

fn run_step(
    state: &State,
    (token, step, args): (u64, u32, Vec<Lifted>),
) -> Result<Lifted, SdkError> {
    let (_, shared, _) = state.live()?;
    let chain = state.chains.iter().find(|chain| chain.token == token);
    let chain = chain.ok_or(SdkError::Invalid("no such chain in this call".into()))?;
    let entries = chain.steps.get(step as usize);
    let entries = entries.ok_or(SdkError::Invalid("no such step".into()))?;
    let mut args = lifted_all(args)?;
    shared.fire(&chain.hook, entries, &mut args, Fired::Step);
    crossed(&args.into_iter().next().unwrap_or_default())
}

fn get_option(state: &State, name: &str) -> Result<Option<Lifted>, SdkError> {
    state.grants.option(name).map_err(denied)?;
    let value = state.site()?.get_option(name).map_err(failed)?;
    value.as_ref().map(crossed).transpose()
}

fn set_option(
    state: &State,
    (name, value, autoload): (String, Lifted, Option<bool>),
    add: bool,
) -> Result<bool, SdkError> {
    state.grants.option(&name).map_err(denied)?;
    let site = state.site()?;
    let value = lifted(value)?;
    if add {
        site.add_option(&name, value, autoload).map_err(failed)
    } else {
        site.update_option(&name, value, autoload).map_err(failed)
    }
}

fn delete_option(state: &State, name: &str) -> Result<bool, SdkError> {
    state.grants.option(name).map_err(denied)?;
    state.site()?.delete_option(name).map_err(failed)
}

/// The transient's grant, and a name whose timeout option fits the 191
/// characters of `option_name` after `_transient_timeout_` or its site form.
fn transient(state: &State, name: &str, site: bool) -> Result<(), SdkError> {
    state.grants.transient(name, site).map_err(denied)?;
    let most = if site { 167 } else { 172 };
    if name.chars().count() > most {
        return Err(SdkError::Invalid(format!(
            "a transient name is at most {most} characters"
        )));
    }
    Ok(())
}

fn get_transient(state: &State, (name, site): (String, bool)) -> Result<Option<Lifted>, SdkError> {
    transient(state, &name, site)?;
    let value = state.site()?.get_transient(&name, site).map_err(failed)?;
    value.as_ref().map(crossed).transpose()
}

fn set_transient(
    state: &State,
    (name, value, expiration, site): (String, Lifted, i64, bool),
) -> Result<bool, SdkError> {
    transient(state, &name, site)?;
    let host = state.site()?;
    host.set_transient(&name, lifted(value)?, expiration, site)
        .map_err(failed)
}

fn delete_transient(state: &State, (name, site): (String, bool)) -> Result<bool, SdkError> {
    transient(state, &name, site)?;
    state.site()?.delete_transient(&name, site).map_err(failed)
}

fn get_role(state: &State, role: &str) -> Result<Option<Caps>, SdkError> {
    state.site()?.role(role).map_err(failed)
}

fn add_role(
    state: &State,
    (role, display_name, caps): (String, String, Caps),
) -> Result<bool, SdkError> {
    state.grants.role(&role).map_err(denied)?;
    for (cap, _) in &caps {
        state.grants.capability(cap).map_err(denied)?;
    }
    let site = state.site()?;
    site.add_role(&role, &display_name, &caps).map_err(failed)
}

fn remove_role(state: &State, role: &str) -> Result<bool, SdkError> {
    state.grants.role(role).map_err(denied)?;
    state.site()?.remove_role(role).map_err(failed)
}

/// A role's capability given, denied, or taken away without a grant.
fn role_cap(state: &State, role: &str, cap: &str, grant: Option<bool>) -> Result<bool, SdkError> {
    state.grants.capability(cap).map_err(denied)?;
    state.site()?.role_cap(role, cap, grant).map_err(failed)
}

/// The host of an HTTP or HTTPS URL.
fn url_host(url: &str) -> Option<String> {
    let parts = hyscribe_kses::parse_url(url.as_bytes())?;
    let scheme = parts.scheme?;
    let web = scheme.eq_ignore_ascii_case(b"http") || scheme.eq_ignore_ascii_case(b"https");
    let host = parts.host.filter(|host| web && !host.is_empty())?;
    String::from_utf8(host).ok()
}

/// A header an extension may send: a token for a name, not one of the
/// client's own, and a value without line breaks or NUL.
fn header((name, value): &(String, String)) -> bool {
    let token = |b: u8| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b);
    !name.is_empty()
        && name.bytes().all(token)
        && !OWN_HEADERS.contains(&name.to_ascii_lowercase().as_str())
        && !value.contains(['\r', '\n', '\0'])
}

fn request(
    state: &State,
    (method, url, headers, body, timeout): (String, String, Vec<(String, String)>, Vec<u8>, u32),
) -> Result<Incoming, SdkError> {
    let host = url_host(&url).ok_or(SdkError::Invalid(format!("{url} is not an HTTP URL")))?;
    if !state.grants.host(&host) {
        return Err(denied(format!("host {host} is not in the manifest")));
    }
    if !["HEAD", "OPTIONS"]
        .iter()
        .chain(&METHODS)
        .any(|known| *known == method)
    {
        return Err(SdkError::Invalid(format!("{method} is not a method")));
    }
    if let Some((name, _)) = headers.iter().find(|given| !header(given)) {
        return Err(SdkError::Invalid(format!(
            "{name} is not a header it sends"
        )));
    }
    let request = Outgoing {
        method,
        url,
        headers,
        body,
        timeout,
    };
    let grants = state.grants.clone();
    let allowed = move |host: &str| grants.host(host);
    state.site()?.fetch(&request, &allowed).map_err(failed)
}

fn get_meta(
    state: &State,
    (kind, id, key): (ObjectKind, u64, String),
) -> Result<Vec<Lifted>, SdkError> {
    state.grants.meta(&key).map_err(denied)?;
    let values = state.site()?.get_meta(kind, id, &key).map_err(failed)?;
    values.iter().map(crossed).collect()
}

fn add_meta(
    state: &State,
    (kind, id, key, value, unique): (ObjectKind, u64, String, Lifted, bool),
) -> Result<Option<u64>, SdkError> {
    state.grants.meta(&key).map_err(denied)?;
    let site = state.site()?;
    site.add_meta(kind, id, &key, lifted(value)?, unique)
        .map_err(failed)
}

fn update_meta(
    state: &State,
    (kind, id, key, value, previous): (ObjectKind, u64, String, Lifted, Option<Lifted>),
) -> Result<bool, SdkError> {
    state.grants.meta(&key).map_err(denied)?;
    let site = state.site()?;
    let previous = previous.map(lifted).transpose()?;
    site.update_meta(kind, id, &key, lifted(value)?, previous)
        .map_err(failed)
}

fn delete_meta(
    state: &State,
    (kind, id, key, value): (ObjectKind, u64, String, Option<Lifted>),
) -> Result<bool, SdkError> {
    state.grants.meta(&key).map_err(denied)?;
    let site = state.site()?;
    let value = value.map(lifted).transpose()?;
    site.delete_meta(kind, id, &key, value).map_err(failed)
}

fn register_meta(
    state: &mut State,
    (kind, subtype, key, args): (ObjectKind, String, String, String),
) -> Result<(), SdkError> {
    state.grants.meta(&key).map_err(denied)?;
    let args = json(&args)?;
    state.registering()?.meta.push(MetaKey {
        kind,
        subtype,
        key,
        args,
    });
    Ok(())
}

/// A name `register_post_type()` or `register_taxonomy()` accepts: 1 to
/// `most` characters.
fn named(name: &str, what: &str, most: usize) -> Result<(), SdkError> {
    if (1..=most).contains(&name.chars().count()) {
        Ok(())
    } else {
        Err(SdkError::Invalid(format!(
            "{what} names must be between 1 and {most} characters in length."
        )))
    }
}

fn register_post_type(state: &mut State, (name, args): (String, String)) -> Result<(), SdkError> {
    state.grants.post_type(&name).map_err(denied)?;
    named(&name, "Post type", 20)?;
    let args = json(&args)?;
    state.registering()?.post_types.push((name, args));
    Ok(())
}

fn register_taxonomy(
    state: &mut State,
    (name, object_types, args): (String, Vec<String>, String),
) -> Result<(), SdkError> {
    state.grants.taxonomy(&name).map_err(denied)?;
    for object_type in &object_types {
        state.grants.post_type(object_type).map_err(denied)?;
    }
    named(&name, "Taxonomy", 32)?;
    let args = json(&args)?;
    state.registering()?.taxonomies.push(Taxonomy {
        name,
        object_types,
        args,
    });
    Ok(())
}

/// The post types a query reads: `post` unless it names others; `any`
/// is refused, as the extension names what it reads.
fn post_types(args: &Value) -> Result<Vec<String>, SdkError> {
    let entries = args
        .as_array()
        .ok_or(SdkError::Invalid("query variables are an object".into()))?;
    let named = match lookup(entries, "post_type") {
        None => vec![Value::String(b"post".to_vec())],
        Some(Value::String(name)) => vec![Value::String(name.clone())],
        Some(Value::Array(names)) => names.iter().map(|(_, name)| name.clone()).collect(),
        Some(_) => vec![Value::Null],
    };
    named
        .into_iter()
        .map(|name| match name {
            Value::String(name) if name != b"any" => {
                Ok(String::from_utf8_lossy(&name).into_owned())
            }
            _ => Err(SdkError::Invalid(
                "post_type names the post types read".into(),
            )),
        })
        .collect()
}

fn query(state: &State, args: &str) -> Result<Found, SdkError> {
    let site = state.site()?;
    let args = json(args)?;
    for post_type in post_types(&args)? {
        state.readable(site, &post_type)?;
    }
    let mut found = site.query(&args).map_err(failed)?;
    found
        .posts
        .retain(|post| state.readable(site, &post.post_type).is_ok());
    Ok(found)
}

fn get_post(state: &State, id: u64) -> Result<Option<Post>, SdkError> {
    let site = state.site()?;
    let post = site.post(id).map_err(failed)?;
    if let Some(post) = &post {
        state.readable(site, &post.post_type)?;
    }
    Ok(post)
}

fn register_route(state: &mut State, route: RouteIn) -> Result<(), SdkError> {
    state.grants.rest(&route.namespace).map_err(denied)?;
    if !route.path.starts_with('/') {
        return Err(SdkError::Invalid("a route starts with /".into()));
    }
    let known = |method: &String| METHODS.contains(&method.as_str());
    if route.methods.is_empty() || !route.methods.iter().all(known) {
        return Err(SdkError::Invalid(
            "methods are GET, POST, PUT, PATCH or DELETE".into(),
        ));
    }
    let args = json(&route.args)?;
    state.registering()?.routes.push(Route {
        namespace: route.namespace,
        path: route.path,
        methods: route.methods,
        callback: route.callback,
        permission: route.permission,
        args,
    });
    Ok(())
}

fn add_shortcode(state: &mut State, (tag, callback): (String, u32)) -> Result<(), SdkError> {
    state.grants.shortcode(&tag).map_err(denied)?;
    state.registering()?.shortcodes.push((tag, callback));
    Ok(())
}

fn remove_shortcode(state: &mut State, tag: String) -> bool {
    if let Ok(registered) = state.registering() {
        let before = registered.shortcodes.len();
        registered.shortcodes.retain(|(added, _)| *added != tag);
        return registered.shortcodes.len() != before;
    }
    let own = state.live().is_ok_and(|(_, shared, me)| {
        let extension = shared.extensions.list.get(me);
        extension.is_some_and(|extension| {
            extension
                .registered
                .shortcodes
                .iter()
                .any(|(added, _)| *added == tag)
        })
    });
    own && state.removed_shortcodes.insert(tag)
}

fn register_block(
    state: &mut State,
    (path, render): (String, Option<u32>),
) -> Result<(), SdkError> {
    if !relative(&path) {
        return Err(SdkError::Invalid(format!(
            "{path} is not a folder inside the package"
        )));
    }
    state.registering()?.blocks.push(Block { path, render });
    Ok(())
}

fn register_asset(state: &State, asset: Asset) -> Result<(), SdkError> {
    if asset.handle.is_empty() {
        return Err(SdkError::Invalid("an asset has a handle".into()));
    }
    if !asset.src.is_empty() && !relative(&asset.src) {
        return Err(SdkError::Invalid(format!(
            "{} is not a file inside public/",
            asset.src
        )));
    }
    state
        .site()?
        .register_asset(&state.slug, &asset)
        .map_err(failed)
}

fn schedule(
    state: &State,
    (timestamp, recurrence, hook, args): (i64, Option<String>, String, Vec<Lifted>),
) -> Result<(), SdkError> {
    state.grants.cron(&hook).map_err(denied)?;
    let site = state.site()?;
    let args = lifted_all(args)?;
    site.schedule(timestamp, recurrence.as_deref(), &hook, &args)
        .map_err(failed)
}

fn unschedule(state: &State, (hook, args): (String, Vec<Lifted>)) -> Result<u32, SdkError> {
    state.grants.cron(&hook).map_err(denied)?;
    let site = state.site()?;
    site.unschedule(&hook, &lifted_all(args)?).map_err(failed)
}

fn next_scheduled(state: &State, (hook, args): (String, Vec<Lifted>)) -> Option<i64> {
    state.grants.cron(&hook).ok()?;
    state.site().ok()?.next_scheduled(&hook, &lift_all(args)?)
}

fn send(state: &State, message: &Message) -> Result<bool, SdkError> {
    state.grants.mail(&message.attachments).map_err(denied)?;
    state.site()?.mail(message).map_err(failed)
}

fn user_can(state: &State, (capability, args): (String, Vec<Lifted>)) -> bool {
    let site = state.site();
    let args = lift_all(args);
    site.ok()
        .zip(args)
        .is_some_and(|(site, args)| site.user_can(&capability, &args))
}

fn translate(state: &State, (text, context): (String, Option<String>)) -> String {
    match state.site() {
        Ok(site) => site.translate(&state.slug, &text, context.as_deref()),
        Err(_) => text,
    }
}

fn translate_plural(
    state: &State,
    (single, plural, number, context): (String, String, u64, Option<String>),
) -> String {
    match state.site() {
        Ok(site) => {
            site.translate_plural(&state.slug, (&single, &plural), number, context.as_deref())
        }
        Err(_) if number == 1 => single,
        Err(_) => plural,
    }
}

fn templates(state: &mut State, files: Vec<TemplateFile>) -> Result<(), SdkError> {
    let php = |file: &TemplateFile| relative(&file.file) && file.file.ends_with(".php");
    if let Some(file) = files.iter().find(|file| !php(file)) {
        return Err(SdkError::Invalid(format!(
            "{} is not a PHP file inside the theme",
            file.file
        )));
    }
    state.registering()?.templates.extend(files);
    Ok(())
}

/// A theme's call, answered by the site for its stylesheet.
fn theme_call(state: &State, call: ThemeCall) -> Result<Value, SdkError> {
    state.site()?.theme(&state.slug, &call).map_err(failed)
}

/// Whether the site answered `true`; anything else, an error included, is false.
fn yes(answer: Result<Value, SdkError>) -> bool {
    matches!(answer, Ok(Value::Bool(true)))
}

/// A call whose arguments cross as PHP values.
fn with_args(
    state: &State,
    args: Vec<Lifted>,
    call: impl FnOnce(Vec<Value>) -> ThemeCall,
) -> Result<Value, SdkError> {
    theme_call(state, call(lifted_all(args)?))
}

/// `get_header()` and the others: the file to render, when it is text.
fn part(
    state: &State,
    (kind, slug, name, args): (Part, String, Option<String>, Vec<Lifted>),
) -> Option<String> {
    let answer = with_args(state, args, |args| ThemeCall::Part(kind, slug, name, args));
    match answer {
        Ok(Value::String(file)) => String::from_utf8(file).ok(),
        _ => None,
    }
}

fn register_sidebar(state: &State, args: &str) -> Result<String, SdkError> {
    let id = theme_call(state, ThemeCall::Sidebar(json(args)?))?;
    Ok(String::from_utf8_lossy(&id.to_php_string()).into_owned())
}

fn editor_style(state: &State, files: Vec<String>) -> Result<(), SdkError> {
    if let Some(file) = files.iter().find(|file| !relative(file)) {
        return Err(SdkError::Invalid(format!(
            "{file} is not a file inside public/"
        )));
    }
    theme_call(state, ThemeCall::EditorStyle(files)).map(drop)
}

fn get_mod(
    state: &State,
    (name, default): (String, Option<Lifted>),
) -> Result<Option<Lifted>, SdkError> {
    let default = default.map(lifted).transpose()?;
    let value = theme_call(state, ThemeCall::GetMod(name, default))?;
    Some(value)
        .filter(|value| *value != Value::Null)
        .as_ref()
        .map(crossed)
        .transpose()
}

fn customize(
    state: &State,
    (kind, id, args, callbacks): (CustomizeKind, String, String, Vec<(String, u32)>),
) -> Result<(), SdkError> {
    let call = ThemeCall::Customize(kind, id, json(&args)?, callbacks);
    theme_call(state, call).map(drop)
}

/// A host function timed as the host's: `dynamic` times its own.
macro_rules! timed {
    (func_wrap, $body:expr) => {
        timed($body)
    };
    (func_new, $body:expr) => {
        $body
    };
}

/// The SDK versions linked: each minor version only adds interfaces, so
/// an interface of 0.1 is linked under both names (ADR 0022).
const VERSIONS: [&str; 3] = ["0.1.0", "0.2.0", "0.3.0"];

/// Defines one interface's functions under each version from `since` on,
/// each a closure of the store and its arguments, without an error path
/// that defining them never takes.
macro_rules! interface {
    ($linker:expr, $name:literal, $since:literal, { $($method:ident $func:literal => $body:expr),* $(,)? }) => {
        VERSIONS.iter().skip($since).try_for_each(|version| {
            $linker
                .instance(&format!("hyscribe:sdk/{}@{version}", $name))
                .and_then(|mut instance| {
                    let defined = [$(instance.$method($func, timed!($method, $body))),*];
                    defined.into_iter().collect::<wasmtime::Result<()>>()
                })
        })
    };
}

/// Links the SDK's interfaces, and nothing else: no WASI.
pub(crate) fn link(linker: &mut Linker<State>) -> wasmtime::Result<()> {
    let defined = [
        interface!(linker, "hooks", 0, {
            func_wrap "add" => |mut cx: Cx<'_>, args| answer(add(cx.data_mut(), args)),
            func_wrap "remove" => |mut cx: Cx<'_>, args| Ok((remove(cx.data_mut(), args),)),
            func_wrap "chain" => |mut cx: Cx<'_>, (hook,): (String,)| answer(chain(cx.data_mut(), hook)),
            func_wrap "run-step" => |cx: Cx<'_>, args| answer(run_step(cx.data(), args)),
            func_wrap "current" => |cx: Cx<'_>, (): ()| {
                Ok((cx.data().live().ok().and_then(|(_, shared, _)| shared.current()),))
            },
            func_wrap "did-action" => |cx: Cx<'_>, (hook,): (String,)| {
                Ok((cx.data().live().map_or(0, |(_, shared, _)| shared.did_action(&hook)),))
            },
        }),
        interface!(linker, "options", 0, {
            func_wrap "get" => |cx: Cx<'_>, (name,): (String,)| answer(get_option(cx.data(), &name)),
            func_wrap "update" => |cx: Cx<'_>, args| answer(set_option(cx.data(), args, false)),
            func_wrap "add" => |cx: Cx<'_>, args| answer(set_option(cx.data(), args, true)),
            func_wrap "delete" => |cx: Cx<'_>, (name,): (String,)| answer(delete_option(cx.data(), &name)),
        }),
        interface!(linker, "meta", 0, {
            func_wrap "get" => |cx: Cx<'_>, args| answer(get_meta(cx.data(), args)),
            func_wrap "add" => |cx: Cx<'_>, args| answer(add_meta(cx.data(), args)),
            func_wrap "update" => |cx: Cx<'_>, args| answer(update_meta(cx.data(), args)),
            func_wrap "delete" => |cx: Cx<'_>, args| answer(delete_meta(cx.data(), args)),
            func_wrap "register" => |mut cx: Cx<'_>, args| answer(register_meta(cx.data_mut(), args)),
        }),
        interface!(linker, "content", 0, {
            func_wrap "register-post-type" => |mut cx: Cx<'_>, args| answer(register_post_type(cx.data_mut(), args)),
            func_wrap "register-taxonomy" => |mut cx: Cx<'_>, args| answer(register_taxonomy(cx.data_mut(), args)),
        }),
        interface!(linker, "query", 0, {
            func_new "posts" => dynamic(|state, params| {
                result_val(query(state, &text(params.first())).map(|found| Some(found_val(found))))
            }),
            func_new "get-post" => dynamic(|state, params| {
                let post = get_post(state, id(params.first()));
                result_val(post.map(|post| Some(Val::Option(post.map(|post| Box::new(post_val(post)))))))
            }),
        }),
        interface!(linker, "rest", 0, {
            func_new "register" => dynamic(|state, params| {
                result_val(register_route(state, route_in(params)).map(|()| None))
            }),
        }),
        interface!(linker, "shortcodes", 0, {
            func_wrap "add" => |mut cx: Cx<'_>, args| answer(add_shortcode(cx.data_mut(), args)),
            func_wrap "remove" => |mut cx: Cx<'_>, (tag,): (String,)| Ok((remove_shortcode(cx.data_mut(), tag),)),
        }),
        interface!(linker, "blocks", 0, {
            func_wrap "register" => |mut cx: Cx<'_>, args| answer(register_block(cx.data_mut(), args)),
        }),
        interface!(linker, "assets", 0, {
            func_new "register" => dynamic(|state, params| {
                result_val(register_asset(state, asset(params)).map(|()| None))
            }),
            func_wrap "enqueue" => |cx: Cx<'_>, (kind, handle): (AssetKind, String)| {
                let site = cx.data().site();
                answer(site.and_then(|site| site.enqueue(kind, &handle).map_err(failed)))
            },
            func_wrap "add-inline" => |cx: Cx<'_>, (kind, handle, data, before): (AssetKind, String, String, bool)| {
                let site = cx.data().site();
                answer(site.and_then(|site| site.add_inline(kind, &handle, &data, before).map_err(failed)))
            },
        }),
        interface!(linker, "cron", 0, {
            func_wrap "schedule" => |cx: Cx<'_>, args| answer(schedule(cx.data(), args)),
            func_wrap "unschedule" => |cx: Cx<'_>, args| answer(unschedule(cx.data(), args)),
            func_wrap "next" => |cx: Cx<'_>, args| Ok((next_scheduled(cx.data(), args),)),
        }),
        interface!(linker, "mail", 0, {
            func_new "send" => dynamic(|state, params| {
                result_val(send(state, &message(params)).map(|sent| Some(Val::Bool(sent))))
            }),
        }),
        interface!(linker, "auth", 0, {
            func_wrap "current-user" => |cx: Cx<'_>, (): ()| Ok((cx.data().site().map_or(0, |site| site.current_user()),)),
            func_wrap "user-can" => |cx: Cx<'_>, args| Ok((user_can(cx.data(), args),)),
            func_wrap "create-nonce" => |cx: Cx<'_>, (action,): (String,)| {
                Ok((cx.data().site().map_or_else(|_| String::new(), |site| site.create_nonce(&action)),))
            },
            func_wrap "verify-nonce" => |cx: Cx<'_>, (nonce, action): (String, String)| {
                Ok((cx.data().site().map_or(0, |site| site.verify_nonce(&nonce, &action)),))
            },
        }),
        interface!(linker, "i18n", 0, {
            func_wrap "translate" => |cx: Cx<'_>, args| Ok((translate(cx.data(), args),)),
            func_wrap "translate-plural" => |cx: Cx<'_>, args| Ok((translate_plural(cx.data(), args),)),
        }),
        interface!(linker, "transients", 1, {
            func_wrap "get" => |cx: Cx<'_>, args| answer(get_transient(cx.data(), args)),
            func_wrap "set" => |cx: Cx<'_>, args| answer(set_transient(cx.data(), args)),
            func_wrap "delete" => |cx: Cx<'_>, args| answer(delete_transient(cx.data(), args)),
        }),
        interface!(linker, "http", 1, {
            func_wrap "send" => |cx: Cx<'_>, args| answer(request(cx.data(), args)),
        }),
        interface!(linker, "roles", 1, {
            func_wrap "get" => |cx: Cx<'_>, (role,): (String,)| answer(get_role(cx.data(), &role)),
            func_wrap "add" => |cx: Cx<'_>, args| answer(add_role(cx.data(), args)),
            func_wrap "remove" => |cx: Cx<'_>, (role,): (String,)| answer(remove_role(cx.data(), &role)),
            func_wrap "add-cap" => |cx: Cx<'_>, (role, cap, grant): (String, String, bool)| {
                answer(role_cap(cx.data(), &role, &cap, Some(grant)))
            },
            func_wrap "remove-cap" => |cx: Cx<'_>, (role, cap): (String, String)| {
                answer(role_cap(cx.data(), &role, &cap, None))
            },
        }),
        interface!(linker, "template", 2, {
            func_wrap "have-posts" => |cx: Cx<'_>, (): ()| Ok((yes(theme_call(cx.data(), ThemeCall::HavePosts)),)),
            func_wrap "the-post" => |cx: Cx<'_>, (): ()| {
                let _ = theme_call(cx.data(), ThemeCall::ThePost);
                Ok(())
            },
            func_wrap "rewind-posts" => |cx: Cx<'_>, (): ()| {
                let _ = theme_call(cx.data(), ThemeCall::RewindPosts);
                Ok(())
            },
            func_wrap "reset-postdata" => |cx: Cx<'_>, (): ()| {
                let _ = theme_call(cx.data(), ThemeCall::ResetPostdata);
                Ok(())
            },
            func_wrap "in-the-loop" => |cx: Cx<'_>, (): ()| Ok((yes(theme_call(cx.data(), ThemeCall::InTheLoop)),)),
            func_wrap "conditional" => |cx: Cx<'_>, (name, args): (String, Vec<Lifted>)| {
                answer(with_args(cx.data(), args, |args| ThemeCall::Conditional(name, args)).map(|is| is == Value::Bool(true)))
            },
            func_wrap "print" => |cx: Cx<'_>, (name, args): (String, Vec<Lifted>)| {
                answer(with_args(cx.data(), args, |args| ThemeCall::Print(name, args)).map(|printed| printed.to_php_string()))
            },
            func_wrap "get" => |cx: Cx<'_>, (name, args): (String, Vec<Lifted>)| {
                answer(with_args(cx.data(), args, |args| ThemeCall::Get(name, args)).and_then(|value| crossed(&value)))
            },
            func_wrap "part" => |cx: Cx<'_>, args| Ok((part(cx.data(), args),)),
        }),
        interface!(linker, "theme-setup", 2, {
            func_new "templates" => dynamic(|state, params| {
                result_val(templates(state, template_files(params)).map(|()| None))
            }),
            func_wrap "add-support" => |cx: Cx<'_>, (feature, args): (String, Vec<Lifted>)| {
                answer(with_args(cx.data(), args, |args| ThemeCall::AddSupport(feature, args)).map(drop))
            },
            func_wrap "remove-support" => |cx: Cx<'_>, (feature,): (String,)| {
                Ok((yes(theme_call(cx.data(), ThemeCall::RemoveSupport(feature))),))
            },
            func_wrap "register-nav-menus" => |cx: Cx<'_>, (locations,): (Vec<(String, String)>,)| {
                answer(theme_call(cx.data(), ThemeCall::NavMenus(locations)).map(drop))
            },
            func_wrap "register-sidebar" => |cx: Cx<'_>, (args,): (String,)| answer(register_sidebar(cx.data(), &args)),
            func_wrap "add-image-size" => |cx: Cx<'_>, (name, width, height, crop): (String, u32, u32, Lifted)| {
                let call = lifted(crop).and_then(|crop| theme_call(cx.data(), ThemeCall::ImageSize(name, width, height, crop)));
                answer(call.map(drop))
            },
            func_wrap "set-post-thumbnail-size" => |cx: Cx<'_>, (width, height, crop): (u32, u32, Lifted)| {
                let call = lifted(crop).and_then(|crop| theme_call(cx.data(), ThemeCall::ThumbnailSize(width, height, crop)));
                answer(call.map(drop))
            },
            func_wrap "add-editor-style" => |cx: Cx<'_>, (files,): (Vec<String>,)| answer(editor_style(cx.data(), files)),
            func_wrap "set-content-width" => |cx: Cx<'_>, (width,): (u32,)| {
                answer(theme_call(cx.data(), ThemeCall::ContentWidth(width)).map(drop))
            },
            func_wrap "register-block-style" => |cx: Cx<'_>, (block, args): (String, String)| {
                let call = json(&args).and_then(|args| theme_call(cx.data(), ThemeCall::BlockStyle(block, args)));
                answer(call.map(drop))
            },
            func_wrap "register-block-pattern" => |cx: Cx<'_>, (name, args): (String, String)| {
                let call = json(&args).and_then(|args| theme_call(cx.data(), ThemeCall::BlockPattern(name, args)));
                answer(call.map(drop))
            },
            func_wrap "get-mod" => |cx: Cx<'_>, args| answer(get_mod(cx.data(), args)),
            func_wrap "set-mod" => |cx: Cx<'_>, (name, value): (String, Lifted)| {
                let call = lifted(value).and_then(|value| theme_call(cx.data(), ThemeCall::SetMod(name, value)));
                answer(call.map(|set| set == Value::Bool(true)))
            },
            func_wrap "remove-mod" => |cx: Cx<'_>, (name,): (String,)| {
                answer(theme_call(cx.data(), ThemeCall::RemoveMod(name)).map(|removed| removed == Value::Bool(true)))
            },
        }),
        interface!(linker, "customize", 2, {
            func_wrap "add" => |cx: Cx<'_>, args| answer(customize(cx.data(), args)),
            func_wrap "remove" => |cx: Cx<'_>, (kind, id): (CustomizeKind, String)| {
                Ok((yes(theme_call(cx.data(), ThemeCall::Uncustomize(kind, id))),))
            },
        }),
    ];
    defined.into_iter().collect()
}

#[cfg(test)]
mod tests;

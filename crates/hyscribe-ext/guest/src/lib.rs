// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The test extension (ADR 0022): drives every call of SDK 0.1 for the
//! host's tests, each round answering what the host said as text, and
//! misbehaves on purpose (a trap, an endless loop, a memory grab, a stack
//! overflow, an answer that does not decode). Built on `hyscribe-sdk`,
//! without `std`, so the component imports only the SDK's interfaces.

#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::fmt::Debug;
use core::hint::black_box;
use core::sync::atomic::{AtomicU32, Ordering};

use hyscribe_sdk::assets::{self, Asset, AssetKind};
use hyscribe_sdk::bindings::hyscribe::sdk::hooks as raw_hooks;
use hyscribe_sdk::bindings::hyscribe::sdk::query as raw_query;
use hyscribe_sdk::bindings::hyscribe::sdk::types::ValueKind;
use hyscribe_sdk::mail::{self, Message};
use hyscribe_sdk::query::{self, Query};
use hyscribe_sdk::{
    ObjectKind, Plugin, SdkError, Value, auth, blocks, content, cron, hooks, i18n, json, meta, options, rest,
    shortcodes,
};

hyscribe_sdk::callbacks! {
    enum Cb {
        Content => content_filter,
        Trap => trap,
        Spin => spin,
        Grow => grow,
        Late => late,
        OrderB => order_b,
        OrderA => order_a,
        Options => options_round,
        Cron => cron_round,
        Echo => echo,
        Meta => meta_round,
        Query => query_round,
        Mail => mail_round,
        Auth => auth_round,
        Assets => assets_round,
        Fire => fire_round,
        LinkOne => link_one,
        LinkTwo => link_two,
        Remove => remove_round,
        Shortcode => shortcode,
        Route => route,
        Permission => permission,
        Render => render,
        Current => current,
        Seen => seen,
        BadJson => bad_json,
        Refuse => refuse,
        Deep => deep,
        Count => count,
        Nest => nest,
        BodyClass => body_class,
        Footer => footer,
    }
}

struct Guest;

type Answer = Result<Value, String>;

fn text(s: String) -> Answer {
    Ok(Value::Text(s))
}

fn first(args: &[Value]) -> String {
    args.first().and_then(Value::text).unwrap_or_default().to_string()
}

fn content_filter(args: Vec<Value>) -> Answer {
    match args.first() {
        Some(Value::Text(content)) => text(format!("{content}<!-- ext -->")),
        _ => Err("no such callback, or not its argument".into()),
    }
}

fn trap(_: Vec<Value>) -> Answer {
    core::arch::wasm32::unreachable()
}

fn spin(_: Vec<Value>) -> Answer {
    let mut n = 0u64;
    loop {
        n = black_box(n.wrapping_add(1));
    }
}

fn grow(args: Vec<Value>) -> Answer {
    let mib = args.first().and_then(Value::int).unwrap_or_default();
    let len = black_box(vec![1u8; (mib as usize) << 20]).len();
    Ok(Value::Int(len as i64))
}

fn order_b(args: Vec<Value>) -> Answer {
    text(first(&args) + "b")
}

fn order_a(args: Vec<Value>) -> Answer {
    text(first(&args) + "a")
}

fn echo(args: Vec<Value>) -> Answer {
    Ok(args.into_iter().next().unwrap_or(Value::Null))
}

fn count(args: Vec<Value>) -> Answer {
    Ok(Value::Int(args.len() as i64))
}

fn link_one(args: Vec<Value>) -> Answer {
    text(first(&args) + &i18n::translate("1"))
}

fn link_two(args: Vec<Value>) -> Answer {
    text(first(&args) + &i18n::translate("2"))
}

fn seen(args: Vec<Value>) -> Answer {
    i18n::translate("seen");
    text(format!("seen:{}", first(&args)))
}

/// Fires `ext_chain` from inside a chain: run as another extension's step,
/// the extension that fired the outer chain is running and is skipped.
fn nest(args: Vec<Value>) -> Answer {
    Ok(hooks::filter::<Guest>("ext_chain", Value::Text(first(&args)), &[]))
}

/// The page hooks' calls in this instance: a fresh instance per request
/// starts again at one, whatever other requests ran.
static CALLS: AtomicU32 = AtomicU32::new(0);

/// What the option `ext_hooks` holds: whether the page hooks change the
/// page, and `trap` or `refuse` to misbehave, activation included.
fn switched() -> Option<String> {
    match options::get("ext_hooks") {
        Ok(Some(Value::Text(on))) => Some(on),
        _ => None,
    }
}

/// `body_class`: two classes added to the list, which crosses as JSON.
fn body_class(args: Vec<Value>) -> Answer {
    let classes = args.into_iter().next().unwrap_or(Value::Null);
    match (switched().as_deref(), classes) {
        (Some("trap"), _) => trap(Vec::new()),
        (Some(_), Value::Json(list)) => {
            let calls = CALLS.fetch_add(1, Ordering::Relaxed) + 1;
            let open = list.strip_suffix(']').unwrap_or("[");
            let comma = if open.ends_with('[') { "" } else { "," };
            Ok(Value::Json(format!("{open}{comma}\"ext-body\",\"ext-n{calls}\"]")))
        }
        (_, classes) => Ok(classes),
    }
}

/// `admin_footer_text`: a span added to the text.
fn footer(args: Vec<Value>) -> Answer {
    let footer = args.into_iter().next().unwrap_or(Value::Null);
    match (switched().as_deref(), footer) {
        (Some("trap"), _) => trap(Vec::new()),
        (Some(_), Value::Text(footer)) => {
            let calls = CALLS.fetch_add(1, Ordering::Relaxed) + 1;
            text(format!("{footer}<span id=\"ext-footer\">ext-n{calls}</span>"))
        }
        (_, footer) => Ok(footer),
    }
}

fn bad_json(_: Vec<Value>) -> Answer {
    Ok(Value::Json("{".into()))
}

fn refuse(_: Vec<Value>) -> Answer {
    Err("guest says no".into())
}

fn recurse(n: u64) -> u64 {
    if n == 0 { 0 } else { black_box(recurse(black_box(n - 1))) + 1 }
}

fn deep(_: Vec<Value>) -> Answer {
    Ok(Value::Int(recurse(black_box(u64::MAX)) as i64))
}

fn current(_: Vec<Value>) -> Answer {
    text(format!("{:?} {}", hooks::current(), hooks::did_action("ext_action")))
}

fn shortcode(args: Vec<Value>) -> Answer {
    text(format!("[{args:?}]"))
}

fn route(args: Vec<Value>) -> Answer {
    match args.first() {
        Some(Value::Json(request)) => Ok(Value::Json(format!("{{\"got\":{request}}}"))),
        _ => Err("a request is JSON".into()),
    }
}

fn permission(args: Vec<Value>) -> Answer {
    let request = match args.first() {
        Some(Value::Json(request)) => request.as_str(),
        _ => "",
    };
    if request.contains("allow") {
        Ok(Value::Bool(true))
    } else if request.contains("error") {
        Err("permission failed".into())
    } else {
        Ok(Value::Json("{\"code\":\"rest_forbidden\"}".into()))
    }
}

fn render(args: Vec<Value>) -> Answer {
    text(format!("<div>{args:?}</div>"))
}

fn rounds(steps: &[&dyn Debug]) -> Answer {
    let all: Vec<String> = steps.iter().map(|step| format!("{step:?}")).collect();
    text(all.join(" "))
}

/// Every registration, after `init`: each refused.
fn late(_: Vec<Value>) -> Answer {
    rounds(&[
        &hooks::on_filter("the_content", Cb::Content, 10, 1),
        &content::register_post_type("ext_item", "{}"),
        &content::register_taxonomy("ext_kind", &["ext_item".into()], "{}"),
        &meta::register(ObjectKind::Post, "ext_item", "_ext", "{}"),
        &rest::route("ext/v1", "/late", &["GET"], Cb::Route, Cb::Permission, "{}"),
        &shortcodes::register("ext", Cb::Shortcode),
        &blocks::register("blocks/late", None),
    ])
}

fn options_round(_: Vec<Value>) -> Answer {
    let one = Value::Int(1);
    rounds(&[
        &options::add("ext", &one, None),
        &options::add("ext", &one, Some(true)),
        &options::set("ext", &one, None),
        &options::set("ext", &Value::Int(2), Some(false)),
        &options::get("ext"),
        &options::delete("ext"),
        &options::delete("ext"),
        &options::get("ext"),
        &options::get("siteurl"),
        &options::set("siteurl", &one, None),
        &options::add("siteurl", &one, None),
        &options::delete("siteurl"),
        &options::get("ext_fail"),
        &options::set("ext_fail", &one, None),
        &options::add("ext_fail", &one, None),
        &options::delete("ext_fail"),
        &options::set("ext", &Value::Json("{".into()), None),
        &options::get("ext_bytes"),
        &options::set("ext", &Value::Json("[1,{\"a\":1.5}]".into()), None),
        &options::get("ext"),
    ])
}

fn meta_round(_: Vec<Value>) -> Answer {
    let post = ObjectKind::Post;
    let bad = Value::Json("{".into());
    rounds(&[
        &meta::add(post, 1, "_ext", &"a".into(), false),
        &meta::add(post, 1, "_ext", &"b".into(), true),
        &meta::get(post, 1, "_ext"),
        &meta::update(post, 1, "_ext", &"c".into(), Some(&"a".into())),
        &meta::update(post, 1, "_ext_title", &"t".into(), None),
        &meta::get(ObjectKind::User, 1, "_ext_title"),
        &meta::delete(post, 1, "_ext", Some(&"x".into())),
        &meta::delete(post, 1, "_ext", None),
        &meta::get(post, 1, "_ext"),
        &meta::get(ObjectKind::Term, 1, "_wp_secret"),
        &meta::add(ObjectKind::Comment, 1, "_wp_secret", &"a".into(), false),
        &meta::update(post, 1, "_wp_secret", &"a".into(), None),
        &meta::delete(post, 1, "_wp_secret", None),
        &meta::get(post, 1, "_ext_fail"),
        &meta::add(post, 1, "_ext_fail", &"a".into(), false),
        &meta::update(post, 1, "_ext_fail", &"a".into(), None),
        &meta::delete(post, 1, "_ext_fail", None),
        &meta::add(post, 1, "_ext", &bad, false),
        &meta::update(post, 1, "_ext", &bad, None),
        &meta::update(post, 1, "_ext", &"a".into(), Some(&bad)),
        &meta::delete(post, 1, "_ext", Some(&bad)),
        &meta::get(post, 2, "_ext_bytes"),
    ])
}

fn ids(found: Result<query::Found, SdkError>) -> Result<(Vec<u64>, u64, u32), SdkError> {
    found.map(|found| (found.posts.iter().map(|post| post.id).collect(), found.total, found.max_pages))
}

fn query_round(_: Vec<Value>) -> Answer {
    let typed = |json: &str| ids(Query::new().var("post_type", json).posts());
    let post = |id| query::get_post(id).map(|post| post.map(|post| (post.id, post.post_type, post.title)));
    rounds(&[
        &ids(Query::new().posts()),
        &typed("\"ext_item\""),
        &typed("[\"post\",\"ext_item\"]"),
        &typed("\"secret\""),
        &typed("\"any\""),
        &typed("[\"post\",\"any\"]"),
        &typed("5"),
        &ids(Query::new().var("fail", "1").posts()),
        &ids(raw_query::posts("5")),
        &ids(raw_query::posts("{")),
        &post(1),
        &post(3),
        &post(9),
        &post(99),
    ])
}

fn cron_round(_: Vec<Value>) -> Answer {
    let one = [Value::Int(1)];
    let bad = [Value::Json("{".into())];
    rounds(&[
        &cron::schedule(100, None, "ext_event", &one),
        &cron::schedule(50, Some("hourly"), "ext_event", &one),
        &cron::schedule(70, None, "ext_event", &[]),
        &cron::next("ext_event", &one),
        &cron::next("ext_event", &[]),
        &cron::unschedule("ext_event", &one),
        &cron::next("ext_event", &one),
        &cron::next("ext_event", &[]),
        &cron::schedule(1, None, "wp_version_check", &one),
        &cron::unschedule("wp_version_check", &[]),
        &cron::next("wp_version_check", &[]),
        &cron::schedule(1, Some("fail"), "ext_event", &[]),
        &cron::unschedule("ext_event", &["fail".into()]),
        &cron::schedule(1, None, "ext_event", &bad),
        &cron::unschedule("ext_event", &bad),
        &cron::next("ext_event", &bad),
    ])
}

fn message(subject: &str, attachments: &[&str]) -> Message {
    Message {
        to: vec!["a@example.com".into()],
        subject: subject.into(),
        body: "Hi".into(),
        headers: vec!["Reply-To: b@example.com".into()],
        attachments: attachments.iter().map(|path| String::from(*path)).collect(),
    }
}

fn mail_round(_: Vec<Value>) -> Answer {
    rounds(&[
        &mail::send(&message("Hello", &["uploads/ext/a.pdf"])),
        &mail::send(&message("Hello", &["uploads/other/a.pdf"])),
        &mail::send(&message("fail", &[])),
    ])
}

fn auth_round(_: Vec<Value>) -> Answer {
    rounds(&[
        &auth::current_user(),
        &auth::can("edit_posts", &[]),
        &auth::can("manage_options", &[]),
        &auth::can("edit_post", &[Value::Json("{".into())]),
        &auth::create_nonce("ext"),
        &auth::verify_nonce("nonce-ext", "ext"),
        &auth::verify_nonce("bad", "ext"),
        &i18n::translate("Hello"),
        &i18n::translate_with_context("Post", "noun"),
        &i18n::plural("item", "items", 2),
        &i18n::plural_with_context("item", "items", 1, "ctx"),
    ])
}

fn asset(handle: &str, src: &str) -> Asset {
    Asset {
        kind: AssetKind::Script,
        handle: handle.into(),
        src: src.into(),
        deps: vec!["jquery".into()],
        version: Some("1.0".into()),
        placement: "true".into(),
    }
}

fn assets_round(_: Vec<Value>) -> Answer {
    rounds(&[
        &assets::register(&asset("ext-app", "js/app.js")),
        &assets::register(&Asset {
            kind: AssetKind::Style,
            version: None,
            ..asset("ext-inline", "")
        }),
        &assets::register(&asset("", "js/app.js")),
        &assets::register(&asset("ext-up", "../wp-config.php")),
        &assets::register(&asset("fail", "js/app.js")),
        &assets::enqueue(AssetKind::Script, "ext-app"),
        &assets::enqueue(AssetKind::Style, "fail"),
        &assets::add_inline(AssetKind::Script, "ext-app", "var x = 1;", true),
        &assets::add_inline(AssetKind::Style, "fail", "", false),
    ])
}

/// `apply_filters()` and `do_action()` fired by the extension itself.
fn fire_round(_: Vec<Value>) -> Answer {
    let chained = hooks::filter::<Guest>("ext_chain", "x".into(), &[]);
    hooks::fire::<Guest>("ext_action", &["fired".into(), Value::Int(2)]);
    let stepped = hooks::filter::<Guest>("ext_action", "v".into(), &[]);
    let sensitive = hooks::filter::<Guest>("user_has_cap", "v".into(), &[]);
    let granted = hooks::filter::<Guest>("map_meta_cap", "m".into(), &[]);
    let none = hooks::filter::<Guest>("ext_nothing", "n".into(), &[]);
    let nested = hooks::filter::<Guest>("ext_nest", "z".into(), &[]);
    let stale = raw_hooks::run_step(u64::MAX, 0, &[]);
    let steps = raw_hooks::chain("ext_chain").map(|(token, links)| {
        let bad = (ValueKind::Json, 0, 0.0, String::from("{"));
        (links.len(), raw_hooks::run_step(token, 99, &[]), raw_hooks::run_step(token, 0, &[bad]))
    });
    rounds(&[&chained, &stepped, &sensitive, &granted, &none, &nested, &hooks::current(), &stale, &steps])
}

/// Removing its own callbacks and shortcodes for the request.
fn remove_round(_: Vec<Value>) -> Answer {
    let removed = [
        hooks::remove("ext_chain", Cb::LinkOne, 10),
        hooks::remove("ext_chain", Cb::LinkOne, 10),
        hooks::remove("ext_chain", Cb::LinkOne, 99),
        shortcodes::remove("ext"),
        shortcodes::remove("ext"),
        shortcodes::remove("gallery"),
    ];
    let chained = hooks::filter::<Guest>("ext_chain", "x".into(), &[]);
    rounds(&[&removed, &chained])
}

/// What `init` is answered, beside its registrations.
fn in_init() -> String {
    let chained = hooks::filter::<Guest>("ext_chain", "x".into(), &[]);
    let removable = [
        hooks::on_filter("ext_gone", Cb::Echo, 10, 1).is_ok(),
        hooks::remove("ext_gone", Cb::Echo, 10),
        hooks::remove("ext_gone", Cb::Echo, 10),
        shortcodes::register("ext_gone", Cb::Shortcode).is_ok(),
        shortcodes::remove("ext_gone"),
        shortcodes::remove("ext_gone"),
    ];
    let one = Value::Int(1);
    let post = ObjectKind::Post;
    let long_type = "ext_item_named_too_long";
    let long_taxonomy = "ext_kind_named_far_too_long_for_wp";
    let steps: [&dyn Debug; 48] = [
        &options::get("ext"),
        &options::set("ext", &one, None),
        &options::add("ext", &one, None),
        &options::delete("ext"),
        &meta::get(post, 1, "_ext"),
        &meta::add(post, 1, "_ext", &one, false),
        &meta::update(post, 1, "_ext", &one, None),
        &meta::delete(post, 1, "_ext", None),
        &ids(Query::new().posts()),
        &assets::register(&asset("ext-init", "")),
        &cron::schedule(1, None, "ext_event", &[]),
        &cron::unschedule("ext_event", &[]),
        &mail::send(&message("Hello", &[])),
        &raw_hooks::run_step(1, 0, &[]),
        &content::register_post_type(long_type, "{}"),
        &content::register_taxonomy(long_taxonomy, &[], "{}"),
        &assets::add_inline(AssetKind::Script, "x", "", false),
        &options::get("siteurl"),
        &query::get_post(1),
        &auth::current_user(),
        &auth::can("read", &[]),
        &auth::create_nonce("x"),
        &auth::verify_nonce("x", "x"),
        &i18n::translate("x"),
        &i18n::plural("one", "many", 1),
        &i18n::plural("one", "many", 2),
        &hooks::current(),
        &hooks::did_action("x"),
        &cron::next("ext_event", &[]),
        &chained,
        &assets::enqueue(AssetKind::Script, "x"),
        &removable,
        &rest::route("ext/v1", "items", &["GET"], Cb::Route, Cb::Permission, "{}"),
        &rest::route("ext/v1", "/x", &[], Cb::Route, Cb::Permission, "{}"),
        &rest::route("ext/v1", "/x", &["TRACE"], Cb::Route, Cb::Permission, "{}"),
        &rest::route("wp/v2", "/x", &["GET"], Cb::Route, Cb::Permission, "{}"),
        &rest::route("ext/v1", "/x", &["GET"], Cb::Route, Cb::Permission, "{"),
        &content::register_post_type("page", "{}"),
        &content::register_post_type("ext_item", "{"),
        &content::register_taxonomy("category", &[], "{}"),
        &content::register_taxonomy("ext_kind", &["page".into()], "{}"),
        &content::register_taxonomy("ext_kind", &["ext_item".into()], "{"),
        &meta::register(ObjectKind::Post, "ext_item", "_wp_x", "{}"),
        &meta::register(ObjectKind::Post, "ext_item", "_ext", "{"),
        &shortcodes::register("gallery", Cb::Shortcode),
        &blocks::register("../x", None),
        &hooks::on_filter("user_has_cap", Cb::Echo, 10, 1),
        &hooks::on_action("the_content", Cb::Echo, 10, 1),
    ];
    let all: Vec<String> = steps.iter().map(|step| format!("{step:?}")).collect();
    all.join("\n")
}

const IN_INIT: &str = r#"Err(SdkError::Failed("runs in a request, not in init"))
Err(SdkError::Failed("runs in a request, not in init"))
Err(SdkError::Failed("runs in a request, not in init"))
Err(SdkError::Failed("runs in a request, not in init"))
Err(SdkError::Failed("runs in a request, not in init"))
Err(SdkError::Failed("runs in a request, not in init"))
Err(SdkError::Failed("runs in a request, not in init"))
Err(SdkError::Failed("runs in a request, not in init"))
Err(SdkError::Failed("runs in a request, not in init"))
Err(SdkError::Failed("runs in a request, not in init"))
Err(SdkError::Failed("runs in a request, not in init"))
Err(SdkError::Failed("runs in a request, not in init"))
Err(SdkError::Failed("runs in a request, not in init"))
Err(SdkError::Failed("runs in a request, not in init"))
Err(SdkError::Invalid("Post type names must be between 1 and 20 characters in length."))
Err(SdkError::Invalid("Taxonomy names must be between 1 and 32 characters in length."))
Err(SdkError::Failed("runs in a request, not in init"))
Err(SdkError::Denied("option siteurl is not in the manifest"))
Err(SdkError::Failed("runs in a request, not in init"))
0
false
""
0
"x"
"one"
"many"
None
0
None
Text("x")
Err(SdkError::Failed("runs in a request, not in init"))
[true, true, false, true, true, false]
Err(SdkError::Invalid("a route starts with /"))
Err(SdkError::Invalid("methods are GET, POST, PUT, PATCH or DELETE"))
Err(SdkError::Invalid("methods are GET, POST, PUT, PATCH or DELETE"))
Err(SdkError::Denied("REST namespace wp/v2 is not in the manifest"))
Err(SdkError::Invalid("not JSON"))
Err(SdkError::Denied("post type page is not in the manifest"))
Err(SdkError::Invalid("not JSON"))
Err(SdkError::Denied("taxonomy category is not in the manifest"))
Err(SdkError::Denied("post type page is not in the manifest"))
Err(SdkError::Invalid("not JSON"))
Err(SdkError::Denied("meta key _wp_x is not in the manifest"))
Err(SdkError::Invalid("not JSON"))
Err(SdkError::Denied("shortcode gallery is not in the manifest"))
Err(SdkError::Invalid("../x is not a folder inside the package"))
Err(SdkError::Denied("user_has_cap is not in the manifest"))
Err(SdkError::Denied("the_content is not in the manifest"))"#;

const FILTERS: [(&str, Cb, i32, u32); 28] = [
    ("the_content", Cb::Content, 10, 1),
    ("ext_trap", Cb::Trap, 10, 1),
    ("ext_loop", Cb::Spin, 10, 1),
    ("ext_grow", Cb::Grow, 10, 1),
    ("ext_late", Cb::Late, 10, 1),
    ("ext_order", Cb::OrderB, 10, 1),
    ("ext_order", Cb::OrderA, 9, 1),
    ("ext_options", Cb::Options, 10, 1),
    ("ext_cron", Cb::Cron, 10, 1),
    ("ext_echo", Cb::Echo, 10, 1),
    ("ext_meta", Cb::Meta, 10, 1),
    ("ext_query", Cb::Query, 10, 1),
    ("ext_mail", Cb::Mail, 10, 1),
    ("ext_auth", Cb::Auth, 10, 1),
    ("ext_assets", Cb::Assets, 10, 1),
    ("ext_fire", Cb::Fire, 10, 1),
    ("ext_chain", Cb::LinkOne, 10, 1),
    ("ext_chain", Cb::LinkTwo, 20, 1),
    ("ext_remove", Cb::Remove, 10, 1),
    ("ext_current", Cb::Current, 10, 1),
    ("ext_bad_json", Cb::BadJson, 10, 1),
    ("ext_error", Cb::Refuse, 10, 1),
    ("ext_stack", Cb::Deep, 10, 1),
    ("ext_count", Cb::Count, 10, 2),
    ("map_meta_cap", Cb::Echo, 10, 1),
    ("ext_nest", Cb::Nest, 10, 1),
    ("body_class", Cb::BodyClass, 10, 1),
    ("admin_footer_text", Cb::Footer, 10, 1),
];

fn why(e: SdkError) -> String {
    match e {
        SdkError::Denied(why) | SdkError::Invalid(why) | SdkError::Failed(why) => why,
    }
}

impl Plugin for Guest {
    fn init() -> Result<(), String> {
        for (hook, callback, priority, accepted) in FILTERS {
            hooks::on_filter(hook, callback, priority, accepted).map_err(why)?;
        }
        hooks::on_action("ext_action", Cb::Seen, 10, 2).map_err(why)?;
        content::register_post_type("ext_item", &json::object(&[("public", "true")])).map_err(why)?;
        content::register_taxonomy("ext_kind", &["ext_item".into()], "{}").map_err(why)?;
        meta::register(ObjectKind::Post, "ext_item", "_ext", "{\"single\":true}").map_err(why)?;
        rest::route("ext/v1", "/items", &["GET", "POST"], Cb::Route, Cb::Permission, "{}").map_err(why)?;
        shortcodes::register("ext", Cb::Shortcode).map_err(why)?;
        blocks::register("blocks/card", Some(Cb::Render.into())).map_err(why)?;
        blocks::register("blocks/static", None).map_err(why)?;
        let answered = in_init();
        if answered == IN_INIT { Ok(()) } else { Err(answered) }
    }

    fn activate() -> Result<(), String> {
        match switched().as_deref() {
            Some("trap") => trap(Vec::new()).map(|_| ()),
            Some("refuse") => Err("refused".into()),
            _ => options::add("ext_active", &Value::Bool(true), None).map(|_| ()).map_err(why),
        }
    }

    fn deactivate() -> Result<(), String> {
        if switched().as_deref() == Some("trap") {
            return trap(Vec::new()).map(|_| ());
        }
        match options::delete("ext_active").map_err(why)? {
            true => Ok(()),
            false => Err("not active".into()),
        }
    }

    fn invoke(callback: u32, args: Vec<Value>) -> Result<Value, String> {
        Cb::invoke(callback, args)
    }
}

hyscribe_sdk::plugin!(Guest);

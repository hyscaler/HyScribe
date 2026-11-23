// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Every call of the SDK against the test host, through a small test
//! plugin and theme built natively, as a port's own tests are.

use alloc::format;
use alloc::string::ToString;
use alloc::vec;

use super::*;
use crate::assets::{self, Asset, AssetKind};
use crate::customize::{self, CustomizeKind};
use crate::mail::{self, Message};
use crate::query::{self, Query};
use crate::theme::{self, TemplateFile};
use crate::{
    SdkError, auth, blocks, content, cron, hooks, http, i18n, meta, options, rest, roles,
    shortcodes, template, transients,
};

crate::callbacks! {
    enum Cb {
        Append => append,
        Count => count,
        Nested => nested,
        Seen => seen,
    }
}

fn text(value: &str) -> Value {
    Value::Text(value.into())
}

fn append(args: Vec<Value>) -> Result<Value, String> {
    let first = args.first().and_then(Value::text).unwrap_or_default();
    Ok(text(&format!("{first}+demo")))
}

fn count(args: Vec<Value>) -> Result<Value, String> {
    Ok(Value::Int(args.len() as i64))
}

/// Fires `demo_inner` from inside a callback, as a port fires its own hooks.
fn nested(args: Vec<Value>) -> Result<Value, String> {
    let first = args.into_iter().next().unwrap_or(Value::Null);
    Ok(hooks::filter::<Demo>("demo_inner", first, &[]))
}

fn seen(_: Vec<Value>) -> Result<Value, String> {
    Err(hooks::current().unwrap_or_default())
}

/// The test plugin, and the test theme.
struct Demo;

impl Plugin for Demo {
    fn init() -> Result<(), String> {
        let why = |e: SdkError| format!("{e:?}");
        hooks::on_filter("the_content", Cb::Append, 10, 1).map_err(why)?;
        hooks::on_filter("demo_count", Cb::Count, 10, 2).map_err(why)?;
        hooks::on_filter("demo_outer", Cb::Nested, 10, 1).map_err(why)?;
        hooks::on_filter("demo_inner", Cb::Append, 20, 1).map_err(why)?;
        hooks::on_action("demo_seen", Cb::Seen, 10, 0).map_err(why)?;
        hooks::on_filter("demo_gone", Cb::Append, 10, 1).map_err(why)?;
        assert!(hooks::remove("demo_gone", Cb::Append, 10));
        assert!(!hooks::remove("demo_gone", Cb::Append, 10));
        assert_eq!(
            options::get("x"),
            Err(SdkError::Failed("runs in a request, not in init".into()))
        );
        theme::templates(&[
            file("index.php"),
            file("header.php"),
            file("header-wide.php"),
            file("footer.php"),
            file("sidebar-a.php"),
            file("template-parts/content.php"),
        ])
        .map_err(why)
    }

    fn activate() -> Result<(), String> {
        options::add("demo_active", &Value::Bool(true), None)
            .map(drop)
            .map_err(|e| format!("{e:?}"))
    }

    fn deactivate() -> Result<(), String> {
        Err("kept".into())
    }

    fn invoke(callback: u32, args: Vec<Value>) -> Result<Value, String> {
        Cb::invoke(callback, args)
    }
}

impl Theme for Demo {
    fn render(file: &str) -> Result<Vec<u8>, String> {
        let mut page = template::print("wp_head", &[]).map_err(|e| format!("{e:?}"))?;
        page.extend_from_slice(file.as_bytes());
        Ok(page)
    }
}

fn file(name: &str) -> TemplateFile {
    TemplateFile {
        file: name.into(),
        name: None,
        post_types: Vec::new(),
    }
}

fn host() -> Host<Demo> {
    let host = Host::new();
    assert_eq!(host.init(), Ok(()));
    host
}

fn failed(call: &str) -> SdkError {
    SdkError::Failed(format!("{call} failed in the test host"))
}

#[test]
fn registers_in_init_only() {
    let host = host();
    host.add("the_content", 1, |_| Value::Null);
    let hooks_added = host.state(|state| state.hooks());
    assert_eq!(hooks_added.len(), 5);
    assert_eq!(
        hooks_added.get(1),
        Some(&Hook {
            hook: "demo_count".into(),
            action: false,
            callback: Cb::Count as u32,
            priority: 10,
            accepted_args: 2,
        })
    );
    assert!(hooks_added.iter().any(|hook| hook.action));
    let denied = Err(SdkError::Denied("registered in init only".into()));
    assert_eq!(hooks::on_filter("x", Cb::Append, 10, 1), denied);
    assert_eq!(hooks::on_action("x", Cb::Append, 10, 1), denied);
    assert_eq!(content::register_post_type("book", "{}"), denied);
    assert_eq!(content::register_taxonomy("genre", &[], "{}"), denied);
    assert_eq!(meta::register(ObjectKind::Post, "post", "_k", "{}"), denied);
    let route = rest::route("demo/v1", "/x", &["GET"], Cb::Append, Cb::Count, "{}");
    assert_eq!(route, denied);
    assert_eq!(shortcodes::register("demo", Cb::Append), denied);
    assert_eq!(blocks::register("blocks/a", None), denied);
    assert_eq!(theme::templates(&[]), denied);
    host.state(|state| state.registering = true);
    assert_eq!(
        content::register_post_type("book", "{\"public\":true}"),
        Ok(())
    );
    let types = ["book".to_string()];
    assert_eq!(content::register_taxonomy("genre", &types, "{}"), Ok(()));
    assert_eq!(meta::register(ObjectKind::User, "", "_k", "{}"), Ok(()));
    assert_eq!(
        rest::route(
            "demo/v1",
            "/x",
            &["GET", "POST"],
            Cb::Append,
            Cb::Count,
            "{}"
        ),
        Ok(())
    );
    assert_eq!(shortcodes::register("demo", Cb::Append), Ok(()));
    assert_eq!(blocks::register("blocks/a", Some(3)), Ok(()));
    host.state(|state| state.fails.insert("blocks.register"));
    assert_eq!(
        blocks::register("blocks/b", None),
        Err(failed("blocks.register"))
    );
    host.state(|state| {
        assert_eq!(
            state.post_types,
            [("book".into(), "{\"public\":true}".into())]
        );
        assert_eq!(
            state.taxonomies,
            [("genre".into(), vec!["book".into()], "{}".into())]
        );
        let key = [String::new(), "_k".into(), "{}".into()];
        assert_eq!(state.meta_keys, [(ObjectKind::User, key)]);
        let route = state.routes.first().cloned();
        assert_eq!(
            route.map(|r| (r.namespace, r.methods.len(), r.permission)),
            Some(("demo/v1".into(), 2, 1))
        );
        assert_eq!(state.blocks, [("blocks/a".into(), Some(3))]);
        assert_eq!(state.templates.len(), 6);
    });
    assert!(shortcodes::remove("demo"));
    assert!(!shortcodes::remove("demo"));
}

#[test]
fn fires_hooks_as_core_and_as_the_plugin() {
    let host = host();
    host.add("the_content", 5, |args| {
        let first = args.first().and_then(Value::text).unwrap_or_default();
        text(&format!("{first}+core"))
    });
    host.add("demo_inner", 30, |args| {
        args.into_iter().next().unwrap_or(Value::Null)
    });
    host.add("demo_inner", 10, |_| text("other"));
    assert_eq!(
        host.filter("the_content", text("p"), &[]),
        text("p+core+demo")
    );
    assert_eq!(
        host.filter("demo_count", Value::Null, &[text("a"), text("b")]),
        Value::Int(2)
    );
    assert_eq!(
        host.filter("demo_outer", text("x"), &[]),
        text("other+demo")
    );
    assert_eq!(host.filter("demo_gone", text("x"), &[]), text("x"));
    assert_eq!(
        hooks::filter::<Demo>("the_content", text("q"), &[]),
        text("q+core+demo")
    );
    let ran = Rc::new(core::cell::Cell::new(0));
    let counted = ran.clone();
    host.add("demo_fired", 10, move |_| {
        counted.set(counted.get() + 1);
        Value::Null
    });
    hooks::fire::<Demo>("demo_fired", &[text("a")]);
    assert_eq!(ran.get(), 1);
    // An action prints the strings its callbacks answer, no other answer.
    assert_eq!(host.action("demo_fired", &[]), "");
    for _ in 0..2 {
        assert_eq!(host.action("demo_seen", &[]), "");
    }
    assert_eq!(hooks::did_action("demo_seen"), 2);
    assert_eq!(hooks::did_action("demo_other"), 0);
    host.add("demo_seen", 20, |_| {
        text(&hooks::current().unwrap_or_default())
    });
    assert_eq!(host.action("demo_seen", &[]), "demo_seen");
    assert_eq!(
        host.filter("demo_seen", Value::Null, &[]),
        text("demo_seen")
    );
    assert_eq!(hooks::current(), None);
    assert!(hooks::remove("the_content", Cb::Append, 10));
    assert!(!hooks::remove("the_content", Cb::Append, 10));
    assert!(!hooks::remove("the_content", Cb::Count, 10));
    assert_eq!(host.filter("the_content", text("p"), &[]), text("p+core"));
    let no_step = Err(SdkError::Invalid("no such step".into()));
    assert_eq!(crate::sys::hooks::run_step(99, 0, &[]), no_step);
    assert_eq!(crate::sys::hooks::run_step(1, 99, &[]), no_step);
    host.state(|state| state.fails.insert("hooks.run-step"));
    assert_eq!(
        crate::sys::hooks::run_step(0, 0, &[]),
        Err(failed("hooks.run-step"))
    );
    host.state(|state| state.fails.insert("hooks.chain"));
    assert_eq!(
        hooks::filter::<Demo>("demo_inner", text("y"), &[]),
        text("y")
    );
}

#[test]
fn options_and_transients() {
    let _host = host();
    let one = Value::Int(1);
    assert_eq!(options::get("a"), Ok(None));
    assert_eq!(options::add("a", &one, None), Ok(true));
    assert_eq!(options::add("a", &Value::Int(2), Some(true)), Ok(false));
    assert_eq!(options::get("a"), Ok(Some(one.clone())));
    assert_eq!(options::set("a", &one, None), Ok(false));
    assert_eq!(options::set("a", &text("b"), Some(false)), Ok(true));
    assert_eq!(options::get("a"), Ok(Some(text("b"))));
    assert_eq!(options::delete("a"), Ok(true));
    assert_eq!(options::delete("a"), Ok(false));
    assert_eq!(transients::get("t"), Ok(None));
    assert_eq!(transients::set("t", &one, 60), Ok(true));
    assert_eq!(transients::get("t"), Ok(Some(one.clone())));
    assert_eq!(transients::get_site("t"), Ok(None));
    assert_eq!(transients::set_site("t", &text("s"), 0), Ok(true));
    assert_eq!(transients::get_site("t"), Ok(Some(text("s"))));
    assert_eq!(transients::delete("t"), Ok(true));
    assert_eq!(transients::delete("t"), Ok(false));
    assert_eq!(transients::delete_site("t"), Ok(true));
    assert_eq!(transients::delete_site("t"), Ok(false));
    with(|state| state.fails.insert("options.get"));
    assert_eq!(options::get("a"), Err(failed("options.get")));
}

#[test]
fn meta_as_wordpress_keeps_it() {
    let _host = host();
    let post = ObjectKind::Post;
    assert_eq!(meta::get(post, 1, "_k"), Ok(Vec::new()));
    assert_eq!(meta::add(post, 1, "_k", &text("a"), true), Ok(Some(1)));
    assert_eq!(meta::add(post, 1, "_k", &text("b"), true), Ok(None));
    assert_eq!(meta::add(post, 1, "_k", &text("b"), false), Ok(Some(2)));
    assert_eq!(meta::get(post, 1, "_k"), Ok(vec![text("a"), text("b")]));
    assert_eq!(meta::get(ObjectKind::Term, 1, "_k"), Ok(Vec::new()));
    let a = text("a");
    assert_eq!(meta::update(post, 1, "_k", &text("c"), Some(&a)), Ok(true));
    assert_eq!(meta::update(post, 1, "_k", &text("c"), Some(&a)), Ok(false));
    assert_eq!(meta::get(post, 1, "_k"), Ok(vec![text("c"), text("b")]));
    assert_eq!(meta::update(post, 1, "_k", &text("d"), None), Ok(true));
    assert_eq!(meta::update(post, 1, "_k", &text("d"), None), Ok(false));
    assert_eq!(meta::update(post, 2, "_k", &text("e"), None), Ok(true));
    assert_eq!(meta::get(post, 2, "_k"), Ok(vec![text("e")]));
    assert_eq!(meta::delete(post, 1, "_k", Some(&text("x"))), Ok(false));
    assert_eq!(meta::add(post, 1, "_k", &text("f"), false), Ok(Some(3)));
    assert_eq!(meta::delete(post, 1, "_k", Some(&text("f"))), Ok(true));
    assert_eq!(meta::get(post, 1, "_k"), Ok(vec![text("d"), text("d")]));
    assert_eq!(meta::delete(post, 1, "_k", None), Ok(true));
    assert_eq!(meta::get(post, 1, "_k"), Ok(Vec::new()));
}

#[test]
fn queries_routes_assets_cron_and_mail() {
    let host = host();
    let post = query::Post {
        id: 4,
        title: "Four".into(),
        ..query::Post::default()
    };
    host.state(|state| {
        state.posts.push(post.clone());
        state.found.total = 9;
    });
    let found = Query::new()
        .var("post_type", "\"book\"")
        .var("paged", "2")
        .posts();
    assert_eq!(found.map(|found| found.total), Ok(9));
    assert_eq!(query::get_post(4), Ok(Some(post)));
    assert_eq!(query::get_post(5), Ok(None));
    host.state(|state| assert_eq!(state.queries, ["{\"post_type\":\"book\",\"paged\":2}"]));
    let asset = Asset {
        kind: AssetKind::Style,
        handle: "demo".into(),
        src: "style.css".into(),
        deps: Vec::new(),
        version: Some("1".into()),
        placement: "all".into(),
    };
    assert_eq!(assets::register(&asset), Ok(()));
    assert_eq!(assets::enqueue(AssetKind::Style, "demo"), Ok(()));
    assert_eq!(
        assets::add_inline(AssetKind::Script, "demo", "x()", true),
        Ok(())
    );
    host.state(|state| {
        assert_eq!(state.assets, std::slice::from_ref(&asset));
        assert_eq!(state.enqueued, [(AssetKind::Style, "demo".into())]);
        assert_eq!(
            state.inline,
            [(AssetKind::Script, "demo".into(), "x()".into(), true)]
        );
    });
    let args = [Value::Int(1)];
    assert_eq!(cron::schedule(20, Some("daily"), "demo_job", &args), Ok(()));
    assert_eq!(cron::schedule(10, None, "demo_job", &args), Ok(()));
    assert_eq!(cron::schedule(5, None, "demo_job", &[]), Ok(()));
    assert_eq!(cron::schedule(1, None, "demo_other", &args), Ok(()));
    assert_eq!(cron::schedule(2, None, "demo_other", &args), Ok(()));
    assert_eq!(cron::next("demo_job", &args), Some(10));
    assert_eq!(cron::unschedule("demo_job", &args), Ok(2));
    assert_eq!(cron::next("demo_job", &args), None);
    assert_eq!(cron::next("demo_job", &[]), Some(5));
    host.state(|state| {
        assert_eq!(state.events.len(), 3);
        state.fails.insert("cron.next");
    });
    assert_eq!(cron::next("demo_job", &[]), None);
    let message = Message {
        to: vec!["a@example.com".into()],
        subject: "Hi".into(),
        body: "Body".into(),
        headers: Vec::new(),
        attachments: Vec::new(),
    };
    assert_eq!(mail::send(&message), Ok(true));
    host.state(|state| assert_eq!(state.mails, [message]));
}

#[test]
fn users_translations_roles_and_requests() {
    let host = host();
    assert_eq!(auth::current_user(), 0);
    assert!(!auth::can("edit_posts", &[]));
    host.state(|state| {
        state.user = 3;
        state.caps.insert("edit_posts".into());
    });
    assert_eq!(auth::current_user(), 3);
    assert!(auth::can("edit_posts", &[Value::Int(1)]));
    assert_eq!(auth::create_nonce("save"), "nonce-save");
    assert_eq!(auth::verify_nonce("nonce-save", "save"), 1);
    assert_eq!(auth::verify_nonce("nonce-x", "save"), 0);
    host.state(|state| state.fails.insert("auth.create-nonce"));
    assert_eq!(auth::verify_nonce("", "save"), 0);
    assert_eq!(i18n::translate("Hi"), "Hi");
    assert_eq!(i18n::translate_with_context("Post", "noun"), "Post");
    assert_eq!(i18n::plural("item", "items", 1), "item");
    assert_eq!(i18n::plural_with_context("item", "items", 2, "x"), "items");
    assert_eq!(roles::get("demo"), Ok(None));
    assert_eq!(roles::add("demo", "Demo", &[("read", true)]), Ok(true));
    assert_eq!(roles::add("demo", "Again", &[]), Ok(false));
    assert_eq!(roles::get("demo"), Ok(Some(vec![("read".into(), true)])));
    assert_eq!(roles::add_cap("demo", "read", false), Ok(true));
    assert_eq!(roles::add_cap("demo", "edit", true), Ok(true));
    assert_eq!(
        roles::get("demo"),
        Ok(Some(vec![("read".into(), false), ("edit".into(), true)]))
    );
    assert_eq!(roles::remove_cap("demo", "read"), Ok(true));
    assert_eq!(roles::get("demo"), Ok(Some(vec![("edit".into(), true)])));
    assert_eq!(roles::add_cap("none", "read", true), Ok(false));
    assert_eq!(roles::remove_cap("none", "read"), Ok(false));
    assert_eq!(roles::remove("demo"), Ok(true));
    assert_eq!(roles::remove("demo"), Ok(false));
    let answer: Response = (200, vec![("x".into(), b"1".to_vec())], b"ok".to_vec());
    host.state(|state| {
        state
            .responses
            .insert("https://api.test/".into(), answer.clone())
    });
    let sent = http::send("POST", "https://api.test/", &[("a", "b")], b"body", 5);
    assert_eq!(sent, Ok(answer));
    assert_eq!(
        http::send("GET", "https://other.test/", &[], b"", 1),
        Err(SdkError::Failed("no answer for https://other.test/".into()))
    );
    host.state(|state| {
        let first = state.sent.first().cloned();
        let expected = Sent {
            method: "POST".into(),
            url: "https://api.test/".into(),
            headers: vec![("a".into(), "b".into())],
            body: b"body".to_vec(),
            timeout: 5,
        };
        assert_eq!(first, Some(expected));
        assert_eq!(state.sent.len(), 2);
        state.fails.insert("http.send");
    });
    let refused = http::send("GET", "https://api.test/", &[], b"", 1);
    assert_eq!(refused, Err(failed("http.send")));
}

#[test]
fn a_theme_renders_over_the_loop_and_its_tags() {
    let host = host();
    assert_eq!(
        host.render("index.php"),
        Err("SdkError::Invalid(\"wp_head is not served by the test host\")".into())
    );
    host.state(|state| {
        state.printed.insert("wp_head".into(), b"<head>".to_vec());
        state.values.insert("get_the_ID".into(), Value::Int(4));
        state.conditionals.insert("is_home".into());
        state.posts = vec![query::Post::default(), query::Post::default()];
    });
    assert_eq!(host.render("index.php"), Ok(b"<head>index.php".to_vec()));
    assert_eq!(template::is("is_home", &[]), Ok(true));
    assert_eq!(template::is("is_page", &[Value::Int(2)]), Ok(false));
    assert_eq!(template::get("get_the_ID", &[]), Ok(Value::Int(4)));
    assert_eq!(
        template::get("get_bloginfo", &[]),
        Err(SdkError::Invalid(
            "get_bloginfo is not served by the test host".into()
        ))
    );
    let mut seen = 0;
    while template::have_posts() {
        template::the_post();
        assert!(template::in_the_loop());
        seen += 1;
    }
    assert_eq!(seen, 2);
    assert!(!template::in_the_loop());
    template::rewind_posts();
    assert!(template::have_posts());
    template::wp_reset_postdata();
    assert_eq!(host.state(|state| state.resets), 1);
    assert_eq!(template::get_header(None, &[]), Some("header.php".into()));
    assert_eq!(
        template::get_header(Some("wide"), &[]),
        Some("header-wide.php".into())
    );
    assert_eq!(
        template::get_header(Some("x"), &[]),
        Some("header.php".into())
    );
    assert_eq!(template::get_footer(None, &[]), Some("footer.php".into()));
    assert_eq!(
        template::get_sidebar(Some("a"), &[]),
        Some("sidebar-a.php".into())
    );
    assert_eq!(template::get_sidebar(None, &[]), None);
    let part = template::get_template_part("template-parts/content", Some("page"), &[]);
    assert_eq!(part, Some("template-parts/content.php".into()));
    host.state(|state| {
        state.fails.insert("template.have-posts");
        state.fails.insert("template.print");
        state.fails.insert("template.get");
    });
    assert!(!template::have_posts());
    assert_eq!(
        template::print("wp_head", &[]),
        Err(failed("template.print"))
    );
    assert_eq!(
        template::get("get_the_ID", &[]),
        Err(failed("template.get"))
    );
}

#[test]
fn a_theme_sets_itself_up() {
    let host = host();
    assert_eq!(theme::add_support("title-tag", &[]), Ok(()));
    assert_eq!(theme::add_support("html5", &[text("search-form")]), Ok(()));
    assert!(theme::remove_support("title-tag"));
    assert!(!theme::remove_support("title-tag"));
    assert_eq!(theme::register_nav_menus(&[("menu-1", "Header")]), Ok(()));
    assert_eq!(theme::register_sidebar("{}"), Ok("sidebar-1".into()));
    assert_eq!(theme::register_sidebar("{}"), Ok("sidebar-2".into()));
    assert_eq!(
        theme::add_image_size("wide", 1200, 0, &Value::Bool(true)),
        Ok(())
    );
    assert_eq!(
        theme::set_post_thumbnail_size(800, 600, &Value::Bool(false)),
        Ok(())
    );
    assert_eq!(theme::add_editor_style(&["editor.css"]), Ok(()));
    assert_eq!(theme::set_content_width(800), Ok(()));
    assert_eq!(theme::register_block_style("core/button", "{}"), Ok(()));
    assert_eq!(theme::register_block_pattern("demo/hero", "{}"), Ok(()));
    assert_eq!(theme::get_mod("color", None), Ok(None));
    assert_eq!(
        theme::get_mod("color", Some(&text("blue"))),
        Ok(Some(text("blue")))
    );
    assert_eq!(theme::set_mod("color", &text("red")), Ok(true));
    assert_eq!(
        theme::get_mod("color", Some(&text("blue"))),
        Ok(Some(text("red")))
    );
    assert_eq!(theme::remove_mod("color"), Ok(true));
    assert_eq!(theme::remove_mod("color"), Ok(false));
    let callbacks = [("sanitize_callback", 2)];
    assert_eq!(
        customize::add(CustomizeKind::Setting, "links", "{}", &callbacks),
        Ok(())
    );
    assert_eq!(
        customize::add(CustomizeKind::Control, "links", "{}", &[]),
        Ok(())
    );
    assert!(customize::remove(CustomizeKind::Setting, "links"));
    assert!(!customize::remove(CustomizeKind::Setting, "links"));
    assert!(!customize::remove(CustomizeKind::Panel, "links"));
    host.state(|state| {
        assert_eq!(
            state.supports,
            [("html5".into(), vec![text("search-form")])]
        );
        assert_eq!(state.nav_menus, [("menu-1".into(), "Header".into())]);
        assert_eq!(
            state.image_sizes,
            [("wide".into(), 1200, 0, Value::Bool(true))]
        );
        assert_eq!(state.thumbnail, Some((800, 600, Value::Bool(false))));
        assert_eq!(state.editor_styles, ["editor.css"]);
        assert_eq!(state.content_width, Some(800));
        assert_eq!(state.block_styles, [("core/button".into(), "{}".into())]);
        assert_eq!(state.block_patterns, [("demo/hero".into(), "{}".into())]);
        let left = Customized {
            kind: CustomizeKind::Control,
            id: "links".into(),
            args: "{}".into(),
            callbacks: Vec::new(),
        };
        assert_eq!(state.customized, [left]);
        state.fails.insert("theme-setup.remove-support");
        state.fails.insert("customize.remove");
    });
    assert_eq!(theme::add_support("x", &[]), Ok(()));
    assert!(!theme::remove_support("x"));
    assert!(!customize::remove(CustomizeKind::Control, "links"));
}

#[test]
fn the_lifecycle_and_the_wire() {
    let host: Host<Demo> = Host::default();
    host.state(|state| state.registering = true);
    assert_eq!(
        options::get("x"),
        Err(SdkError::Failed("runs in a request, not in init".into()))
    );
    host.state(|state| state.registering = false);
    assert_eq!(host.activate(), Ok(()));
    assert_eq!(options::get("demo_active"), Ok(Some(Value::Bool(true))));
    assert_eq!(host.deactivate(), Err("kept".into()));
    drop(host);
    assert_eq!(with(|state| state.options.len()), 0);
    assert_eq!(
        format!("{:?}", SdkError::Denied("a".into())),
        "SdkError::Denied(\"a\")"
    );
    assert_eq!(
        format!("{:?}", SdkError::Invalid("b".into())),
        "SdkError::Invalid(\"b\")"
    );
    assert_eq!(
        format!("{:?}", SdkError::Failed("c".into())),
        "SdkError::Failed(\"c\")"
    );
    let kinds = [
        ObjectKind::Post,
        ObjectKind::User,
        ObjectKind::Term,
        ObjectKind::Comment,
    ];
    assert_eq!(
        format!("{kinds:?}"),
        "[ObjectKind::Post, ObjectKind::User, ObjectKind::Term, ObjectKind::Comment]"
    );
    let hook_kinds = [
        crate::sys::hooks::HookKind::Action,
        crate::sys::hooks::HookKind::Filter,
    ];
    assert_eq!(
        format!("{hook_kinds:?}"),
        "[HookKind::Action, HookKind::Filter]"
    );
    assert_eq!(
        format!("{:?}", [AssetKind::Script, AssetKind::Style]),
        "[AssetKind::Script, AssetKind::Style]"
    );
    use crate::sys::template::PartKind;
    let parts = [
        PartKind::Header,
        PartKind::Footer,
        PartKind::Sidebar,
        PartKind::TemplatePart,
    ];
    assert_eq!(
        format!("{parts:?}"),
        "[PartKind::Header, PartKind::Footer, PartKind::Sidebar, PartKind::TemplatePart]"
    );
    let customized = [
        CustomizeKind::Panel,
        CustomizeKind::Section,
        CustomizeKind::Setting,
        CustomizeKind::Control,
        CustomizeKind::Partial,
    ];
    assert_eq!(
        format!("{customized:?}"),
        "[CustomizeKind::Panel, CustomizeKind::Section, CustomizeKind::Setting, CustomizeKind::Control, CustomizeKind::Partial]"
    );
    let values = [
        Value::Null,
        Value::Bool(true),
        Value::Int(-2),
        Value::Float(1.5),
        text("t"),
        Value::Json("[]".into()),
    ];
    for value in values {
        assert_eq!(Value::__from_wit(value.__to_wit()), value);
    }
}

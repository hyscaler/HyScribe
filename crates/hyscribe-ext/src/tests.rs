// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::BTreeMap;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use hyscribe_phpser::{Key, Value, lookup};

use super::*;
use crate::faults::{Counted, Kind, Record};
use crate::host::State;

#[path = "../tests/guest/mod.rs"]
mod guest;

/// A site in memory, failing where a name asks it to, and logging what
/// the extension did that leaves no other trace.
#[derive(Default)]
pub(crate) struct TestSite {
    options: Mutex<BTreeMap<String, Value>>,
    meta: Mutex<BTreeMap<(u8, u64, String), Vec<Value>>>,
    events: Mutex<Vec<(i64, String, Vec<Value>)>>,
    pub(crate) log: Mutex<Vec<String>>,
    pub(crate) now: AtomicU64,
}

fn text(s: &str) -> Value {
    Value::String(s.as_bytes().to_vec())
}

fn post(id: u64, post_type: &str) -> Post {
    Post {
        id,
        post_type: post_type.into(),
        title: format!("Post {id}"),
        ..Post::default()
    }
}

const FAIL: &str = "the database went away";

fn held<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn fails(name: &str) -> Result<(), String> {
    if name.ends_with("fail") {
        Err(FAIL.into())
    } else {
        Ok(())
    }
}

impl TestSite {
    pub(crate) fn new() -> Arc<TestSite> {
        let site = TestSite::default();
        held(&site.options).insert("ext_bytes".into(), Value::String(vec![0xff]));
        Arc::new(site)
    }

    pub(crate) fn logged(&self) -> Vec<String> {
        held(&self.log).clone()
    }

    fn note(&self, line: String) {
        held(&self.log).push(line);
    }

    pub(crate) fn option(&self, name: &str) -> Option<Value> {
        held(&self.options).get(name).cloned()
    }
}

impl Site for TestSite {
    fn now(&self) -> u64 {
        self.now.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn get_option(&self, name: &str) -> Result<Option<Value>, String> {
        fails(name)?;
        Ok(self.option(name))
    }

    fn update_option(&self, name: &str, value: Value, _: Option<bool>) -> Result<bool, String> {
        fails(name)?;
        let mut options = self.options.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(options.insert(name.into(), value.clone()) != Some(value))
    }

    fn add_option(&self, name: &str, value: Value, _: Option<bool>) -> Result<bool, String> {
        fails(name)?;
        let mut options = self.options.lock().unwrap_or_else(PoisonError::into_inner);
        let added = !options.contains_key(name);
        options.entry(name.into()).or_insert(value);
        Ok(added)
    }

    fn delete_option(&self, name: &str) -> Result<bool, String> {
        fails(name)?;
        let mut options = self.options.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(options.remove(name).is_some())
    }

    fn get_meta(&self, kind: ObjectKind, id: u64, key: &str) -> Result<Vec<Value>, String> {
        fails(key)?;
        if key.ends_with("bytes") {
            return Ok(vec![Value::String(vec![0xff])]);
        }
        let meta = self.meta.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(meta
            .get(&(kind as u8, id, key.into()))
            .cloned()
            .unwrap_or_default())
    }

    fn add_meta(
        &self,
        kind: ObjectKind,
        id: u64,
        key: &str,
        value: Value,
        unique: bool,
    ) -> Result<Option<u64>, String> {
        fails(key)?;
        let mut meta = self.meta.lock().unwrap_or_else(PoisonError::into_inner);
        let values = meta.entry((kind as u8, id, key.into())).or_default();
        if unique && !values.is_empty() {
            return Ok(None);
        }
        values.push(value);
        Ok(Some(values.len() as u64))
    }

    fn update_meta(
        &self,
        kind: ObjectKind,
        id: u64,
        key: &str,
        value: Value,
        previous: Option<Value>,
    ) -> Result<bool, String> {
        fails(key)?;
        let mut meta = self.meta.lock().unwrap_or_else(PoisonError::into_inner);
        let values = meta.entry((kind as u8, id, key.into())).or_default();
        match previous {
            Some(previous) => {
                let found = values.iter_mut().filter(|held| **held == previous);
                Ok(found.map(|held| *held = value.clone()).count() > 0)
            }
            None => {
                *values = vec![value];
                Ok(true)
            }
        }
    }

    fn delete_meta(
        &self,
        kind: ObjectKind,
        id: u64,
        key: &str,
        value: Option<Value>,
    ) -> Result<bool, String> {
        fails(key)?;
        let mut meta = self.meta.lock().unwrap_or_else(PoisonError::into_inner);
        let values = meta.entry((kind as u8, id, key.into())).or_default();
        let before = values.len();
        values.retain(|held| value.as_ref().is_some_and(|value| held != value));
        Ok(values.len() != before)
    }

    fn public_post_type(&self, post_type: &str) -> bool {
        post_type == "post"
    }

    fn query(&self, args: &Value) -> Result<Found, String> {
        let failing = args.as_array().and_then(|entries| lookup(entries, "fail"));
        if failing.is_some() {
            return Err(FAIL.into());
        }
        let posts = vec![post(1, "post"), post(2, "ext_item"), post(3, "secret")];
        Ok(Found {
            posts,
            total: 3,
            max_pages: 1,
        })
    }

    fn post(&self, id: u64) -> Result<Option<Post>, String> {
        match id {
            99 => Err(FAIL.into()),
            1 => Ok(Some(post(1, "post"))),
            3 => Ok(Some(post(3, "secret"))),
            _ => Ok(None),
        }
    }

    fn schedule(
        &self,
        timestamp: i64,
        recurrence: Option<&str>,
        hook: &str,
        args: &[Value],
    ) -> Result<(), String> {
        fails(recurrence.unwrap_or_default())?;
        let mut events = self.events.lock().unwrap_or_else(PoisonError::into_inner);
        events.push((timestamp, hook.into(), args.to_vec()));
        Ok(())
    }

    fn unschedule(&self, hook: &str, args: &[Value]) -> Result<u32, String> {
        if args.contains(&text("fail")) {
            return Err(FAIL.into());
        }
        let mut events = self.events.lock().unwrap_or_else(PoisonError::into_inner);
        let before = events.len();
        events.retain(|(_, held, given)| held != hook || given != args);
        Ok((before - events.len()) as u32)
    }

    fn next_scheduled(&self, hook: &str, args: &[Value]) -> Option<i64> {
        let events = held(&self.events);
        let matching = events
            .iter()
            .filter(|(_, held, given)| held == hook && given == args);
        matching.map(|(at, _, _)| *at).min()
    }

    fn mail(&self, message: &Message) -> Result<bool, String> {
        fails(&message.subject)?;
        self.note(format!("mail {:?} {:?}", message.to, message.attachments));
        Ok(true)
    }

    fn current_user(&self) -> u64 {
        7
    }

    fn user_can(&self, capability: &str, _: &[Value]) -> bool {
        capability == "edit_posts"
    }

    fn create_nonce(&self, action: &str) -> String {
        format!("nonce-{action}")
    }

    fn verify_nonce(&self, nonce: &str, action: &str) -> u8 {
        u8::from(nonce == format!("nonce-{action}"))
    }

    fn translate(&self, domain: &str, text: &str, context: Option<&str>) -> String {
        self.note(format!("{domain}:{text}"));
        let context = context
            .map(|context| format!("|{context}"))
            .unwrap_or_default();
        format!("{text}{context}@{domain}")
    }

    fn translate_plural(
        &self,
        domain: &str,
        (single, plural): (&str, &str),
        number: u64,
        context: Option<&str>,
    ) -> String {
        let text = if number == 1 { single } else { plural };
        self.translate(domain, text, context)
    }

    fn register_asset(&self, slug: &str, asset: &Asset) -> Result<(), String> {
        fails(&asset.handle)?;
        self.note(format!("asset {slug} {asset:?}"));
        Ok(())
    }

    fn enqueue(&self, kind: AssetKind, handle: &str) -> Result<(), String> {
        fails(handle)?;
        self.note(format!("enqueue {kind:?} {handle}"));
        Ok(())
    }

    fn add_inline(
        &self,
        kind: AssetKind,
        handle: &str,
        data: &str,
        before: bool,
    ) -> Result<(), String> {
        fails(handle)?;
        self.note(format!("inline {kind:?} {handle} {data} {before}"));
        Ok(())
    }
}

fn names(names: &str) -> std::collections::BTreeSet<String> {
    names
        .split(' ')
        .filter(|name| !name.is_empty())
        .map(String::from)
        .collect()
}

/// Everything the test extension asks for.
pub(crate) fn grants() -> Grants {
    Grants {
        filters: names(
            "the_content ext_trap ext_loop ext_grow ext_late ext_order ext_options ext_cron ext_echo \
             ext_meta ext_query ext_mail ext_auth ext_assets ext_fire ext_chain ext_remove ext_current \
             ext_bad_json ext_error ext_stack ext_count ext_gone ext_nest",
        ),
        actions: names("ext_action ext_event"),
        sensitive: names("map_meta_cap"),
        shortcodes: names("ext ext_gone"),
        options: names("ext ext_fail ext_bytes ext_active"),
        meta: names("_ext _ext_*"),
        post_types: names("ext_item ext_item_named_too_long"),
        taxonomies: names("ext_kind ext_kind_named_far_too_long_for_wp"),
        rest: names("ext/v1"),
        mail: true,
        filesystem: names("uploads/ext"),
    }
}

pub(crate) struct Loaded {
    pub(crate) host: Host,
    pub(crate) extensions: Arc<Extensions>,
    pub(crate) folders: Vec<tempfile::TempDir>,
}

impl Loaded {
    pub(crate) fn request(&self, site: &Arc<TestSite>) -> Request {
        let site: Arc<dyn Site> = site.clone();
        Request::new(&self.host, &self.extensions, site)
    }

    fn record(&self, ext: usize) -> Option<Record> {
        faults::read(self.folders.get(ext)?.path())
    }
}

pub(crate) fn load(slugs: &[&str], limits: Limits) -> Result<Loaded, Fault> {
    let host = Host::new()?;
    let mut list = Vec::new();
    let mut folders = Vec::new();
    for slug in slugs {
        let folder = tempfile::tempdir().map_err(|e| Fault::Refused(e.to_string()))?;
        list.push(host.load(Package {
            slug,
            wasm: guest::component(),
            grants: grants(),
            limits,
            folder: folder.path(),
        })?);
        folders.push(folder);
    }
    let extensions = Arc::new(Extensions::new(list));
    Ok(Loaded {
        host,
        extensions,
        folders,
    })
}

fn round(request: &Request, hook: &str) -> String {
    match request.filter(hook, Value::Null, &[]) {
        Value::String(text) => String::from_utf8_lossy(&text).into_owned(),
        other => format!("{other:?}"),
    }
}

#[test]
fn filters_by_priority_in_a_fresh_instance() -> Result<(), Fault> {
    let loaded = load(&["ext"], Limits::default())?;
    let request = loaded.request(&TestSite::new());
    assert!(request.attached("the_content"));
    assert!(!request.attached("the_title"));
    assert_eq!(
        request.filter("the_content", text("<p>Hi</p>"), &[]),
        text("<p>Hi</p><!-- ext -->")
    );
    assert_eq!(request.filter("ext_order", text(""), &[]), text("ab"));
    assert_eq!(request.filter("the_title", text("Hi"), &[]), text("Hi"));
    let given = [text("a"), text("b"), text("c")];
    assert_eq!(
        request.filter("ext_count", Value::Null, &given),
        Value::Int(2)
    );
    assert_eq!(request.reports(), Vec::new());
    Ok(())
}

#[test]
fn every_kind_of_value_crosses_both_ways() -> Result<(), Fault> {
    let loaded = load(&["ext"], Limits::default())?;
    let request = loaded.request(&TestSite::new());
    let values = [
        Value::Null,
        Value::Bool(true),
        Value::Bool(false),
        Value::Int(-3),
        Value::Float(hyscribe_phpser::Float::from_f64(1.5)),
        text("é"),
        Value::Array(vec![(Key::Int(0), Value::Int(1))]),
    ];
    for value in values {
        assert_eq!(request.filter("ext_echo", value.clone(), &[]), value);
    }
    Ok(())
}

#[test]
fn registers_in_init_only() -> Result<(), Fault> {
    let loaded = load(&["ext"], Limits::default())?;
    let extension = loaded.extensions.list().first().ok_or(Fault::Disabled)?;
    assert_eq!(extension.slug(), "ext");
    assert_eq!(extension.hooks().len(), 27);
    assert_eq!(
        extension.hooks().last(),
        Some(&Hook {
            hook: "ext_action".into(),
            action: true,
            callback: 24,
            priority: 10,
            accepted_args: 2,
        })
    );
    let public = Value::Array(vec![(Key::String("public".into()), Value::Bool(true))]);
    assert_eq!(extension.post_types(), [("ext_item".to_string(), public)]);
    assert_eq!(
        extension.taxonomies(),
        [Taxonomy {
            name: "ext_kind".into(),
            object_types: vec!["ext_item".into()],
            args: Value::Array(Vec::new()),
        }]
    );
    let single = Value::Array(vec![(Key::String("single".into()), Value::Bool(true))]);
    assert_eq!(
        extension.meta(),
        [MetaKey {
            kind: ObjectKind::Post,
            subtype: "ext_item".into(),
            key: "_ext".into(),
            args: single,
        }]
    );
    assert_eq!(
        extension.routes(),
        [Route {
            namespace: "ext/v1".into(),
            path: "/items".into(),
            methods: vec!["GET".into(), "POST".into()],
            callback: 20,
            permission: 21,
            args: Value::Array(Vec::new()),
        }]
    );
    assert_eq!(extension.shortcodes(), [("ext".to_string(), 19)]);
    assert_eq!(
        extension.blocks(),
        [
            Block {
                path: "blocks/card".into(),
                render: Some(22)
            },
            Block {
                path: "blocks/static".into(),
                render: None
            },
        ]
    );
    let request = loaded.request(&TestSite::new());
    assert_eq!(
        round(&request, "ext_late"),
        [r#"Err(SdkError::Denied("registered in init only"))"#; 7].join(" ")
    );
    Ok(())
}

#[test]
fn options_are_checked_against_their_grant() -> Result<(), Fault> {
    let loaded = load(&["ext"], Limits::default())?;
    let site = TestSite::new();
    let request = loaded.request(&site);
    let denied = r#"Err(SdkError::Denied("option siteurl is not in the manifest"))"#;
    let failed = r#"Err(SdkError::Failed("the database went away"))"#;
    let expected = [
        "Ok(true) Ok(false) Ok(false) Ok(true) Ok(Some(Int(2))) Ok(true) Ok(false) Ok(None)",
        denied,
        denied,
        denied,
        denied,
        failed,
        failed,
        failed,
        failed,
        r#"Err(SdkError::Invalid("a value that does not decode"))"#,
        r#"Err(SdkError::Failed("a value that does not cross"))"#,
        r#"Ok(true) Ok(Some(Json("[1,{\"a\":1.5}]")))"#,
    ];
    assert_eq!(round(&request, "ext_options"), expected.join(" "));
    assert!(site.option("siteurl").is_none());
    Ok(())
}

#[test]
fn meta_is_checked_against_its_grant() -> Result<(), Fault> {
    let loaded = load(&["ext"], Limits::default())?;
    let request = loaded.request(&TestSite::new());
    let denied = r#"Err(SdkError::Denied("meta key _wp_secret is not in the manifest"))"#;
    let failed = r#"Err(SdkError::Failed("the database went away"))"#;
    let invalid = r#"Err(SdkError::Invalid("a value that does not decode"))"#;
    let expected = [
        r#"Ok(Some(1)) Ok(None) Ok([Text("a")]) Ok(true) Ok(true) Ok([]) Ok(false) Ok(true) Ok([])"#,
        denied,
        denied,
        denied,
        denied,
        failed,
        failed,
        failed,
        failed,
        invalid,
        invalid,
        invalid,
        invalid,
        r#"Err(SdkError::Failed("a value that does not cross"))"#,
    ];
    assert_eq!(round(&request, "ext_meta"), expected.join(" "));
    Ok(())
}

#[test]
fn queries_read_public_and_granted_post_types() -> Result<(), Fault> {
    let loaded = load(&["ext"], Limits::default())?;
    let request = loaded.request(&TestSite::new());
    let invalid = r#"Err(SdkError::Invalid("post_type names the post types read"))"#;
    let expected = [
        "Ok(([1, 2], 3, 1)) Ok(([1, 2], 3, 1)) Ok(([1, 2], 3, 1))",
        r#"Err(SdkError::Denied("post type secret is not in the manifest"))"#,
        invalid,
        invalid,
        invalid,
        r#"Err(SdkError::Failed("the database went away"))"#,
        r#"Err(SdkError::Invalid("query variables are an object"))"#,
        r#"Err(SdkError::Invalid("not JSON"))"#,
        r#"Ok(Some((1, "post", "Post 1")))"#,
        r#"Err(SdkError::Denied("post type secret is not in the manifest"))"#,
        "Ok(None)",
        r#"Err(SdkError::Failed("the database went away"))"#,
    ];
    assert_eq!(round(&request, "ext_query"), expected.join(" "));
    Ok(())
}

#[test]
fn cron_hooks_are_checked_against_their_grant() -> Result<(), Fault> {
    let loaded = load(&["ext"], Limits::default())?;
    let request = loaded.request(&TestSite::new());
    let denied = r#"Err(SdkError::Denied("wp_version_check is not in the manifest"))"#;
    let failed = r#"Err(SdkError::Failed("the database went away"))"#;
    let invalid = r#"Err(SdkError::Invalid("a value that does not decode"))"#;
    let expected = [
        "Ok(()) Ok(()) Ok(()) Some(50) Some(70) Ok(2) None Some(70)",
        denied,
        denied,
        "None",
        failed,
        failed,
        invalid,
        invalid,
        "None",
    ];
    assert_eq!(round(&request, "ext_cron"), expected.join(" "));
    Ok(())
}

#[test]
fn mail_assets_users_and_translations() -> Result<(), Fault> {
    let loaded = load(&["ext"], Limits::default())?;
    let site = TestSite::new();
    let request = loaded.request(&site);
    let mail = [
        "Ok(true)",
        r#"Err(SdkError::Denied("uploads/other/a.pdf is not in a folder the manifest grants"))"#,
        r#"Err(SdkError::Failed("the database went away"))"#,
    ];
    assert_eq!(round(&request, "ext_mail"), mail.join(" "));
    let auth = r#"7 true false false "nonce-ext" 1 0 "Hello@ext" "Post|noun@ext" "items@ext" "item|ctx@ext""#;
    assert_eq!(round(&request, "ext_auth"), auth);
    let failed = r#"Err(SdkError::Failed("the database went away"))"#;
    let assets = [
        "Ok(()) Ok(())",
        r#"Err(SdkError::Invalid("an asset has a handle"))"#,
        r#"Err(SdkError::Invalid("../wp-config.php is not a file inside public/"))"#,
        failed,
        "Ok(())",
        failed,
        "Ok(())",
        failed,
    ];
    assert_eq!(round(&request, "ext_assets"), assets.join(" "));
    let logged = site.logged();
    assert_eq!(
        logged.first().map(String::as_str),
        Some(r#"mail ["a@example.com"] ["uploads/ext/a.pdf"]"#)
    );
    assert!(
        logged
            .iter()
            .any(|line| line.starts_with("asset ext Asset { kind: Script, handle: \"ext-app\""))
    );
    assert!(logged.contains(&"enqueue Script ext-app".to_string()));
    assert!(logged.contains(&"inline Script ext-app var x = 1; true".to_string()));
    Ok(())
}

#[test]
fn chains_interleave_extensions_by_priority() -> Result<(), Fault> {
    let loaded = load(&["ext", "ext-b"], Limits::default())?;
    let site = TestSite::new();
    let request = loaded.request(&site);
    assert_eq!(
        request.filter("ext_chain", text("y"), &[]),
        text("y1@ext1@ext-b2@ext2@ext-b")
    );
    assert_eq!(
        request.filter("ext_action", text("v"), &[]),
        text("seen:seen:v")
    );
    request.action("ext_action", &[text("w"), Value::Int(2)]);
    assert_eq!(round(&request, "ext_current"), r#"Some("ext_current") 1"#);
    let expected = [
        r#"Text("x1@ext1@ext-b2@ext2@ext-b")"#,
        r#"Text("seen:v")"#,
        r#"Text("v")"#,
        r#"Text("m")"#,
        r#"Text("n")"#,
        r#"Text("z1@ext2@ext1@ext1@ext-b2@ext2@ext-b")"#,
        r#"Some("ext_fire")"#,
        r#"Err(SdkError::Invalid("no such chain in this call"))"#,
        r#"Ok((4, Err(SdkError::Invalid("no such step")), Err(SdkError::Invalid("a value that does not decode"))))"#,
    ];
    // Both run their round and `ext-b`'s answer comes last: its nested chain
    // skips `ext-b` itself, which is running, and runs `ext`'s callbacks.
    let fired = request.filter("ext_fire", Value::Null, &[]);
    assert_eq!(fired, text(&expected.join(" ")));
    let seen = site
        .logged()
        .into_iter()
        .filter(|line| line.ends_with(":seen"))
        .count();
    assert_eq!(seen, 12);
    assert_eq!(request.reports(), Vec::new());
    Ok(())
}

#[test]
fn a_run_of_other_extensions_is_one_step() -> Result<(), Fault> {
    let loaded = load(&["ext", "ext-b", "ext-c"], Limits::default())?;
    let request = loaded.request(&TestSite::new());
    assert_eq!(
        request.filter("ext_chain", text("y"), &[]),
        text("y1@ext1@ext-b1@ext-c2@ext2@ext-b2@ext-c")
    );
    // `ext-c` answers last: its view of `ext_chain`'s six callbacks is four
    // links, `ext`'s and `ext-b`'s at each priority run as one step.
    let fired = round(&request, "ext_fire");
    assert!(
        fired.starts_with(r#"Text("x1@ext1@ext-b1@ext-c2@ext2@ext-b2@ext-c")"#),
        "{fired}"
    );
    assert!(fired.ends_with(r#"Ok((4, Err(SdkError::Invalid("no such step")), Err(SdkError::Invalid("a value that does not decode"))))"#), "{fired}");
    Ok(())
}

#[test]
fn removes_its_own_callbacks_for_the_request() -> Result<(), Fault> {
    let loaded = load(&["ext"], Limits::default())?;
    let request = loaded.request(&TestSite::new());
    assert_eq!(
        round(&request, "ext_remove"),
        r#"[true, false, false, true, false, false] Text("x2@ext")"#
    );
    assert_eq!(request.filter("ext_chain", text("y"), &[]), text("y2@ext"));
    assert_eq!(request.shortcode("ext", Value::Null, Value::Null), None);
    let next = loaded.request(&TestSite::new());
    assert_eq!(
        next.filter("ext_chain", text("y"), &[]),
        text("y1@ext2@ext")
    );
    Ok(())
}

#[test]
fn serves_shortcodes_blocks_and_routes() -> Result<(), Fault> {
    let loaded = load(&["ext"], Limits::default())?;
    let request = loaded.request(&TestSite::new());
    let atts = Value::Array(vec![(Key::String("id".into()), Value::Int(5))]);
    assert_eq!(
        request.shortcode("ext", atts, text("inner")),
        Some(text(
            r#"[[Json("{\"id\":5}"), Text("inner"), Text("ext")]]"#
        ))
    );
    assert_eq!(request.shortcode("gallery", Value::Null, Value::Null), None);
    assert_eq!(
        request.block(0, 0, [Value::Null, text("c"), Value::Bool(false)]),
        Some(text(r#"<div>[Null, Text("c"), Bool(false)]</div>"#))
    );
    assert_eq!(
        request.block(0, 1, [Value::Null, Value::Null, Value::Null]),
        None
    );
    assert_eq!(
        request.block(0, 2, [Value::Null, Value::Null, Value::Null]),
        None
    );
    assert_eq!(
        request.block(1, 0, [Value::Null, Value::Null, Value::Null]),
        None
    );
    let asking = |what: &str| Value::Array(vec![(Key::String("ask".into()), text(what))]);
    let got = Value::Array(vec![(Key::String("got".into()), asking("allow"))]);
    assert_eq!(request.rest(0, 0, asking("allow")), Rest::Answered(got));
    let forbidden = Value::Array(vec![(Key::String("code".into()), text("rest_forbidden"))]);
    assert_eq!(
        request.rest(0, 0, asking("deny")),
        Rest::Forbidden(forbidden)
    );
    assert_eq!(request.rest(0, 0, asking("error")), Rest::Failed);
    assert_eq!(request.rest(0, 1, Value::Null), Rest::Failed);
    assert_eq!(request.rest(1, 0, Value::Null), Rest::Failed);
    let refused = Report {
        extension: "ext".into(),
        hook: "ext/v1/items".into(),
        callback: Some(21),
        fault: Fault::Guest("permission failed".into()),
    };
    assert_eq!(request.reports(), [refused]);
    Ok(())
}

#[test]
fn an_answer_that_fails_keeps_the_value() -> Result<(), Fault> {
    let loaded = load(&["ext"], Limits::default())?;
    let request = loaded.request(&TestSite::new());
    assert_eq!(
        request.filter("the_content", Value::Int(7), &[]),
        Value::Int(7)
    );
    assert_eq!(
        request.filter("ext_bad_json", text("kept"), &[]),
        text("kept")
    );
    assert_eq!(request.filter("ext_error", text("kept"), &[]), text("kept"));
    assert_eq!(
        request.filter("ext_echo", Value::String(vec![0xff]), &[]),
        Value::String(vec![0xff])
    );
    let report = |hook: &str, callback, fault| Report {
        extension: "ext".into(),
        hook: hook.into(),
        callback: Some(callback),
        fault,
    };
    assert_eq!(
        request.reports(),
        [
            report(
                "the_content",
                0,
                Fault::Guest("no such callback, or not its argument".into())
            ),
            report(
                "ext_bad_json",
                25,
                Fault::Value("its answer does not decode".into())
            ),
            report("ext_error", 26, Fault::Guest("guest says no".into())),
            report(
                "ext_echo",
                9,
                Fault::Value("an argument does not cross".into())
            ),
        ]
    );
    assert_eq!(
        request.filter("the_content", text("x"), &[]),
        text("x<!-- ext -->")
    );
    assert_eq!(loaded.record(0), Some(Record::default()));
    Ok(())
}

#[test]
fn three_faults_in_ten_minutes_disable() -> Result<(), Fault> {
    let loaded = load(&["ext"], Limits::default())?;
    let site = TestSite::new();
    for (at, hook) in [
        (1_000, "ext_trap"),
        (1_300, "ext_stack"),
        (1_600, "ext_trap"),
    ] {
        site.now.store(at, std::sync::atomic::Ordering::Relaxed);
        let request = loaded.request(&site);
        assert_eq!(request.filter(hook, text("kept"), &[]), text("kept"));
        assert_eq!(
            request.filter("the_content", text("kept"), &[]),
            text("kept")
        );
        let reports = request.reports();
        assert!(
            matches!(
                reports.as_slice(),
                [Report {
                    fault: Fault::Trap(_),
                    ..
                }]
            ),
            "{reports:?}"
        );
    }
    let extension = loaded.extensions.list().first().ok_or(Fault::Disabled)?;
    assert!(extension.disabled());
    assert_eq!(extension.record().disabled(), Some(1_600));
    assert_eq!(extension.record().faults().len(), 3);
    assert_eq!(loaded.record(0), Some(extension.record()));
    let request = loaded.request(&site);
    assert_eq!(request.filter("the_content", text("x"), &[]), text("x"));
    assert_eq!(request.reports(), Vec::new());
    assert_eq!(request.activate(0), Err(Fault::Disabled));
    let folder = loaded.folders.first().ok_or(Fault::Disabled)?.path();
    let again = Package {
        slug: "ext",
        wasm: guest::component(),
        grants: grants(),
        limits: Limits::default(),
        folder,
    };
    assert_eq!(loaded.host.load(again).err(), Some(Fault::Disabled));
    faults::clear(folder).map_err(|e| Fault::Refused(e.to_string()))?;
    let cleared = Package {
        slug: "ext",
        wasm: guest::component(),
        grants: grants(),
        limits: Limits::default(),
        folder,
    };
    assert!(!loaded.host.load(cleared)?.disabled());
    Ok(())
}

#[test]
fn faults_further_apart_do_not_disable() -> Result<(), Fault> {
    let loaded = load(&["ext"], Limits::default())?;
    let site = TestSite::new();
    for at in [1_000, 1_300, 1_601] {
        site.now.store(at, std::sync::atomic::Ordering::Relaxed);
        loaded.request(&site).filter("ext_trap", Value::Null, &[]);
    }
    let extension = loaded.extensions.list().first().ok_or(Fault::Disabled)?;
    assert!(!extension.disabled());
    assert_eq!(
        loaded.record(0).map(|record| record.faults().len()),
        Some(3)
    );
    Ok(())
}

#[test]
fn a_loop_is_stopped_at_its_deadline() -> Result<(), Fault> {
    let loaded = load(&["ext"], Limits::default())?;
    let request = loaded.request(&TestSite::new());
    let started = Instant::now();
    assert_eq!(request.filter("ext_loop", text("kept"), &[]), text("kept"));
    let took = started.elapsed();
    assert!(
        took >= Duration::from_millis(49) && took < Duration::from_secs(2),
        "{took:?}"
    );
    assert_eq!(
        request.filter("the_content", text("kept"), &[]),
        text("kept")
    );
    let reports = request.reports();
    assert_eq!(
        reports
            .iter()
            .map(|report| &report.fault)
            .collect::<Vec<_>>(),
        [&Fault::Timeout]
    );
    let counted = loaded.record(0).map(|record| record.faults().to_vec());
    assert_eq!(
        counted,
        Some(vec![Counted {
            at: 0,
            kind: Kind::Timeout,
            hook: "ext_loop".into()
        }])
    );
    Ok(())
}

#[test]
fn memory_stops_at_its_limit() -> Result<(), Fault> {
    let loaded = load(&["ext"], Limits::default())?;
    let request = loaded.request(&TestSite::new());
    assert_eq!(
        request.filter("ext_grow", Value::Int(4), &[]),
        Value::Int(4 << 20)
    );
    assert_eq!(
        request.filter("ext_grow", Value::Int(32), &[]),
        Value::Int(32)
    );
    let reports = request.reports();
    assert_eq!(
        reports
            .iter()
            .map(|report| &report.fault)
            .collect::<Vec<_>>(),
        [&Fault::Memory]
    );
    let roomy = Limits::asked(&[("memory_mib", 64)]).map_err(Fault::Refused)?;
    let loaded = load(&["ext"], roomy)?;
    let request = loaded.request(&TestSite::new());
    assert_eq!(
        request.filter("ext_grow", Value::Int(32), &[]),
        Value::Int(32 << 20)
    );
    Ok(())
}

#[test]
fn activates_and_deactivates_in_instances_of_their_own() -> Result<(), Fault> {
    let loaded = load(&["ext"], Limits::default())?;
    let site = TestSite::new();
    let request = loaded.request(&site);
    assert_eq!(
        request.deactivate(0),
        Err(Fault::Guest("not active".into()))
    );
    assert_eq!(request.activate(0), Ok(()));
    assert_eq!(site.option("ext_active"), Some(Value::Bool(true)));
    assert!(matches!(request.deactivate(0), Err(Fault::Trap(_))));
    assert_eq!(
        request.activate(1),
        Err(Fault::Refused("no such extension".into()))
    );
    let hooks: Vec<_> = request
        .reports()
        .into_iter()
        .map(|report| report.hook)
        .collect();
    assert_eq!(hooks, ["deactivate", "deactivate"]);
    assert_eq!(
        loaded.record(0).map(|record| record.faults().len()),
        Some(1)
    );
    Ok(())
}

#[test]
fn a_fault_record_that_cannot_be_written_is_reported() -> Result<(), Fault> {
    let host = Host::new()?;
    let folder = tempfile::tempdir().map_err(|e| Fault::Refused(e.to_string()))?;
    let gone = folder.path().join("gone");
    let package = Package {
        slug: "ext",
        wasm: guest::component(),
        grants: grants(),
        limits: Limits::default(),
        folder: &gone,
    };
    let extensions = Arc::new(Extensions::new(vec![host.load(package)?]));
    let site = TestSite::new();
    let request = Request::new(&host, &extensions, site.clone());
    request.filter("ext_trap", Value::Null, &[]);
    let faults: Vec<_> = request
        .reports()
        .into_iter()
        .map(|report| report.fault)
        .collect();
    assert!(
        matches!(faults.as_slice(), [Fault::Unrecorded(_), Fault::Trap(_)]),
        "{faults:?}"
    );
    let request = Request::new(&host, &extensions, site);
    request.activate(0)?;
    assert!(matches!(request.deactivate(0), Err(Fault::Trap(_))));
    assert!(matches!(
        request.reports().first(),
        Some(Report {
            fault: Fault::Unrecorded(_),
            ..
        })
    ));
    Ok(())
}

#[test]
fn grants_nothing_by_default() -> Result<(), Fault> {
    let host = Host::new()?;
    let folder = tempfile::tempdir().map_err(|e| Fault::Refused(e.to_string()))?;
    let package = |grants| Package {
        slug: "ext",
        wasm: guest::component(),
        grants,
        limits: Limits::default(),
        folder: folder.path(),
    };
    assert_eq!(
        host.load(package(Grants::default())).err(),
        Some(Fault::Refused("the_content is not in the manifest".into()))
    );
    let mut partial = grants();
    partial.filters.remove("ext_loop");
    assert_eq!(
        host.load(package(partial)).err(),
        Some(Fault::Refused("ext_loop is not in the manifest".into()))
    );
    let mut named = grants();
    named.filters.insert("authenticate".into());
    assert_eq!(
        host.load(package(named)).err(),
        Some(Fault::Refused(
            "authenticate is a sensitive hook: name it under [hooks] sensitive".into()
        ))
    );
    Ok(())
}

#[test]
fn refuses_what_is_not_an_extension() -> Result<(), Fault> {
    let host = Host::new()?;
    let folder = tempfile::tempdir().map_err(|e| Fault::Refused(e.to_string()))?;
    let package = |wasm, limits| Package {
        slug: "ext",
        wasm,
        grants: grants(),
        limits,
        folder: folder.path(),
    };
    assert!(matches!(
        host.load(package(b"not wasm", Limits::default())),
        Err(Fault::Refused(_))
    ));
    let empty = [0, b'a', b's', b'm', 0x0d, 0, 1, 0];
    assert_eq!(
        host.load(package(&empty, Limits::default())).err(),
        Some(Fault::Refused("no hyscribe:sdk/guest@0.1.0".into()))
    );
    // A deadline of no ticks interrupts `init` at its first check.
    let hurried = Limits {
        call_ms: 0,
        ..Limits::default()
    };
    assert_eq!(
        host.load(package(guest::component(), hurried)).err(),
        Some(Fault::Timeout)
    );
    let starved = Limits {
        memory_mib: 0,
        ..Limits::default()
    };
    assert_eq!(
        host.load(package(guest::component(), starved)).err(),
        Some(Fault::Memory)
    );
    std::fs::write(folder.path().join(faults::FILE), "unreadable\n")
        .map_err(|e| Fault::Refused(e.to_string()))?;
    assert_eq!(
        host.load(package(guest::component(), Limits::default()))
            .err(),
        Some(Fault::Disabled)
    );
    Ok(())
}

#[test]
fn bounds_tables_and_memory_at_their_limits() -> Result<(), Fault> {
    let host = Host::new()?;
    let grants = Arc::new(Grants::default());
    let slug: Arc<str> = Arc::from("ext");
    let mut state = State::store(
        &host.engine,
        (&grants, &slug),
        host::Phase::Init,
        1 << 16,
        1,
    )
    .into_data();
    use wasmtime::ResourceLimiter;
    assert_eq!(state.table_growing(0, 10_000, None).ok(), Some(true));
    assert_eq!(state.table_growing(0, 10_001, None).ok(), Some(false));
    assert_eq!(state.memory_growing(0, 1 << 16, None).ok(), Some(true));
    assert!(!state.over);
    assert_eq!(
        state.memory_growing(0, (1 << 16) + 1, None).ok(),
        Some(false)
    );
    assert!(state.over);
    Ok(())
}

#[test]
fn dropping_the_host_stops_its_ticker() -> Result<(), Fault> {
    let host = Host::new()?;
    let stop = host.stop.clone();
    drop(host);
    assert_eq!(Arc::strong_count(&stop), 1);
    assert!(stop.load(std::sync::atomic::Ordering::Relaxed));
    Ok(())
}

// SPDX-License-Identifier: AGPL-3.0-or-later

use std::path::Path;

use hyscribe_config::{Database, Mode};
use hyscribe_ext::Site as _;
use hyscribe_ext::installed;
use hyscribe_testkit::fixture_database;

use super::*;

/// The test extension, built from source as `hyscribe-ext`'s tests build it.
#[allow(dead_code, reason = "only its component is used here")]
#[path = "../../../hyscribe-ext/tests/guest/mod.rs"]
pub(crate) mod guest;

/// The test extension's manifest.
pub(crate) const MANIFEST: &str = include_str!("../../../hyscribe-ext/guest/hyscribe.ext.toml");

/// The plugin file it stands in for.
pub(crate) const FILE: &str = "hyscribe-test/hyscribe-test.php";

/// Installs the test extension in the plugins folder `plugins`, not yet
/// approved, and gives its folder.
pub(crate) fn install(plugins: &Path) -> std::path::PathBuf {
    let folder = plugins.join("hyscribe-test");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("hyscribe.ext.toml"), MANIFEST).unwrap();
    std::fs::write(folder.join("extension.wasm"), guest::component()).unwrap();
    folder
}

fn text(s: &str) -> Value {
    Value::String(s.as_bytes().to_vec())
}

fn saves() -> Database {
    Database {
        name: "wordpress_saves".into(),
        ..fixture_database()
    }
}

/// The autoload `name` is stored with.
async fn autoloaded(writer: &Writer, name: &str) -> Option<String> {
    let mut write = writer.begin().await.unwrap();
    write.option_autoload(name).await.unwrap()
}

/// A page view's calls each commit on their own; options are read and
/// written as `get_option()` and its kin do, autoload as asked.
#[tokio::test(flavor = "multi_thread")]
async fn a_page_view_s_calls_commit_each_on_its_own() {
    let _saves = crate::admin::tests::SAVES.lock().await;
    hyscribe_testkit::writes::load_default_install(&saves())
        .await
        .unwrap();
    let writer = Arc::new(
        Writer::open(&saves(), Mode::Primary)
            .await
            .unwrap()
            .unwrap(),
    );
    let run = |call: fn(&Live) -> Result<Value, String>| {
        let live = Live(Through::Own(Some(writer.clone()), Handle::current()), None);
        tokio::task::spawn_blocking(move || call(&live))
    };
    let missing = run(|live| {
        live.get_option("ext_hooks")
            .map(|v| Value::Bool(v.is_some()))
    });
    assert_eq!(missing.await.unwrap(), Ok(Value::Bool(false)));
    let added = run(|live| {
        live.add_option("ext_hooks", text("on"), Some(true))
            .map(Value::Bool)
    });
    assert_eq!(added.await.unwrap(), Ok(Value::Bool(true)));
    assert_eq!(
        autoloaded(&writer, "ext_hooks").await.as_deref(),
        Some("on")
    );
    let again = run(|live| {
        live.add_option("ext_hooks", text("x"), None)
            .map(Value::Bool)
    });
    assert_eq!(again.await.unwrap(), Ok(Value::Bool(false)));
    let read = run(|live| live.get_option("ext_hooks").map(Option::unwrap_or_default));
    assert_eq!(read.await.unwrap(), Ok(text("on")));
    let off = run(|live| {
        live.update_option("ext_hooks", text("off"), Some(false))
            .map(Value::Bool)
    });
    assert_eq!(off.await.unwrap(), Ok(Value::Bool(true)));
    assert_eq!(
        autoloaded(&writer, "ext_hooks").await.as_deref(),
        Some("off")
    );
    let kept = run(|live| {
        live.update_option("ext_hooks", text("x"), None)
            .map(Value::Bool)
    });
    assert_eq!(kept.await.unwrap(), Ok(Value::Bool(true)));
    assert_eq!(
        autoloaded(&writer, "ext_hooks").await.as_deref(),
        Some("off")
    );
    let deleted = run(|live| live.delete_option("ext_hooks").map(Value::Bool));
    assert_eq!(deleted.await.unwrap(), Ok(Value::Bool(true)));
    let gone = run(|live| live.delete_option("ext_hooks").map(Value::Bool));
    assert_eq!(gone.await.unwrap(), Ok(Value::Bool(false)));
    let protected = run(|live| {
        live.update_option("alloptions", text("x"), None)
            .map(Value::Bool)
    });
    assert_eq!(
        protected.await.unwrap(),
        Err("Refused(\"protected option\")".into())
    );
}

/// Transients, meta and roles are read and written as WordPress's
/// functions do, site transients apart, meta of each kind of object.
#[tokio::test(flavor = "multi_thread")]
async fn transients_meta_and_roles_go_through_wordpress_s_functions() {
    let _saves = crate::admin::tests::SAVES.lock().await;
    hyscribe_testkit::writes::load_default_install(&saves())
        .await
        .unwrap();
    let writer = Writer::open(&saves(), Mode::Primary).await.unwrap();
    let live = Live(Through::Own(writer.map(Arc::new), Handle::current()), None);
    let answers = tokio::task::spawn_blocking(move || {
        let post = ObjectKind::Post;
        let transients = [
            format!(
                "{:?}",
                live.set_transient("ext_cache", text("a"), 60, false)
            ),
            format!("{:?}", live.set_transient("ext_site", text("s"), 0, true)),
            format!("{:?}", live.get_transient("ext_cache", false)),
            format!("{:?}", live.get_transient("ext_site", true)),
            format!("{:?}", live.get_transient("ext_site", false)),
            format!("{:?}", live.delete_transient("ext_cache", false)),
            format!("{:?}", live.delete_transient("ext_site", true)),
            format!("{:?}", live.get_transient("ext_site", true)),
        ];
        let meta = [
            format!(
                "{:?}",
                live.add_meta(post, 1, "_ext", text("a"), true)
                    .map(|id| id.is_some())
            ),
            format!("{:?}", live.add_meta(post, 1, "_ext", text("b"), true)),
            format!(
                "{:?}",
                live.update_meta(post, 1, "_ext", text("b"), Some(text("a")))
            ),
            format!("{:?}", live.get_meta(post, 1, "_ext")),
            format!(
                "{:?}",
                live.update_meta(ObjectKind::User, 1, "_ext", text("u"), None)
            ),
            format!("{:?}", live.get_meta(ObjectKind::User, 1, "_ext")),
            format!("{:?}", live.delete_meta(post, 1, "_ext", Some(text("x")))),
            format!("{:?}", live.delete_meta(post, 1, "_ext", None)),
            format!("{:?}", live.get_meta(post, 1, "_ext")),
            format!("{:?}", live.get_meta(ObjectKind::Term, 1, "_ext")),
            format!(
                "{:?}",
                live.add_meta(ObjectKind::Comment, 0, "_ext", text("c"), false)
            ),
        ];
        let read = [("read".to_string(), true)];
        let roles = [
            format!("{:?}", live.add_role("ext_manager", "Ext Manager", &read)),
            format!("{:?}", live.role_cap("ext_manager", "ext_cap", Some(true))),
            format!("{:?}", live.role_cap("ext_manager", "read", None)),
            format!("{:?}", live.role("ext_manager")),
            format!("{:?}", live.remove_role("ext_manager")),
            format!("{:?}", live.role("ext_manager")),
        ];
        (transients.join(" "), meta.join(" "), roles.join(" "))
    });
    let (transients, meta, roles) = answers.await.unwrap();
    assert_eq!(
        roles,
        r#"Ok(true) Ok(true) Ok(true) Ok(Some([("ext_cap", true)])) Ok(true) Ok(None)"#
    );
    assert_eq!(
        transients,
        "Ok(true) Ok(true) Ok(Some(String([97]))) Ok(Some(String([115]))) Ok(None) Ok(true) Ok(true) Ok(None)"
    );
    assert_eq!(
        meta,
        "Ok(true) Ok(None) Ok(true) Ok([String([98])]) Ok(true) Ok([String([117])]) Ok(false) Ok(true) Ok([]) Ok([]) Ok(None)"
    );
}

/// An extension's request goes through the site's client with WordPress's
/// user agent; without a client, or from an action, it fails.
#[tokio::test(flavor = "multi_thread")]
async fn requests_go_through_the_site_s_client() {
    let pages = Arc::new(crate::rest::tests::url_details::Pages::default());
    let client =
        hyscribe_fetch::Client::new(hyscribe_fetch::RootCertStore::empty(), pages, Arc::new(()));
    let outbound = Outbound(
        Some(Arc::new(client)),
        "http://home.test".into(),
        Handle::current(),
    );
    let live = Live(Through::Own(None, Handle::current()), Some(outbound));
    let request = |url: &str| Outgoing {
        method: "GET".into(),
        url: url.into(),
        headers: Vec::new(),
        body: Vec::new(),
        timeout: 5,
    };
    let answers = tokio::task::spawn_blocking(move || {
        let page = live.fetch(&request("http://pages.test/agent"), &|_| true);
        let lost = live.fetch(&request("http://nowhere.test/"), &|_| true);
        let (sender, _) = mpsc::channel(1);
        let acting = Live(Through::Action(sender), None);
        let off = acting.fetch(&request("http://pages.test/agent"), &|_| true);
        (page, lost, off)
    });
    let (page, lost, off) = answers.await.unwrap();
    let (status, _, body) = page.unwrap();
    assert_eq!(status, 200);
    let title = "<title>WordPress/7.1.3; http://home.test</title>";
    assert!(String::from_utf8(body).unwrap().contains(title));
    assert_eq!(lost, Err("Error { kind: Resolve, hop: 0 }".into()));
    assert_eq!(off, Err(NO_FETCHES.into()));
}

/// Without a writer, or over a database it cannot write, a call fails.
#[tokio::test(flavor = "multi_thread")]
async fn calls_fail_where_nothing_can_be_written() {
    let read_only = Live(Through::Own(None, Handle::current()), None);
    let answer = tokio::task::spawn_blocking(move || read_only.get_option("x"));
    assert_eq!(answer.await.unwrap(), Err(READ_ONLY.into()));
    let missing = Database {
        prefix: hyscribe_config::Prefix::new("missing_").unwrap(),
        ..fixture_database()
    };
    let writer = Writer::open(&missing, Mode::Primary).await.unwrap();
    let live = Live(Through::Own(writer.map(Arc::new), Handle::current()), None);
    let answer = tokio::task::spawn_blocking(move || live.get_option("x"));
    assert!(answer.await.unwrap().is_err());
    let reader = Database {
        user: "wordpress_read".into(),
        password: hyscribe_config::Secret::new("reference-only"),
        ..fixture_database()
    };
    let writer = Writer::open(&reader, Mode::Primary).await.unwrap();
    let live = Live(Through::Own(writer.map(Arc::new), Handle::current()), None);
    let answer = tokio::task::spawn_blocking(move || live.add_option("x", Value::Null, None));
    assert!(answer.await.unwrap().is_err());
    let (sender, jobs) = mpsc::channel(1);
    drop(jobs);
    let live = Live(Through::Action(sender), None);
    let answer = tokio::task::spawn_blocking(move || live.delete_option("x"));
    assert_eq!(answer.await.unwrap(), Err(GONE.into()));
}

/// What is not served answers so, or as no one logged in; translations
/// are the text as given.
#[test]
fn what_is_not_served_yet() {
    let (sender, _) = mpsc::channel(1);
    let live = Live(Through::Action(sender), None);
    let unserved = Err(UNSERVED.to_string());
    assert_eq!(
        live.query(&Value::Null),
        unserved.clone().map(|()| Found::default())
    );
    assert_eq!(live.post(1), unserved.clone().map(|()| None));
    assert_eq!(live.schedule(1, None, "h", &[]), unserved.clone());
    assert_eq!(live.unschedule("h", &[]), unserved.clone().map(|()| 0));
    assert_eq!(live.next_scheduled("h", &[]), None);
    let message = Message {
        to: Vec::new(),
        subject: String::new(),
        body: String::new(),
        headers: Vec::new(),
        attachments: Vec::new(),
    };
    assert_eq!(live.mail(&message), unserved.clone().map(|()| true));
    assert_eq!(live.current_user(), 0);
    assert!(!live.user_can("read", &[]));
    assert_eq!(live.create_nonce("a"), "");
    assert_eq!(live.verify_nonce("n", "a"), 0);
    let asset = Asset {
        kind: AssetKind::Script,
        handle: "h".into(),
        src: String::new(),
        deps: Vec::new(),
        version: None,
        placement: String::new(),
    };
    assert_eq!(live.register_asset("ext", &asset), unserved.clone());
    assert_eq!(live.enqueue(AssetKind::Style, "h"), unserved.clone());
    assert_eq!(live.add_inline(AssetKind::Script, "h", "", false), unserved);
    let call = hyscribe_ext::ThemeCall::HavePosts;
    assert_eq!(live.theme("t", &call), Err(UNSERVED.to_string()));
    assert!(live.public_post_type("attachment") && live.public_post_type("post"));
    assert!(live.public_post_type("page") && !live.public_post_type("ext_item"));
    assert_eq!(live.translate("ext", "Hello", Some("ctx")), "Hello");
    let forms = ("item", "items");
    let plural = [0, 1, 2].map(|n| live.translate_plural("ext", forms, n, None));
    assert_eq!(plural, ["items", "item", "items"]);
    assert!(live.now() > 1_700_000_000);
}

/// The plugin files of `active_plugins`: its strings, as text.
#[test]
fn active_plugins_files() {
    let listed = [
        (Key::Int(0), text("a/a.php")),
        (Key::Int(1), Value::String(vec![0xff])),
        (Key::Int(2), Value::Int(3)),
        (Key::Int(3), text("b.php")),
    ];
    assert_eq!(files(&listed), ["a/a.php", "b.php"]);
}

/// A site without a plugins folder runs no extension.
#[tokio::test]
async fn no_extensions_without_a_plugins_folder() {
    let site = crate::tests::oracle_site().await;
    assert!(request(&site).is_none());
}

/// A fault is logged with its extension, hook, callback and fault.
#[test]
fn faults_are_logged() {
    let report = Report {
        extension: "ext".into(),
        hook: "the_content".into(),
        callback: Some(3),
        fault: hyscribe_ext::Fault::Timeout,
    };
    let mut out = Vec::new();
    log(&[report.clone(), report], &mut out);
    let line = "extension ext: the_content Some(3): Timeout\n";
    assert_eq!(String::from_utf8(out).unwrap(), line.repeat(2));
}

/// An action's extensions read the action's own writes, and report their
/// faults; the action's rollback leaves nothing behind.
#[tokio::test(flavor = "multi_thread")]
async fn an_action_s_extensions_report_their_faults() {
    let dir = tempfile::tempdir().unwrap();
    install(dir.path());
    let runtime = hyscribe_ext::Runtime::new(dir.path());
    let found = installed::find(dir.path(), FILE).unwrap();
    let mut acted = Acted::new(|live| runtime.activating(&[], &found, live)).unwrap();
    let writer = Writer::open(&fixture_database(), Mode::Primary).await;
    let writer = writer.unwrap().unwrap();
    let mut write = writer.begin().await.unwrap();
    update_option(&mut write, "ext_hooks", &text("trap"), None)
        .await
        .unwrap();
    let run: Run = Box::new(|request| {
        request
            .index("hyscribe-test")
            .map(|at| request.activate(at))
    });
    // The action waits for its extensions until their thread says it is done.
    let ran = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        acted.run(&mut write, run),
    );
    let activated = ran.await.expect("the extensions' thread ends the wait");
    assert!(matches!(activated, Some(Err(hyscribe_ext::Fault::Trap(_)))));
    let reports = acted.reports();
    assert!(matches!(reports.as_slice(), [Report { hook, .. }] if hook == "activate"));
}

/// A panic in the extensions' thread is the action's.
#[tokio::test(flavor = "multi_thread")]
#[should_panic(expected = "in the extensions' thread")]
async fn a_panic_on_the_extensions_thread_is_the_action_s() {
    let dir = tempfile::tempdir().unwrap();
    install(dir.path());
    let runtime = hyscribe_ext::Runtime::new(dir.path());
    let found = installed::find(dir.path(), FILE).unwrap();
    let mut acted = Acted::new(|live| runtime.activating(&[], &found, live)).unwrap();
    assert_eq!(acted.reports(), Vec::new());
    let writer = Writer::open(&fixture_database(), Mode::Primary).await;
    let writer = writer.unwrap().unwrap();
    let mut write = writer.begin().await.unwrap();
    acted
        .run(
            &mut write,
            Box::new(|_| panic!("in the extensions' thread")),
        )
        .await;
}

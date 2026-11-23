// SPDX-License-Identifier: AGPL-3.0-or-later

//! The `theme` world (ADR 0023), over the test theme.

use std::path::Path;

use super::*;
use crate::manifest::Kind as Of;

/// What the test theme asks for.
fn theme_grants() -> Grants {
    Grants {
        filters: ["ext_theme", "ext_theme_head"].map(String::from).into(),
        actions: [
            "wp_enqueue_scripts",
            "wp_head",
            "wp_footer",
            "after_setup_theme",
        ]
        .map(String::from)
        .into(),
        ..Grants::default()
    }
}

fn package<'a>(kind: Of, wasm: &'a [u8], grants: Grants, folder: &'a Path) -> Package<'a> {
    Package {
        kind,
        sdk: "0.3",
        slug: "hyscribe-theme",
        wasm,
        grants,
        limits: Limits::default(),
        folder,
    }
}

/// The test theme loaded alone, as a request's set.
fn theme(limits: Limits) -> Result<Loaded, Fault> {
    let host = Host::new()?;
    let folder = tempfile::tempdir().map_err(|e| Fault::Refused(e.to_string()))?;
    let loaded = host.load(Package {
        limits,
        ..package(Of::Theme, guest::theme(), theme_grants(), folder.path())
    })?;
    let extensions = Arc::new(Extensions::new(vec![Arc::new(loaded)]));
    Ok(Loaded {
        host,
        extensions,
        folders: vec![folder],
    })
}

const CALLS: &str = "(true, (), false, (), ()) Ok(true) Ok(false) Err(SdkError::Failed(\"the database went away\")) \
Err(SdkError::Invalid(\"a value that does not decode\")) Ok([60, 116, 104, 101, 95, 116, 105, 116, 108, 101, 62]) \
Ok([255]) Err(SdkError::Failed(\"the database went away\")) Err(SdkError::Invalid(\"a value that does not decode\")) \
Ok(Int(7)) Err(SdkError::Failed(\"a value that does not cross\")) Err(SdkError::Failed(\"the database went away\")) \
Err(SdkError::Invalid(\"a value that does not decode\")) None Some(\"footer-x.php\") None \
Some(\"template-parts/content-page.php\") None Ok(()) Err(SdkError::Invalid(\"a value that does not decode\")) \
true false Ok(()) Ok(\"sidebar-1\") Err(SdkError::Invalid(\"not JSON\")) Ok(()) \
Err(SdkError::Invalid(\"a value that does not decode\")) Ok(()) Err(SdkError::Invalid(\"a value that does not decode\")) \
Ok(()) Err(SdkError::Invalid(\"../x.css is not a file inside public/\")) Ok(()) Ok(()) Err(SdkError::Invalid(\"not JSON\")) \
Ok(()) Err(SdkError::Invalid(\"not JSON\")) Ok(None) Ok(true) Ok(Some(Text(\"red\"))) Ok(Some(Int(3))) \
Err(SdkError::Failed(\"a value that does not cross\")) Err(SdkError::Invalid(\"a value that does not decode\")) \
Err(SdkError::Invalid(\"a value that does not decode\")) Ok(true) Ok(false) Ok(()) Err(SdkError::Invalid(\"not JSON\")) \
true false Err(SdkError::Denied(\"registered in init only\"))";

#[test]
fn a_theme_calls_its_world_in_a_request() -> Result<(), Fault> {
    let loaded = theme(Limits::default())?;
    let extension = loaded.extensions.list().first().cloned();
    let templates = extension.as_deref().map(Extension::templates);
    let file = |file: &str, name: Option<&str>, post_types: &[&str]| TemplateFile {
        file: file.into(),
        name: name.map(String::from),
        post_types: post_types.iter().map(|t| String::from(*t)).collect(),
    };
    assert_eq!(
        templates,
        Some(
            &[
                file("index.php", None, &[]),
                file("single.php", None, &[]),
                file("page-wide.php", Some("Wide"), &["page", "post"]),
                file("template-parts/header.php", None, &[]),
                file("front-page.php", None, &[]),
            ][..]
        )
    );
    let site = TestSite::new();
    let request = loaded.request(&site);
    assert_eq!(round(&request, "ext_theme"), CALLS);
    assert_eq!(request.reports(), Vec::new());
    let logged = site.logged();
    let calls: Vec<&str> = logged
        .iter()
        .filter_map(|line| line.strip_prefix("theme hyscribe-theme "))
        .collect();
    assert_eq!(calls.len(), 39, "{calls:#?}");
    assert_eq!(
        calls.get(..5),
        Some(
            &[
                "HavePosts",
                "ThePost",
                "InTheLoop",
                "RewindPosts",
                "ResetPostdata"
            ][..]
        )
    );
    for call in [
        "Part(Header, \"header\", None, [])",
        "Part(Footer, \"footer\", Some(\"x\"), [Int(1)])",
        "Part(Sidebar, \"sidebar\", Some(\"bytes\"), [])",
        "Part(TemplatePart, \"template-parts/content\", Some(\"page\"), [])",
        "AddSupport(\"title-tag\", [])",
        "NavMenus([(\"menu-1\", \"Header\"), (\"menu-2\", \"Footer\")])",
        "ImageSize(\"wide\", 1200, 0, Bool(true))",
        "ThumbnailSize(800, 600, Bool(true))",
        "EditorStyle([\"editor.css\"])",
        "ContentWidth(800)",
        "Customize(Section, \"links\", Array([]), [(\"active_callback\", 1)])",
        "Uncustomize(Panel, \"y\")",
    ] {
        assert!(calls.contains(&call), "{call} in {calls:#?}");
    }
    Ok(())
}

#[test]
fn a_component_loads_as_its_own_kind_only() -> Result<(), Fault> {
    let host = Host::new()?;
    let folder = tempfile::tempdir().map_err(|e| Fault::Refused(e.to_string()))?;
    let as_plugin = package(Of::Plugin, guest::theme(), theme_grants(), folder.path());
    let Err(Fault::Refused(why)) = host.load(as_plugin) else {
        panic!("a theme loaded as a plugin");
    };
    assert!(why.starts_with("hyscribe:sdk/"), "{why}");
    assert!(
        why.ends_with("@0.3.0 is a theme's, not a plugin's"),
        "{why}"
    );
    let as_theme = package(Of::Theme, guest::component(), grants(), folder.path());
    assert_eq!(
        host.load(as_theme).err(),
        Some(Fault::Refused("no hyscribe:sdk/theme-guest@0.3.0".into()))
    );
    assert!(extension::of("hyscribe:sdk/template@0.3.0", "template"));
    assert!(!extension::of("hyscribe:sdk/template@0.3.0", "theme-setup"));
    assert!(!extension::of("other:sdk/template@0.3.0", "template"));
    assert!(!extension::of("hyscribe:sdk/template", "template"));
    Ok(())
}

use hyscribe_model::hooks::{Dispatch, scoped};

/// The test theme's request as the dispatcher of the hooks its templates'
/// host calls fire.
fn dispatching(loaded: &Loaded, site: &Arc<TestSite>) -> (Arc<Request>, Arc<dyn Dispatch>) {
    let request = Arc::new(loaded.request(site));
    let dispatch: Arc<dyn Dispatch> = request.clone();
    (request, dispatch)
}

#[test]
fn renders_in_an_instance_of_its_own() -> Result<(), Fault> {
    let loaded = theme(Limits::default())?;
    let site = TestSite::new();
    let (request, dispatch) = dispatching(&loaded, &site);
    assert_eq!(request.templates().map(<[_]>::len), Some(5));
    // `wp_head` fires a hook the theme answers while it renders.
    let page =
        b"<head>\n<head><!-- theme hook --></head>\n<main>index</main>\n<wp_footer>".to_vec();
    for _ in 0..2 {
        assert_eq!(
            scoped(&dispatch, || request.render("index.php")),
            Ok(page.clone())
        );
    }
    let refused = Fault::Guest("no template missing.php".into());
    assert_eq!(request.render("missing.php"), Err(refused.clone()));
    let reports = request.reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(
        reports.first().map(|r| (&r.hook, &r.fault)),
        Some((&"missing.php".to_string(), &refused))
    );
    // A theme's callbacks still run after its own error.
    assert_eq!(
        request.filter("ext_theme_head", text("h"), &[]),
        text("h<!-- theme hook -->")
    );
    Ok(())
}

/// An action that prints prints what each callback answers, core's
/// callbacks running before those of the same priority and after the rest.
#[test]
fn callbacks_print_what_they_answer_on_an_action_that_prints() -> Result<(), Fault> {
    let loaded = theme(Limits::default())?;
    let request = loaded.request(&TestSite::new());
    let mut asked = Vec::new();
    let mut core = |upto: Option<i32>| {
        asked.push(upto);
        format!("[core {upto:?}]").into_bytes()
    };
    let printed = request.printing("ext_theme_head", &[text("h")], &mut core);
    let expected = "[core Some(10)]h<!-- theme hook -->[core None]";
    assert_eq!(String::from_utf8_lossy(&printed), expected);
    assert_eq!(asked, [Some(10), None]);
    // A hook no callback is on prints core's only, and counts as fired.
    let printed = request.printing("wp_body_open", &[], &mut |_| b"core".to_vec());
    assert_eq!(printed, b"core");
    assert_eq!(request.0.did_action("wp_body_open"), 1);
    // An answer that is no string, or none, prints nothing.
    let plugins = load(&["ext"], Limits::default())?;
    let request = plugins.request(&TestSite::new());
    for hook in ["ext_count", "ext_error"] {
        assert_eq!(request.printing(hook, &[], &mut |_| Vec::new()), b"");
    }
    // Callbacks at two priorities: core's run before each.
    let mut asked = Vec::new();
    let mut core = |upto: Option<i32>| {
        asked.push(upto);
        Vec::new()
    };
    request.printing("ext_chain", &[text("x")], &mut core);
    assert_eq!(asked, [Some(10), Some(20), None]);
    Ok(())
}

#[test]
fn host_calls_are_left_out_of_rendering_time() -> Result<(), Fault> {
    let limits = Limits {
        call_ms: 20,
        request_ms: 30,
        ..Limits::default()
    };
    let loaded = theme(limits)?;
    let request = loaded.request(&TestSite::new());
    for _ in 0..2 {
        assert_eq!(request.render("slow.php"), Ok(b"slow".to_vec()));
    }
    assert_eq!(request.reports(), Vec::new());
    Ok(())
}

#[test]
fn a_fault_in_rendering_stops_the_theme() -> Result<(), Fault> {
    let loaded = theme(Limits::default())?;
    let site = TestSite::new();
    let request = loaded.request(&site);
    assert!(matches!(request.render("trap.php"), Err(Fault::Trap(_))));
    let stopped = Fault::Refused("stopped in this request".into());
    assert_eq!(request.render("index.php"), Err(stopped));
    assert_eq!(request.filter("ext_theme_head", text("h"), &[]), text("h"));
    let request = loaded.request(&site);
    assert_eq!(request.render("spin.php"), Err(Fault::Timeout));
    let request = loaded.request(&site);
    assert!(matches!(request.render("trap.php"), Err(Fault::Trap(_))));
    let record = loaded.record(0).map(|record| record.disabled().is_some());
    assert_eq!(record, Some(true));
    let request = loaded.request(&site);
    assert_eq!(request.render("index.php"), Err(Fault::Disabled));
    Ok(())
}

/// A dispatcher that renders again from inside the theme's render.
struct Nested(Arc<Request>, Mutex<Option<Result<Vec<u8>, Fault>>>);

impl Dispatch for Nested {
    fn attached(&self, _: &str) -> bool {
        true
    }

    fn filter(&self, _: &str, value: Value, _: &[Value]) -> Value {
        *held(&self.1) = Some(self.0.render("index.php"));
        value
    }

    fn action(&self, _: &str, _: &[Value]) {}
}

#[test]
fn a_theme_renders_once_at_a_time_and_a_plugin_never() -> Result<(), Fault> {
    let loaded = theme(Limits::default())?;
    let request = Arc::new(loaded.request(&TestSite::new()));
    let nested = Arc::new(Nested(request.clone(), Mutex::new(None)));
    let dispatch: Arc<dyn Dispatch> = nested.clone();
    assert!(scoped(&dispatch, || request.render("index.php")).is_ok());
    let inner = held(&nested.1).clone();
    assert_eq!(
        inner,
        Some(Err(Fault::Refused("the theme is rendering".into())))
    );
    let plugins = load(&["ext"], Limits::default())?;
    let request = plugins.request(&TestSite::new());
    assert_eq!(request.templates(), None);
    assert_eq!(
        request.render("index.php"),
        Err(Fault::Refused("no theme".into()))
    );
    Ok(())
}

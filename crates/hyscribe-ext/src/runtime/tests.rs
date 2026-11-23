// SPDX-License-Identifier: AGPL-3.0-or-later

use std::path::PathBuf;

use hyscribe_phpser::Value;

use super::*;
use crate::faults;
use crate::tests::{TestSite, component};

const MANIFEST: &[u8] = include_bytes!("../../guest/hyscribe.ext.toml");
const FILE: &str = "hyscribe-test/hyscribe-test.php";

/// A plugins folder with the test extension installed, not yet approved.
fn installed() -> (tempfile::TempDir, PathBuf, Runtime) {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("hyscribe-test");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join(manifest::FILE), MANIFEST).unwrap();
    std::fs::write(folder.join(COMPONENT), component()).unwrap();
    let runtime = Runtime::new(dir.path(), &dir.path().join("themes"));
    (dir, folder, runtime)
}

fn approve(runtime: &Runtime) {
    let found = installed::find(runtime.plugins(), FILE).unwrap();
    installed::approve(&found).unwrap();
}

fn files(files: &[&str]) -> Vec<String> {
    files.iter().map(|file| (*file).to_string()).collect()
}

fn site() -> Arc<dyn Site> {
    TestSite::new()
}

/// The set a request runs over.
fn set_of(request: &Request) -> *const Extensions {
    Arc::as_ptr(&request.0.extensions)
}

fn text(s: &str) -> Value {
    Value::String(s.as_bytes().to_vec())
}

#[test]
fn runs_only_what_is_installed_and_approved() {
    let (_dir, _, runtime) = installed();
    let listed = files(&[FILE, "missing/missing.php", "hello.php", "../x/x.php"]);
    let (request, reports) = runtime.request(&listed, None, site());
    assert!(request.is_none());
    assert_eq!(reports, Vec::new());
    approve(&runtime);
    let (request, reports) = runtime.request(&files(&["missing/missing.php", FILE]), None, site());
    let request = request.unwrap();
    assert_eq!(reports, Vec::new());
    assert_eq!(request.index("hyscribe-test"), Some(0));
    assert_eq!(request.index("other"), None);
    assert_eq!(request.filter("ext_echo", text("x"), &[]), text("x"));
    assert!(runtime.request(&[], None, site()).0.is_none());
    assert!(format!("{runtime:?}").starts_with("Runtime { plugins: "));
}

#[test]
fn loads_each_version_once() {
    let (_dir, folder, runtime) = installed();
    approve(&runtime);
    let first = runtime.request(&files(&[FILE]), None, site()).0.unwrap();
    let again = runtime.request(&files(&[FILE]), None, site()).0.unwrap();
    assert_eq!(set_of(&first), set_of(&again));
    // Approving again writes a new file: a new version.
    approve(&runtime);
    let approved = runtime.request(&files(&[FILE]), None, site()).0.unwrap();
    assert_ne!(set_of(&first), set_of(&approved));
    // An update that does not load is reported once, and runs nothing.
    std::fs::write(folder.join(COMPONENT), b"not wasm").unwrap();
    let (request, reports) = runtime.request(&files(&[FILE]), None, site());
    assert!(request.is_none());
    assert!(
        matches!(
            reports.as_slice(),
            [Report { extension, hook, callback: None, fault: Fault::Refused(_) }]
                if extension == "hyscribe-test" && hook == "load"
        ),
        "{reports:?}"
    );
    assert_eq!(runtime.request(&files(&[FILE]), None, site()).1, Vec::new());
    std::fs::remove_file(folder.join(COMPONENT)).unwrap();
    let reports = runtime.request(&files(&[FILE]), None, site()).1;
    assert!(matches!(
        reports.as_slice(),
        [Report {
            fault: Fault::Refused(_),
            ..
        }]
    ));
    std::fs::remove_dir_all(&folder).unwrap();
    assert_eq!(runtime.request(&files(&[FILE]), None, site()).1, Vec::new());
}

#[test]
fn a_disabled_extension_is_not_loaded() {
    let (_dir, folder, runtime) = installed();
    approve(&runtime);
    std::fs::write(folder.join(faults::FILE), "nonsense").unwrap();
    let (request, reports) = runtime.request(&files(&[FILE]), None, site());
    assert!(request.is_none());
    assert!(matches!(
        reports.as_slice(),
        [Report {
            fault: Fault::Disabled,
            ..
        }]
    ));
}

#[test]
fn activates_with_the_grants_its_activation_approves() {
    let (_dir, folder, runtime) = installed();
    let found = installed::find(runtime.plugins(), FILE).unwrap();
    let request = runtime.activating(&[], &found, site()).unwrap();
    assert_eq!(request.activate(0), Ok(()));
    approve(&runtime);
    let active = files(&[FILE]);
    let request = runtime.activating(&active, &found, site()).unwrap();
    assert_eq!(request.index("hyscribe-test"), Some(0));
    assert_eq!(request.activate(1), Ok(()));
    std::fs::write(folder.join(COMPONENT), b"not wasm").unwrap();
    let refused = runtime.activating(&[], &found, site());
    assert!(matches!(refused, Err(Fault::Refused(_))));
}

/// The test extension's page hooks: off without `ext_hooks`, then changing
/// the page with a count of the instance's calls, or trapping.
#[test]
fn the_page_hooks_follow_ext_hooks() {
    let (_dir, _, runtime) = installed();
    approve(&runtime);
    let site = TestSite::new();
    let on = |site: &Arc<TestSite>| {
        let dyn_site: Arc<dyn Site> = site.clone();
        runtime.request(&files(&[FILE]), None, dyn_site).0.unwrap()
    };
    let classes = || Value::Array(vec![(hyscribe_phpser::Key::Int(0), text("home"))]);
    let request = on(&site);
    assert_eq!(request.filter("body_class", classes(), &[]), classes());
    let footer = text("<span>Thanks</span>");
    assert_eq!(
        request.filter("admin_footer_text", footer.clone(), &[]),
        footer
    );
    site.set_option("ext_hooks", "on");
    let request = on(&site);
    let listed = request.filter("body_class", classes(), &[]);
    let expected = ["home", "ext-body", "ext-n1"].map(text);
    let values: Vec<Value> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|(_, v)| v.clone())
        .collect();
    assert_eq!(values, expected);
    let empty = request.filter("body_class", Value::Array(Vec::new()), &[]);
    assert_eq!(empty.as_array().map(<[_]>::len), Some(2));
    let footer = request.filter("admin_footer_text", text("<span>Thanks</span>"), &[]);
    let expected = "<span>Thanks</span><span id=\"ext-footer\">ext-n3</span>";
    assert_eq!(footer, text(expected));
    assert_eq!(
        request.filter("admin_footer_text", Value::Int(1), &[]),
        Value::Int(1)
    );
    // A fresh instance per request starts its count again.
    let fresh = on(&site).filter("admin_footer_text", text(""), &[]);
    assert_eq!(fresh, text("<span id=\"ext-footer\">ext-n1</span>"));
    site.set_option("ext_hooks", "trap");
    for hook in ["body_class", "admin_footer_text"] {
        let request = on(&site);
        assert_eq!(request.filter(hook, text("x"), &[]), text("x"));
        assert!(matches!(
            request.reports().as_slice(),
            [Report {
                fault: Fault::Trap(_),
                ..
            }]
        ));
    }
}

const THEME: &[u8] = include_bytes!("../../guest/theme/hyscribe.ext.toml");

/// A themes folder beside the plugins folder, with the test theme
/// installed and approved, and a theme with a manifest only.
fn themes(dir: &tempfile::TempDir) -> PathBuf {
    let themes = dir.path().join("themes");
    let folder = themes.join("hyscribe-theme");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join(manifest::FILE), THEME).unwrap();
    std::fs::write(folder.join(COMPONENT), crate::tests::theme_component()).unwrap();
    let found = installed::theme(&themes, "hyscribe-theme", "hyscribe-theme").unwrap();
    installed::approve(&found).unwrap();
    let parent = themes.join("parent");
    std::fs::create_dir_all(&parent).unwrap();
    let named = String::from_utf8_lossy(THEME).replace("hyscribe-theme", "parent");
    std::fs::write(parent.join(manifest::FILE), named).unwrap();
    themes
}

#[test]
fn loads_the_active_theme_after_the_plugins() {
    let (dir, _, runtime) = installed();
    approve(&runtime);
    let folder = themes(&dir);
    assert_eq!(runtime.themes(), folder);
    let theme = |stylesheet, template| {
        Some(Active {
            stylesheet,
            template,
        })
    };
    let both = runtime.request(
        &files(&[FILE]),
        theme("hyscribe-theme", "hyscribe-theme"),
        site(),
    );
    let request = both.0.unwrap();
    assert_eq!(both.1, Vec::new());
    assert_eq!(request.index("hyscribe-test"), Some(0));
    assert_eq!(request.index("hyscribe-theme"), Some(1));
    assert_eq!(request.templates().map(<[_]>::len), Some(5));
    let child = runtime
        .request(&[], theme("hyscribe-theme", "parent"), site())
        .0;
    assert_eq!(
        child.and_then(|request| request.index("hyscribe-theme")),
        Some(0)
    );
    // A child whose parent is not installed does not run.
    let orphan = runtime.request(&[], theme("hyscribe-theme", "gone"), site());
    assert!(orphan.0.is_none() && orphan.1.is_empty());
    // Neither does a theme that is only a manifest, nor a plugin's slug.
    for stylesheet in ["parent", "hyscribe-test", "../x", "twentytwentyfive"] {
        let request = runtime.request(&[], theme(stylesheet, stylesheet), site());
        assert!(request.0.is_none(), "{stylesheet}");
    }
    let alone = runtime.request(&[], theme("hyscribe-theme", "hyscribe-theme"), site());
    assert_eq!(
        alone
            .0
            .and_then(|request| request.templates().map(<[_]>::len)),
        Some(5)
    );
    assert!(format!("{runtime:?}").contains("themes: "));
}

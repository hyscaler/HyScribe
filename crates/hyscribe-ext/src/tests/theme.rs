// SPDX-License-Identifier: AGPL-3.0-or-later

//! The `theme` world (ADR 0023), over the test theme.

use std::path::Path;

use super::*;
use crate::manifest::Kind as Of;

/// What the test theme asks for.
fn theme_grants() -> Grants {
    Grants {
        filters: ["ext_theme", "ext_theme_head"].map(String::from).into(),
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
                file("page-wide.php", Some("Wide"), &["page", "post"]),
                file("template-parts/header.php", None, &[]),
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

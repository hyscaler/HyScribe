// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::{hs_admin_page, unhex};

use super::*;
use crate::appearance::tests::{check, rows};
use crate::tests::{BROWSER, Role, SITE, first_difference, request};

const GOLDEN: &str = include_str!("../../tests/golden/plugins.tsv");

/// Where PHP names its plugins folder, HyScribe names its own.
fn folder(php: &str) -> String {
    let path = "<code>/var/www/html/wp-content/plugins</code>";
    assert!(php.contains(path));
    php.replace(path, &format!("<code>{PLUGIN_DIR}</code>"))
}

/// The plugin the test extension stands in for, as its manifest has it.
pub(crate) fn installed() -> Plugin {
    Plugin {
        file: "hyscribe-test/hyscribe-test.php".into(),
        name: "HyScribe Test".into(),
        description: "Drives every call of the extension SDK for HyScribe's tests.".into(),
        version: "0.1.0".into(),
        author: "HyScribe".into(),
        author_uri: "https://hyscribe.com/".into(),
        plugin_uri: "https://hyscribe.com/plugins/hyscribe-test/".into(),
        grants: vec![("Options", vec!["ext".into()])],
        sensitive: vec!["map_meta_cap".into()],
        limits: None,
        approved: true,
        disabled: false,
    }
}

/// `page` without HyScribe's own part of its rows.
fn without_grants(page: &str) -> String {
    let mut out = String::new();
    let mut rest = page;
    while let Some((before, after)) = rest.split_once("<div class=\"hyscribe-grants ") {
        out.push_str(before);
        rest = after.split_once("</div>").map_or("", |(_, tail)| tail);
    }
    out + rest
}

/// With no plugin installed, the plugin folder of the PHP oracle's image
/// is the one place the page differs.
#[test]
fn plugins_are_php_s() {
    let mut failures = Vec::new();
    let rows = rows("plugins.php");
    let canonical = format!("{SITE}/hs-admin/plugins");
    let request = request(&canonical, "/hs-admin/plugins", &BROWSER);
    for (role, php) in &rows {
        let php = php.as_deref().map(folder);
        let ours = plugins(role, &request, &[], &Args::default()).unwrap();
        failures.extend(check(role, ours, php.as_ref()));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(rows.len(), 13);
}

/// With the test extension installed, each recorded view is PHP's but for
/// HyScribe's own part of the rows.
#[test]
fn installed_plugins_are_listed_as_php_lists_them() {
    let mut failures = Vec::new();
    let mut count = 0;
    for line in GOLDEN.lines() {
        let [variant, query, role, page] = line.split('\t').collect::<Vec<_>>()[..] else {
            panic!("{line}");
        };
        count += 1;
        let php = folder(&hs_admin_page(&String::from_utf8(unhex(page)).unwrap()));
        let site = Role(Box::leak(format!("{variant}:{role}").into_boxed_str()));
        let uri = format!("/hs-admin/plugins{query}");
        let canonical = format!(
            "{SITE}{}",
            text(&remove_query_arg(&TEMPORARY, uri.as_bytes()))
        );
        let request = request(&canonical, &uri, &BROWSER);
        let args = Args::parse(query.trim_start_matches('?')).unwrap();
        let plugin = Plugin {
            approved: variant != "x13inactive",
            ..installed()
        };
        let Some(Answer::Page(ours)) = plugins(&site, &request, &[plugin], &args) else {
            panic!("{variant} {query}");
        };
        let shown = ours.contains("hyscribe-grants");
        let inactive = php.contains("<tr class=\"inactive\"");
        assert_eq!(shown, inactive, "{variant} {query}");
        let ours = without_grants(&ours);
        if ours != php {
            failures.push(format!(
                "{variant} {query}: {}",
                first_difference(&php, &ours)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(count, 12);
}

#[test]
fn args_are_those_the_port_takes() {
    let parsed = |query| Args::parse(query);
    assert_eq!(
        parsed("plugin_status=Active&paged=1&s="),
        Some(Args {
            status: "active".into(),
            notice: None
        })
    );
    let notice = |query| parsed(query).unwrap().notice;
    assert_eq!(notice("deactivate=true"), Some("Plugin deactivated."));
    assert_eq!(notice("deactivate=1&activate=1"), Some("Plugin activated."));
    assert_eq!(notice(""), None);
    for query in [
        "paged=2",
        "s=x",
        "orderby=name",
        "error=true",
        "deleted=1",
        "a[]=1",
    ] {
        assert_eq!(parsed(query), None, "{query}");
    }
}

/// Where PHP would write the option back, or find a file, the screen is not
/// ported; an option never set names no plugin.
#[test]
fn unusual_active_plugins_are_not_ported() {
    assert_eq!(active_plugins(None), Some(vec![]));
    assert_eq!(active_plugins(Some(b"a:0:{}".to_vec())), Some(vec![]));
    for stored in [
        &b""[..],
        b"s:3:\"abc\";",
        b"a:1:{i:0;i:5;}",
        b"a:1:{i:0;s:0:\"\";}",
    ] {
        assert_eq!(active_plugins(Some(stored.to_vec())), None);
    }
    let canonical = format!("{SITE}/hs-admin/plugins");
    let request = request(&canonical, "/hs-admin/plugins", &BROWSER);
    let site = Role("o36bad:administrator");
    assert_eq!(plugins(&site, &request, &[], &Args::default()), None);
    let site = Role("x13badrecent:administrator");
    assert_eq!(plugins(&site, &request, &[], &Args::default()), None);
    // One who may not activate gets no Activate link, nor the screen.
    let editor = Role("x13inactive:editor");
    let refused = plugins(&editor, &request, &[installed()], &Args::default());
    assert!(matches!(refused, Some(Answer::Denied(_))));
}

/// Paths that climb or name a drive are invalid paths, others missing
/// files, as `validate_file()` tells them.
#[test]
fn plugin_paths_are_validated_as_php_s() {
    for file in [
        "../",
        "../a.php",
        "a/../../b.php",
        "a/../b.php",
        "c:/a.php",
        "a\\..\\b.php",
    ] {
        assert!(invalid_path(file), "{file}");
    }
    for file in ["a.php", "a/b.php", "a/b..php", "x../", "..", "a/.."] {
        assert!(!invalid_path(file), "{file}");
    }
}

#[test]
fn recently_activated_keeps_a_week() {
    let now = 2_000_000_000;
    let stored =
        b"a:4:{s:1:\"a\";i:2000000000;s:1:\"b\";i:1999395200;s:1:\"c\";i:1999395199;i:0;b:0;}";
    let kept = recently_activated(Some(stored), now).unwrap();
    let files: Vec<Vec<u8>> = kept.iter().map(|(key, _)| key.to_php_string()).collect();
    assert_eq!(files, [b"a".to_vec(), b"b".to_vec()]);
    assert_eq!(recently_activated(None, now), Some(vec![]));
    assert_eq!(recently_activated(Some(b"s:1:\"x\";"), now), None);
}

/// HyScribe's own part of a row: what an extension asks for until its
/// activation approves it, and why HyScribe stopped one.
#[test]
fn grants_are_listed_before_approval() {
    let plugin = installed();
    assert_eq!(grants(&plugin, true), "");
    let asked = grants(&plugin, false);
    assert_eq!(
        asked,
        "<div class=\"hyscribe-grants notice notice-warning notice-alt inline\"><p><strong>Activating this plugin allows it:</strong></p><ul class=\"ul-disc\"><li>Options: <code>ext</code></li></ul><p><strong>Security sensitive hooks:</strong> <code>map_meta_cap</code>. They can change who is logged in, what they may do, or what is filtered.</p></div>"
    );
    let changed = Plugin {
        approved: false,
        disabled: true,
        grants: vec![],
        sensitive: vec![],
        limits: Some((200, 1_000, 32)),
        ..installed()
    };
    let text = grants(&changed, true);
    assert!(text.contains("HyScribe stopped running this plugin"));
    assert!(text.contains("asks for more than was approved"));
    assert!(text.contains("<p>Nothing beyond its own code.</p>"));
    assert!(text.contains("up to 200 ms a call, 1000 ms a request and 32 MiB"));
    let stopped = grants(
        &Plugin {
            disabled: true,
            ..installed()
        },
        true,
    );
    assert!(stopped.starts_with("<div class=\"hyscribe-grants") && stopped.contains("stopped"));
}

/// Two plugins of one name get row ids that stay apart, and a plugin
/// without a description, author or links shows none.
#[test]
fn rows_of_bare_plugins() {
    let bare = Plugin {
        file: "bare/bare.php".into(),
        name: "Bare".into(),
        version: String::new(),
        ..Plugin::default()
    };
    let twin = Plugin {
        file: "twin/twin.php".into(),
        ..bare.clone()
    };
    let triplet = Plugin {
        file: "triplet/triplet.php".into(),
        ..bare.clone()
    };
    let site = Role("x13inactive:administrator");
    let canonical = format!("{SITE}/hs-admin/plugins");
    let request = request(&canonical, "/hs-admin/plugins", &BROWSER);
    let Some(Answer::Page(page)) =
        plugins(&site, &request, &[bare, twin, triplet], &Args::default())
    else {
        panic!("no page");
    };
    assert!(page.contains("id=\"activate-bare\"") && page.contains("id=\"activate-bare-2\""));
    assert!(page.contains("id=\"activate-bare-3\""));
    assert!(page.contains("<div class='plugin-description'><p>&nbsp;</p></div>"));
    assert!(page.contains("<div class='inactive second plugin-version-author-uri'></div>"));
    let author = Plugin {
        author_uri: String::new(),
        ..installed()
    };
    let Some(Answer::Page(page)) = plugins(&site, &request, &[author], &Args::default()) else {
        panic!("no page");
    };
    assert!(page.contains("Version 0.1.0 | By HyScribe | "));
}

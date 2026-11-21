// SPDX-License-Identifier: AGPL-3.0-or-later

use super::toml::{Item, parse};
use super::*;
use crate::tests::grants;

const GUEST: &str = include_str!("../../guest/hyscribe.ext.toml");

/// The smallest plugin manifest, with `extra` after its `[extension]`.
fn plugin(extra: &str) -> String {
    format!(
        "[extension]\nkind = \"plugin\"\nid = \"demo\"\nversion = \"1.0\"\nsdk = \"0.1\"\n\
         replaces = \"demo/demo.php\"\nlicense = \"GPL-2.0-or-later\"\nname = \"Demo\"\n{extra}"
    )
}

fn refused(text: &str) -> String {
    Manifest::parse(text.as_bytes()).expect_err(text)
}

#[test]
fn reads_the_test_extensions_manifest() {
    let manifest = Manifest::parse(GUEST.as_bytes()).unwrap();
    assert_eq!(manifest.kind, Kind::Plugin);
    assert_eq!(manifest.id, "hyscribe-test");
    assert_eq!(manifest.version, "0.1.0");
    assert_eq!(manifest.sdk, "0.2");
    assert_eq!(
        manifest.replaces.as_deref(),
        Some("hyscribe-test/hyscribe-test.php")
    );
    assert_eq!(manifest.replaces_versions.as_deref(), Some(">=0.1"));
    assert_eq!(manifest.compatibility.as_deref(), Some("full"));
    assert_eq!(manifest.license, "Apache-2.0 OR MIT");
    assert_eq!(manifest.name, "HyScribe Test");
    assert_eq!(manifest.author, "HyScribe");
    assert_eq!(manifest.author_uri, "https://hyscribe.com/");
    assert_eq!(
        manifest.plugin_uri,
        "https://hyscribe.com/plugins/hyscribe-test/"
    );
    assert!(manifest.description.starts_with("Drives every call"));
    assert_eq!(manifest.grants, grants());
    assert_eq!(manifest.limits, Limits::default());
}

#[test]
fn the_smallest_manifest_grants_nothing() {
    let manifest = Manifest::parse(plugin("").as_bytes()).unwrap();
    assert_eq!(manifest.grants, Grants::default());
    assert_eq!(manifest.description, "");
    assert_eq!(manifest.replaces_versions, None);
    assert_eq!(manifest.compatibility, None);
}

#[test]
fn limits_and_a_theme() {
    let asked = plugin("[limits]\ncall_ms = 200\nmemory_mib = +32\n");
    let manifest = Manifest::parse(asked.as_bytes()).unwrap();
    assert_eq!(
        manifest.limits,
        Limits::asked(&[("call_ms", 200), ("memory_mib", 32)]).unwrap()
    );
    let theme = "[extension]\nkind = 'theme'\nid = 't'\nversion = '1'\nsdk = '0.1'\nlicense = 'MIT'\nname = 'T'\n";
    assert_eq!(Manifest::parse(theme.as_bytes()).unwrap().kind, Kind::Theme);
    assert!(refused(&format!("{theme}replaces = 't/t.php'\n")).contains("a theme replaces no"));
}

#[test]
fn refuses_what_the_format_does_not_allow() {
    let cases: &[(&str, &str)] = &[
        ("[extension]\n", "kind must be plugin or theme"),
        (
            "[extension]\nkind = 'other'\n",
            "kind must be plugin or theme",
        ),
        (
            "[extension]\nkind = 1\n",
            "[extension] kind must be a string",
        ),
        (
            "[extension]\nkind = 'plugin'\nid = 'Demo'\n",
            "id must be a slug",
        ),
        ("[extension]\nkind = 'plugin'\n", "id must be a slug"),
        (
            "[extension]\nkind = 'plugin'\nid = 'd'\n",
            "version must be given",
        ),
        (
            "[extension]\nkind = 'plugin'\nid = 'd'\nversion = 'a b'\n",
            "version must be a valid",
        ),
        (
            "[extension]\nkind = 'plugin'\nid = 'd'\nversion = '1'\nsdk = '0.3'\n",
            "sdk must be",
        ),
    ];
    for (text, why) in cases {
        assert!(refused(text).contains(why), "{text}: {}", refused(text));
    }
    let more: &[(&str, &str)] = &[
        (
            "compatibility = 'some'\n",
            "compatibility must be a valid value",
        ),
        (
            "replaces_versions = '>=1\t'\n",
            "replaces_versions must be a valid value",
        ),
        ("name2 = 'x'\n", "[extension] has no key name2"),
        ("[other]\n", "has no table [other]"),
        (
            "[hooks]\nactions = 'init'\n",
            "actions must be a list of names",
        ),
        (
            "[hooks]\nactions = ['in it']\n",
            "actions must be a list of names",
        ),
        (
            "[hooks]\nactions = ['']\n",
            "actions must be a list of names",
        ),
        (
            "[hooks]\nactions = ['map_meta_cap']\n",
            "map_meta_cap is a sensitive hook",
        ),
        ("[hooks]\nonly = []\n", "[hooks] has no key only"),
        (
            "[capabilities]\nmail = 'yes'\n",
            "mail must be true or false",
        ),
        (
            "[capabilities]\nfilesystem = ['../x']\n",
            "filesystem ../x is not a folder",
        ),
        (
            "[capabilities]\ndb_tables = []\n",
            "[capabilities] has no key db_tables",
        ),
        ("[meta]\nkeys = ['*']\n", "a meta grant names its keys"),
        (
            "[transients]\nnames = ['*']\n",
            "a transient grant names its transients",
        ),
        (
            "[transients]\nsite = ['*']\n",
            "a transient grant names its transients",
        ),
        ("[transients]\nkeys = []\n", "[transients] has no key keys"),
        (
            "[roles]\nnames = ['ext', 'subscriber']\n",
            "subscriber is WordPress's own role: [roles] names its own",
        ),
        ("[roles]\ncaps = []\n", "[roles] has no key caps"),
        ("[http]\nurls = []\n", "[http] has no key urls"),
        (
            "[limits]\ncall_ms = '200'\n",
            "[limits] call_ms must be an integer",
        ),
        (
            "[limits]\ncall_ms = 900\n",
            "call_ms must be from 50 to 500",
        ),
        ("[limits]\nwall_ms = 900\n", "[limits] has no key wall_ms"),
    ];
    for (extra, why) in more {
        let text = plugin(extra);
        assert!(refused(&text).contains(why), "{extra}: {}", refused(&text));
    }
    let description = format!("description = '{}'\n", "x".repeat(1_001));
    assert!(refused(&plugin(&description)).contains("description must be a valid value"));
    assert!(refused(&plugin("author = 'a\\u0007'\n").replace('\'', "\"")).contains("author"));
    assert!(refused(&"#".repeat(65 << 10)).contains("larger than"));
    assert!(refused("a = 1\n").contains("line 1: a is outside a table"));
    let largest = format!("{}{}", plugin(""), "#".repeat(65_536 - plugin("").len()));
    assert_eq!(largest.len(), 65_536);
    assert!(Manifest::parse(largest.as_bytes()).is_ok());
    assert!(
        Manifest::parse(b"[extension]\nid = '\xff'\n")
            .unwrap_err()
            .contains("not UTF-8")
    );
}

#[test]
fn a_plugin_replaces_a_file_in_its_own_folder() {
    let without = plugin("").replace("replaces = \"demo/demo.php\"\n", "");
    for text in [
        without.clone(),
        plugin("").replace("demo/demo.php", "other/demo.php"),
        plugin("").replace("demo/demo.php", "demo/demo.inc"),
        plugin("").replace("demo/demo.php", "demo/sub/demo.php"),
        plugin("").replace("demo/demo.php", "demo-demo.php"),
        plugin("").replace("demo/demo.php", "demo/.php"),
    ] {
        assert!(refused(&text).contains("replaces must be the plugin file demo/<file>.php"));
    }
    for license in ["GPL-2.0-only", "GPL-2.0"] {
        let text = plugin("").replace("GPL-2.0-or-later", license);
        assert!(refused(&text).contains("ADR 0004"), "{license}");
    }
}

#[test]
fn reads_the_toml_subset() {
    let text = "# a comment\r\n\n[a] # here too\nb = \"x\\\"\\\\\\b\\t\\n\\f\\r\\u00e9\\U0001F600\"\nc = 'lit\\eral'\n\
                d = -12\ne = 0\nf = true\ng = false\nh = [\n  'one', # first\n  \"two\",\n]\ni = []\nj = ['k']\n\
                [l]\nm = \"tab\there\"";
    let tables = parse(text).unwrap();
    let a = &tables[0];
    assert_eq!(a.0, "a");
    assert_eq!(
        a.1,
        vec![
            (
                "b".into(),
                Item::Text("x\"\\\u{8}\t\n\u{c}\r\u{e9}\u{1F600}".into())
            ),
            ("c".into(), Item::Text("lit\\eral".into())),
            ("d".into(), Item::Int(-12)),
            ("e".into(), Item::Int(0)),
            ("f".into(), Item::Bool(true)),
            ("g".into(), Item::Bool(false)),
            ("h".into(), Item::List(vec!["one".into(), "two".into()])),
            ("i".into(), Item::List(vec![])),
            ("j".into(), Item::List(vec!["k".into()])),
        ]
    );
    assert_eq!(
        tables[1],
        (
            "l".into(),
            vec![("m".into(), Item::Text("tab\there".into()))]
        )
    );
    assert_eq!(parse(""), Ok(vec![]));
}

#[test]
fn refuses_the_rest_of_toml() {
    let cases = [
        ("a = 1\n", "line 1: a is outside a table"),
        ("[a]\n[a]\n", "line 2: [a] is named twice"),
        ("[a]\nb = 1\nb = 2\n", "line 3: [a] b is named twice"),
        ("[a.b]\n", "expected ]"),
        ("[[a]]\n", "expected a bare key"),
        ("[a]\nb.c = 1\n", "expected ="),
        ("[a]\nb = 1.5\n", "unexpected text after the value"),
        ("[a]\nb = 1979-05-27\n", "unexpected text after the value"),
        ("[a]\nb = 007\n", "expected a value"),
        ("[a]\nb = +\n", "expected a value"),
        ("[a]\nb = 99999999999999999999\n", "expected a value"),
        ("[a]\nb = trueish\n", "unexpected text after the value"),
        ("[a]\nb = yes\n", "expected a value"),
        ("[a]\nb = tru\n", "expected a value"),
        ("[a]\n= 1\n", "line 2: expected a bare key"),
        ("[a]\nb = { c = 1 }\n", "expected a value"),
        (
            "[a]\nb = \"\"\"x\"\"\"\n",
            "unexpected text after the value",
        ),
        ("[a]\nb = \"open\n", "a control character in a string"),
        ("[a]\nb = \"open", "unterminated string"),
        ("[a]\nb = \"\\", "unterminated string"),
        ("[a]\nb = \"\\q\"\n", "an unknown escape"),
        (
            "[a]\nb = \"\\uD800\"\n",
            "an escape that is not a character",
        ),
        ("[a]\nb = \"\\u00\"\n", "an escape that is not a character"),
        (
            "[a]\nb = \"\\u00g0\"\n",
            "an escape that is not a character",
        ),
        ("[a]\nb = [1]\n", "expected a string"),
        ("[a]\nb = ['x' 'y']\n", "expected , or ] in the array"),
        ("[a]\nb = ['x',", "expected a string"),
        ("[a]\nb = 1 c\n", "unexpected text after the value"),
        ("[a]\nb = 1\rc\n", "unexpected text after the value"),
    ];
    for (text, why) in cases {
        let error = parse(text).expect_err(text);
        assert!(error.contains(why), "{text:?}: {error}");
    }
}

/// A Unicode escape cut short by the end of the text is no character.
#[test]
fn a_short_escape_at_the_end() {
    let error = parse("[a]\nb = \"\\u00").unwrap_err();
    assert!(
        error.contains("an escape that is not a character"),
        "{error}"
    );
}

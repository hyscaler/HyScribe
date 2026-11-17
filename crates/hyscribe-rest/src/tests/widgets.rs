// SPDX-License-Identifier: AGPL-3.0-or-later

//! The sidebars, widgets and widget types: `widgets.tsv` over
//! `reference/rest/widgets.sql`'s retrieved widgets, `widgets-default.tsv`
//! on the default install, whose widgets PHP stores as it reads them, and
//! the states in which `retrieve_widgets()` or `get_settings()` would
//! store them, or a widget is not the block widget.

use std::sync::OnceLock;

use super::content::default_install_with;
use super::*;

const WIDGETS_SQL: &str = include_str!("../../../../reference/rest/widgets.sql");

fn retrieved() -> &'static Content {
    static CONTENT: OnceLock<Content> = OnceLock::new();
    CONTENT.get_or_init(|| default_install_with(WIDGETS_SQL))
}

#[test]
fn widgets_match_php() {
    let golden = include_str!("../../tests/golden/widgets.tsv");
    let goldens = [(retrieved(), golden, false)];
    let (failures, not_served) = mismatches(&goldens, true, &|_| false);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!((rest_cases(golden).len(), not_served), (6 * 35, 0));
}

/// The sidebars and widgets of the default install are answered 501, as
/// PHP stores them; its widget types are served.
#[test]
fn stored_widgets_are_not_served() {
    let golden = include_str!("../../tests/golden/widgets-default.tsv");
    let refused = |case: &RestCase| case.flag("refused");
    let goldens = [(content::default_install(), golden, false)];
    let (failures, not_served) = mismatches(&goldens, true, &refused);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!((rest_cases(golden).len(), not_served), (6 * 6, 6 * 4));
    // Their `Allow` header runs the read's permission check too.
    let served = |method: &str| {
        let case = RestCase {
            who: "visitor".into(),
            method: method.into(),
            target: "/?rest_route=/wp/v2/widgets".into(),
            flags: String::new(),
            status: 0,
            headers: Vec::new(),
            body: Vec::new(),
            sent: Vec::new(),
        };
        serve_over(content::default_install(), &case).status
    };
    assert_eq!((served("OPTIONS"), served("POST")), (501, 501));
}

/// The default install with `rows` added, leaked for the tests.
fn with(rows: &str) -> &'static Content {
    Box::leak(Box::new(default_install_with(rows)))
}

/// An option's row, as mysqldump writes it.
fn option(name: &str, value: &str) -> String {
    format!("INSERT INTO `wp_options` VALUES (9950,'{name}','{value}','auto');\n")
}

/// The administrator's status for `target` over `content`.
fn status(content: &'static Content, target: &str) -> u16 {
    get(content, "administrator", target).status
}

/// `retrieve_widgets()` leaves nothing to store where the sidebars' widgets
/// are empty or only their version, and stores them changed for a list by
/// name, a string, or sidebars it maps to the inactive one; so does the
/// theme's widgets from before.
#[test]
fn widgets_php_would_store_are_not_served() {
    let sidebars = "/?rest_route=/wp/v2/sidebars";
    for empty in [r#"s:0:\"\";"#, r#"a:1:{s:13:\"array_version\";i:3;}"#] {
        let content = with(&option("sidebars_widgets", empty));
        assert_eq!(
            get(content, "administrator", sidebars).body,
            b"[]",
            "{empty}"
        );
        let inactive = get(
            content,
            "administrator",
            "/?rest_route=/wp/v2/sidebars/wp_inactive_widgets",
        );
        let data = hyscribe_phpser::json_decode(&inactive.body).unwrap();
        assert_eq!(data.get_str("widgets"), Some(&Value::Array(Vec::new())));
    }
    for stored in [
        r#"s:1:\"x\";"#,
        r#"a:2:{s:19:\"wp_inactive_widgets\";a:1:{s:1:\"k\";s:7:\"block-2\";}s:13:\"array_version\";i:3;}"#,
        r#"a:2:{s:9:\"sidebar-1\";a:1:{s:1:\"k\";s:7:\"block-2\";}s:13:\"array_version\";i:3;}"#,
        r#"a:6:{s:19:\"wp_inactive_widgets\";s:7:\"block-2\";s:18:\"orphaned_widgets_1\";a:1:{i:0;s:7:\"block-3\";}s:18:\"orphaned_widgets_2\";N;s:9:\"sidebar-1\";a:2:{i:0;s:7:\"block-4\";i:1;s:8:\"block-99\";}s:9:\"sidebar-2\";s:1:\"x\";s:13:\"array_version\";i:3;}"#,
    ] {
        assert_eq!(
            status(with(&option("sidebars_widgets", stored)), sidebars),
            501,
            "{stored}"
        );
    }
    let old = r#"a:1:{s:16:\"sidebars_widgets\";a:2:{s:4:\"time\";i:1;s:4:\"data\";a:0:{}}}"#;
    let mods = [WIDGETS_SQL, &option("theme_mods_twentytwentyfive", old)].concat();
    assert_eq!(status(with(&mods), sidebars), 501);
    // The block widgets listed as `retrieve_widgets()` leaves them, but in
    // another sidebar, with one that is no widget, missing some, or
    // without their version.
    let blocks = r#"i:0;s:7:\"block-2\";i:1;s:7:\"block-3\";i:2;s:7:\"block-4\";i:3;s:7:\"block-5\";i:4;s:7:\"block-6\";"#;
    let version = r#"s:13:\"array_version\";i:3;"#;
    for stored in [
        format!(r#"a:2:{{s:9:\"sidebar-1\";a:5:{{{blocks}}}{version}}}"#),
        format!(
            r#"a:2:{{s:19:\"wp_inactive_widgets\";a:6:{{{blocks}i:5;s:8:\"block-99\";}}{version}}}"#
        ),
        format!(r#"a:2:{{s:19:\"wp_inactive_widgets\";a:1:{{i:0;s:7:\"block-2\";}}{version}}}"#),
        format!(r#"a:1:{{s:19:\"wp_inactive_widgets\";a:5:{{{blocks}}}}}"#),
    ] {
        assert_eq!(
            status(with(&option("sidebars_widgets", &stored)), sidebars),
            501,
            "{stored}"
        );
    }
    let stored = format!(r#"a:2:{{s:19:\"wp_inactive_widgets\";a:5:{{{blocks}}}{version}}}"#);
    assert_eq!(
        status(with(&option("sidebars_widgets", &stored)), sidebars),
        200
    );
}

/// `get_settings()` stores a widget's settings that are missing, `false`
/// or in the old form; settings that are not an array are none.
#[test]
fn widget_settings_php_would_store_are_not_served() {
    let sidebars = "/?rest_route=/wp/v2/sidebars";
    for stored in ["b:0;", "a:1:{i:2;a:0:{}}"] {
        let rows = [WIDGETS_SQL, &option("widget_text", stored)].concat();
        assert_eq!(status(with(&rows), sidebars), 501, "{stored}");
    }
    let rows = [WIDGETS_SQL, &option("widget_text", r#"s:1:\"x\";"#)].concat();
    assert_eq!(status(with(&rows), sidebars), 200);
    let base = retrieved();
    let mut options = base.options.clone();
    options.remove("widget_text");
    let missing: &'static Content = Box::leak(Box::new(Content {
        options,
        usermeta: base.usermeta.clone(),
        users: base.users.clone(),
        reads: base.reads,
        ..Content::default()
    }));
    assert_eq!(status(missing, sidebars), 501);
}

/// The form alone, or the instance alone, as `_fields` asks for them.
#[test]
fn a_widget_s_form_or_instance_is_served_alone() {
    let target = "/?rest_route=/wp/v2/widgets/block-2&context=edit&_fields=";
    for field in ["rendered_form", "instance"] {
        let served = get(retrieved(), "administrator", &format!("{target}{field}"));
        let data = hyscribe_phpser::json_decode(&served.body).unwrap();
        let value = data.get_str(field).cloned().unwrap_or_default();
        assert!(!matches!(value, Value::Null), "{field}");
    }
}

/// Only the block widget's form and instance are ported, with content
/// that is a string and the salts to sign it.
#[test]
fn other_widgets_forms_are_not_served() {
    let text = r#"a:2:{i:2;a:1:{s:4:\"text\";s:1:\"x\";}s:12:\"_multiwidget\";i:1;}"#;
    let listed = r#"a:2:{s:19:\"wp_inactive_widgets\";a:6:{i:0;s:7:\"block-2\";i:1;s:7:\"block-3\";i:2;s:7:\"block-4\";i:3;s:7:\"block-5\";i:4;s:7:\"block-6\";i:5;s:6:\"text-2\";}s:13:\"array_version\";i:3;}"#;
    let rows = [
        option("widget_text", text),
        option("sidebars_widgets", listed),
    ];
    let content = with(&rows.concat().replace("9950,'sidebars", "9951,'sidebars"));
    let widgets = "/?rest_route=/wp/v2/widgets";
    let served = get(content, "administrator", widgets);
    assert!(String::from_utf8_lossy(&served.body).contains(r#""id":"text-2""#));
    assert_eq!(status(content, &format!("{widgets}&context=edit")), 501);
    let number = r#"a:2:{i:2;a:1:{s:7:\"content\";i:5;}s:12:\"_multiwidget\";i:1;}"#;
    let listed = r#"a:2:{s:19:\"wp_inactive_widgets\";a:1:{i:0;s:7:\"block-2\";}s:13:\"array_version\";i:3;}"#;
    let rows = [
        option("widget_block", number),
        option("sidebars_widgets", listed),
    ];
    let content = with(&rows.concat().replace("9950,'sidebars", "9951,'sidebars"));
    assert_eq!(status(content, widgets), 200);
    assert_eq!(status(content, &format!("{widgets}&context=edit")), 501);
    let unsalted = Oracle {
        salts: false,
        ..Oracle::over(retrieved(), false)
    };
    let case = RestCase {
        who: "administrator".into(),
        method: "GET".into(),
        target: format!("{widgets}/block-2&context=edit"),
        flags: String::new(),
        status: 0,
        headers: Vec::new(),
        body: Vec::new(),
        sent: Vec::new(),
    };
    assert_eq!(serve_with(&unsalted, &case).status, 501);
}

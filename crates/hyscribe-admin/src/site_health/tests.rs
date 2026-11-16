// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::{hs_admin_page, unhex};

use super::*;
use crate::tests::{BROWSER, Role, SITE, first_difference, request};

/// Which rows or lines of a section to compare, by label or name.
type Keep<'a> = &'a dyn Fn(&str) -> bool;

const ACCORDION: &str = "<div id=\"health-check-debug\" class=\"health-check-accordion\">\n\n";
const ACCORDION_END: &str = "\t\t\t</div>\n</div>\n\n<div class=\"clear\">";

/// The golden of `reference/site-health/golden.sh` for `role`.
fn golden(role: &str) -> String {
    let golden = include_str!("../../tests/golden/site-health.tsv");
    let row = golden
        .lines()
        .find(|line| line.starts_with(&format!("site-health.php?tab=debug\t{role}\t")))
        .unwrap();
    String::from_utf8(unhex(row.rsplit('\t').next().unwrap())).unwrap()
}

/// The Info tab of `role`'s site.
fn ours(role: &'static str) -> Option<String> {
    let canonical = format!("{SITE}/hs-admin/site-health?tab=debug");
    let path = "/hs-admin/site-health?tab=debug";
    info(&Role(role), &request(&canonical, path, &BROWSER))
}

/// `page` split into its frame, its copied text (unescaped) and its
/// accordion's sections by ID, in order.
fn parts(page: &str) -> (String, String, Vec<(String, String)>) {
    let (head, rest) = page.split_once("data-clipboard-text=\"").unwrap();
    let (clipboard, rest) = rest.split_once("\">").unwrap();
    let (middle, rest) = rest.split_once(ACCORDION).unwrap();
    let (sections, tail) = rest.split_once(ACCORDION_END).unwrap();
    let frame = format!("{head}data-clipboard-text=\"\">{middle}{ACCORDION}{ACCORDION_END}{tail}");
    let marker = "\t\t\t\t\t<h3 class=\"health-check-accordion-heading\">";
    let sections = sections
        .split(marker)
        .skip(1)
        .map(|section| {
            let id = section
                .split_once("id=\"health-check-section-")
                .and_then(|(_, rest)| rest.split_once('"'))
                .unwrap()
                .0;
            (id.to_string(), format!("{marker}{section}"))
        })
        .collect();
    let clipboard = clipboard
        .replace("&quot;", "\"")
        .replace("&#039;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&");
    (frame, clipboard, sections)
}

/// A section's markup with only the rows whose labels `keep` accepts.
fn rows(section: &str, keep: impl Fn(&str) -> bool) -> String {
    let (head, rest) = section.split_once("<tr>").unwrap();
    let (rows, tail) = rest.rsplit_once("</tr>").unwrap();
    let kept: String = format!("<tr>{rows}</tr>")
        .split_inclusive("</tr>")
        .filter(|row| {
            keep(
                row.split_once("\">")
                    .unwrap()
                    .1
                    .split_once("</th>")
                    .unwrap()
                    .0,
            )
        })
        .collect();
    format!("{head}{kept}{tail}")
}

/// The copied text's section `id`, with only the lines `keep` accepts.
fn copied(clipboard: &str, id: &str, keep: impl Fn(&str) -> bool) -> String {
    let start = format!("### {id} ###\n\n");
    let section = clipboard.split_once(&start).unwrap().1;
    let section = section.split_once("\n\n").unwrap().0;
    section
        .lines()
        .filter(|line| keep(line.split_once(':').unwrap().0))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The page around the sections, and every section and field HyScribe
/// keeps, byte for byte; the theme's folder (built into the binary) and
/// PHP's settings are left out.
#[test]
fn the_info_tab_is_php_s() {
    let php = hs_admin_page(&golden("administrator"));
    let page = ours("administrator").unwrap();
    let (php_frame, php_copied, php_sections) = parts(&php);
    let (frame, clipboard, sections) = parts(&page);
    assert!(
        frame == php_frame,
        "{}",
        first_difference(&php_frame, &frame)
    );
    let ids: Vec<&str> = sections.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "wp-core",
            "wp-active-theme",
            "wp-media",
            "hyscribe",
            "wp-database"
        ]
    );
    let php_ids: Vec<&str> = php_sections.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(
        php_ids,
        [
            "wp-core",
            "wp-paths-sizes",
            "wp-active-theme",
            "wp-media",
            "wp-server",
            "wp-database",
            "wp-constants",
            "wp-filesystem"
        ]
    );
    let section = |sections: &[(String, String)], id: &str| {
        sections.iter().find(|(s, _)| s == id).unwrap().1.clone()
    };
    let media = |label: &str| label == "Max effective file size";
    let theme = |label: &str| label != "Theme directory location";
    let database = |label: &str| {
        ![
            "Database Extension",
            "Client version",
            "Database host",
            "Max allowed packet size",
            "Max connections number",
        ]
        .contains(&label)
    };
    let kept: [(&str, Keep); 4] = [
        ("wp-core", &|_| true),
        ("wp-active-theme", &theme),
        ("wp-media", &media),
        ("wp-database", &database),
    ];
    for (id, keep) in kept {
        let php = rows(&section(&php_sections, id), keep);
        let ours = rows(&section(&sections, id), keep);
        assert!(ours == php, "{id}{}", first_difference(&php, &ours));
    }
    let lines: [(&str, Keep); 4] = [
        ("wp-core", &|_| true),
        ("wp-active-theme", &|name| name != "theme_path"),
        ("wp-media", &|name| name == "max_effective_size"),
        ("wp-database", &|name| name == "server_version"),
    ];
    for (id, keep) in lines {
        assert_eq!(
            copied(&clipboard, id, keep),
            copied(&php_copied, id, keep),
            "{id}"
        );
    }
    assert!(clipboard.starts_with("`\n### wp-core ###\n\nversion: 7.1.3\n"));
    assert!(clipboard.ends_with("server_version: 11.8.9-MariaDB-ubu2404\n\n`"));
    // The database's own row and the drop-ins, plugins and other themes,
    // of which HyScribe has none.
    assert!(
        section(&sections, "wp-database")
            .contains("<tr><th scope=\"row\">Database Extension</th><td>sqlx-mysql</td></tr>")
    );
    assert!(!clipboard.contains("private") && !clipboard.contains("wordpress\n"));
}

/// PHP refuses who may not view the checks; so does HyScribe.
#[test]
fn others_are_refused() {
    assert!(golden("editor").starts_with("403\t"));
    assert_eq!(ours("editor"), None);
}

/// HyScribe's own section in primary mode over HTTPS, and the site's
/// fields under other settings.
#[test]
fn the_hyscribe_section_is_its_own() {
    let page = ours("o35:administrator").unwrap();
    let (_, clipboard, sections) = parts(&page);
    let target = format!("{}-{SYSTEM}", std::env::consts::ARCH);
    let version = env!("CARGO_PKG_VERSION");
    assert_eq!(
        copied(&clipboard, "hyscribe", |_| true),
        format!(
            "version: {version}\nmode: primary\nbinary: /srv/hyscribe/hyscribe\nbuild_target: {target}\nupload_max_size: 2097152\nlogin_limit_address: 3\nlogin_limit_account: 20\nlogin_limit_window: 60"
        )
    );
    let hyscribe = &sections.iter().find(|(id, _)| id == "hyscribe").unwrap().1;
    assert!(hyscribe.contains("\t\t\t\t<p>HyScribe serves this site in place of PHP and a web server. The limits below are set in its <code>.env</code> file.</p>\t\t\t\t<table"));
    assert!(hyscribe.contains(&format!("<tr><th scope=\"row\">HyScribe version</th><td>{version}</td></tr><tr><th scope=\"row\">Mode</th><td>Primary</td></tr><tr><th scope=\"row\">Binary location</th><td>/srv/hyscribe/hyscribe</td></tr><tr><th scope=\"row\">Build target</th><td>{target}</td></tr><tr><th scope=\"row\">Maximum upload file size (UPLOAD_MAX_SIZE)</th><td>2 MB</td></tr><tr><th scope=\"row\">Failed logins allowed per address (LOGIN_LIMIT_ADDRESS)</th><td>3</td></tr><tr><th scope=\"row\">Failed logins allowed per account (LOGIN_LIMIT_ACCOUNT)</th><td>20</td></tr><tr><th scope=\"row\">Login limit window (LOGIN_LIMIT_WINDOW)</th><td>60 seconds</td></tr>")));
    assert_eq!(
        copied(&clipboard, "wp-core", |_| true),
        "version: 7.1.3\nsite_language: de_DE\nuser_language: de_DE\ntimezone: +01:00\npermalink: /%postname%/\nhttps_status: true\nmultisite: false\nuser_registration: 1\nblog_public: 0\ndefault_comment_status: closed\nenvironment_type: production\nuser_count: -1"
    );
    let core = &sections[0].1;
    assert!(core.contains("<tr><th scope=\"row\">Permalink structure</th><td>/%postname%/</td></tr><tr><th scope=\"row\">Is this site using HTTPS?</th><td>Yes</td></tr><tr><th scope=\"row\">Is this a multisite?</th><td>No</td></tr><tr><th scope=\"row\">Can anyone register on this site?</th><td>Yes</td></tr><tr><th scope=\"row\">Is this site discouraging search engines?</th><td>Yes</td></tr><tr><th scope=\"row\">Default comment status</th><td>Closed</td></tr>"));
    let shadow = ours("administrator").unwrap();
    assert!(shadow.contains("<tr><th scope=\"row\">Mode</th><td>Shadow (read only)</td></tr>"));
    assert!(shadow.contains("mode: shadow\n"));
    let zoned = ours("o35zone:administrator").unwrap();
    assert!(zoned.contains("timezone: Europe/Berlin\n"));
    assert!(zoned.contains("user_language: fr_FR\n"));
}

/// `wp_timezone_string()` of an offset: whole hours and minutes, signed.
#[test]
fn offsets_are_php_s() {
    let zone = |offset: &str| {
        let offset = Value::String(offset.as_bytes().to_vec()).to_float();
        offset_string(offset)
    };
    for (offset, text) in [
        ("0", "+00:00"),
        ("", "+00:00"),
        ("5.5", "+05:30"),
        ("5.75", "+05:45"),
        ("-0.5", "-00:30"),
        ("-3.75", "-03:45"),
        ("12", "+12:00"),
        ("-11", "-11:00"),
    ] {
        assert_eq!(zone(offset), text, "{offset}");
    }
}

/// `format()` and the accordion: counts, booleans, empty values and private
/// fields as `WP_Debug_Data` prints them.
#[test]
fn sections_print_as_php_s() {
    let sections = [
        Section::new("empty", "Empty", Vec::new()),
        Section {
            show_count: true,
            ..Section::new(
                "listed",
                "A & B",
                vec![
                    Field::new("a", "<A>", "x & y"),
                    Field::new("b", "B", "Yes").debug(Debug::Bool(true)),
                    Field::new("c", "C", "No").debug(Debug::Bool(false)),
                    Field::new("d", "D", "").debug(Debug::Text(String::new())),
                    Field::new("e", "E", "0"),
                    Field::new("f", "F", "secret").private(),
                ],
            )
        },
    ];
    assert_eq!(
        format(&sections),
        "`\n### listed (6) ###\n\na: x & y\nb: true\nc: false\nd: undefined\ne: 0\n\n`"
    );
    let html = accordion(&sections);
    assert!(!html.contains("health-check-section-empty"));
    assert!(html.contains("\t\t\t\t\t\tA &amp; B\t\t\t\t\t\t(6)\t\t\t\t\t</span>"));
    assert!(html.contains("<tr><th scope=\"row\">&lt;A&gt;</th><td>x &amp; y</td></tr>"));
    assert!(html.contains("<tr><th scope=\"row\">F</th><td>secret</td></tr>"));
}

// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::{hs_admin_page, unhex};

use super::*;
use crate::tests::{BROWSER, Role, SITE, first_difference, options, request};

pub(crate) const GOLDEN: &str = include_str!("../../tests/golden/appearance.tsv");

/// The rows of `reference/appearance/golden.sh` for `file`: the test site,
/// and the page or `None` for a refusal.
pub(crate) fn rows(file: &str) -> Vec<(Role, Option<String>)> {
    GOLDEN
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            (f[1] == file).then(|| {
                let role: &'static str = match f[0] {
                    "default" => Box::leak(f[2].to_string().into_boxed_str()),
                    variant => Box::leak(format!("{variant}:{}", f[2]).into_boxed_str()),
                };
                let recorded = String::from_utf8(unhex(f[3])).unwrap();
                let page = if let Some(refusal) = recorded.strip_prefix("403\t") {
                    assert!(refusal.contains("Sorry, you are not allowed to access this page."));
                    None
                } else {
                    Some(hs_admin_page(&recorded))
                };
                (Role(role), page)
            })
        })
        .collect()
}

/// `ours` against the recorded page, or the refusal.
pub(crate) fn check(role: &Role, ours: Answer, php: Option<&String>) -> Option<String> {
    match (ours, php) {
        (Answer::Page(ours), Some(php)) if &ours == php => None,
        (Answer::Page(ours), Some(php)) => {
            Some(format!("{}: {}", role.0, first_difference(php, &ours)))
        }
        (Answer::Denied("Sorry, you are not allowed to access this page."), None) => None,
        _ => Some(format!("{}: not the recorded answer", role.0)),
    }
}

#[test]
fn themes_are_php_s() {
    let message = auto_update_message(&options()["cron"], 1_794_500_000);
    let mut failures = Vec::new();
    let rows = rows("themes.php");
    let path = "/hs-admin/themes";
    let canonical = format!("{SITE}{path}");
    for (role, php) in &rows {
        let php = php
            .as_ref()
            .map(|php| php.replace("%auto-update%", &message));
        let ours = themes(role, &request(&canonical, path, &BROWSER)).unwrap();
        failures.extend(check(role, ours, php.as_ref()));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(rows.len(), 8);
}

#[test]
fn another_active_theme_is_not_ported() {
    let canonical = format!("{SITE}/hs-admin/themes");
    let request = request(&canonical, "/hs-admin/themes", &BROWSER);
    let page = themes(&Role("a5other:administrator"), &request);
    assert!(page.is_none());
}

/// The message PHP made at the time it recorded, and the other schedules.
#[test]
fn the_automatic_update_message_is_php_s() {
    let line = GOLDEN.lines().last().unwrap();
    let f: Vec<&str> = line.split('\t').collect();
    assert_eq!(f[0], "auto-update");
    let now = f[1].parse().unwrap();
    assert_eq!(auto_update_message(&options()["cron"], now), f[2]);
    assert_eq!(
        auto_update_message(b"a:1:{s:7:\"version\";i:2;}", now),
        "Automatic update not scheduled. There may be a problem with WP-Cron."
    );
    let cron = "a:2:{s:10:\"1794600000\";a:1:{s:16:\"wp_version_check\";a:1:{s:32:\"40cd750bba9870f18aada2478b24840a\";a:0:{}}}s:7:\"version\";i:2;}";
    assert_eq!(
        auto_update_message(cron.as_bytes(), 1_794_500_000),
        "Automatic update scheduled in 1 day."
    );
    assert_eq!(
        auto_update_message(cron.as_bytes(), 1_794_600_000),
        "Automatic update scheduled in 1 second."
    );
    assert_eq!(
        auto_update_message(cron.replace("40cd", "50cd").as_bytes(), now),
        "Automatic update not scheduled. There may be a problem with WP-Cron."
    );
    assert_eq!(
        auto_update_message(b"", now),
        "Automatic update not scheduled. There may be a problem with WP-Cron."
    );
}

/// The plugin folder of the PHP oracle's image.
#[test]
fn plugins_are_php_s() {
    let mut failures = Vec::new();
    let rows = rows("plugins.php");
    let canonical = format!("{SITE}/hs-admin/plugins");
    let request = request(&canonical, "/hs-admin/plugins", &BROWSER);
    for (role, php) in &rows {
        let ours = plugins(role, "/var/www/html/wp-content/plugins", &request);
        failures.extend(check(role, ours, php.as_ref()));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(rows.len(), 6);
}

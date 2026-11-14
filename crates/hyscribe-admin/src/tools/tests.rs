// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::{hs_admin_page, unhex};

use super::*;
use crate::appearance::tests::{check, rows};
use crate::page::Request;
use crate::tests::{BROWSER, Role, SITE, first_difference, request};

/// Every case of `reference/admin-page/golden.sh`.
#[test]
fn the_tools_screen_is_php_s() {
    let golden = include_str!("../../tests/golden/pages.tsv");
    let mut failures = Vec::new();
    for line in golden.lines().filter(|line| line.contains("\ttools.php\t")) {
        let f: Vec<&str> = line.split('\t').collect();
        let role: &'static str = Box::leak(f[0].to_string().into_boxed_str());
        let canonical = format!("{SITE}/hs-admin/tools");
        let ours = tools(
            &Role(role),
            &request(&canonical, "/hs-admin/tools", &BROWSER),
        )
        .unwrap();
        let php = hs_admin_page(&String::from_utf8(unhex(f[2])).unwrap());
        if ours != php {
            failures.push(format!("{role} {}{}", f[1], first_difference(&php, &ours)));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn import_and_export_are_php_s() {
    let mut failures = Vec::new();
    for (file, screen) in [
        (
            "import.php",
            (|role, request| import(role, request)) as fn(&Role, &Request) -> Answer,
        ),
        ("export.php", |role, request| export(role, request)),
    ] {
        let path = format!("/hs-admin/{}", file.trim_end_matches(".php"));
        let canonical = format!("{SITE}{path}");
        let rows = rows(file);
        assert_eq!(rows.len(), 5);
        for (role, php) in &rows {
            let ours = screen(role, &request(&canonical, &path, &BROWSER));
            failures.extend(check(role, ours, php.as_ref()));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The months with posts of a type, trashed ones too, newest first.
#[test]
fn export_dates_are_every_month_with_posts() {
    assert_eq!(
        date_options(&Role("s1media:administrator"), "attachment"),
        "<option value=\"2025-03\">March 2025</option><option value=\"2024-12\">December 2024</option>"
    );
    // Undated media alone lists no month.
    assert_eq!(
        date_options(&Role("a5zero:administrator"), "attachment"),
        ""
    );
}

/// `wp_dropdown_categories()` with `hide_empty` and flat, by ID.
#[test]
fn export_categories_are_those_with_posts() {
    assert_eq!(
        categories_dropdown(&Role("terms:administrator")),
        "<select  name='cat' id='cat' class='postform'>\n\t<option value='0' selected='selected'>All</option>\n\t<option class=\"level-0\" value=\"1\">Uncategorized</option>\n\t<option class=\"level-0\" value=\"401\">News</option>\n\t<option class=\"level-0\" value=\"403\">City</option>\n\t<option class=\"level-0\" value=\"404\">World</option>\n</select>\n"
    );
}

#[test]
fn personal_data_requests_are_php_s() {
    let (mut failures, mut pages) = (Vec::new(), 0);
    for (file, requests) in [
        ("export-personal-data.php", Requests::Export),
        ("erase-personal-data.php", Requests::Erase),
    ] {
        let path = format!("/hs-admin/{}", file.trim_end_matches(".php"));
        let canonical = format!("{SITE}{path}");
        let rows = rows(file);
        pages += rows.len();
        for (role, php) in &rows {
            let ours = privacy_requests(role, requests, &request(&canonical, &path, &BROWSER));
            failures.extend(check(role, ours.unwrap(), php.as_ref()));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(pages, 11);
}

#[test]
fn listed_requests_are_not_ported() {
    let path = "/hs-admin/export-personal-data";
    let canonical = format!("{SITE}{path}");
    let request = request(&canonical, path, &BROWSER);
    let page = privacy_requests(
        &Role("a5requests:administrator"),
        Requests::Export,
        &request,
    );
    assert!(page.is_none());
}

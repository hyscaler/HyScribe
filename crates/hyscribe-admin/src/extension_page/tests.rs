// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::{hs_admin_page, unhex};

use super::*;
use crate::menu::{Look, admin_menu, plugin_page_hook};
use crate::tests::{BROWSER, Role, SITE, first_difference, request};

/// `reference/plugins/golden.sh`'s views of the test extension's pages.
const GOLDEN: &str = include_str!("../../../hyscribe-http/tests/golden/extension-admin.tsv");

const SUBMIT: &str = "<p class=\"submit\"><input type=\"submit\" name=\"submit\" id=\"submit\" class=\"button button-primary\" value=\"Save Changes\"  /></p>";

/// What the test extension's page `slug` prints on the page `uri` asked.
fn printed(site: &Role, slug: &str, uri: &str) -> String {
    match slug {
        "hyscribe-test-top" => {
            "<div class=\"wrap\"><h1>HyScribe Test</h1><p id=\"ext-top\">ext-top</p></div>".into()
        }
        "hyscribe-test-sub" => {
            "<div class=\"wrap\"><h1>HyScribe Test Tools</h1><p id=\"ext-sub\">ext-sub</p></div>"
                .into()
        }
        _ => format!(
            "<div class=\"wrap\"><h1>HyScribe Test Settings</h1><form action=\"options.php\" method=\"post\">{}\
             <h2 id=\"wp-settings-section-hyscribe_test_main-1\">Main</h2>\n<p id=\"ext-section\">ext-section</p>\
             <table class=\"form-table\" role=\"presentation\"><tr><th scope=\"row\"><label for=\"ext_title\">Title</label></th>\
             <td><input name=\"ext_title\" type=\"text\" id=\"ext_title\" value=\"Hello World\" class=\"regular-text\" /></td></tr></table>\
             <table class=\"form-table\" role=\"presentation\"><tr class=\"ext-row\"><th scope=\"row\">More</th>\
             <td><span id=\"ext-more\">ext-more</span></td></tr></table>\
             <h2 id=\"wp-settings-section-hyscribe_test_empty-2\">Empty</h2>\n{SUBMIT}</form></div>",
            settings_fields(site, "hyscribe_test", uri)
        ),
    }
}

/// Each recorded view of the test extension's pages: PHP's page for one
/// who may reach it, with the extension's output where its callback
/// prints, and the refusal for one who may not.
#[test]
fn extension_pages_are_php_s() {
    let mut failures = Vec::new();
    for line in GOLDEN.lines() {
        let [variant, path, role, status, page] = line.split('\t').collect::<Vec<_>>()[..] else {
            panic!("{line}");
        };
        let site = Role(Box::leak(format!("{variant}:{role}").into_boxed_str()));
        let (file, query) = path.split_once('?').unwrap();
        let slug = query.strip_prefix("page=").unwrap();
        let uri = format!("/hs-admin/{}?{query}", file.trim_end_matches(".php"));
        let canonical = format!("{SITE}{uri}");
        let request = request(&canonical, &uri, &BROWSER);
        let screen = Screen {
            pagenow: file,
            typenow: "",
            taxonomy: "",
            parent_file: None,
            submenu_file: None,
            plugin_page: Some(slug),
        };
        let reached = admin_menu(&site, &screen, Look::Block).is_some();
        assert_eq!(reached, status == "200", "{role} {path}");
        if !reached {
            continue;
        }
        let hook = plugin_page_hook(&site, file, slug).unwrap();
        let title = match slug {
            "hyscribe-test-top" => "HyScribe Test",
            "hyscribe-test-sub" => "HyScribe Test Tools",
            _ => "HyScribe Test Settings",
        };
        let printed = printed(&site, slug, &uri);
        let titles = (title, printed.as_bytes());
        let ours = extension_page(&site, (file, slug, &hook), titles, &request).unwrap();
        let php = hs_admin_page(&String::from_utf8(unhex(page)).unwrap());
        if ours != php {
            failures.push(format!("{role} {path}: {}", first_difference(&php, &ours)));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

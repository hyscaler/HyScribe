// SPDX-License-Identifier: AGPL-3.0-or-later

//! Admin pages and the settings API (SDK 0.4), over the test extension.

use super::*;

const SUBMIT: &str = "<p class=\"submit\"><input type=\"submit\" name=\"submit\" id=\"submit\" class=\"button button-primary\" value=\"Save Changes\"  /></p>";

fn page(
    parent: Option<&str>,
    titles: (&str, &str),
    cap: &str,
    slug: &str,
    callback: u32,
) -> AdminPage {
    AdminPage {
        parent: parent.map(String::from),
        page_title: titles.0.into(),
        menu_title: titles.1.into(),
        capability: cap.into(),
        slug: slug.into(),
        callback,
        icon: String::new(),
        position: None,
    }
}

#[test]
fn pages_and_settings_are_registered_in_init() -> Result<(), Fault> {
    let loaded = load(&["ext"], Limits::default())?;
    let request = loaded.request(&TestSite::new());
    let top = "hyscribe-test-top";
    let pages = [
        page(None, ("HyScribe Test", "HyScribe Test"), "read", top, 36),
        page(
            Some(top),
            ("HyScribe Test Tools", "Tools"),
            "manage_options",
            "hyscribe-test-sub",
            37,
        ),
        page(
            Some("options-general.php"),
            ("HyScribe Test Settings", "HyScribe Test"),
            "manage_options",
            "hyscribe-test",
            38,
        ),
    ];
    assert_eq!(request.pages(), pages.map(|page| (0, page)).to_vec());
    let setting = Setting {
        group: "hyscribe_test".into(),
        option: "ext_title".into(),
    };
    assert_eq!(request.settings(), vec![setting]);
    // The sanitize callback is a filter on `sanitize_option_{$option}`.
    let sanitized = request.filter("sanitize_option_ext_title", text("Hi <b>you</b>!"), &[]);
    assert_eq!(sanitized, text("Hi byoub"));
    Ok(())
}

#[test]
fn a_page_prints_what_its_callback_answers() -> Result<(), Fault> {
    let loaded = load(&["ext"], Limits::default())?;
    let site = TestSite::new();
    site.set_option("ext_title", "Hello");
    let request = loaded.request(&site);
    let printed = |slug| String::from_utf8(request.page(0, slug)).unwrap();
    assert_eq!(
        printed("hyscribe-test-top"),
        "<div class=\"wrap\"><h1>HyScribe Test</h1><p id=\"ext-top\">ext-top</p></div>"
    );
    // Its sections and fields as `do_settings_sections()` prints them, each
    // titled section counting on in the request, as `wp_unique_id()` does.
    let settings = |n: u32| {
        format!(
            "<div class=\"wrap\"><h1>HyScribe Test Settings</h1><form action=\"options.php\" method=\"post\"><fields hyscribe_test>\
             <h2 id=\"wp-settings-section-hyscribe_test_main-{}\">Main</h2>\n<p id=\"ext-section\">ext-section</p>\
             <table class=\"form-table\" role=\"presentation\"><tr><th scope=\"row\"><label for=\"ext_title\">Title</label></th>\
             <td><input name=\"ext_title\" type=\"text\" id=\"ext_title\" value=\"Hello\" class=\"regular-text\" /></td></tr></table>\
             <table class=\"form-table\" role=\"presentation\"><tr class=\"ext-row\"><th scope=\"row\">More</th>\
             <td><span id=\"ext-more\">ext-more</span></td></tr></table>\
             <h2 id=\"wp-settings-section-hyscribe_test_empty-{}\">Empty</h2>\n{SUBMIT}</form></div>",
            n,
            n + 1
        )
    };
    assert_eq!(printed("hyscribe-test"), settings(1));
    assert_eq!(printed("hyscribe-test"), settings(3));
    assert_eq!(printed("nope"), "");
    assert!(request.page(1, "hyscribe-test").is_empty());
    // Without its fields the callback answers an error: nothing prints.
    site.set_option("ext_hooks", "fail");
    assert_eq!(printed("hyscribe-test"), "");
    assert_eq!(request.reports().len(), 1);
    Ok(())
}

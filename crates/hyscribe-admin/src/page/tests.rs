// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::menu::MenuUser;
use crate::tests::Role;

/// An administrator whose site and options a test sets.
#[derive(Default)]
struct Fake {
    color: Option<&'static [u8]>,
    preferences: Option<&'static [u8]>,
    tested: bool,
    home: &'static str,
    site: &'static str,
    /// The stored user settings.
    stored: Option<&'static [u8]>,
    /// The settings cookie.
    settings: Option<&'static str>,
}

impl MenuUser for Fake {
    fn can(&self, cap: &str) -> bool {
        Role("administrator").can(cap)
    }
    fn moderated_comments(&self) -> u64 {
        0
    }
    fn transient(&self, _: &str) -> Option<Vec<u8>> {
        None
    }
}

impl BarSite for Fake {
    fn home_url(&self) -> String {
        format!("{}/", self.home)
    }
    fn admin_url(&self, path: &str) -> String {
        format!("{}/hs-admin/{path}", self.site)
    }
    fn blogname(&self) -> String {
        "Tools".into()
    }
    fn logout_url(&self) -> String {
        String::new()
    }
    fn names(&self) -> (String, String) {
        ("admin".into(), "admin".into())
    }
    fn avatar(&self, _: i64) -> String {
        String::new()
    }
}

impl Urls for Fake {
    fn site_url(&self) -> String {
        self.site.into()
    }
    fn admin_url(&self, path: &str) -> String {
        BarSite::admin_url(self, path)
    }
    fn fill(&self, text: &str) -> String {
        text.into()
    }
}

impl PageSite for Fake {
    fn user_option(&self, _: &str) -> Option<Vec<u8>> {
        self.stored.map(<[u8]>::to_vec)
    }
    fn settings_cookie(&self) -> Option<String> {
        self.settings.map(str::to_string)
    }
    fn user_id(&self) -> u64 {
        7
    }
    fn nonce(&self, action: &str) -> String {
        format!("<{action}>")
    }
    fn admin_color(&self) -> Option<Vec<u8>> {
        self.color.map(<[u8]>::to_vec)
    }
    fn preferences(&self) -> Option<Vec<u8>> {
        self.preferences.map(<[u8]>::to_vec)
    }
    fn compression_tested(&self) -> bool {
        self.tested
    }
    fn login_url(&self) -> String {
        format!("{}/hs-login", self.site)
    }
    fn template(&self) -> String {
        "a b".into()
    }
    fn stylesheet(&self) -> String {
        "a b".into()
    }
    fn font_faces(&self, _: bool) -> Vec<u8> {
        Vec::new()
    }
}

fn site() -> Fake {
    Fake {
        home: "https://example.test",
        site: "https://example.test/wp",
        ..Fake::default()
    }
}

fn page<'a>(help: &'a [HelpTab<'a>], sidebar: &'a str, typenow: &'a str) -> Page<'a> {
    Page {
        screen: Screen {
            pagenow: "tools.php",
            typenow,
            taxonomy: "",
            parent_file: None,
            submenu_file: None,
        },
        id: "x'y",
        title: "Tools",
        help,
        sidebar,
        screen_options: "",
        styles: &[],
        scripts: &[],
        late_scripts: &[],
        script_data: &[],
        script_after: &[],
        late_styles: &[],
        admin_head: "",
        admin_footer: "",
        footer_scripts: "",
        content: "",
        canonical: "https://example.test/wp/hs-admin/tools",
        origin: "https://example.test",
        view: None,
    }
}

fn render(site: &Fake, page: &Page) -> String {
    admin_page(site, page, &Browser::default()).unwrap()
}

#[test]
fn a_scheme_without_a_stylesheet() {
    let site = Fake {
        color: Some(b"fresh"),
        ..site()
    };
    let html = render(&site, &page(&[], "", ""));
    assert!(!html.contains("id='colors-css'"));
    assert!(html.contains("\"base\":\"#a7aaad\",\"focus\":\"#72aee6\""));
    assert!(html.contains(" admin-color-fresh "));
}

/// No scheme, WordPress's `modern` or an unknown one shows Coffee (ADR 0008).
#[test]
fn coffee_is_the_default() {
    for color in [None, Some(&b"modern"[..]), Some(b"teal")] {
        let site = Fake { color, ..site() };
        let html = render(&site, &page(&[], "", ""));
        assert!(html.contains("/wp/hs-admin/css/colors/coffee/colors.min.css?ver="));
        assert!(html.contains("\"base\":\"#f3f2f1\""));
        assert!(html.contains(" admin-color-coffee "), "{color:?}");
    }
}

#[test]
fn the_title_the_head_script_and_the_body() {
    let html = render(&site(), &page(&[], "", "page"));
    assert!(html.contains("<title>Tools &#8212; HyScribe</title>"));
    assert!(html.contains("var ajaxurl = '/wp/hs-admin/admin-ajax',"));
    assert!(html.contains("pagenow = 'x\\'y',\n\ttypenow = 'page',"));
    assert!(html.contains("tools-php auto-fold admin-bar post-type-page branch-"));
    assert!(html.contains(" wp-theme-ab\">"));
    assert!(html.contains("\"url\":\"/wp/\",\"uid\":\"7\""));
    assert!(html.contains("\"secure\":\"1\""));
    assert!(html.contains("var userId = 7;"));
    assert!(html.contains("var serverData = \"\";\n\t\t\t\tvar userId = \"7\";"));
}

/// `get_user_setting()`'s `mfold` folds the menu and a truthy `unfold` stops
/// it folding by itself; the cookie wins over the stored settings.
#[test]
fn the_menu_folds_as_the_user_set_it() {
    for (cookie, classes) in [
        (None, "tools-php folded auto-fold admin-bar "),
        (Some("mfold=o&unfold=1"), "tools-php admin-bar "),
        (Some("unfold=0"), "tools-php auto-fold admin-bar "),
    ] {
        let site = Fake {
            settings: cookie,
            stored: Some(b"mfold=f"),
            ..site()
        };
        assert!(
            render(&site, &page(&[], "", "")).contains(classes),
            "{cookie:?}"
        );
    }
}

#[test]
fn stored_preferences() {
    for (stored, data) in [
        (&b"a:1:{s:1:\"a\";s:2:\"<b\";}"[..], "{\"a\":\"\\u003Cb\"}"),
        (b"plain", "\"plain\""),
        (b"a:2:{}", "false"),
    ] {
        let site = Fake {
            preferences: Some(stored),
            ..site()
        };
        let html = render(&site, &page(&[], "", ""));
        assert!(
            html.contains(&format!("var serverData = {data};")),
            "{data}"
        );
    }
}

#[test]
fn another_home_host_and_origin() {
    let other = Fake {
        home: "https://Other.test:8080",
        tested: true,
        ..site()
    };
    let html = render(
        &other,
        &Page {
            origin: "http://example.test",
            view: None,
            ..page(&[], "", "")
        },
    );
    assert!(html.contains("request = (function(){ var xhr"));
    assert!(html.contains("<div id=\"wp-auth-check-wrap\" class=\"hidden fallback\">"));
    assert!(!html.contains("wp-auth-check-form"));
    assert!(html.contains("</span></button>\n\t\t<div class=\"wp-auth-fallback\">"));
    assert!(!html.contains("compressionNonce"));
    let same = render(&site(), &page(&[], "", ""));
    assert!(same.contains("request = true;"));
    assert!(same.contains(
        "data-src=\"https://example.test/wp/hs-login?interim-login=1&#038;wp_lang=en_US\""
    ));
    assert!(same.contains("var compressionNonce = \"\\u003Cupdate_can_compress_scripts\\u003E\";"));
}

#[test]
fn help_tabs_and_their_sidebar() {
    let tabs = [
        HelpTab {
            id: "a\"",
            title: "<A>",
            content: "<p>a</p>",
        },
        HelpTab {
            id: "b",
            title: "B",
            content: "<p>b</p>",
        },
    ];
    let html = render(&site(), &page(&tabs, "", ""));
    assert!(html.contains("class=\"hidden no-sidebar\""));
    assert!(html.contains("<li id=\"tab-link-a&quot;\" class=\"active\">"));
    assert!(html.contains("<li id=\"tab-link-b\">"));
    assert!(html.contains("\t&lt;A&gt;\t"));
    assert!(html.contains("<div id=\"tab-panel-b\" class=\"help-tab-content\">"));
    assert!(!html.contains("contextual-help-sidebar"));
    assert!(html.contains("id=\"contextual-help-link\""));
    let bare = render(&site(), &page(&[], "", ""));
    assert!(!bare.contains("screen-meta-links"));
    assert!(bare.contains(
        "\t\t\t</div>\n\t\t\t\t</div>\n\t\t\n<div class=\"clear\"></div></div><!-- wpbody-content -->"
    ));
}

#[test]
fn a_denied_screen_has_no_page() {
    let page = page(&[], "", "");
    assert!(admin_page(&Role("subscriber"), &page, &Browser::default()).is_none());
}

#[test]
fn page_classes_and_escapes() {
    assert_eq!(admin_page_class("a.b..c/d_e-f"), "a-b-c-d_e-f");
    assert_eq!(
        esc_js("a'b\"c\\\\d\\e\r\nf<\\"),
        "a\\'b&quot;c\\\\de\\nf&lt;"
    );
    assert_eq!(
        esc_js("&#x27;&#X27;&#39;&#27;&#x39;&#X0027;x\\0\0y"),
        "\\'\\'\\'&amp;#27;&#x39;\\'x\\0\\0y"
    );
}

#[test]
fn versions_as_classes() {
    let classes = |v| version_classes(v);
    assert_eq!(classes("7.1.3"), ("7-1".into(), "7-1-3".into()));
    assert_eq!(classes("7.0.2-RC1"), ("7".into(), "7-0-2".into()));
    assert_eq!(classes("8"), ("8".into(), "8".into()));
    assert_eq!(classes("10.2.3"), ("10-2".into(), "10-2-3".into()));
}

#[test]
fn phones_and_ipv6_hosts() {
    let phone = Browser {
        user_agent: b"Android",
        mobile_hint: None,
    };
    let html = admin_page(&site(), &page(&[], "", ""), &phone).unwrap();
    assert!(html.contains(" locale-en-us mobile no-customize-support "));
    assert!(html.contains("class=\"nojq nojs mobile\""));
    let ipv6 = Fake {
        home: "http://[::2]",
        site: "http://[::1]",
        ..site()
    };
    assert!(render(&ipv6, &page(&[], "", "")).contains("request = (function(){"));
}

/// Screen options without help tabs get their link alone.
#[test]
fn screen_options_without_help() {
    let page = Page {
        screen_options: "<div id=\"screen-options-wrap\"></div>",
        ..page(&[], "", "")
    };
    let html = render(&site(), &page);
    assert!(html.contains("id=\"show-settings-link\""));
    assert!(!html.contains("id=\"contextual-help-link\""));
}

/// Chromium's version is the digits after the first `Chrome/` with any,
/// cast as PHP casts them.
#[test]
fn chromium_versions_are_php_s() {
    let version = |agent: &str| chromium_version(agent.as_bytes());
    assert_eq!(
        version("Mozilla/5.0 Chrome/137.0.0.0 Safari/537.36"),
        Some(137)
    );
    assert_eq!(version("Chrome/x Chrome/140.1"), Some(140));
    assert_eq!(version("Chrome/ Chrome/"), None);
    assert_eq!(version("Chrome/"), None);
    assert_eq!(version("Chromium/137"), None);
    assert_eq!(version(""), None);
    assert_eq!(version("Chrome/007"), Some(7));
    assert_eq!(version("Chrome/99999999999999999999999"), Some(i64::MAX));
}

/// A host of `localhost` or under it is a secure context, whatever its
/// port or case; a name only containing it is not.
#[test]
fn localhost_is_a_secure_context() {
    assert!(localhost("http://localhost:8081"));
    assert!(localhost("http://LOCALHOST"));
    assert!(localhost("http://site.localhost:80"));
    assert!(localhost("http://:localhost"));
    assert!(!localhost("http://localhost.example.test"));
    assert!(!localhost("http://127.0.0.1:8081"));
    assert!(!localhost("http://"));
}

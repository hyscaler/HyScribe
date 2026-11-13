// SPDX-License-Identifier: AGPL-3.0-or-later

//! The frame of every hs-admin screen, as `admin-header.php` and
//! `admin-footer.php` print it around the screen's own markup.

use hyscribe_kses::{esc_attr, esc_html, esc_url, parse_url};
use hyscribe_phpser::{Key, Maybe, Value, add_query_arg, maybe_unserialize};
use hyscribe_render::head::{admin_import_map, emoji_detection_script, json_hex_tag};
use hyscribe_render::post::sanitize_html_class;

use crate::admin_bar::{BarSite, Browser, admin_bar};
use crate::assets::{Assets, Urls, cookie_path};
use crate::menu::{Screen, admin_menu, commands};

/// `$wp_version`.
const VERSION: &str = include_str!("../../../wordpress/VERSION");

const CUSTOMIZE: &str = include_str!("page/customize.html");
const AUTH_CHECK: &str = include_str!("page/auth-check.html");
const CANONICAL: &str = include_str!("page/canonical.html");
const COMPRESSION: &str = include_str!("page/compression.html");

/// The interim login form, which only a login page on the same domain gets.
const FORM: &str = "\t\t\t<div id=\"wp-auth-check-form\" class=\"loading\" data-src=\"%login_src%\"></div>\n\t\t\t<div class=\"wp-auth-fallback\">";

/// What the frame asks of the site and its logged-in user.
pub trait PageSite: BarSite + Urls {
    /// `get_current_user_id()`.
    fn user_id(&self) -> u64;
    /// `wp_create_nonce( $action )` for the user.
    fn nonce(&self, action: &str) -> String;
    /// `get_user_option( 'admin_color' )`, as stored.
    fn admin_color(&self) -> Option<Vec<u8>>;
    /// The user's `{prefix}persisted_preferences` meta, as stored.
    fn preferences(&self) -> Option<Vec<u8>>;
    /// `get_site_option( 'can_compress_scripts' )` is set.
    fn compression_tested(&self) -> bool;
    /// `wp_login_url()`.
    fn login_url(&self) -> String;
    /// `get_template()`.
    fn template(&self) -> String;
    /// The theme's font faces as the admin prints them.
    fn font_faces(&self) -> Vec<u8>;
}

/// A help tab: ID, title and content.
pub struct HelpTab<'a> {
    /// The tab's ID, which its link and panel IDs carry.
    pub id: &'a str,
    /// Its title, as text.
    pub title: &'a str,
    /// Its content, as markup.
    pub content: &'a str,
}

/// A screen, as its file sets it up before `admin-header.php`.
pub struct Page<'a> {
    /// The screen the menu shows.
    pub screen: Screen<'a>,
    /// `$current_screen->id`.
    pub id: &'a str,
    /// `$title`.
    pub title: &'a str,
    /// The help tabs.
    pub help: &'a [HelpTab<'a>],
    /// The help sidebar, as markup.
    pub sidebar: &'a str,
    /// `WP_Screen::render_screen_options()`'s markup, empty for none.
    pub screen_options: &'a str,
    /// The styles the screen enqueues.
    pub styles: &'a [&'a str],
    /// The scripts the screen enqueues.
    pub scripts: &'a [&'a str],
    /// The scripts the screen's markup enqueues as it prints.
    pub late_scripts: &'a [&'a str],
    /// The screen's localized script data: handle and its `var` statement.
    pub script_data: &'a [(&'a str, String)],
    /// What the screen's `admin_footer` hooks print.
    pub admin_footer: &'a str,
    /// What the screen prints after the footer scripts.
    pub footer_scripts: &'a str,
    /// The screen's markup, between the header and the footer.
    pub content: &'a str,
    /// The requested URL without `wp_removable_query_args()`.
    pub canonical: &'a str,
    /// The scheme and host of the request.
    pub origin: &'a str,
    /// The admin bar's View link to what the screen is about: label and URL.
    pub view: Option<(&'a str, &'a str)>,
}

/// A color scheme `register_admin_color_schemes()` adds.
struct Scheme {
    name: &'static str,
    stylesheet: bool,
    /// Base, focus and current icon colors.
    icons: [&'static str; 3],
}

const SCHEMES: [Scheme; 9] = [
    Scheme {
        name: "modern",
        stylesheet: true,
        icons: ["#f3f1f1", "#fff", "#fff"],
    },
    Scheme {
        name: "fresh",
        stylesheet: false,
        icons: ["#a7aaad", "#72aee6", "#fff"],
    },
    Scheme {
        name: "light",
        stylesheet: true,
        icons: ["#999", "#ccc", "#ccc"],
    },
    Scheme {
        name: "blue",
        stylesheet: true,
        icons: ["#e5f8ff", "#fff", "#fff"],
    },
    Scheme {
        name: "midnight",
        stylesheet: true,
        icons: ["#f1f2f3", "#fff", "#fff"],
    },
    Scheme {
        name: "sunrise",
        stylesheet: true,
        icons: ["#f3f1f1", "#fff", "#fff"],
    },
    Scheme {
        name: "ectoplasm",
        stylesheet: true,
        icons: ["#ece6f6", "#fff", "#fff"],
    },
    Scheme {
        name: "ocean",
        stylesheet: true,
        icons: ["#f2fcff", "#fff", "#fff"],
    },
    Scheme {
        name: "coffee",
        stylesheet: true,
        icons: ["#f3f2f1", "#fff", "#fff"],
    },
];

/// `esc_js()`: special characters as entities, backslashes stripped, then
/// quotes, backslashes and NULs slashed and line breaks as `\n`.
fn esc_js(text: &str) -> String {
    let safe = esc_attr(text.as_bytes());
    let mut stripped = String::new();
    let mut chars = safe.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('0') => stripped.push('\0'),
                Some(next) => stripped.push(next),
                None => break,
            },
            c => stripped.push(c),
        }
    }
    let mut out = String::new();
    for c in apostrophes(&stripped).chars() {
        match c {
            '\r' => {}
            '\n' => out.push_str("\\n"),
            '\0' => out.push_str("\\0"),
            '\\' | '\'' | '"' => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out
}

/// `preg_replace( '/&#(x)?0*(?(1)27|39);?/i', "'", $text )`.
fn apostrophes(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("&#") {
        let (head, tail) = rest.split_at(at);
        out.push_str(head);
        let after = tail.get(2..).unwrap_or_default();
        let (hex, digits) = match after.strip_prefix(['x', 'X']) {
            Some(digits) => (true, digits),
            None => (false, after),
        };
        let digits = digits.trim_start_matches('0');
        let code = if hex { "27" } else { "39" };
        match digits.strip_prefix(code) {
            Some(end) => {
                out.push('\'');
                rest = end.strip_prefix(';').unwrap_or(end);
            }
            None => {
                out.push_str("&#");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

pub(crate) fn text(value: &str) -> Value {
    Value::String(value.as_bytes().to_vec())
}

pub(crate) fn object(entries: Vec<(&str, Value)>) -> Value {
    Value::Array(
        entries
            .into_iter()
            .map(|(key, value)| (Key::String(key.as_bytes().to_vec()), value))
            .collect(),
    )
}

pub(crate) fn json(value: &Value) -> String {
    String::from_utf8_lossy(&json_hex_tag(value)).into_owned()
}

/// The page `site` shows its user, or `None` when
/// `user_can_access_admin_page()` turns them away.
pub fn admin_page(site: &dyn PageSite, page: &Page, browser: &Browser) -> Option<String> {
    let menu = admin_menu(site, &page.screen)?;
    let urls: &dyn Urls = site;
    let color = site.admin_color().unwrap_or_default();
    let scheme = SCHEMES
        .iter()
        .find(|scheme| scheme.name.as_bytes() == color)
        .unwrap_or(&SCHEMES[0]);
    let mut assets = Assets::new(true);
    enqueue(&mut assets, site, page, scheme);
    for (handle, data) in page.script_data {
        assets.set_script_data(handle, "data", Value::String(data.as_bytes().to_vec()));
    }
    let mut out = head(site, page);
    out.extend(assets.print_admin_styles(urls));
    out.extend(site.font_faces());
    out.extend(assets.print_head_scripts(urls));
    let [base, focus, current] = scheme.icons;
    let icons = object(vec![(
        "icons",
        object(vec![
            ("base", text(base)),
            ("focus", text(focus)),
            ("current", text(current)),
        ]),
    )]);
    out.extend(format!("<script>var _wpColorScheme = {};</script>\n", json(&icons)).into_bytes());
    let canonical = esc_url(page.canonical.as_bytes());
    out.extend(
        CANONICAL
            .replace("%canonical%", &String::from_utf8_lossy(&canonical))
            .into_bytes(),
    );
    let mut html = String::from_utf8_lossy(&out).into_owned();
    let color = sanitize_html_class(&color, b"modern");
    html.push_str(&body(site, page, &String::from_utf8_lossy(&color), browser));
    html.push_str(&menu);
    html.push_str("<div id=\"wpcontent\">\n\n\t\t");
    html.push_str(&admin_bar(site, browser, page.view));
    html.push_str("<div id=\"wpbody\" role=\"main\">\n\n<div id=\"wpbody-content\">\n");
    html.push_str(&screen_meta(page));
    html.push_str(page.content);
    html.push_str(&footer(site, page, &mut assets));
    Some(html)
}

/// `admin_enqueue_scripts`: what every screen enqueues, with this
/// request's data.
fn enqueue(assets: &mut Assets, site: &dyn PageSite, page: &Page, scheme: &Scheme) {
    // `admin.php` enqueues the admin bar's, the screen its own, then
    // `admin-header.php` and its hooks the rest.
    assets.enqueue_style("admin-bar");
    for style in page.styles {
        assets.enqueue_style(style);
    }
    for style in [
        "colors",
        "wp-auth-check",
        "wp-commands",
        "wp-view-transitions-admin",
        "wp-emoji-styles",
    ] {
        assets.enqueue_style(style);
    }
    assets.enqueue_script("common");
    assets.enqueue_script("admin-bar");
    for script in page.scripts {
        assets.enqueue_script(script);
    }
    for script in [
        "utils",
        "svg-painter",
        "wp-auth-check",
        "wp-commands",
        "wp-core-commands",
    ] {
        assets.enqueue_script(script);
    }
    let list = |entries: Vec<String>| {
        Value::Array(
            entries
                .into_iter()
                .enumerate()
                .map(|(i, entry)| (Key::Int(i as i64), Value::String(entry.into_bytes())))
                .collect(),
        )
    };
    assets.set_style_data(
        "admin-bar",
        "after",
        list(vec![
            "@media print { #wpadminbar { display:none; } }".into(),
        ]),
    );
    assets.set_style_data("wp-emoji-styles", "after", list(vec![EMOJI_STYLES.into()]));
    if scheme.stylesheet {
        let url = BarSite::admin_url(site, &format!("css/colors/{}/colors.min.css", scheme.name));
        assets.set_color_scheme(Some(url));
    }
    let user_id = site.user_id();
    let secure = if Urls::site_url(site).starts_with("https://") {
        "1"
    } else {
        ""
    };
    let settings = object(vec![
        ("url", text(&cookie_path(&Urls::site_url(site)))),
        ("uid", text(&user_id.to_string())),
        ("time", text("%time%")),
        ("secure", text(secure)),
    ]);
    assets.set_script_data(
        "utils",
        "data",
        text(&format!("var userSettings = {};", json(&settings))),
    );
    let heartbeat = object(vec![("nonce", text(&site.nonce("heartbeat-nonce")))]);
    assets.set_script_data(
        "heartbeat",
        "data",
        text(&format!("var heartbeatSettings = {};", json(&heartbeat))),
    );
    assets.set_script_data(
        "wp-data",
        "after",
        list(vec![format!(
            "( function() {{\n\tvar userId = {user_id};\n\tvar storageKey = \"WP_DATA_USER_\" + userId;\n\twp.data\n\t\t.use( wp.data.plugins.persistence, {{ storageKey: storageKey }} );\n}} )();"
        )]),
    );
    let preferences = site
        .preferences()
        .map(|stored| match maybe_unserialize(&stored) {
            Maybe::Value(value) => value,
            Maybe::Plain(plain) => Value::String(plain.to_vec()),
            Maybe::Invalid(_) => Value::Bool(false),
        })
        .unwrap_or(text(""));
    assets.set_script_data(
        "wp-preferences",
        "after",
        list(vec![format!(
            "( function() {{\n\t\t\t\tvar serverData = {};\n\t\t\t\tvar userId = \"{user_id}\";\n\t\t\t\tvar persistenceLayer = wp.preferencesPersistence.__unstableCreatePersistenceLayer( serverData, userId );\n\t\t\t\tvar preferencesStore = wp.preferences.store;\n\t\t\t\twp.data.dispatch( preferencesStore ).setPersistenceLayer( persistenceLayer );\n\t\t\t}} ) ();",
            json(&preferences)
        )]),
    );
    let menu_commands = commands(site)
        .into_iter()
        .enumerate()
        .map(|(i, (label, url, name))| {
            (
                Key::Int(i as i64),
                object(vec![
                    ("label", text(&label)),
                    ("url", text(&url)),
                    ("name", text(&name)),
                ]),
            )
        })
        .collect();
    let palette = object(vec![
        ("is_network_admin", Value::Bool(false)),
        ("menu_commands", Value::Array(menu_commands)),
    ]);
    assets.set_script_data(
        "wp-core-commands",
        "after",
        list(vec![format!(
            "wp.coreCommands.initializeCommandPalette( {} );",
            json(&palette)
        )]),
    );
}

/// The emoji styles `wp_enqueue_emoji_styles()` adds inline.
const EMOJI_STYLES: &str = "\n\timg.wp-smiley, img.emoji {\n\t\tdisplay: inline !important;\n\t\tborder: none !important;\n\t\tbox-shadow: none !important;\n\t\theight: 1em !important;\n\t\twidth: 1em !important;\n\t\tmargin: 0 0.07em !important;\n\t\tvertical-align: -0.1em !important;\n\t\tbackground: none !important;\n\t\tpadding: 0 !important;\n\t}";

/// From the doctype to the head's first script, as `_wp_admin_html_begin()`
/// and `admin-header.php` print them.
fn head(site: &dyn PageSite, page: &Page) -> Vec<u8> {
    let blogname = site.blogname();
    let title = if blogname == page.title {
        format!("{} &#8212; WordPress", page.title)
    } else {
        format!("{} &lsaquo; {blogname} &#8212; WordPress", page.title)
    };
    // `admin_url( 'admin-ajax.php', 'relative' )`.
    let ajaxurl = cookie_path(&BarSite::admin_url(site, "admin-ajax.php"));
    let ajaxurl = ajaxurl.strip_suffix('/').unwrap_or(&ajaxurl);
    format!(
        "<!DOCTYPE html>\n<html class=\"wp-toolbar\"\n\tlang=\"en-US\">\n<head>\n<meta http-equiv=\"Content-Type\" content=\"text/html; charset=UTF-8\" />\n\t<title>{}</title>\n<script>\naddLoadEvent = function(func){{if(typeof jQuery!=='undefined')jQuery(function(){{func();}});else if(typeof wpOnload!=='function'){{wpOnload=func;}}else{{var oldonload=wpOnload;wpOnload=function(){{oldonload();func();}}}}}};\nvar ajaxurl = '{}',\n\tpagenow = '{}',\n\ttypenow = '{}',\n\tadminpage = '{}',\n\tthousandsSeparator = ',',\n\tdecimalPoint = '.',\n\tisRtl = 0;\n</script>\n",
        esc_html(title.as_bytes()),
        esc_js(ajaxurl),
        esc_js(page.id),
        esc_js(page.screen.typenow),
        esc_js(&admin_page_class(page.screen.pagenow)),
    )
    .into_bytes()
}

/// `$hook_suffix` as a class: what is not a letter, digit, `_` or `-`
/// becomes `-`.
fn admin_page_class(hook_suffix: &str) -> String {
    let mut class = String::new();
    let mut run = false;
    for c in hook_suffix.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            class.push(c);
            run = false;
        } else if !run {
            class.push('-');
            run = true;
        }
    }
    class
}

/// The `branch-` and `version-` classes: `(float)` of a version, and its
/// leading digits and dots, with dashes for dots.
fn version_classes(version: &str) -> (String, String) {
    let numeric = |dots: usize| {
        let mut seen = 0;
        version
            .chars()
            .take_while(|&c| {
                seen += usize::from(c == '.');
                c.is_ascii_digit() || (c == '.' && seen <= dots)
            })
            .collect::<String>()
    };
    let branch = numeric(1).parse::<f64>().unwrap_or_default().to_string();
    (
        branch.replace('.', "-"),
        numeric(usize::MAX).replace('.', "-"),
    )
}

/// From `</head>` to the menu.
fn body(site: &dyn PageSite, page: &Page, color: &str, browser: &Browser) -> String {
    let (branch, version) = version_classes(VERSION.trim());
    let mut classes = format!(
        "{} auto-fold admin-bar",
        admin_page_class(page.screen.pagenow)
    );
    if !page.screen.typenow.is_empty() {
        classes.push_str(&format!(" post-type-{}", page.screen.typenow));
    }
    if !page.screen.taxonomy.is_empty() {
        classes.push_str(&format!(" taxonomy-{}", page.screen.taxonomy));
    }
    let template = sanitize_html_class(site.template().as_bytes(), b"");
    let mobile = if browser.is_mobile() { " mobile" } else { "" };
    classes.push_str(&format!(
        " branch-{branch} version-{version} admin-color-{color} locale-en-us{mobile} no-customize-support svg wp-theme-{}",
        String::from_utf8_lossy(&template)
    ));
    let mut out = format!(
        "\t<meta name=\"viewport\" content=\"width=device-width,initial-scale=1.0\"></head>\n<body class=\"wp-admin wp-core-ui no-js {}\">\n<script>\n\tdocument.body.className = document.body.className.replace('no-js','js');\n</script>\n\n",
        esc_attr(classes.as_bytes())
    );
    if site.can("customize") {
        let host = |url: String| {
            parse_url(url.as_bytes())
                .and_then(|parts| parts.host)
                .map(|host| host.to_ascii_lowercase())
        };
        let request = if host(BarSite::admin_url(site, "")) == host(BarSite::home_url(site)) {
            "request = true;"
        } else {
            "request = (function(){ var xhr = new XMLHttpRequest(); return ('withCredentials' in xhr); })();"
        };
        out.push_str(&CUSTOMIZE.replace("request = true;", request));
    }
    out.push_str("\n<div id=\"wpwrap\">\n\n");
    out
}

/// `WP_Screen::render_screen_meta()`, with the screen's own options.
fn screen_meta(page: &Page) -> String {
    let mut out = format!(
        "\t\t<div id=\"screen-meta\" class=\"metabox-prefs\">\n\n\t\t\t<div id=\"contextual-help-wrap\" class=\"{}\" tabindex=\"-1\" aria-label=\"Contextual Help Tab\">\n\t\t\t\t<div id=\"contextual-help-back\"></div>\n\t\t\t\t<div id=\"contextual-help-columns\">\n\t\t\t\t\t<div class=\"contextual-help-tabs\">\n\t\t\t\t\t\t<ul>\n\t\t\t\t\t\t",
        if page.sidebar.is_empty() {
            "hidden no-sidebar"
        } else {
            "hidden"
        }
    );
    let mut class = " class=\"active\"";
    for tab in page.help {
        let link = esc_attr(format!("tab-link-{}", tab.id).as_bytes());
        let panel = format!("tab-panel-{}", tab.id);
        out.push_str(&format!(
            "\n\t\t\t\t\t\t\t<li id=\"{link}\"{class}>\n\t\t\t\t\t\t\t\t<a href=\"{}\" aria-controls=\"{}\">\n\t\t\t\t\t\t\t\t\t{}\t\t\t\t\t\t\t\t</a>\n\t\t\t\t\t\t\t</li>\n\t\t\t\t\t\t\t",
            String::from_utf8_lossy(&esc_url(format!("#{panel}").as_bytes())),
            esc_attr(panel.as_bytes()),
            esc_html(tab.title.as_bytes())
        ));
        class = "";
    }
    out.push_str("\t\t\t\t\t\t</ul>\n\t\t\t\t\t</div>\n\n\t\t\t\t\t");
    if !page.sidebar.is_empty() {
        out.push_str(&format!(
            "\t\t\t\t\t<div class=\"contextual-help-sidebar\">\n\t\t\t\t\t\t{}\t\t\t\t\t</div>\n\t\t\t\t\t",
            page.sidebar
        ));
    }
    out.push_str("\n\t\t\t\t\t<div class=\"contextual-help-tabs-wrap\">\n\t\t\t\t\t\t");
    let mut classes = "help-tab-content active";
    for tab in page.help {
        out.push_str(&format!(
            "\n\t\t\t\t\t\t\t<div id=\"{}\" class=\"{classes}\">\n\t\t\t\t\t\t\t\t{}\t\t\t\t\t\t\t</div>\n\t\t\t\t\t\t\t",
            esc_attr(format!("tab-panel-{}", tab.id).as_bytes()),
            tab.content
        ));
        classes = "help-tab-content";
    }
    out.push_str("\t\t\t\t\t</div>\n\t\t\t\t</div>\n\t\t\t</div>\n\t\t");
    out.push_str(page.screen_options);
    out.push_str("\t\t</div>\n\t\t");
    if page.help.is_empty() && page.screen_options.is_empty() {
        return out;
    }
    out.push_str("\t\t<div id=\"screen-meta-links\">\n\t\t");
    if !page.screen_options.is_empty() {
        out.push_str("\t\t\t<div id=\"screen-options-link-wrap\" class=\"hide-if-no-js screen-meta-toggle\">\n\t\t\t<button type=\"button\" id=\"show-settings-link\" class=\"button button-compact show-settings\" aria-controls=\"screen-options-wrap\" aria-expanded=\"false\">Screen Options</button>\n\t\t\t</div>\n\t\t\t");
    }
    if !page.help.is_empty() {
        out.push_str("\t\t\t<div id=\"contextual-help-link-wrap\" class=\"hide-if-no-js screen-meta-toggle\">\n\t\t\t<button type=\"button\" id=\"contextual-help-link\" class=\"button button-compact show-settings\" aria-controls=\"contextual-help-wrap\" aria-expanded=\"false\">Help</button>\n\t\t\t</div>\n\t\t");
    }
    out.push_str("\t\t</div>\n\t\t");
    out
}

/// From the end of the screen's markup to `</html>`.
fn footer(site: &dyn PageSite, page: &Page, assets: &mut Assets) -> String {
    let urls: &dyn Urls = site;
    let mut out = format!(
        "\n<div class=\"clear\"></div></div><!-- wpbody-content -->\n<div class=\"clear\"></div></div><!-- wpbody -->\n<div class=\"clear\"></div></div><!-- wpcontent -->\n\n<div id=\"wpfooter\" role=\"contentinfo\">\n\t\t<p id=\"footer-left\" class=\"alignleft\">\n\t\t<span id=\"footer-thankyou\">Thank you for creating with <a href=\"https://wordpress.org/\">WordPress</a>.</span>\t</p>\n\t<p id=\"footer-upgrade\" class=\"alignright\">\n\t\tVersion {}\t</p>\n\t<div class=\"clear\"></div>\n</div>\n",
        VERSION.trim()
    );
    out.push_str(page.admin_footer);
    let login = site.login_url();
    let same_domain = login.starts_with(page.origin);
    let src = String::from_utf8_lossy(&add_query_arg(
        &[(b"interim-login", Some(b"1")), (b"wp_lang", Some(b"en_US"))],
        login.as_bytes(),
    ))
    .into_owned();
    let mut check = AUTH_CHECK.to_string();
    if !same_domain {
        check = check
            .replace("class=\"hidden\"", "class=\"hidden fallback\"")
            .replace(FORM, "\t\t<div class=\"wp-auth-fallback\">");
    }
    let check = check
        .replace(
            "%login_src%",
            &String::from_utf8_lossy(&esc_url(src.as_bytes())),
        )
        .replace(
            "%login%",
            &String::from_utf8_lossy(&esc_url(login.as_bytes())),
        );
    out.push_str(&check);
    out.push('\t');
    let siteurl = urls.site_url();
    for script in page.late_scripts {
        assets.enqueue_script(script);
    }
    let imports = assets.module_dependencies();
    let mut tail = admin_import_map(&siteurl, &imports);
    tail.extend(assets.print_footer_scripts(urls));
    tail.extend(emoji_detection_script(&siteurl));
    tail.extend(page.footer_scripts.as_bytes());
    out.push_str(&String::from_utf8_lossy(&tail));
    if !site.compression_tested() {
        let nonce = json(&text(&site.nonce("update_can_compress_scripts")));
        out.push_str(&COMPRESSION.replace("%nonce%", &nonce));
        out.push('\t');
    }
    out.push_str("\n<div class=\"clear\"></div></div><!-- wpwrap -->\n<script>if(typeof wpOnload==='function')wpOnload();</script>\n</body>\n</html>\n");
    out
}

#[cfg(test)]
mod tests;

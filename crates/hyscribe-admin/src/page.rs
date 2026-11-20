// SPDX-License-Identifier: AGPL-3.0-or-later

//! The frame of every hs-admin screen, as `admin-header.php` and
//! `admin-footer.php` print it around the screen's own markup.

use hyscribe_kses::{esc_attr, esc_html, esc_url, parse_url};
use hyscribe_phpser::{
    Key, Maybe, Value, add_query_arg, is_empty_string, maybe_unserialize, parse_str, stripslashes,
    urlencode,
};
use hyscribe_render::head::{admin_script_modules, emoji_detection_script, json_hex_tag};
use hyscribe_render::kses::wp_kses_post;
use hyscribe_render::post::sanitize_html_class;
use hyscribe_render::theme::Theme;

use crate::admin_bar::{BarSite, Browser, admin_bar};
use crate::assets::{Assets, Urls, cookie_path, site_icon_tags};
use crate::menu::{Look, Screen, admin_menu, clean_links, commands};

/// `wp_removable_query_args()`: what the canonical admin URL leaves out.
pub const REMOVABLE: [&[u8]; 28] = [
    b"activate",
    b"activated",
    b"admin_email_remind_later",
    b"approved",
    b"core-major-auto-updates-saved",
    b"deactivate",
    b"delete_count",
    b"deleted",
    b"disabled",
    b"doing_wp_cron",
    b"enabled",
    b"error",
    b"hotkeys_highlight_first",
    b"hotkeys_highlight_last",
    b"ids",
    b"locked",
    b"message",
    b"same",
    b"saved",
    b"settings-updated",
    b"skipped",
    b"spammed",
    b"trashed",
    b"unspammed",
    b"untrashed",
    b"update",
    b"updated",
    b"wp-post-new-reload",
];

/// `wp_get_admin_notice( $message )` with an `id` (none when empty), its
/// classes after `notice`, and the message in a paragraph when `wrap`.
pub(crate) fn admin_notice(message: &str, id: &str, classes: &str, wrap: bool) -> String {
    let id = if id.is_empty() {
        String::new()
    } else {
        format!("id=\"{id}\" ")
    };
    if wrap {
        format!("<div {id}class=\"notice {classes}\"><p>{message}</p></div>")
    } else {
        format!("<div {id}class=\"notice {classes}\">{message}</div>")
    }
}

/// `wp_admin_notice()`: the notice through `wp_kses_post()`, then with
/// `nonce` in the place of each `_wpnonce` argument's `NONCE`.
pub(crate) fn wp_admin_notice(
    message: &str,
    id: &str,
    classes: &str,
    wrap: bool,
    nonce: &str,
) -> String {
    let notice = admin_notice(message, id, classes, wrap);
    let notice = String::from_utf8_lossy(&wp_kses_post(notice.as_bytes(), b"")).into_owned();
    notice.replace(
        &format!(";_wpnonce={}", crate::list_table::NONCE),
        &format!(";_wpnonce={nonce}"),
    )
}

/// `$wp_version`.
pub(crate) const VERSION: &str = include_str!("../../../wordpress/VERSION");

const CUSTOMIZE: &str = include_str!("page/customize.html");
const AUTH_CHECK: &str = include_str!("page/auth-check.html");
const CANONICAL: &str = include_str!("page/canonical.html");
const COMPRESSION: &str = include_str!("page/compression.html");
const RELOAD: &str = "\t<script>\n\t\tif ( typeof performance !== 'undefined' && performance.navigation && performance.navigation.type === 2 ) {\n\t\t\tdocument.location.reload( true );\n\t\t}\n\t</script>\n\t";

/// The interim login form, which only a login page on the same domain gets.
const FORM: &str = "\t\t\t<div id=\"wp-auth-check-form\" class=\"loading\" data-src=\"%login_src%\"></div>\n\t\t\t<div class=\"wp-auth-fallback\">";

/// What the frame asks of the site and its logged-in user.
pub trait PageSite: BarSite + Urls {
    /// `get_user_option( $name )`, as stored.
    fn user_option(&self, name: &str) -> Option<Vec<u8>>;
    /// The settings cookie, `wp-settings-{$user_id}`, as sent.
    fn settings_cookie(&self) -> Option<String>;
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
    /// `get_stylesheet()`.
    fn stylesheet(&self) -> String;
    /// The embedded theme the site's stylesheet names, or the default
    /// one where it names a theme the site does not have.
    fn active_theme(&self) -> &Theme;
    /// The bundled theme's font faces as the admin prints them, those of
    /// its `theme.json` only `with_theme`, as its style variations' always.
    fn font_faces(&self, with_theme: bool) -> Vec<u8>;
}

/// Whether the site's active theme is one of the embedded block themes, and
/// not one it does not have, which is classic.
pub(crate) fn has_block_theme(site: &dyn PageSite) -> bool {
    site.stylesheet() == site.active_theme().slug()
}

/// The Customize link of the Appearance menu, `None` while the active theme
/// is an embedded block theme: any other is one the site does not have.
fn customize_link(site: &dyn PageSite, page: &Page) -> Option<String> {
    if has_block_theme(site) {
        return None;
    }
    // `$_SERVER['REQUEST_URI']` without `wp_removable_query_args()`.
    let uri = page.canonical.strip_prefix(page.origin).unwrap_or_default();
    let url = format!(
        "customize.php?return={}",
        String::from_utf8_lossy(&urlencode(&stripslashes(uri.as_bytes())))
    );
    Some(String::from_utf8_lossy(&esc_url(url.as_bytes())).into_owned())
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
    /// The screen's inline scripts after a handle's: handle and script.
    pub script_after: &'a [(&'a str, String)],
    /// The styles the screen's markup enqueues as it prints.
    pub late_styles: &'a [&'a str],
    /// What the screen's `admin_head` hooks print.
    pub admin_head: &'a str,
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

/// The request a screen answers.
pub struct Request<'a> {
    /// The requested URL without `wp_removable_query_args()`.
    pub canonical: &'a str,
    /// The scheme and host of the request.
    pub origin: &'a str,
    /// `$_SERVER['REQUEST_URI']`, slashed.
    pub uri: &'a str,
    /// The browser that asks.
    pub browser: &'a Browser<'a>,
}

/// `$_GET` of `query`: each name and its string; `None` if one is not a
/// string or not text.
pub fn query_strings(query: &str) -> Option<Vec<(String, String)>> {
    let parsed = parse_str(query.as_bytes());
    let mut args = Vec::new();
    for (key, value) in parsed.as_array().unwrap_or_default() {
        let Value::String(value) = value else {
            return None;
        };
        let key = match key {
            Key::String(key) => String::from_utf8(key.clone()).ok()?,
            Key::Int(key) => key.to_string(),
        };
        args.push((key, String::from_utf8(value.clone()).ok()?));
    }
    Some(args)
}

/// A color scheme `register_admin_color_schemes()` adds.
pub(crate) struct Scheme {
    pub(crate) name: &'static str,
    /// Its name in the color picker.
    pub(crate) label: &'static str,
    pub(crate) stylesheet: bool,
    /// Base, focus and current icon colors.
    pub(crate) icons: [&'static str; 3],
    /// The colors the picker shows.
    pub(crate) palette: &'static [&'static str],
}

pub(crate) const SCHEMES: [Scheme; 9] = [
    Scheme {
        name: "modern",
        label: "Default",
        stylesheet: true,
        icons: ["#f3f1f1", "#fff", "#fff"],
        palette: &["#1e1e1e", "#3858e9", "#7b90ff"],
    },
    Scheme {
        name: "fresh",
        label: "Fresh",
        stylesheet: false,
        icons: ["#a7aaad", "#72aee6", "#fff"],
        palette: &["#1d2327", "#2c3338", "#2271b1", "#72aee6"],
    },
    Scheme {
        name: "light",
        label: "Light",
        stylesheet: true,
        icons: ["#999", "#ccc", "#ccc"],
        palette: &["#e5e5e5", "#6a6a6a", "#c64606", "#007cba"],
    },
    Scheme {
        name: "blue",
        label: "Blue",
        stylesheet: true,
        icons: ["#e5f8ff", "#fff", "#fff"],
        palette: &["#183751", "#245278", "#437aa8", "#e1a948"],
    },
    Scheme {
        name: "midnight",
        label: "Midnight",
        stylesheet: true,
        icons: ["#f1f2f3", "#fff", "#fff"],
        palette: &["#232a2e", "#333c42", "#69a8bb", "#cf4339"],
    },
    Scheme {
        name: "sunrise",
        label: "Sunrise",
        stylesheet: true,
        icons: ["#f3f1f1", "#fff", "#fff"],
        palette: &["#6f2724", "#8a312d", "#ad631e", "#ccaf0b"],
    },
    Scheme {
        name: "ectoplasm",
        label: "Ectoplasm",
        stylesheet: true,
        icons: ["#ece6f6", "#fff", "#fff"],
        palette: &["#392751", "#4a3369", "#646c3e", "#d46f15"],
    },
    Scheme {
        name: "ocean",
        label: "Ocean",
        stylesheet: true,
        icons: ["#f2fcff", "#fff", "#fff"],
        palette: &["#2b3f44", "#39535a", "#567958", "#aa9d88"],
    },
    Scheme {
        name: "coffee",
        label: "Coffee",
        stylesheet: true,
        icons: ["#f3f2f1", "#fff", "#fff"],
        palette: &["#382e27", "#5c4c40", "#916745", "#9ea476"],
    },
];

/// `esc_js()`: special characters as entities, backslashes stripped, then
/// quotes, backslashes and NULs slashed and line breaks as `\n`.
pub(crate) fn esc_js(text: &str) -> String {
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
    admin_page_with(site, page, browser, &Extras::default())
}

/// What a screen adds to [`Page`]: the block editor's.
#[derive(Default)]
pub struct Extras<'a> {
    /// What the `admin_body_class` filter returns.
    pub body_class: &'a str,
    /// `$current_screen->is_block_editor()`.
    pub block_editor: bool,
    /// The script modules the screen enqueues.
    pub modules: &'a [String],
    /// The screen's inline scripts and styles, added to what is registered.
    pub assets: Option<&'a dyn Fn(&mut Assets)>,
    /// What the screen prints after the footer scripts, in place of the
    /// emoji detection script it removes.
    pub footer: Option<&'a str>,
}

/// [`admin_page`] with what `extras` adds.
pub fn admin_page_with(
    site: &dyn PageSite,
    page: &Page,
    browser: &Browser,
    extras: &Extras,
) -> Option<String> {
    let customize = customize_link(site, page);
    // A missing child of an embedded theme has its parent's `theme.json`.
    let parent = site.template() == site.active_theme().slug();
    let look = customize
        .as_deref()
        .map_or(Look::Block, |url| Look::Classic(url, parent));
    let menu = admin_menu(site, &page.screen, look)?;
    let urls: &dyn Urls = site;
    let color = site.admin_color().unwrap_or_default();
    let scheme = SCHEMES
        .iter()
        .find(|scheme| scheme.name.as_bytes() == color)
        .unwrap_or(&SCHEMES[0]);
    let mut assets = Assets::new(true);
    enqueue(&mut assets, site, page, (scheme, browser), look);
    for (handle, data) in page.script_data {
        assets.set_script_data(handle, "data", Value::String(data.as_bytes().to_vec()));
    }
    for (handle, script) in page.script_after {
        let after = Value::Array(vec![(Key::Int(0), text(script))]);
        assets.set_script_data(handle, "after", after);
    }
    if let Some(more) = extras.assets {
        more(&mut assets);
    }
    let mut out = head(site, page);
    out.extend(assets.print_admin_styles(urls));
    match look {
        Look::Block => out.extend(site.font_faces(true)),
        Look::Classic(_, true) => out.extend(site.font_faces(false)),
        Look::Classic(_, false) => {}
    }
    // `wp_page_reload_on_back_button_js()`, on the editors' screens.
    if matches!(page.screen.pagenow, "post.php" | "post-new.php") {
        out.extend(RELOAD.as_bytes());
    }
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
    out.push(b'\t');
    out.extend(site_icon_tags(&|size| site.site_icon(size)).into_bytes());
    let mut html = String::from_utf8_lossy(&out).into_owned();
    let color = sanitize_html_class(&color, b"modern");
    html.push_str(&body(
        site,
        (page, extras),
        &String::from_utf8_lossy(&color),
        browser,
    ));
    html.push_str(&menu);
    html.push_str("<div id=\"wpcontent\">\n\n\t\t");
    html.push_str(&admin_bar(site, browser, page.view));
    html.push_str("<div id=\"wpbody\" role=\"main\">\n\n<div id=\"wpbody-content\">\n");
    html.push_str(&screen_meta(page));
    html.push_str(page.content);
    html.push_str(&footer(site, (page, extras), &mut assets));
    Some(clean_links(&html))
}

/// `admin_enqueue_scripts`: what every screen enqueues, with this
/// request's data.
fn enqueue(
    assets: &mut Assets,
    site: &dyn PageSite,
    page: &Page,
    (scheme, browser): (&Scheme, &Browser),
    look: Look,
) {
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
    let secure_site = Urls::site_url(site).starts_with("https://");
    // `wp_set_client_side_media_processing_flag()`, `admin_init`'s: in a
    // secure context, and under isolation for Chromium 137 and later.
    if secure_site || localhost(page.origin) {
        let mut flags = vec!["window.__clientSideMediaProcessing = true;".to_string()];
        if chromium_version(browser.user_agent).is_some_and(|version| version >= 137) {
            flags.push("window.__documentIsolationPolicy = true;".into());
        }
        assets.set_script_data("wp-block-editor", "before", list(flags));
    }
    let user_id = site.user_id();
    let secure = if secure_site { "1" } else { "" };
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
    let menu_commands = commands(site, look)
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

/// The host of `origin` is `localhost` or under it, a secure context.
fn localhost(origin: &str) -> bool {
    let host = origin.rsplit("//").next().unwrap_or_default();
    let host = host.to_ascii_lowercase();
    // `strtok( $host, ':' )` skips the separators before its token.
    let host = host.split(':').find(|part| !part.is_empty());
    let host = host.unwrap_or_default();
    host == "localhost" || host.ends_with(".localhost")
}

/// `wp_get_chromium_major_version()`: the digits after the first `Chrome/`
/// that has any, as PHP casts them (saturating).
pub fn chromium_version(user_agent: &[u8]) -> Option<i64> {
    let mut starts = user_agent.windows(7).enumerate();
    starts.find_map(|(at, window)| {
        let digits = user_agent.get(at + 7..).filter(|_| window == b"Chrome/")?;
        let digits = digits.iter().take_while(|byte| byte.is_ascii_digit());
        let version = digits.clone().fold(0i64, |version, digit| {
            version
                .saturating_mul(10)
                .saturating_add(i64::from(digit - b'0'))
        });
        (digits.count() > 0).then_some(version)
    })
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
fn body(
    site: &dyn PageSite,
    (page, extras): (&Page, &Extras),
    color: &str,
    browser: &Browser,
) -> String {
    let (branch, version) = version_classes(VERSION.trim());
    let cookie = site.settings_cookie();
    let setting = |name| user_setting(site, cookie.as_deref(), name);
    // The filter's classes come first.
    let mut classes = format!(
        "{} {}",
        extras.body_class,
        admin_page_class(page.screen.pagenow)
    );
    classes = classes.trim_start().to_string();
    // `WP_Site_Health::admin_body_class()` comes first, as a filter's class.
    if page.id == "site-health" {
        classes.insert_str(0, "site-health ");
    }
    if setting("mfold").as_deref() == Some("f") {
        classes.push_str(" folded");
    }
    if setting("unfold").is_none_or(|unfold| is_empty_string(unfold.as_bytes())) {
        classes.push_str(" auto-fold");
    }
    classes.push_str(" admin-bar");
    if !page.screen.typenow.is_empty() {
        classes.push_str(&format!(" post-type-{}", page.screen.typenow));
    }
    if !page.screen.taxonomy.is_empty() {
        classes.push_str(&format!(" taxonomy-{}", page.screen.taxonomy));
    }
    let template = site.template();
    let mobile = if browser.is_mobile() { " mobile" } else { "" };
    classes.push_str(&format!(
        " branch-{branch} version-{version} admin-color-{color} locale-en-us{mobile} no-customize-support svg{} wp-theme-{}",
        if extras.block_editor { " block-editor-page wp-embed-responsive" } else { "" },
        String::from_utf8_lossy(&sanitize_html_class(template.as_bytes(), b""))
    ));
    let stylesheet = site.stylesheet();
    if stylesheet != template {
        classes.push_str(&format!(
            " wp-child-theme-{}",
            String::from_utf8_lossy(&sanitize_html_class(stylesheet.as_bytes(), b""))
        ));
    }
    let mut out = format!(
        "<meta name=\"viewport\" content=\"width=device-width,initial-scale=1.0\">{}</head>\n<body class=\"wp-admin wp-core-ui no-js {}\">\n<script>\n\tdocument.body.className = document.body.className.replace('no-js','js');\n</script>\n\n",
        page.admin_head,
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
fn footer(site: &dyn PageSite, (page, extras): (&Page, &Extras), assets: &mut Assets) -> String {
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
    let mut tail = admin_script_modules(&siteurl, &imports, extras.modules);
    for style in page.late_styles {
        assets.enqueue_style(style);
    }
    tail.extend(assets.print_admin_styles(urls));
    tail.extend(assets.print_footer_scripts(urls));
    match extras.footer {
        Some(footer) => tail.extend(footer.as_bytes()),
        None => tail.extend(emoji_detection_script(&siteurl)),
    }
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

/// `get_user_setting( $name )`: from the settings cookie when sent, else
/// the stored `user-settings`; none when the cleaned cookie has no `=`.
pub(crate) fn user_setting(
    site: &dyn PageSite,
    cookie: Option<&str>,
    name: &str,
) -> Option<String> {
    let stored = match cookie {
        Some(cookie) => {
            let kept: String = cookie
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '=' | '&' | '_' | '-'))
                .collect();
            kept.find('=')
                .filter(|at| *at > 0)
                .map(|_| kept.into_bytes())
        }
        None => site.user_option("user-settings"),
    }?;
    let settings = hyscribe_phpser::parse_str(&stored);
    let setting = settings.get_str(name).and_then(Value::as_bytes)?;
    Some(String::from_utf8_lossy(setting).into_owned())
}

#[cfg(test)]
mod tests;

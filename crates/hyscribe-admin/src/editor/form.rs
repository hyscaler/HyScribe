// SPDX-License-Identifier: AGPL-3.0-or-later

//! `edit-form-blocks.php`: the editor's own settings, its initial edits, its
//! markup with the meta boxes area, and the styles and scripts it enqueues.

use hyscribe_model::Post;
use hyscribe_phpser::{Key, Value};

use super::EditorSite;
use super::settings::Settings;
use crate::admin_bar::{BarSite, Browser};
use crate::list_table::wp_referer_field;
use crate::page::{object, text};

/// The post being edited, as the view found it.
pub struct Editing<'a> {
    /// The post, an auto-draft for `post-new.php`.
    pub post: &'a Post,
    /// The edit lock the view set, `time:user`.
    pub lock: &'a str,
    /// The browser that asks.
    pub browser: &'a Browser<'a>,
    /// `$_SERVER['REQUEST_URI']`, slashed.
    pub uri: &'a str,
}

/// `get_allowed_block_template_part_areas()`.
const AREAS: [(&str, &str, &str, &str, &str); 4] = [
    (
        "uncategorized",
        "General",
        "General templates often perform a specific role like displaying post content, and are not tied to any particular area.",
        "layout",
        "div",
    ),
    (
        "header",
        "Header",
        "The Header template defines a page area that typically contains a title, logo, and main navigation.",
        "header",
        "header",
    ),
    (
        "footer",
        "Footer",
        "The Footer template defines a page area that typically contains site credits, social links, or any other combination of blocks.",
        "footer",
        "footer",
    ),
    (
        "navigation-overlay",
        "Navigation Overlay",
        "The Navigation Overlay template defines an overlay area that typically contains navigation links and can be toggled open and closed.",
        "navigation-overlay",
        "div",
    ),
];

/// `user_can_richedit()` in `browser`, for a user whose `rich_editing` is
/// as stored; the browser as `vars.php` tells it.
fn user_can_richedit(rich_editing: Option<Vec<u8>>, browser: &Browser) -> bool {
    if rich_editing.as_deref() != Some(b"true") {
        return false;
    }
    let agent = String::from_utf8_lossy(browser.user_agent);
    let has = |name: &str| agent.contains(name);
    if has("Lynx") {
        false
    } else if has("Edg") || has("Chrome") {
        true
    } else if has("Safari") {
        let webkit = agent.split_once("AppleWebKit/").map_or(0, |(_, rest)| {
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            digits.parse::<u32>().unwrap_or(0)
        });
        !browser.is_mobile() || webkit >= 534
    } else if (has("MSIE") || has("Trident")) && has("Win") {
        has("Trident/7.0;")
    } else if has("Gecko") {
        true
    } else {
        has("Opera") && !browser.is_mobile()
    }
}

/// The settings `edit-form-blocks.php` gives `get_block_editor_settings()`.
pub fn custom_settings(site: &dyn EditorSite, editing: &Editing) -> Settings {
    let post = editing.post;
    let templates = site.theme().page_templates(&post.post_type);
    let templates = match templates.is_empty() {
        true => Value::Array(Vec::new()),
        false => {
            let named = templates
                .iter()
                .map(|(slug, title)| (slug.as_str(), text(title)));
            let all = [("", text("Default template"))].into_iter().chain(named);
            object(all.collect())
        }
    };
    let id = post.id;
    let lock = object(vec![
        ("isLocked", Value::Bool(false)),
        ("activePostLock", text(editing.lock)),
    ]);
    let utils = object(vec![
        ("nonce", text(&site.nonce(&format!("lock-post_{id}")))),
        (
            "unlockNonce",
            text(&site.nonce(&format!("update-post_{id}"))),
        ),
        ("ajaxUrl", text(&BarSite::admin_url(site, "admin-ajax.php"))),
    ]);
    let custom_fields = site.user_option("enable_custom_fields");
    let areas = AREAS
        .iter()
        .zip(0..)
        .map(|((area, label, description, icon, tag), i)| {
            let area = object(vec![
                ("area", text(area)),
                ("label", text(label)),
                ("description", text(description)),
                ("icon", text(icon)),
                ("area_tag", text(tag)),
            ]);
            (Key::Int(i), area)
        });
    let rich = user_can_richedit(site.user_option("rich_editing"), editing.browser);
    let mut settings = Settings::default();
    for (key, value) in [
        ("availableTemplates", templates),
        ("disablePostFormats", Value::Bool(false)),
        ("titlePlaceholder", text("Add title")),
        ("bodyPlaceholder", text("Type / to choose a block")),
        ("autosaveInterval", Value::Int(60)),
        ("richEditingEnabled", Value::Bool(rich)),
        ("postLock", lock),
        ("postLockUtils", utils),
        ("supportsLayout", Value::Bool(true)),
        ("supportsTemplateMode", Value::Bool(true)),
        (
            "enableCustomFields",
            Value::Bool(!Value::String(custom_fields.unwrap_or_default()).is_empty()),
        ),
        (
            "__experimentalAdditionalBlockPatterns",
            Value::Array(Vec::new()),
        ),
        (
            "__experimentalAdditionalBlockPatternCategories",
            Value::Array(Vec::new()),
        ),
        ("defaultTemplatePartAreas", Value::Array(areas.collect())),
    ] {
        settings.set(key, value);
    }
    settings
}

/// The styles the editor's page enqueues before `admin-header.php`, but
/// `global-styles-css-custom-properties`: the global stylesheet's variables
/// inline, after `imgareaselect`.
pub const STYLES: [&str; 6] = [
    "media-views",
    "imgareaselect",
    "editor-buttons",
    "wp-edit-post",
    "wp-block-directory",
    "wp-format-library",
];

/// The scripts the editor's page enqueues before `admin-header.php`.
pub const SCRIPTS: [&str; 14] = [
    "heartbeat",
    "wp-edit-post",
    "media-editor",
    "media-audiovideo",
    "mce-view",
    "image-edit",
    "editor",
    "quicktags",
    "wplink",
    "jquery-ui-autocomplete",
    "thickbox",
    "media-upload",
    "wp-block-directory",
    "wp-format-library",
];

/// The inline scripts the page adds after registered scripts', by handle.
pub fn scripts_after(settings: &Value, edits: &Value, post: &Post) -> Vec<(&'static str, String)> {
    let json = |value: &Value| {
        String::from_utf8_lossy(&hyscribe_render::head::json_hex_tag(value)).into_owned()
    };
    let sources = super::blocks::bindings_sources();
    let blocks = [
        format!(
            "wp.blocks.setCategories( {} );",
            json(&super::blocks::block_categories())
        ),
        format!(
            "wp.blocks.unstable__bootstrapServerSideBlockDefinitions({});",
            json(&super::blocks::server_block_settings())
        ),
        format!(
            "for ( const source of {} ) {{ wp.blocks.registerBlockBindingsSource( source ); }}",
            json(&sources)
        ),
    ];
    vec![
        ("wp-blocks", blocks.join("\n")),
        (
            "heartbeat",
            "jQuery( function() {\n\t\twp.heartbeat.interval( 10 );\n\t} );".into(),
        ),
        (
            "wp-edit-post",
            format!(
                "( function() {{\n\twindow._wpLoadBlockEditor = new Promise( function( resolve ) {{\n\t\twp.domReady( function() {{\n\t\t\tresolve( wp.editPost.initializeEditor( 'editor', \"{}\", {}, {}, {} ) );\n\t\t}} );\n\t}} );\n}} )();",
                post.post_type,
                post.id,
                json(settings),
                json(edits)
            ),
        ),
    ]
}

/// The initial edits of a new post: its empty title, content and excerpt,
/// as far as its type supports them.
pub fn initial_edits(post: &Post) -> Value {
    if post.status != "auto-draft" {
        return Value::Array(Vec::new());
    }
    let mut edits = vec![("title", text("")), ("content", text(""))];
    if post.post_type == "post" {
        edits.push(("excerpt", text("")));
    }
    object(edits)
}

/// The page's `block-editor` markup, with the meta boxes area of a post
/// with no meta boxes, for a request without a referer.
pub fn body(site: &dyn EditorSite, editing: &Editing) -> String {
    let post = editing.post;
    let new = post.status == "auto-draft";
    let kind = if post.post_type == "page" {
        "Page"
    } else {
        "Post"
    };
    let title = format!("{} {kind}", if new { "Add" } else { "Edit" });
    let id = post.id;
    let referer_field = wp_referer_field(editing.uri);
    let auto_draft = if new {
        "<input type='hidden' id='auto_draft' name='auto_draft' value='1' />"
    } else {
        ""
    };
    let nonce = |action: &str| site.nonce(action);
    let location = |name: &str| {
        format!(
            "\t\t\t<form class=\"metabox-location-{name}\" onsubmit=\"return false;\">\n\t\t\t<div id=\"poststuff\" class=\"sidebar-open\">\n\t\t\t\t<div id=\"postbox-container-2\" class=\"postbox-container\">\n\t\t\t\t\t<div id=\"{name}-sortables\" class=\"meta-box-sortables\"></div>\t\t\t\t</div>\n\t\t\t</div>\n\t\t</form>\n"
        )
    };
    format!(
        "<div class=\"block-editor\">\n\t<h1 class=\"screen-reader-text hide-if-no-js\">{title}</h1>\n\t<div id=\"editor\" class=\"block-editor__container hide-if-no-js\"></div>\n\t<div id=\"metaboxes\" class=\"hidden\">\n\t\t\t<form class=\"metabox-base-form\">\n\t<input type=\"hidden\" id=\"_wpnonce\" name=\"_wpnonce\" value=\"{}\" />{referer_field}\t<input type=\"hidden\" id=\"user-id\" name=\"user_ID\" value=\"{}\" />\n\t<input type=\"hidden\" id=\"hiddenaction\" name=\"action\" value=\"editpost\" />\n\t<input type=\"hidden\" id=\"originalaction\" name=\"originalaction\" value=\"editpost\" />\n\t<input type=\"hidden\" id=\"post_type\" name=\"post_type\" value=\"{}\" />\n\t<input type=\"hidden\" id=\"original_post_status\" name=\"original_post_status\" value=\"{}\" />\n\t<input type=\"hidden\" id=\"referredby\" name=\"referredby\" value=\"\" />\n\n\t<input type=\"hidden\" name=\"_wp_original_http_referer\" value=\"\" />{auto_draft}<input type='hidden' id='post_ID' name='post_ID' value='{id}' /><input type=\"hidden\" id=\"meta-box-order-nonce\" name=\"meta-box-order-nonce\" value=\"{}\" /><input type=\"hidden\" id=\"closedpostboxesnonce\" name=\"closedpostboxesnonce\" value=\"{}\" /><input type=\"hidden\" id=\"samplepermalinknonce\" name=\"samplepermalinknonce\" value=\"{}\" />\t</form>\n\t<form id=\"toggle-custom-fields-form\" method=\"post\" action=\"{}\">\n\t\t<input type=\"hidden\" id=\"toggle-custom-fields-nonce\" name=\"toggle-custom-fields-nonce\" value=\"{}\" />{referer_field}\t\t<input type=\"hidden\" name=\"action\" value=\"toggle-custom-fields\" />\n\t</form>\n{}{}{}\t\t\t</div>\n\n\t\t<div class=\"wrap hide-if-js block-editor-no-js\">\n\t\t<h1 class=\"wp-heading-inline\">{title}</h1>\n\t\t<div class=\"notice notice-error\"><p>The block editor requires JavaScript. Please enable JavaScript in your browser settings, or install the <a href=\"{}?action=install-plugin&amp;plugin=classic-editor&amp;_wpnonce={}\">Classic Editor plugin</a>.</p></div>\t</div>\n</div>\n\n",
        nonce(&format!("update-post_{id}")),
        site.user_id(),
        post.post_type,
        post.status,
        nonce("meta-box-order"),
        nonce("closedpostboxes"),
        nonce("samplepermalink"),
        BarSite::admin_url(site, "post.php"),
        nonce("toggle-custom-fields"),
        location("side"),
        location("normal"),
        location("advanced"),
        BarSite::admin_url(site, "update.php"),
        nonce("install-plugin_classic-editor"),
    )
}

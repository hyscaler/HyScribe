// SPDX-License-Identifier: AGPL-3.0-or-later

//! `edit-form-blocks.php`: the editor's own settings, its initial edits, its
//! markup with the meta boxes area, and the styles and scripts it enqueues.

use hyscribe_model::Post;
use hyscribe_phpser::{Key, Value};
use hyscribe_render::template_types::PART_AREAS;

use super::EditorSite;
use super::settings::Settings;
use crate::admin_bar::{BarSite, Browser};
use crate::list_table::wp_referer_field;
use crate::page::{json, object, text};

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

/// `get_allowed_block_template_part_areas()` as PHP lists it.
pub(crate) fn part_areas() -> Value {
    let areas = PART_AREAS.iter().zip(0..).map(|(area, i)| {
        let area = object(vec![
            ("area", text(area.area)),
            ("label", text(area.label)),
            ("description", text(area.description)),
            ("icon", text(area.icon)),
            ("area_tag", text(area.tag)),
        ]);
        (Key::Int(i), area)
    });
    Value::Array(areas.collect())
}

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

/// `user_can_richedit()` for the user of `site`.
pub(crate) fn rich_editing(site: &dyn EditorSite, browser: &Browser) -> bool {
    user_can_richedit(site.user_option("rich_editing"), browser)
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
    let rich = rich_editing(site, editing.browser);
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
        ("defaultTemplatePartAreas", part_areas()),
    ] {
        settings.set(key, value);
    }
    settings
}

/// The styles the editor's page enqueues before `admin-header.php`; the
/// last, an alias, holds the global stylesheet's variables.
pub const STYLES: [&str; 7] = [
    "media-views",
    "imgareaselect",
    "editor-buttons",
    "wp-edit-post",
    "wp-block-directory",
    "wp-format-library",
    "global-styles-css-custom-properties",
];

/// The scripts the editor's page enqueues before `admin-header.php`.
pub const SCRIPTS: [&str; 15] = [
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
    "wp-block-styles",
    "wp-block-directory",
    "wp-format-library",
];

/// The editors' inline scripts of `wp-blocks`: the block categories, the
/// server's block definitions and the bindings sources.
pub(crate) fn block_scripts() -> [String; 3] {
    [
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
            json(&super::blocks::bindings_sources())
        ),
    ]
}

/// The inline scripts the page adds after registered scripts', by handle.
pub fn scripts_after(settings: &Value, edits: &Value, post: &Post) -> Vec<(&'static str, String)> {
    vec![
        ("wp-blocks", block_scripts().join("\n")),
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
        // `the_block_editor_meta_boxes()`: no meta boxes, and its nonces'
        // refresh.
        ("wp-edit-post", META_BOXES.into()),
        ("heartbeat", NONCES.into()),
    ]
}

/// The meta boxes of each location, none.
const META_BOXES: &str = "window._wpLoadBlockEditor.then( function() {\n\t\twp.data.dispatch( 'core/edit-post' ).setAvailableMetaBoxesPerLocation( {\"side\":[],\"normal\":[],\"advanced\":[]} );\n\t} );";

/// The meta box loader's nonces, refreshed with the heartbeat.
const NONCES: &str = "( function( $ ) {\n\t\tvar check, timeout;\n\n\t\tfunction schedule() {\n\t\t\tcheck = false;\n\t\t\twindow.clearTimeout( timeout );\n\t\t\ttimeout = window.setTimeout( function() { check = true; }, 300000 );\n\t\t}\n\n\t\t$( document ).on( 'heartbeat-send.wp-refresh-nonces', function( e, data ) {\n\t\t\tvar post_id, $authCheck = $( '#wp-auth-check-wrap' );\n\n\t\t\tif ( check || ( $authCheck.length && ! $authCheck.hasClass( 'hidden' ) ) ) {\n\t\t\t\tif ( ( post_id = $( '#post_ID' ).val() ) && $( '#_wpnonce' ).val() ) {\n\t\t\t\t\tdata['wp-refresh-metabox-loader-nonces'] = {\n\t\t\t\t\t\tpost_id: post_id\n\t\t\t\t\t};\n\t\t\t\t}\n\t\t\t}\n\t\t}).on( 'heartbeat-tick.wp-refresh-nonces', function( e, data ) {\n\t\t\tvar nonces = data['wp-refresh-metabox-loader-nonces'];\n\n\t\t\tif ( nonces ) {\n\t\t\t\tif ( nonces.replace ) {\n\t\t\t\t\tif ( nonces.replace.metabox_loader_nonce && window._wpMetaBoxUrl && wp.url ) {\n\t\t\t\t\t\twindow._wpMetaBoxUrl= wp.url.addQueryArgs( window._wpMetaBoxUrl, { 'meta-box-loader-nonce': nonces.replace.metabox_loader_nonce } );\n\t\t\t\t\t}\n\n\t\t\t\t\tif ( nonces.replace._wpnonce ) {\n\t\t\t\t\t\t$( '#_wpnonce' ).val( nonces.replace._wpnonce );\n\t\t\t\t\t}\n\t\t\t\t}\n\t\t\t}\n\t\t}).ready( function() {\n\t\t\tschedule();\n\t\t});\n\t} )( jQuery );";

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
    // `wp_original_referer_field()`, which a draft has none of.
    let original = match post.status.as_str() {
        "draft" => "",
        _ => "<input type=\"hidden\" name=\"_wp_original_http_referer\" value=\"\" />",
    };
    let location = |name: &str| {
        format!(
            "\t\t\t<form class=\"metabox-location-{name}\" onsubmit=\"return false;\">\n\t\t\t<div id=\"poststuff\" class=\"sidebar-open\">\n\t\t\t\t<div id=\"postbox-container-2\" class=\"postbox-container\">\n\t\t\t\t\t<div id=\"{name}-sortables\" class=\"meta-box-sortables\"></div>\t\t\t\t</div>\n\t\t\t</div>\n\t\t</form>\n"
        )
    };
    format!(
        "<div class=\"block-editor\">\n\t<h1 class=\"screen-reader-text hide-if-no-js\">{title}</h1>\n\t<div id=\"editor\" class=\"block-editor__container hide-if-no-js\"></div>\n\t<div id=\"metaboxes\" class=\"hidden\">\n\t\t\t<form class=\"metabox-base-form\">\n\t<input type=\"hidden\" id=\"_wpnonce\" name=\"_wpnonce\" value=\"{}\" />{referer_field}\t<input type=\"hidden\" id=\"user-id\" name=\"user_ID\" value=\"{}\" />\n\t<input type=\"hidden\" id=\"hiddenaction\" name=\"action\" value=\"editpost\" />\n\t<input type=\"hidden\" id=\"originalaction\" name=\"originalaction\" value=\"editpost\" />\n\t<input type=\"hidden\" id=\"post_type\" name=\"post_type\" value=\"{}\" />\n\t<input type=\"hidden\" id=\"original_post_status\" name=\"original_post_status\" value=\"{}\" />\n\t<input type=\"hidden\" id=\"referredby\" name=\"referredby\" value=\"\" />\n\n\t{original}{auto_draft}<input type='hidden' id='post_ID' name='post_ID' value='{id}' /><input type=\"hidden\" id=\"meta-box-order-nonce\" name=\"meta-box-order-nonce\" value=\"{}\" /><input type=\"hidden\" id=\"closedpostboxesnonce\" name=\"closedpostboxesnonce\" value=\"{}\" /><input type=\"hidden\" id=\"samplepermalinknonce\" name=\"samplepermalinknonce\" value=\"{}\" />\t</form>\n\t<form id=\"toggle-custom-fields-form\" method=\"post\" action=\"{}\">\n\t\t<input type=\"hidden\" id=\"toggle-custom-fields-nonce\" name=\"toggle-custom-fields-nonce\" value=\"{}\" />{referer_field}\t\t<input type=\"hidden\" name=\"action\" value=\"toggle-custom-fields\" />\n\t</form>\n{}{}{}\t\t\t</div>\n\n\t\t<div class=\"wrap hide-if-js block-editor-no-js\">\n\t\t<h1 class=\"wp-heading-inline\">{title}</h1>\n\t\t<div class=\"notice notice-error\"><p>The block editor requires JavaScript. Please enable JavaScript in your browser settings, or install the <a href=\"{}?action=install-plugin&amp;plugin=classic-editor&amp;_wpnonce={}\">Classic Editor plugin</a>.</p></div>\t</div>\n</div>\n\n",
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

#[cfg(test)]
mod tests;

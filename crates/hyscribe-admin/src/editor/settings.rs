// SPDX-License-Identifier: AGPL-3.0-or-later

//! `get_block_editor_settings()` for the post editor, over the bundled
//! block theme and what a default install hooks to it.

use std::sync::OnceLock;

use hyscribe_model::Post;
use hyscribe_phpser::{Key, Value, float_to_string, json_decode};
use hyscribe_render::blocks::{Block, parse_blocks};

use super::EditorSite;
use super::blocks::{bindings_supported_attributes, block_categories};
use crate::page::{object, text};

const MIME_TYPES: &[u8] = include_bytes!("../../../../wordpress/wp-includes/mime-types.json");
const DEFAULT_EDITOR_STYLES: &str = "css/dist/block-editor/default-editor-styles.css";
const EDITOR_STYLE: &str = "assets/css/editor-style.css";

/// An ordered PHP array, whose keys keep their place when set again.
#[derive(Default)]
pub struct Settings(pub Vec<(Key, Value)>);

impl Settings {
    /// `$settings[ $key ] = $value`.
    pub fn set(&mut self, key: &str, value: Value) {
        let key = Key::String(key.as_bytes().to_vec());
        match self.0.iter_mut().find(|(k, _)| *k == key) {
            Some((_, old)) => *old = value,
            None => self.0.push((key, value)),
        }
    }
}

fn list(items: impl IntoIterator<Item = Value>) -> Value {
    Value::Array(
        items
            .into_iter()
            .zip(0..)
            .map(|(v, i)| (Key::Int(i), v))
            .collect(),
    )
}

/// `get_allowed_mime_types()`: htm, html and js only with `unfiltered_html`.
fn allowed_mime_types(unfiltered: bool) -> Value {
    static TYPES: OnceLock<Value> = OnceLock::new();
    let types = TYPES.get_or_init(|| json_decode(MIME_TYPES).unwrap_or_default());
    let mut out = Vec::new();
    for (key, mime) in types.as_array().unwrap_or_default() {
        out.push((key.clone(), mime.clone()));
        let after: &[u8] = &key.to_php_string();
        let added = match after {
            b"css" => ("htm|html", "text/html"),
            b"rtf" => ("js", "application/javascript"),
            _ => continue,
        };
        if unfiltered {
            out.push((Key::String(added.0.as_bytes().to_vec()), text(added.1)));
        }
    }
    Value::Array(out)
}

/// `get_default_block_editor_settings()` with the bundled theme's supports.
fn defaults(site: &dyn EditorSite) -> Settings {
    let option = |name: &str| site.option(name).unwrap_or_default();
    let names = [
        ("thumbnail", "Thumbnail"),
        ("medium", "Medium"),
        ("large", "Large"),
        ("full", "Full Size"),
    ];
    let sizes = names
        .iter()
        .map(|(slug, name)| object(vec![("slug", text(slug)), ("name", text(name))]));
    let size = option("image_default_size");
    let default_size = names
        .iter()
        .find(|(slug, _)| slug.as_bytes() == size)
        .map_or("large", |(slug, _)| slug);
    // `wp_get_registered_image_subsizes()` of the sizes offered.
    let dimensions = names.iter().take(3).map(|(slug, _)| {
        let int = |name: String| Value::String(option(&name)).to_int();
        let size = object(vec![
            ("width", Value::Int(int(format!("{slug}_size_w")))),
            ("height", Value::Int(int(format!("{slug}_size_h")))),
            (
                "crop",
                Value::Bool(!Value::String(option(&format!("{slug}_crop"))).is_empty()),
            ),
        ]);
        (Key::String(slug.as_bytes().to_vec()), size)
    });
    let css = hyscribe_assets::wp_includes(DEFAULT_EDITOR_STYLES)
        .unwrap_or_default()
        .1;
    let upload = if site.can("upload_files") {
        2_097_152
    } else {
        0
    };
    let mut settings = Settings::default();
    for (key, value) in [
        ("alignWide", Value::Bool(false)),
        ("allowedBlockTypes", Value::Bool(true)),
        (
            "allowedMimeTypes",
            allowed_mime_types(site.can("unfiltered_html")),
        ),
        (
            "defaultEditorStyles",
            list([object(vec![("css", Value::String(css.to_vec()))])]),
        ),
        ("blockCategories", block_categories()),
        ("isRTL", Value::Bool(false)),
        ("imageDefaultSize", text(default_size)),
        ("imageDimensions", Value::Array(dimensions.collect())),
        ("imageEditing", Value::Bool(true)),
        ("imageSizes", list(sizes)),
        ("maxUploadFileSize", Value::Int(upload)),
        (
            "__experimentalDashboardLink",
            text(&crate::admin_bar::BarSite::admin_url(site, "")),
        ),
        ("__unstableGalleryWithImageBlocks", Value::Bool(true)),
        // `get_classic_theme_supports_block_editor_settings()`.
        ("disableCustomColors", Value::Bool(false)),
        ("disableCustomFontSizes", Value::Bool(false)),
        ("disableCustomGradients", Value::Bool(false)),
        ("disableLayoutStyles", Value::Bool(false)),
        ("enableCustomLineHeight", Value::Bool(false)),
        ("enableCustomSpacing", Value::Bool(false)),
        ("enableCustomUnits", Value::Bool(false)),
    ] {
        settings.set(key, value);
    }
    settings
}

fn style(css: Value, kind: &str, global: bool) -> Value {
    object(vec![
        ("css", css),
        ("__unstableType", text(kind)),
        ("isGlobalStyles", Value::Bool(global)),
    ])
}

/// `$settings['styles']`: the global stylesheet's parts, the theme's editor
/// style, then `WP_Duotone::add_editor_settings()`'s.
fn styles(site: &dyn EditorSite) -> Value {
    let global = site.theme().global_styles();
    let mut styles = Vec::new();
    for kind in ["variables", "presets"] {
        let css = global.stylesheet_of(kind);
        if !css.is_empty() {
            styles.push(style(text(&css), "presets", true));
        }
    }
    let css = global.stylesheet_of("styles");
    if !css.is_empty() {
        styles.push(style(text(&css), "theme", true));
    }
    // The Customizer's CSS, which the site has none of.
    styles.push(style(text(""), "user", false));
    styles.push(style(
        text(&global.stylesheet_of("custom-css")),
        "user",
        true,
    ));
    // `get_block_editor_theme_styles()`: the stylesheet `add_editor_style()` names.
    let file = format!("wp-content/themes/{}/{EDITOR_STYLE}", site.theme().slug());
    let css = hyscribe_render::theme::twentytwentyfive_file(EDITOR_STYLE)
        .unwrap_or_default()
        .1;
    styles.push(object(vec![
        ("css", Value::String(css.to_vec())),
        (
            "baseURL",
            text(&format!("{}/{file}", crate::assets::Urls::site_url(site))),
        ),
        ("__unstableType", text("theme")),
        ("isGlobalStyles", Value::Bool(false)),
    ]));
    let duotone = duotone_presets(global.settings());
    if !duotone.is_empty() {
        let svgs: String = duotone
            .iter()
            .map(|(id, colors)| filter_svg(id, colors))
            .collect();
        styles.push(object(vec![
            ("assets", text(&svgs)),
            ("__unstableType", text("svgs")),
            ("isGlobalStyles", Value::Bool(false)),
        ]));
        let properties: String = duotone
            .iter()
            .map(|(id, _)| {
                let slug = id.trim_start_matches("wp-duotone-");
                format!("--wp--preset--duotone--{slug}:url(#{id});")
            })
            .collect();
        styles.push(style(
            text(&format!(":root{{{properties}}}")),
            "presets",
            false,
        ));
    }
    list(styles)
}

/// `WP_Duotone::get_all_global_styles_presets()`: each origin's duotone
/// presets by filter ID, a later one replacing an earlier one's.
fn duotone_presets(settings: &Value) -> Vec<(String, Vec<Vec<u8>>)> {
    let mut presets: Vec<(String, Vec<Vec<u8>>)> = Vec::new();
    let origins = settings
        .get_path(&["color", "duotone"])
        .and_then(Value::as_array);
    for (_, origin) in origins.unwrap_or_default() {
        for (_, preset) in origin.as_array().unwrap_or_default() {
            let slug = preset
                .get_str("slug")
                .map(Value::to_php_string)
                .unwrap_or_default();
            let id = format!(
                "wp-duotone-{}",
                hyscribe_theme_json::kebab_case(&String::from_utf8_lossy(&slug))
            );
            let colors = preset
                .get_str("colors")
                .and_then(Value::as_array)
                .unwrap_or_default();
            let colors = colors.iter().map(|(_, c)| c.to_php_string()).collect();
            match presets.iter_mut().find(|(known, _)| *known == id) {
                Some((_, old)) => *old = colors,
                None => presets.push((id, colors)),
            }
        }
    }
    presets
}

/// `WP_Duotone::colord_parse_hex()` of a hex color: red, green, blue and
/// alpha as `get_filter_svg()` prints them; `None` for any other color.
fn channels(color: &[u8]) -> Option<[String; 4]> {
    let hex = std::str::from_utf8(color.strip_prefix(b"#")?).ok()?;
    let byte = |s: &str| u8::from_str_radix(s, 16).ok();
    let doubled = |i: usize| byte(&hex.get(i..=i)?.repeat(2));
    let pair = |i: usize| byte(hex.get(i..i + 2)?);
    let (rgb, alpha) = match hex.len() {
        3 => ([doubled(0)?, doubled(1)?, doubled(2)?], None),
        4 => ([doubled(0)?, doubled(1)?, doubled(2)?], Some(doubled(3)?)),
        6 => ([pair(0)?, pair(2)?, pair(4)?], None),
        8 => ([pair(0)?, pair(2)?, pair(4)?], Some(pair(6)?)),
        _ => return None,
    };
    // An integer divided by 255 is an integer only at 0 and 255.
    let share = |n: u8| float_to_string(f64::from(n) / 255.0);
    let alpha = alpha.map_or("1".into(), |a| {
        float_to_string((f64::from(a) / 255.0 * 100.0).round() / 100.0)
    });
    Some([share(rgb[0]), share(rgb[1]), share(rgb[2]), alpha])
}

/// `WP_Duotone::get_filter_svg()`, its white space collapsed.
fn filter_svg(id: &str, colors: &[Vec<u8>]) -> String {
    let parsed: Vec<[String; 4]> = colors.iter().filter_map(|c| channels(c)).collect();
    let table = |i: usize| {
        parsed
            .iter()
            .map(|c| c[i].as_str())
            .collect::<Vec<_>>()
            .join(" ")
    };
    let matrix = " .299 .587 .114 0 0".repeat(4);
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 0 0\" width=\"0\" height=\"0\" focusable=\"false\" role=\"none\" style=\"visibility: hidden; position: absolute; left: -9999px; overflow: hidden;\" ><defs><filter id=\"{id}\"><feColorMatrix color-interpolation-filters=\"sRGB\" type=\"matrix\" values=\"{matrix} \" /><feComponentTransfer color-interpolation-filters=\"sRGB\" ><feFuncR type=\"table\" tableValues=\"{}\" /><feFuncG type=\"table\" tableValues=\"{}\" /><feFuncB type=\"table\" tableValues=\"{}\" /><feFuncA type=\"table\" tableValues=\"{}\" /></feComponentTransfer><feComposite in2=\"SourceGraphic\" operator=\"in\" /></filter></defs></svg>",
        table(0),
        table(1),
        table(2),
        table(3)
    )
}

/// The presets under `path` of the custom origin, else the theme's, else
/// the default.
fn by_origin(features: &Value, path: &[&str]) -> Option<Value> {
    let presets = features.get_path(path)?;
    ["custom", "theme", "default"]
        .iter()
        .find_map(|origin| presets.get_str(origin).filter(|v| **v != Value::Null))
        .cloned()
        .or(Some(Value::Null))
}

/// Takes the feature at `path` out of `features`.
fn take(features: &mut Value, path: &[&str]) -> Option<Value> {
    let (last, parents) = path.split_last()?;
    let mut node = features;
    for part in parents {
        let Value::Array(entries) = node else {
            return None;
        };
        node = entries
            .iter_mut()
            .find(|(k, _)| k.to_php_string() == part.as_bytes())
            .map(|(_, v)| v)?;
    }
    let Value::Array(entries) = node else {
        return None;
    };
    let at = entries
        .iter()
        .position(|(k, _)| k.to_php_string() == last.as_bytes())?;
    Some(entries.remove(at).1)
}

/// `wp_get_post_content_block_attributes()`: the attributes of the first
/// post content block of the template the post would show with.
fn post_content_attributes(site: &dyn EditorSite, post: &Post) -> Option<Value> {
    let template = site.meta(post, "_wp_page_template").unwrap_or_default();
    let fallback = if post.post_type == "page" {
        "page"
    } else {
        "single"
    };
    let slug = match String::from_utf8_lossy(&template).into_owned() {
        slug if slug.is_empty() || slug == "default" => fallback.to_string(),
        slug => slug,
    };
    let blocks = parse_blocks(site.theme().template(&[slug])?.1);
    let block = first_block(&blocks, "core/post-content")?;
    block.attrs.clone().map(Value::Array)
}

/// `wp_get_first_block()`.
fn first_block<'b>(blocks: &'b [Block], name: &str) -> Option<&'b Block> {
    blocks.iter().find_map(|block| {
        if block.name.as_deref() == Some(name) {
            return Some(block);
        }
        first_block(&block.inner_blocks, name)
    })
}

/// `get_block_editor_settings( $custom, $context )` for the post editor of
/// `post`: the defaults, then `custom`, then what the theme's data and the
/// site's options give.
pub fn get_block_editor_settings(site: &dyn EditorSite, custom: Settings, post: &Post) -> Value {
    let mut settings = defaults(site);
    for (key, value) in custom.0 {
        settings.set(&String::from_utf8_lossy(&key.to_php_string()), value);
    }
    settings.set(
        "__experimentalBlockBindingsSupportedAttributes",
        bindings_supported_attributes(),
    );
    settings.set("styles", styles(site));
    let mut features = site.theme().global_styles().settings().clone();
    // Set now, as what follows is taken out of it.
    settings.set("__experimentalFeatures", Value::Null);
    let presets = [
        ("colors", &["color", "palette"][..]),
        ("gradients", &["color", "gradients"]),
        ("fontSizes", &["typography", "fontSizes"]),
    ];
    for (key, path) in presets {
        if let Some(value) = by_origin(&features, path) {
            settings.set(key, value);
        }
    }
    let negated = |value: Value| Value::Bool(value.is_empty());
    let moved: [(&str, &[&str], bool); 7] = [
        ("disableCustomColors", &["color", "custom"], true),
        ("disableCustomGradients", &["color", "customGradient"], true),
        (
            "disableCustomFontSizes",
            &["typography", "customFontSize"],
            true,
        ),
        (
            "enableCustomLineHeight",
            &["typography", "lineHeight"],
            false,
        ),
        ("enableCustomUnits", &["spacing", "units"], false),
        ("enableCustomSpacing", &["spacing", "padding"], false),
        (
            "disableCustomSpacingSizes",
            &["spacing", "customSpacingSize"],
            true,
        ),
    ];
    for (key, path, negate) in moved {
        if let Some(value) = take(&mut features, path) {
            settings.set(key, if negate { negated(value) } else { value });
        }
    }
    if let Some(value) = by_origin(&features, &["spacing", "spacingSizes"]) {
        settings.set("spacingSizes", value);
    }
    settings.set("__experimentalFeatures", features);
    settings.set(
        "__unstableResolvedAssets",
        super::assets::resolved_assets(site),
    );
    settings.set("__unstableIsBlockBasedTheme", Value::Bool(true));
    settings.set("localAutosaveInterval", Value::Int(15));
    settings.set("disableLayoutStyles", Value::Bool(false));
    let option = |name: &str| Value::String(site.option(name).unwrap_or_default());
    let avatar = hyscribe_render::forced_avatar_url(&|name| site.option(name), 96);
    let discussion = object(vec![
        ("commentOrder", option("comment_order")),
        ("commentsPerPage", option("comments_per_page")),
        ("defaultCommentsPage", option("default_comments_page")),
        ("pageComments", option("page_comments")),
        ("threadComments", option("thread_comments")),
        ("threadCommentsDepth", option("thread_comments_depth")),
        ("defaultCommentStatus", option("default_comment_status")),
        ("avatarURL", Value::String(avatar)),
    ]);
    settings.set("__experimentalDiscussionSettings", discussion);
    if let Some(attributes) = post_content_attributes(site, post) {
        settings.set("postContentAttributes", attributes);
    }
    // The `edit_block_binding` capability maps to `edit_post` of the post.
    settings.set(
        "canUpdateBlockBindings",
        Value::Bool(site.can_post("edit_post", post)),
    );
    // `edit_css` maps to `unfiltered_html` on a single site.
    settings.set("canEditCSS", Value::Bool(site.can("unfiltered_html")));
    Value::Array(settings.0)
}

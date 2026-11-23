// SPDX-License-Identifier: AGPL-3.0-or-later

//! Hello Elementor 3.5.2 ported to HyScribe (MVP 10.5, ADR 0023): its
//! `functions.php` (theme features, menu locations, stylesheets, the
//! description meta tag, the version write, its settings' filters, and the
//! `hello-elementor-default` body class of its notifications package) and
//! its templates, file for file. Elementor's own locations, header and
//! footer, the Customizer section and the admin page wait for Elementor
//! (11.7) and SDK 0.2's admin pages.

#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use hyscribe_sdk::assets::{self, Asset, AssetKind};
use hyscribe_sdk::theme::{self, TemplateFile};
use hyscribe_sdk::{Plugin, SdkError, Theme, Value, hooks, options, template};

mod templates;
pub mod version;

/// `HELLO_ELEMENTOR_VERSION`.
pub const VERSION: &str = "3.5.2";

hyscribe_sdk::callbacks! {
    enum Cb {
        ContentWidth => content_width,
        Setup => setup,
        Styles => styles,
        Description => description,
        BodyClass => body_class,
        SkipLink => skip_link,
        HeaderFooter => header_footer,
        PageTitle => page_title,
        HelloStyle => hello_style,
        HelloTheme => hello_theme,
    }
}

/// The filters Hello's settings turn off (`Settings_Controller::apply_settings()`),
/// with their callbacks; the description meta tag's setting removes its
/// action instead.
const SETTINGS: [(&str, Cb); 5] = [
    ("hello_elementor_enable_skip_link", Cb::SkipLink),
    ("hello_elementor_header_footer", Cb::HeaderFooter),
    ("hello_elementor_page_title", Cb::PageTitle),
    ("hello_elementor_enqueue_style", Cb::HelloStyle),
    ("hello_elementor_enqueue_theme_style", Cb::HelloTheme),
];

/// The PHP files Hello's templates are, which the host's hierarchy and
/// part lookups pick from; Elementor's dynamic header and footer are left
/// out until Elementor is ported.
const FILES: [&str; 11] = [
    "index.php",
    "header.php",
    "footer.php",
    "sidebar.php",
    "comments.php",
    "template-parts/404.php",
    "template-parts/archive.php",
    "template-parts/footer.php",
    "template-parts/header.php",
    "template-parts/search.php",
    "template-parts/single.php",
];

/// The theme.
pub struct Hello;

type Answer = Result<Value, String>;

pub(crate) fn why(error: SdkError) -> String {
    format!("{error:?}")
}

/// PHP's truthiness of a value a filter answered.
pub(crate) fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Int(n) => *n != 0,
        Value::Float(f) => *f != 0.0,
        Value::Text(text) => !text.is_empty() && text != "0",
        Value::Json(json) => json != "[]",
    }
}

/// `apply_filters( $hook, true )`, as a boolean.
pub(crate) fn allowed(hook: &str) -> bool {
    truthy(&hooks::filter::<Hello>(hook, Value::Bool(true), &[]))
}

/// `esc_html__( $text, 'hello-elementor' )`.
pub(crate) fn esc_html__(text: &str) -> String {
    let translated = hyscribe_sdk::i18n::translate(text);
    String::from_utf8_lossy(&hyscribe_sdk::page::esc_html(translated.as_bytes())).to_string()
}

/// Whether Hello's setting `suffix` is on: its option is the string `true`.
fn setting_on(suffix: &str) -> Result<bool, SdkError> {
    let option = options::get(&format!("hello_elementor_settings{suffix}"))?;
    Ok(option.as_ref().and_then(Value::text) == Some("true"))
}

/// `hello_elementor_content_width()`, on `after_setup_theme` at 0.
fn content_width(_: Vec<Value>) -> Answer {
    let width = hooks::filter::<Hello>("hello_elementor_content_width", Value::Int(800), &[]);
    let width = width.int().and_then(|n| u32::try_from(n).ok()).unwrap_or(0);
    theme::set_content_width(width)
        .map(|()| Value::Null)
        .map_err(why)
}

/// The first error of calls that were all made, as PHP makes them all.
fn all(results: impl IntoIterator<Item = Result<(), SdkError>>) -> Result<(), String> {
    let results: Vec<Result<(), SdkError>> = results.into_iter().collect();
    results
        .into_iter()
        .find_map(Result::err)
        .map_or(Ok(()), |error| Err(why(error)))
}

/// `hello_elementor_setup()`, on `after_setup_theme`.
fn setup(_: Vec<Value>) -> Answer {
    if template::is("is_admin", &[]).map_err(why)? {
        update_version_in_db()?;
    }
    if allowed("hello_elementor_register_menus") {
        all([
            theme::register_nav_menus(&[("menu-1", &esc_html__("Header"))]),
            theme::register_nav_menus(&[("menu-2", &esc_html__("Footer"))]),
        ])?;
    }
    // `add_post_type_support( 'page', 'excerpt' )` follows the filter once
    // the SDK carries it.
    allowed("hello_elementor_post_type_support");
    if allowed("hello_elementor_add_theme_support") {
        let html5 = r#"["search-form","comment-form","comment-list","gallery","caption","script","style","navigation-widgets"]"#;
        let logo = r#"{"height":100,"width":350,"flex-height":true,"flex-width":true}"#;
        let features: [(&str, &[Value]); 8] = [
            ("post-thumbnails", &[]),
            ("automatic-feed-links", &[]),
            ("title-tag", &[]),
            ("html5", &[Value::Json(html5.into())]),
            ("custom-logo", &[Value::Json(logo.into())]),
            ("align-wide", &[]),
            ("responsive-embeds", &[]),
            ("editor-styles", &[]),
        ];
        let mut done: Vec<Result<(), SdkError>> = features
            .iter()
            .map(|(feature, args)| theme::add_support(feature, args))
            .collect();
        done.push(theme::add_editor_style(&["assets/css/editor-styles.css"]));
        if allowed("hello_elementor_add_woocommerce_support") {
            let woocommerce = [
                "woocommerce",
                "wc-product-gallery-zoom",
                "wc-product-gallery-lightbox",
                "wc-product-gallery-slider",
            ];
            done.extend(woocommerce.map(|feature| theme::add_support(feature, &[])));
        }
        all(done)?;
    }
    Ok(Value::Null)
}

/// `hello_maybe_update_theme_version_in_db()`: the version written when
/// none is stored or an older one is; PHP ignores whether it was written,
/// and so does the port, which a read only site refuses.
fn update_version_in_db() -> Result<(), String> {
    let stored = options::get("hello_theme_version").map_err(why)?;
    let stored = match stored {
        Some(Value::Text(text)) => text,
        Some(Value::Int(n)) => n.to_string(),
        other => String::from(if other.as_ref().is_some_and(truthy) {
            "1"
        } else {
            ""
        }),
    };
    let missing = !truthy(&Value::Text(stored.clone()));
    if missing || version::version_compare(&stored, VERSION).is_lt() {
        let _ = options::set("hello_theme_version", &Value::Text(VERSION.into()), None);
    }
    Ok(())
}

/// `hello_elementor_scripts_styles()`, on `wp_enqueue_scripts`: each
/// stylesheet enqueued as its filter allows, in PHP's order.
fn styles(_: Vec<Value>) -> Answer {
    let sheets = [
        ("hello-elementor", "reset", "hello_elementor_enqueue_style"),
        (
            "hello-elementor-theme-style",
            "theme",
            "hello_elementor_enqueue_theme_style",
        ),
        (
            "hello-elementor-header-footer",
            "header-footer",
            "hello_elementor_header_footer",
        ),
    ];
    for (handle, file, hook) in sheets {
        if !allowed(hook) {
            continue;
        }
        let asset = Asset {
            kind: AssetKind::Style,
            handle: handle.into(),
            src: format!("assets/css/{file}.css"),
            deps: Vec::new(),
            version: Some(VERSION.into()),
            placement: "all".into(),
        };
        assets::register(&asset).map_err(why)?;
        assets::enqueue(AssetKind::Style, handle).map_err(why)?;
    }
    Ok(Value::Null)
}

/// `hello_elementor_display_header_footer()`.
pub(crate) fn display_header_footer() -> bool {
    allowed("hello_elementor_header_footer")
}

/// `hello_elementor_add_description_meta_tag()`, on `wp_head`: the meta tag
/// it prints, which its setting removes.
fn description(_: Vec<Value>) -> Answer {
    let none = Ok(Value::Null);
    if setting_on("_description_meta_tag").map_err(why)?
        || !allowed("hello_elementor_description_meta_tag")
        || !template::is("is_singular", &[]).map_err(why)?
    {
        return none;
    }
    let post = template::get("get_queried_object_id", &[]).map_err(why)?;
    let field = [Value::Text("post_excerpt".into()), post];
    let excerpt = template::print("get_post_field", &field).map_err(why)?;
    if excerpt.is_empty() || excerpt == b"0" {
        return none;
    }
    let stripped = template::print("wp_strip_all_tags", &[bytes_value(&excerpt)]).map_err(why)?;
    let content = hyscribe_sdk::page::esc_html(&stripped);
    let tag = [
        &b"<meta name=\"description\" content=\""[..],
        &content,
        b"\">\n",
    ]
    .concat();
    Ok(bytes_value(&tag))
}

/// Bytes as a value that crosses: text, lossy where they are not UTF-8.
pub(crate) fn bytes_value(bytes: &[u8]) -> Value {
    Value::Text(String::from_utf8_lossy(bytes).to_string())
}

/// The notifications package's `body_class` filter: its class appended.
fn body_class(args: Vec<Value>) -> Answer {
    let classes = args.into_iter().next().unwrap_or(Value::Null);
    Ok(match classes {
        Value::Json(list) if list.ends_with(']') => {
            let open = list.strip_suffix(']').unwrap_or_default();
            let comma = if open.ends_with('[') { "" } else { "," };
            Value::Json(format!("{open}{comma}\"hello-elementor-default\"]"))
        }
        other => other,
    })
}

/// A filter Hello's setting `suffix` turns off with `__return_false`
/// while it is on.
fn off_when(suffix: &str, args: Vec<Value>) -> Answer {
    let value = args.into_iter().next().unwrap_or(Value::Null);
    match setting_on(suffix).map_err(why)? {
        true => Ok(Value::Bool(false)),
        false => Ok(value),
    }
}

fn skip_link(args: Vec<Value>) -> Answer {
    off_when("_skip_link", args)
}

fn header_footer(args: Vec<Value>) -> Answer {
    off_when("_header_footer", args)
}

fn page_title(args: Vec<Value>) -> Answer {
    off_when("_page_title", args)
}

fn hello_style(args: Vec<Value>) -> Answer {
    off_when("_hello_style", args)
}

fn hello_theme(args: Vec<Value>) -> Answer {
    off_when("_hello_theme", args)
}

impl Plugin for Hello {
    fn init() -> Result<(), String> {
        let mut added = vec![
            hooks::on_action("after_setup_theme", Cb::ContentWidth, 0, 0),
            hooks::on_action("after_setup_theme", Cb::Setup, 10, 0),
            hooks::on_action("wp_enqueue_scripts", Cb::Styles, 10, 0),
            hooks::on_action("wp_head", Cb::Description, 10, 0),
            hooks::on_filter("body_class", Cb::BodyClass, 10, 1),
        ];
        added.extend(SETTINGS.map(|(hook, callback)| hooks::on_filter(hook, callback, 10, 1)));
        let files: Vec<TemplateFile> = FILES
            .iter()
            .map(|file| TemplateFile {
                file: (*file).into(),
                name: None,
                post_types: Vec::new(),
            })
            .collect();
        added.push(theme::templates(&files));
        all(added)
    }

    fn activate() -> Result<(), String> {
        Ok(())
    }

    fn deactivate() -> Result<(), String> {
        Ok(())
    }

    fn invoke(callback: u32, args: Vec<Value>) -> Result<Value, String> {
        Cb::invoke(callback, args)
    }
}

impl Theme for Hello {
    fn render(file: &str) -> Result<Vec<u8>, String> {
        templates::render(file)
    }
}

/// The component's exports, written by the SDK for wasm32 only.
#[cfg(target_arch = "wasm32")]
#[allow(unsafe_code)]
mod component {
    hyscribe_sdk::theme!(super::Hello);
}

#[cfg(test)]
mod tests;

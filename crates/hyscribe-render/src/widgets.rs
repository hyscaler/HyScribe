// SPDX-License-Identifier: AGPL-3.0-or-later

//! The legacy widget block and the classic core widgets it renders.

use hyscribe_kses::{esc_html, esc_url};
use hyscribe_phpser::{
    Key, Maybe, Value, base64_decode, decode, is_empty_string, isset, maybe_unserialize, parse_str,
};

use crate::blocks::Block;
use crate::core_blocks::prepared;
use crate::formatting::{CORE_SHORTCODE_TAGS, convert_chars, wptexturize};
use crate::template::Renderer;

/// A core widget class: its `id_base` and class name.
struct Widget {
    id_base: &'static str,
    classname: &'static str,
}

/// The core widgets `wp_widgets_init()` registers, in its order.
const WIDGETS: [Widget; 17] = [
    Widget {
        id_base: "pages",
        classname: "widget_pages",
    },
    Widget {
        id_base: "calendar",
        classname: "widget_calendar",
    },
    Widget {
        id_base: "archives",
        classname: "widget_archive",
    },
    Widget {
        id_base: "media_audio",
        classname: "widget_media_audio",
    },
    Widget {
        id_base: "media_image",
        classname: "widget_media_image",
    },
    Widget {
        id_base: "media_gallery",
        classname: "widget_media_gallery",
    },
    Widget {
        id_base: "media_video",
        classname: "widget_media_video",
    },
    Widget {
        id_base: "meta",
        classname: "widget_meta",
    },
    Widget {
        id_base: "search",
        classname: "widget_search",
    },
    Widget {
        id_base: "text",
        classname: "widget_text",
    },
    Widget {
        id_base: "categories",
        classname: "widget_categories",
    },
    Widget {
        id_base: "recent-posts",
        classname: "widget_recent_entries",
    },
    Widget {
        id_base: "recent-comments",
        classname: "widget_recent_comments",
    },
    Widget {
        id_base: "rss",
        classname: "widget_rss",
    },
    Widget {
        id_base: "tag_cloud",
        classname: "widget_tag_cloud",
    },
    Widget {
        id_base: "nav_menu",
        classname: "widget_nav_menu",
    },
    Widget {
        id_base: "custom_html",
        classname: "widget_custom_html",
    },
];

/// The markup a widget is wrapped in: a sidebar's, or `the_widget()`'s.
struct Args {
    before_widget: Vec<u8>,
    after_widget: Vec<u8>,
    before_title: Vec<u8>,
    after_title: Vec<u8>,
}

impl Renderer<'_> {
    /// `render_block_core_legacy_widget()`.
    pub(crate) fn legacy_widget(&mut self, block: &Block) -> Vec<u8> {
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let attributes = prepared("core/legacy-widget", parsed);
        if let Some(id) = attributes.get_str("id").filter(|id| isset(Some(id))) {
            return self.stored_widget(&id.to_php_string());
        }
        let id_base = attributes.get_str("idBase").filter(|id| isset(Some(id)));
        let id_base = id_base.map(Value::to_php_string).unwrap_or_default();
        let Some(widget) = WIDGETS.iter().find(|w| w.id_base.as_bytes() == id_base) else {
            return Vec::new();
        };
        let encoded = attributes.get_path(&["instance", "encoded"]);
        let hash = attributes.get_path(&["instance", "hash"]);
        let instance = match (encoded, hash) {
            (Some(encoded), Some(hash)) if isset(Some(encoded)) && isset(Some(hash)) => {
                let serialized = base64_decode(&encoded.to_php_string());
                let signed = self.context.site.wp_hash(&serialized);
                if signed.map(String::into_bytes) != Some(hash.to_php_string()) {
                    return Vec::new();
                }
                parse_args(decode(&serialized).unwrap_or(Value::Bool(false)))
            }
            _ => Value::Array(Vec::new()),
        };
        let args = Args {
            before_widget: format!("<div class=\"widget {}\">", widget.classname).into_bytes(),
            after_widget: b"</div>".to_vec(),
            before_title: b"<h2 class=\"widgettitle\">".to_vec(),
            after_title: b"</h2>".to_vec(),
        };
        self.widget(widget, &args, &instance)
    }

    /// `wp_render_widget()` for a widget stored in a sidebar. A block theme
    /// registers no sidebars, so only the inactive widgets render, bare.
    fn stored_widget(&mut self, id: &[u8]) -> Vec<u8> {
        let site = self.context.site;
        let sidebars = option_value(site, "sidebars_widgets");
        // `wp_find_widgets_sidebar()`: the first sidebar listing the widget.
        let sidebar =
            sidebars
                .as_array()
                .unwrap_or_default()
                .iter()
                .find(|(name, ids)| {
                    *name != Key::String(b"array_version".to_vec())
                        && ids.as_array().unwrap_or_default().iter().any(
                            |(_, listed)| matches!(listed, Value::String(listed) if listed == id),
                        )
                });
        if sidebar.map(|(name, _)| name) != Some(&Key::String(b"wp_inactive_widgets".to_vec())) {
            return Vec::new();
        }
        let found = WIDGETS.iter().find_map(|widget| {
            settings(site, widget)
                .into_iter()
                .find(|(number, _)| format!("{}-{number}", widget.id_base).as_bytes() == id)
                .map(|(_, instance)| (widget, instance))
        });
        let Some((widget, instance)) = found else {
            return Vec::new();
        };
        let args = Args {
            before_widget: Vec::new(),
            after_widget: Vec::new(),
            before_title: Vec::new(),
            after_title: Vec::new(),
        };
        self.widget(widget, &args, &instance)
    }

    /// The widget's `widget()` output.
    fn widget(&mut self, widget: &Widget, args: &Args, instance: &Value) -> Vec<u8> {
        match widget.id_base {
            "search" => self.search_widget(args, instance),
            _ => Vec::new(),
        }
    }

    /// `WP_Widget_Search::widget()`.
    fn search_widget(&mut self, args: &Args, instance: &Value) -> Vec<u8> {
        let title = widget_title(instance);
        let home = esc_url(&[&self.context.site.home_url()[..], b"/"].concat());
        let query = hyscribe_kses::esc_attr(&self.context.request.search_query);
        let form = format!(
            "<form role=\"search\" method=\"get\" class=\"search-form\" action=\"{}\">\n\
             \t\t\t\t<label>\n\
             \t\t\t\t\t<span class=\"screen-reader-text\">Search for:</span>\n\
             \t\t\t\t\t<input type=\"search\" class=\"search-field\" placeholder=\"Search &hellip;\" value=\"{query}\" name=\"s\" />\n\
             \t\t\t\t</label>\n\
             \t\t\t\t<input type=\"submit\" class=\"search-submit\" value=\"Search\" />\n\
             \t\t\t</form>",
            String::from_utf8_lossy(&home)
        );
        [
            &args.before_widget[..],
            &titled(args, &title),
            form.as_bytes(),
            &args.after_widget,
        ]
        .concat()
    }
}

/// An option as `get_option()` unserializes it, `null` when unset.
fn option_value(site: &dyn crate::site::Site, name: &str) -> Value {
    let stored = site.option(name).unwrap_or_default();
    match maybe_unserialize(&stored) {
        Maybe::Value(value) => value,
        _ => Value::Null,
    }
}

/// The widget's stored instances by number, as `get_settings()` gives them.
fn settings(site: &dyn crate::site::Site, widget: &Widget) -> Vec<(i64, Value)> {
    let settings = option_value(site, &format!("widget_{}", widget.id_base));
    settings
        .as_array()
        .unwrap_or_default()
        .iter()
        .cloned()
        .filter_map(|(key, instance)| match key {
            Key::Int(number) => Some((number, instance)),
            Key::String(_) => None,
        })
        .collect()
}

/// `wp_parse_args()` of an unserialized instance.
fn parse_args(instance: Value) -> Value {
    match instance {
        Value::Array(_) => instance,
        Value::Object { properties, .. } => Value::Array(properties),
        Value::Null | Value::Bool(false) => Value::Array(Vec::new()),
        other => parse_str(&other.to_php_string()),
    }
}

/// The `title` instance setting through the `widget_title` filters.
fn widget_title(instance: &Value) -> Vec<u8> {
    let title = instance
        .get_str("title")
        .filter(|title| !title.is_empty())
        .map(Value::to_php_string)
        .unwrap_or_default();
    esc_html(&convert_chars(&wptexturize(&title, &CORE_SHORTCODE_TAGS))).into_bytes()
}

/// The title in the sidebar's title markup, if there is one.
fn titled(args: &Args, title: &[u8]) -> Vec<u8> {
    if is_empty_string(title) {
        return Vec::new();
    }
    [&args.before_title[..], title, &args.after_title].concat()
}

#[cfg(test)]
mod tests;

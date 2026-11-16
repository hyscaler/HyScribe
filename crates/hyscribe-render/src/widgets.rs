// SPDX-License-Identifier: AGPL-3.0-or-later

//! The legacy widget block and the classic core widgets it renders.

use hyscribe_kses::{esc_attr, esc_html, esc_url};
use hyscribe_model::{PostQuery, Term};
use hyscribe_phpser::{
    Key, Maybe, Value, base64_decode, hash_equals, is_empty_string, isset, maybe_unserialize,
    parse_str, unserialize,
};

use crate::archives::Format;
use crate::autop::wpautop;
use crate::balance::force_balance_tags;
use crate::blocks::{Block, is_pcre_space, parse_blocks};
use crate::categories::{dropdown_select, terms};
use crate::content::replace_insecure_home_url;
use crate::content_tags::filter_content_tags;
use crate::core_blocks::calendar::request_calendar;
use crate::core_blocks::tag_cloud::{TAXONOMIES, wp_tag_cloud};
use crate::core_blocks::{comment_author_link, comment_link, prepared};
use crate::date;
use crate::formatting::{CORE_SHORTCODE_TAGS, capital_p_dangit, convert_chars, wptexturize};
use crate::head::json_hex_tag;
use crate::media::Filter;
use crate::pages::wp_list_pages;
use crate::permalinks;
use crate::post::{listed_title, the_title};
use crate::site::{Archive, Taxonomy};
use crate::template::{Renderer, replace, site_smilies};

/// The classic widgets' dropdown script up to what a change does.
const SCRIPT_START: &str = "( ( dropdownId ) => {
\tconst dropdown = document.getElementById( dropdownId );
\tfunction onSelectChange() {
\t\tsetTimeout( () => {
\t\t\tif ( 'escape' === dropdown.dataset.lastkey ) {
\t\t\t\treturn;
\t\t\t}
";

/// The rest of the dropdown script, up to its argument.
const SCRIPT_END: &str = "\t\t}, 250 );
\t}
\tfunction onKeyUp( event ) {
\t\tif ( 'Escape' === event.key ) {
\t\t\tdropdown.dataset.lastkey = 'escape';
\t\t} else {
\t\t\tdelete dropdown.dataset.lastkey;
\t\t}
\t}
\tfunction onClick() {
\t\tdelete dropdown.dataset.lastkey;
\t}
\tdropdown.addEventListener( 'keyup', onKeyUp );
\tdropdown.addEventListener( 'click', onClick );
\tdropdown.addEventListener( 'change', onSelectChange );
})( ";

/// A core widget class: its `id_base` and class name.
struct Widget {
    id_base: &'static str,
    classname: &'static str,
}

/// The core widgets `wp_widgets_init()` registers. The links widget needs
/// the link manager, which WordPress has turned off by default since 3.5.
const WIDGETS: [Widget; 18] = [
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
    Widget {
        id_base: "block",
        classname: "widget_block",
    },
];

/// The markup a widget is wrapped in: a sidebar's, or `the_widget()`'s.
#[derive(Default)]
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
                let hash = hash.to_php_string();
                if !signed.is_some_and(|signed| hash_equals(signed.as_bytes(), &hash)) {
                    return Vec::new();
                }
                parse_args(unserialize(&serialized).unwrap_or(Value::Bool(false)))
            }
            _ => Value::Array(Vec::new()),
        };
        let args = Args {
            before_widget: format!("<div class=\"widget {}\">", widget.classname).into_bytes(),
            after_widget: b"</div>".to_vec(),
            before_title: b"<h2 class=\"widgettitle\">".to_vec(),
            after_title: b"</h2>".to_vec(),
        };
        // `the_widget()` numbers its widget -1.
        self.widget(widget, b"-1", &args, &instance)
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
        let found = WIDGETS
            .iter()
            .find_map(|widget| Some((widget, instance(site, widget, id)?)));
        // `display_callback()` skips an instance that is unset or `false`.
        match found {
            Some((widget, instance)) if !matches!(instance, Value::Null | Value::Bool(false)) => {
                let number = id.get(widget.id_base.len() + 1..).unwrap_or_default();
                self.widget(widget, number, &Args::default(), &instance)
            }
            _ => Vec::new(),
        }
    }

    /// The widget's `widget()` output, for the instance `number`.
    fn widget(&mut self, widget: &Widget, number: &[u8], args: &Args, instance: &Value) -> Vec<u8> {
        match widget.id_base {
            "archives" => self.archives_widget(number, args, instance),
            "categories" => self.categories_widget(number, args, instance),
            "calendar" => self.calendar_widget(args, instance),
            "recent-posts" => self.recent_posts_widget(args, instance),
            "recent-comments" => self.recent_comments_widget(number, args, instance),
            "tag_cloud" => self.tag_cloud_widget(args, instance),
            "pages" => self.pages_widget(args, instance),
            "search" => self.search_widget(args, instance),
            "custom_html" => self.custom_html_widget(args, instance),
            "meta" => self.meta_widget(args, instance),
            "text" => self.text_widget(args, instance),
            "media_image" => self.media_image_widget(args, instance),
            "media_gallery" => self.media_gallery_widget(args, instance),
            "block" => self.block_widget(args, instance),
            _ => Vec::new(),
        }
    }

    /// `WP_Widget_Archives::widget()`: the monthly archives as a list, or
    /// as a dropdown that goes to the one picked.
    fn archives_widget(&self, number: &[u8], args: &Args, instance: &Value) -> Vec<u8> {
        let title = widget_title(instance, b"Archives");
        let count = checked(instance, "count");
        let body = if checked(instance, "dropdown") {
            let id = Value::String([b"archives-dropdown-", number].concat());
            let options = self.archive_links(b"monthly", Format::Option, count);
            let attr = esc_attr(&id.to_php_string()).into_bytes();
            [
                &b"\t\t<label class=\"screen-reader-text\" for=\""[..],
                &attr,
                b"\">",
                &title,
                b"</label>\n\t\t<select id=\"",
                &attr,
                b"\" name=\"archive-dropdown\">\n\t\t\t\n\t\t\t<option value=\"\">Select Month</option>\n\t\t\t",
                &options,
                b"\n\t\t</select>\n\n\t\t\t",
                &dropdown_script(
                    "\t\t\tif ( dropdown.value ) {\n\t\t\t\tdocument.location.href = dropdown.value;\n\t\t\t}\n",
                    &id,
                    "WP_Widget_Archives",
                ),
            ]
            .concat()
        } else {
            let links = self.archive_links(b"monthly", Format::Html, count);
            [
                b"\n\t\t\t<ul>\n\t\t\t\t",
                &links[..],
                b"\t\t\t</ul>\n\n\t\t\t",
            ]
            .concat()
        };
        [
            &args.before_widget[..],
            &titled(args, &title),
            &body,
            &args.after_widget,
        ]
        .concat()
    }

    /// `WP_Widget_Categories::widget()`: the categories by name as a list,
    /// or as a dropdown that submits the one picked. The first dropdown of
    /// a request is `cat`, after the query var it sets.
    fn categories_widget(&mut self, number: &[u8], args: &Args, instance: &Value) -> Vec<u8> {
        let site = self.context.site;
        let title = widget_title(instance, b"Categories");
        let count = checked(instance, "count");
        let listing = terms(
            site,
            "category",
            checked(instance, "hierarchical"),
            true,
            false,
            count,
        );
        let body = if checked(instance, "dropdown") {
            let id = if std::mem::replace(&mut self.categories_dropdown_used, true) {
                [b"categories-dropdown-", number].concat()
            } else {
                b"cat".to_vec()
            };
            // `is_category() ? get_query_var( 'cat' ) : 0`: a category
            // archive always has its term, whose ID `cat` then holds.
            let selected = match &self.context.request.archive {
                Some(Archive::Term(Taxonomy::Category, Some(term))) => {
                    term.id.to_string().into_bytes()
                }
                _ => b"0".to_vec(),
            };
            let term_id = |term: &Term| term.id.to_string().into_bytes();
            let id = Value::String(id);
            let id_text = String::from_utf8_lossy(&id.to_php_string()).into_owned();
            let select = dropdown_select(
                &id_text,
                "cat",
                "Select Category",
                &listing,
                count,
                &term_id,
                &selected,
            );
            [
                &b"<form action=\""[..],
                &esc_url(&site.home_url()),
                b"\" method=\"get\"><label class=\"screen-reader-text\" for=\"",
                esc_attr(id_text.as_bytes()).as_bytes(),
                b"\">",
                &title,
                b"</label>",
                &select,
                b"\n</form>",
                &dropdown_script(
                    "\t\t\tif ( dropdown.value && parseInt( dropdown.value ) > 0 && dropdown instanceof HTMLSelectElement ) {\n\t\t\t\tdropdown.parentElement.submit();\n\t\t\t}\n",
                    &id,
                    "WP_Widget_Categories",
                ),
            ]
            .concat()
        } else {
            let items = self.category_list("category", "No categories", &listing, count);
            [
                &b"\n\t\t\t<ul>\n\t\t\t\t"[..],
                &items,
                b"\t\t\t</ul>\n\n\t\t\t",
            ]
            .concat()
        };
        [
            &args.before_widget[..],
            &titled(args, &title),
            &body,
            &args.after_widget,
        ]
        .concat()
    }

    /// `WP_Widget_Calendar::widget()`: the request's month in a wrapper
    /// that only the first calendar widget of a request names.
    fn calendar_widget(&mut self, args: &Args, instance: &Value) -> Vec<u8> {
        let title = widget_title(instance, b"");
        let wrap = if std::mem::replace(&mut self.calendar_widget_shown, true) {
            &b"<div class=\"calendar_wrap\">"[..]
        } else {
            b"<div id=\"calendar_wrap\" class=\"calendar_wrap\">"
        };
        [
            &args.before_widget[..],
            &titled(args, &title),
            wrap,
            request_calendar(&self.context).as_bytes(),
            b"</div>",
            &args.after_widget,
        ]
        .concat()
    }

    /// `WP_Widget_Recent_Posts::widget()`: the newest published posts,
    /// sticky or not, five unless the instance asks for another number;
    /// nothing at all without one.
    fn recent_posts_widget(&self, args: &Args, instance: &Value) -> Vec<u8> {
        let context = &self.context;
        let site = context.site;
        let query = PostQuery {
            post_types: vec!["post".into()],
            limit: u32::try_from(listed(instance)).unwrap_or(u32::MAX),
            ..PostQuery::default()
        };
        let posts = site.query(&query);
        if posts.is_empty() {
            return Vec::new();
        }
        let title = widget_title(instance, b"Recent Posts");
        let date_format =
            String::from_utf8_lossy(&site.option("date_format").unwrap_or_default()).into_owned();
        let mut items = Vec::new();
        for post in &posts {
            let title = listed_title(post);
            let current = if context.request.queried_object_id == post.id {
                &b" aria-current=\"page\""[..]
            } else {
                b""
            };
            items.extend_from_slice(b"\t\t\t\t\t\t\t\t\t\t\t<li>\n\t\t\t\t\t<a href=\"");
            items.extend_from_slice(&esc_url(&permalinks::permalink(site, post)));
            items.extend_from_slice(b"\"");
            items.extend_from_slice(current);
            items.extend_from_slice(b">");
            items.extend_from_slice(&title);
            items.extend_from_slice(b"</a>\n");
            if checked(instance, "show_date") {
                let shown = date::format(&date_format, &post.date, context.zone());
                items.extend(
                    format!("\t\t\t\t\t\t\t\t\t\t\t<span class=\"post-date\">{shown}</span>\n")
                        .into_bytes(),
                );
            }
            items.extend_from_slice(b"\t\t\t\t\t\t\t\t\t</li>\n");
        }
        [
            &b"\n\t\t"[..],
            &args.before_widget,
            b"\n\t\t",
            &titled(args, &title),
            b"\n\t\t<ul>\n",
            &items,
            b"\t\t\t\t\t</ul>\n\n\t\t",
            &args.after_widget,
        ]
        .concat()
    }

    /// `WP_Widget_Recent_Comments::widget()`: the newest approved comments
    /// on published posts, five unless `number` asks otherwise, the first
    /// list of a request as `#recentcomments`.
    fn recent_comments_widget(&mut self, number: &[u8], args: &Args, instance: &Value) -> Vec<u8> {
        let id = if std::mem::replace(&mut self.recent_comments_shown, true) {
            [&b"recentcomments-"[..], number].concat()
        } else {
            b"recentcomments".to_vec()
        };
        let context = &self.context;
        let mut items = Vec::new();
        for comment in context.site.recent_comments(listed(instance)) {
            let title = context
                .site
                .post(comment.post_id)
                .map(|post| the_title(&post))
                .unwrap_or_default();
            items.extend_from_slice(
                b"<li class=\"recentcomments\"><span class=\"comment-author-link\">",
            );
            items.extend(comment_author_link(context, &comment));
            items.extend_from_slice(b"</span> on <a href=\"");
            items.extend(esc_url(&comment_link(context, &comment)));
            items.extend_from_slice(b"\">");
            items.extend(title);
            items.extend_from_slice(b"</a></li>");
        }
        [
            &args.before_widget[..],
            &titled(args, &widget_title(instance, b"Recent Comments")),
            b"<ul id=\"",
            esc_attr(&id).as_bytes(),
            b"\">",
            &items,
            b"</ul>",
            &args.after_widget,
        ]
        .concat()
    }

    /// `WP_Widget_Tag_Cloud::widget()`: the cloud of the instance's
    /// taxonomy when it is registered, else of tags, titled with the
    /// taxonomy's label by default; nothing without terms.
    fn tag_cloud_widget(&self, args: &Args, instance: &Value) -> Vec<u8> {
        let chosen = instance
            .get_str("taxonomy")
            .filter(|taxonomy| !taxonomy.is_empty())
            .map(Value::to_php_string);
        let (taxonomy, label) = TAXONOMIES
            .into_iter()
            .find(|(name, _)| chosen.as_deref() == Some(name.as_bytes()))
            .unwrap_or(("post_tag", "Tags"));
        let cloud = wp_tag_cloud(
            &self.context,
            taxonomy,
            b"8",
            b"22",
            45,
            checked(instance, "count"),
        );
        if cloud.is_empty() {
            return Vec::new();
        }
        [
            &args.before_widget[..],
            &titled(args, &widget_title(instance, label.as_bytes())),
            b"<div class=\"tagcloud\">",
            &cloud,
            b"</div>\n",
            &args.after_widget,
        ]
        .concat()
    }

    /// `WP_Widget_Pages::widget()`: the published pages, by menu order and
    /// title unless the instance sorts otherwise, less those it excludes;
    /// nothing without pages.
    fn pages_widget(&self, args: &Args, instance: &Value) -> Vec<u8> {
        let text = |name| {
            instance
                .get_str(name)
                .filter(|value| !value.is_empty())
                .map(Value::to_php_string)
        };
        let sort_column = match text("sortby") {
            None => b"menu_order, post_title".to_vec(),
            Some(sortby) if sortby == b"menu_order" => b"menu_order, post_title".to_vec(),
            Some(sortby) => sortby,
        };
        let list = wp_list_pages(
            &self.context,
            &sort_column,
            &text("exclude").unwrap_or_default(),
        );
        if list.is_empty() {
            return Vec::new();
        }
        [
            &args.before_widget[..],
            &titled(args, &widget_title(instance, b"Pages")),
            b"\n\t\t\t<ul>\n\t\t\t\t",
            &list,
            b"\t\t\t</ul>\n\n\t\t\t",
            &args.after_widget,
        ]
        .concat()
    }

    /// `WP_Widget_Search::widget()`.
    fn search_widget(&mut self, args: &Args, instance: &Value) -> Vec<u8> {
        let title = widget_title(instance, b"");
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

/// The wrapper class `WP_Widget_Block` gives content whose first block is
/// each of these; any other has `widget_block` alone.
const BLOCK_WIDGET_CLASSES: [(&str, &str); 14] = [
    ("core/paragraph", "widget_block widget_text"),
    ("core/calendar", "widget_block widget_calendar"),
    ("core/search", "widget_block widget_search"),
    ("core/html", "widget_block widget_custom_html"),
    ("core/archives", "widget_block widget_archive"),
    ("core/latest-posts", "widget_block widget_recent_entries"),
    (
        "core/latest-comments",
        "widget_block widget_recent_comments",
    ),
    ("core/tag-cloud", "widget_block widget_tag_cloud"),
    ("core/categories", "widget_block widget_categories"),
    ("core/audio", "widget_block widget_media_audio"),
    ("core/video", "widget_block widget_media_video"),
    ("core/image", "widget_block widget_media_image"),
    ("core/gallery", "widget_block widget_media_gallery"),
    ("core/rss", "widget_block widget_rss"),
];

/// `WP_Widget_Block::get_dynamic_classname()`: by the content's first
/// block, which is no block when the content starts with anything else.
fn block_widget_class(content: &[u8]) -> &'static str {
    let blocks = parse_blocks(content);
    let first = blocks.first().and_then(|block| block.name.as_deref());
    BLOCK_WIDGET_CLASSES
        .iter()
        .find(|(name, _)| first == Some(*name))
        .map_or("widget_block", |(_, class)| class)
}

impl Renderer<'_> {
    /// `WP_Widget_Block::widget()`: the instance through `wp_parse_args()`,
    /// its content through the `widget_block_content` filters, `do_blocks()`
    /// and `wp_filter_content_tags()` (neither `WP_Embed`'s `[embed]` and
    /// autoembed at priority 8 nor other shortcodes are run), in the wrapper
    /// with the class of its first block. A `content` that is an array, which
    /// `parse_blocks()` refuses with a `TypeError`, renders as `Array`.
    fn block_widget(&mut self, args: &Args, instance: &Value) -> Vec<u8> {
        // A stored instance comes as it was saved, an object or a string.
        let instance = parse_args(instance.clone());
        let content = instance
            .get_str("content")
            .map(Value::to_php_string)
            .unwrap_or_default();
        let class = block_widget_class(&content);
        let outer = self.context.loading.in_widget_block_content;
        self.context.loading.in_widget_block_content = true;
        let html = self.render_top_level(&content);
        let html = filter_content_tags(
            self.context.site,
            &self.context.loading,
            &html,
            Filter::WidgetBlock,
        );
        self.context.loading.in_widget_block_content = outer;
        [
            &replace(&args.before_widget, b"widget_block", class.as_bytes())[..],
            &html,
            &args.after_widget,
        ]
        .concat()
    }

    /// The `widget_text_content` filters: `the_content`'s without
    /// `do_blocks()` and `prepend_attachment()`, with `wpautop()` always in
    /// its place at priority 10 and `wp_filter_content_tags()` in their own
    /// context. `WP_Embed`'s `[embed]` and autoembed, `shortcode_unautop()`
    /// and `do_shortcode()` are not ported.
    fn widget_text_content(&self, text: &[u8]) -> Vec<u8> {
        let site = self.context.site;
        let text = wpautop(&wptexturize(text, &CORE_SHORTCODE_TAGS), true);
        let text = capital_p_dangit(&replace_insecure_home_url(&site.home_url(), &text));
        let text = filter_content_tags(site, &self.context.loading, &text, Filter::WidgetText);
        site_smilies(site, &text)
    }

    /// `WP_Widget_Custom_HTML::widget()`.
    fn custom_html_widget(&self, args: &Args, instance: &Value) -> Vec<u8> {
        let title = widget_title(instance, b"");
        let content = instance.get_str("content").map(Value::to_php_string);
        let content = self.balance_tags(&content.unwrap_or_default());
        [
            &with_text_class(&args.before_widget)[..],
            &titled(args, &title),
            b"<div class=\"textwidget custom-html-widget\">",
            &content,
            b"</div>",
            &args.after_widget,
        ]
        .concat()
    }

    /// `WP_Widget_Meta::widget()`, in the XHTML format a block theme gets.
    fn meta_widget(&self, args: &Args, instance: &Value) -> Vec<u8> {
        let context = &self.context;
        let site = context.site;
        let title = widget_title(instance, b"Meta");
        let register = if is_empty_string(&site.option("users_can_register").unwrap_or_default()) {
            Vec::new()
        } else {
            let url = context.site_url(b"/wp-login.php?action=register");
            [b"<li><a href=\"", &url[..], b"\">Register</a></li>"].concat()
        };
        let login = esc_url(&context.site_url(b"/wp-login.php"));
        let feed = |name| esc_url(&permalinks::feed_link(site, name));
        [
            &args.before_widget[..],
            &titled(args, &title),
            b"\n\t\t<ul>\n\t\t\t",
            &register,
            b"\t\t\t<li><a href=\"",
            &login,
            b"\">Log in</a></li>\n\t\t\t<li><a href=\"",
            &feed("rss2"),
            b"\">Entries feed</a></li>\n\t\t\t<li><a href=\"",
            &feed("comments_rss2"),
            b"\">Comments feed</a></li>\n\n\t\t\t<li><a href=\"https://wordpress.org/\">WordPress.org</a></li>\n\t\t</ul>\n\n\t\t",
            &args.after_widget,
        ]
        .concat()
    }

    /// `WP_Widget_Text::widget()`: a visual widget's text gets the
    /// `widget_text_content` filters, a legacy one `wpautop()` when it asks
    /// for it (its `shortcode_unautop()` and `do_shortcode()` are not
    /// ported).
    fn text_widget(&self, args: &Args, instance: &Value) -> Vec<u8> {
        let title = widget_title(instance, b"");
        let set = |name| checked(instance, name);
        let visual = set("visual") && set("filter")
            || matches!(instance.get_str("filter"), Some(Value::String(s)) if s == b"content");
        let text = instance.get_str("text").filter(|text| !text.is_empty());
        let text = self.balance_tags(&text.map(Value::to_php_string).unwrap_or_default());
        let text = if visual {
            self.widget_text_content(&text)
        } else if set("filter") {
            wpautop(&text, true)
        } else {
            text
        };
        [
            &args.before_widget[..],
            &titled(args, &title),
            b"\t\t\t<div class=\"textwidget\">",
            &fit_videos(&text),
            b"</div>\n\t\t",
            &args.after_widget,
        ]
        .concat()
    }

    /// `balanceTags()`: balanced only with the `use_balanceTags` option.
    fn balance_tags(&self, text: &[u8]) -> Vec<u8> {
        let option = self
            .context
            .site
            .option("use_balanceTags")
            .unwrap_or_default();
        if Value::String(option).to_int() == 1 {
            force_balance_tags(text)
        } else {
            text.to_vec()
        }
    }
}

/// The custom HTML widget's `widget_text` class, after each ` class="`.
fn with_text_class(before_widget: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, &byte) in before_widget.iter().enumerate() {
        out.push(byte);
        let head = before_widget.get(..=i).unwrap_or_default();
        let opened = matches!(
            head.get(head.len().saturating_sub(8)..),
            Some([space, b'c', b'l', b'a', b's', b's', b'=', b'"' | b'\''])
                if is_pcre_space(*space)
        );
        if opened {
            out.extend_from_slice(b"widget_text ");
        }
    }
    out
}

/// `inject_video_max_width_style()` on each `video`, `iframe`, `object`
/// or `embed` tag: no `width` or `height` attribute, and `100%` widths.
fn fit_videos(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(start) = text
        .get(at..)
        .and_then(|rest| rest.iter().position(|&b| b == b'<'))
    {
        let start = at + start;
        let tag = text.get(start..).unwrap_or_default();
        let named = [&b"video"[..], b"iframe", b"object", b"embed"]
            .iter()
            .find_map(|name| {
                let after = tag.get(1..=name.len())?;
                (after.eq_ignore_ascii_case(name)
                    && tag.get(name.len() + 1).is_some_and(|b| is_pcre_space(*b)))
                .then_some(name.len())
            });
        let end = named.and_then(|_| tag.iter().position(|&b| b == b'>'));
        let Some(end) = end else {
            out.extend_from_slice(text.get(at..=start).unwrap_or_default());
            at = start + 1;
            continue;
        };
        out.extend_from_slice(text.get(at..start).unwrap_or_default());
        let fitted = without_attribute(tag.get(..=end).unwrap_or_default(), b"height");
        out.extend(full_width(&without_attribute(&fitted, b"width")));
        at = start + end + 1;
    }
    out.extend_from_slice(text.get(at..).unwrap_or_default());
    out
}

/// `preg_replace( '/\sNAME="\d+"/', '', $tag )`.
fn without_attribute(tag: &[u8], name: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(rest) = tag.get(at..).filter(|rest| !rest.is_empty()) {
        let value = rest
            .get(1..)
            .filter(|_| rest.first().is_some_and(|b| is_pcre_space(*b)))
            .and_then(|r| r.strip_prefix(name))
            .and_then(|r| r.strip_prefix(b"=\""));
        let digits = value.map_or(0, |v| v.iter().take_while(|b| b.is_ascii_digit()).count());
        if let Some(value) = value.filter(|v| digits > 0 && v.get(digits) == Some(&b'"')) {
            at = tag.len() - value.len() + digits + 1;
            continue;
        }
        out.extend(rest.first());
        at += 1;
    }
    out
}

/// `preg_replace( '/(?<=width:)\s*\d+px(?=;?)/', '100%', $tag )`.
fn full_width(tag: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(rest) = tag.get(at..).filter(|rest| !rest.is_empty()) {
        if out.ends_with(b"width:") {
            let space = rest.iter().take_while(|b| is_pcre_space(**b)).count();
            let digits = rest
                .get(space..)
                .unwrap_or_default()
                .iter()
                .take_while(|b| b.is_ascii_digit())
                .count();
            if digits > 0 && rest.get(space + digits..space + digits + 2) == Some(b"px") {
                out.extend_from_slice(b"100%");
                at += space + digits + 2;
                continue;
            }
        }
        out.extend(rest.first());
        at += 1;
    }
    out
}

/// The stored instance `id` names, from `get_settings()`, if it is registered.
fn instance(site: &dyn crate::site::Site, widget: &Widget, id: &[u8]) -> Option<Value> {
    let settings = option_value(site, &format!("widget_{}", widget.id_base));
    let numbered = |key: &Key| match key {
        Key::Int(_) => true,
        Key::String(number) => Value::String(number.clone()).is_numeric(),
    };
    settings
        .as_array()
        .unwrap_or_default()
        .iter()
        .find(|(key, _)| {
            numbered(key) && [widget.id_base.as_bytes(), b"-", &key.to_php_string()].concat() == id
        })
        .map(|(_, instance)| instance.clone())
}

/// `wp_parse_args()` of an unserialized instance.
fn parse_args(instance: Value) -> Value {
    match instance {
        Value::Array(_) => instance,
        Value::Object { properties, .. } => Value::Array(properties),
        other => parse_str(&other.to_php_string()),
    }
}

/// The `title` instance setting, or the widget's `default` when it is
/// `empty()`, through the `widget_title` filters.
fn widget_title(instance: &Value, default: &[u8]) -> Vec<u8> {
    let title = instance.get_str("title").filter(|title| !title.is_empty());
    let title = title.map_or(default.to_vec(), Value::to_php_string);
    esc_html(&convert_chars(&wptexturize(&title, &CORE_SHORTCODE_TAGS))).into_bytes()
}

/// A classic widget's inline dropdown script, as `wp_print_inline_script_tag()`
/// prints it: `change` is what picking an option does, `id` the select's,
/// and the source URL names `class`'s `widget()`.
fn dropdown_script(change: &str, id: &Value, class: &str) -> Vec<u8> {
    [
        b"<script>\n",
        SCRIPT_START.as_bytes(),
        change.as_bytes(),
        SCRIPT_END.as_bytes(),
        &json_hex_tag(id),
        format!(" );\n\n//# sourceURL={class}%3A%3Awidget\n</script>\n").as_bytes(),
    ]
    .concat()
}

/// `! empty( $instance[ $name ] )`.
fn checked(instance: &Value, name: &str) -> bool {
    instance
        .get_str(name)
        .is_some_and(|value| !value.is_empty())
}

/// How many entries a recent posts or comments widget lists: its `number`
/// through `absint()`, five for none.
fn listed(instance: &Value) -> u64 {
    match instance
        .get_str("number")
        .filter(|number| !number.is_empty())
        .map_or(0, hyscribe_phpser::absint)
    {
        0 => 5,
        n => n,
    }
}

/// The title in the sidebar's title markup, if there is one.
fn titled(args: &Args, title: &[u8]) -> Vec<u8> {
    if is_empty_string(title) {
        return Vec::new();
    }
    [&args.before_title[..], title, &args.after_title].concat()
}

mod media;

#[cfg(test)]
mod tests;

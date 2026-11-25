// SPDX-License-Identifier: AGPL-3.0-or-later

//! `gallery_shortcode()` under the `html5` gallery support block themes
//! have, for the shortcode and for the gallery widget, which calls it with
//! its instance.
//!
//! Not ported: a gallery in a feed prints the attributes of the shortcode
//! as attributes of its images where PHP does (`link="none"`), through
//! `wp_get_attachment_image()`, which takes no `width`, `height` or
//! `decoding` of its own.

use hyscribe_kses::sanitize_html_class;
use hyscribe_model::{Post, PostQuery, StatusFilter};
use hyscribe_phpser::{Key, Value, absint, is_empty_string, isset, php_trim};

use super::{att, given, set_att};
use crate::attachment::{ImageOptions, Size, attachment_image, metadata};
use crate::blocks::is_pcre_space;
use crate::formatting::{CORE_SHORTCODE_TAGS, wptexturize};
use crate::kses::post_allows;
use crate::query_loop::{order_by, query_limit};
use crate::template::Renderer;
use crate::widgets::media::tag_escape;

/// The post types a post's attachments are not shown for: those that are
/// not publicly viewable, `is_post_type_viewable()`.
const PRIVATE_TYPES: [&str; 13] = [
    "nav_menu_item",
    "revision",
    "customize_changeset",
    "custom_css",
    "oembed_cache",
    "user_request",
    "wp_block",
    "wp_template",
    "wp_template_part",
    "wp_global_styles",
    "wp_navigation",
    "wp_font_family",
    "wp_font_face",
];

impl Renderer<'_> {
    /// `gallery_shortcode( $attr )`.
    pub(crate) fn gallery_shortcode(&mut self, mut attr: Vec<(Key, Value)>) -> Vec<u8> {
        let site = self.context.site;
        let hooks = site.hooks();
        self.gallery_instances += 1;
        let instance = self.gallery_instances;
        if given(&attr, "ids").is_some_and(|ids| !ids.is_empty()) {
            // `ids` is explicitly ordered, unless `orderby` says otherwise.
            if given(&attr, "orderby").is_none_or(Value::is_empty) {
                set_att(&mut attr, "orderby", Value::String(b"post__in".to_vec()));
            }
            let ids = given(&attr, "ids").cloned().unwrap_or_default();
            set_att(&mut attr, "include", ids);
        }
        let output = hooks.post_gallery(&attr, instance);
        if !is_empty_string(&output) {
            return output;
        }
        let text = |value: &str| Value::String(value.as_bytes().to_vec());
        let post = self.context.post_id.and_then(|id| site.post(id));
        let pairs = [
            ("order", text("ASC")),
            ("orderby", text("menu_order ID")),
            ("id", Value::Int(post.map_or(0, |post| post.id as i64))),
            ("itemtag", text("figure")),
            ("icontag", text("div")),
            ("captiontag", text("figcaption")),
            ("columns", Value::Int(3)),
            ("size", text("thumbnail")),
            ("include", text("")),
            ("exclude", text("")),
            ("link", text("")),
        ];
        let atts = self.shortcode_atts("gallery", &pairs, &attr);
        let id = att(&atts, "id").to_int();
        let attachments = if !att(&atts, "include").is_empty() {
            self.gallery_attachments(&atts, None, att(&atts, "include"))
        } else {
            // The children of the post, who must be readable.
            let Ok(parent) = u64::try_from(id) else {
                return Vec::new();
            };
            if parent != 0 && !self.parent_readable(parent) {
                return Vec::new();
            }
            self.gallery_attachments(&atts, Some(parent), att(&atts, "exclude"))
        };
        if attachments.is_empty() {
            return Vec::new();
        }
        let size = att(&atts, "size");
        let link = att(&atts, "link").to_php_string();
        let none = matches!(size, Value::String(size) if size == b"none");
        let (size_name, size_class);
        let image_size = match size {
            Value::Array(sides) => {
                let side = |i| size.get(&Key::Int(i)).cloned().unwrap_or_default();
                size_class = sides
                    .iter()
                    .map(|(_, side)| side.to_php_string())
                    .collect::<Vec<_>>()
                    .join(&b'x');
                Size::from_sides(&side(0), &side(1))
            }
            other => {
                size_class = other.to_php_string();
                size_name = String::from_utf8_lossy(&size_class).into_owned();
                Size::Named(&size_name)
            }
        };
        let linked_size = (!size.is_empty() && !none).then_some(image_size);
        if self.context.request.feed {
            return self.gallery_feed(&attachments, (linked_size, image_size), &link, &attr);
        }
        let tag = |name: &str, fallback: &[u8]| {
            let tag = tag_escape(&att(&atts, name).to_php_string());
            if post_allows(&tag) {
                tag
            } else {
                fallback.to_vec()
            }
        };
        let itemtag = tag("itemtag", b"dl");
        let captiontag = tag("captiontag", b"dd");
        let icontag = tag("icontag", b"dt");
        let columns = att(&atts, "columns").to_int();
        let selector = format!("gallery-{instance}");
        let mut gallery_style = Vec::new();
        if hooks.use_default_gallery_style(false) {
            let width = if columns > 0 { 100 / columns } else { 100 };
            gallery_style = format!(
                "\n\t\t<style>\n\t\t\t#{selector} {{\n\t\t\t\tmargin: auto;\n\t\t\t}}\n\t\t\t#{selector} .gallery-item {{\n\t\t\t\tfloat: left;\n\t\t\t\tmargin-top: 10px;\n\t\t\t\ttext-align: center;\n\t\t\t\twidth: {width}%;\n\t\t\t}}\n\t\t\t#{selector} img {{\n\t\t\t\tborder: 2px solid #cfcfcf;\n\t\t\t}}\n\t\t\t#{selector} .gallery-caption {{\n\t\t\t\tmargin-left: 0;\n\t\t\t}}\n\t\t\t/* see gallery_shortcode() in wp-includes/media.php */\n\t\t</style>\n\t\t"
            )
            .into_bytes();
        }
        let mut div = format!(
            "<div id='{selector}' class='gallery galleryid-{id} gallery-columns-{columns} gallery-size-"
        )
        .into_bytes();
        div.extend(sanitize_html_class(&size_class, b""));
        div.extend_from_slice(b"'>");
        let mut output = hooks.gallery_style([gallery_style, div].concat());
        // `wp_get_attachment_image()`'s class names the size by
        // `implode( 'x', $size )` of the array as given.
        let class = matches!(size, Value::Array(_)).then(|| {
            let class = [&b"attachment-"[..], &size_class, b" size-", &size_class].concat();
            ("class", class)
        });
        for attachment in &attachments {
            let excerpt = php_trim(&attachment.excerpt);
            let described = format!("{selector}-{}", attachment.id).into_bytes();
            let mut attributes: Vec<(&str, Vec<u8>)> = class.iter().cloned().collect();
            if !is_empty_string(excerpt) {
                attributes.push(("aria-describedby", described.clone()));
            }
            let image = match link.as_slice() {
                b"file" => self.attachment_link(attachment, linked_size, false, &attributes),
                b"none" => attachment_image(
                    site,
                    &self.context.loading,
                    attachment.id,
                    image_size,
                    &attributes,
                    &ImageOptions::default(),
                ),
                _ => self.attachment_link(attachment, linked_size, true, &attributes),
            };
            let meta = metadata(site, attachment.id).unwrap_or_default();
            let set = |key| meta.get_str(key).filter(|value| isset(Some(value)));
            let orientation: &[u8] = match (set("height"), set("width")) {
                (Some(height), Some(width)) if height.to_float() > width.to_float() => b"portrait",
                (Some(_), Some(_)) => b"landscape",
                _ => b"",
            };
            for piece in [
                &b"<"[..],
                &itemtag,
                b" class='gallery-item'>\n\t\t\t<",
                &icontag,
            ] {
                output.extend_from_slice(piece);
            }
            output.extend_from_slice(b" class='gallery-icon ");
            output.extend_from_slice(orientation);
            output.extend_from_slice(b"'>\n\t\t\t\t");
            output.extend(image);
            for piece in [&b"\n\t\t\t</"[..], &icontag, b">"] {
                output.extend_from_slice(piece);
            }
            if !is_empty_string(excerpt) {
                output.extend_from_slice(b"\n\t\t\t\t<");
                output.extend_from_slice(&captiontag);
                output.extend_from_slice(b" class='wp-caption-text gallery-caption' id='");
                output.extend_from_slice(&described);
                output.extend_from_slice(b"'>\n\t\t\t\t");
                output.extend(wptexturize(&attachment.excerpt, &CORE_SHORTCODE_TAGS));
                output.extend_from_slice(b"\n\t\t\t\t</");
                output.extend_from_slice(&captiontag);
                output.push(b'>');
            }
            for piece in [&b"</"[..], &itemtag, b">"] {
                output.extend_from_slice(piece);
            }
        }
        output.extend_from_slice(b"\n\t\t</div>\n");
        output
    }

    /// The images `get_posts()` finds for a gallery: those `list` names, or
    /// the children of `parent` but those it names.
    fn gallery_attachments(
        &self,
        atts: &[(String, Value)],
        parent: Option<u64>,
        list: &Value,
    ) -> Vec<Post> {
        let site = self.context.site;
        let ids = wp_parse_id_list(list);
        let mut query = PostQuery {
            post_types: vec!["attachment".into()],
            statuses: vec![StatusFilter {
                status: "inherit".into(),
                author: None,
                post_type: None,
            }],
            mime_types: vec!["image/%".into()],
            ..PostQuery::default()
        };
        match parent {
            // `get_children()`: all of them, but those excluded.
            Some(parent) => {
                query.limit = query_limit(-1);
                query.parents = vec![parent];
                query.exclude = ids;
            }
            // `get_posts()` asks for as many as it includes. With none
            // (only commas and white space in a string), `WP_Query` takes no
            // `post__in` and, for a `posts_per_page` of 0, the option's.
            None if ids.is_empty() => {
                let option = site.option("posts_per_page").unwrap_or_default();
                query.limit = query_limit(Value::String(option).to_int());
            }
            None => {
                query.limit = u32::try_from(ids.len()).unwrap_or(u32::MAX);
                query.include = Some(ids);
            }
        }
        // `WP_Query::parse_order()`.
        let ascending = matches!(att(atts, "order"), Value::String(order) if order.eq_ignore_ascii_case(b"ASC"));
        query.order = order_by(att(atts, "orderby"), ascending, &query);
        site.query(&query)
    }

    /// Whether a visitor may see the attachments of post `parent`: it is
    /// publicly viewable and needs no password.
    fn parent_readable(&self, parent: u64) -> bool {
        let site = self.context.site;
        let Some(parent) = site.post(parent) else {
            return false;
        };
        let status = crate::attachment::post_status(site, &parent);
        status == "publish"
            && !PRIVATE_TYPES.contains(&parent.post_type.as_str())
            && parent.password.is_empty()
    }

    /// A gallery in a feed: each image or link on a line of its own.
    fn gallery_feed(
        &self,
        attachments: &[Post],
        (size, image_size): (Option<Size<'_>>, Size<'_>),
        link: &[u8],
        attr: &[(Key, Value)],
    ) -> Vec<u8> {
        let site = self.context.site;
        let names: Vec<String> = attr
            .iter()
            .map(|(key, _)| String::from_utf8_lossy(&key.to_php_string()).into_owned())
            .collect();
        let all: Vec<(&str, Vec<u8>)> = names
            .iter()
            .map(String::as_str)
            .zip(attr.iter().map(|(_, value)| value.to_php_string()))
            .collect();
        let mut output = b"\n".to_vec();
        for attachment in attachments {
            output.extend(match link {
                link if is_empty_string(link) => self.attachment_link(attachment, size, true, &[]),
                b"none" => attachment_image(
                    site,
                    &self.context.loading,
                    attachment.id,
                    image_size,
                    &all,
                    &ImageOptions::default(),
                ),
                _ => self.attachment_link(attachment, size, false, &[]),
            });
            output.push(b'\n');
        }
        output
    }
}

/// `wp_parse_id_list()`: an array's scalars, or the parts of anything else
/// read as a string between commas and PCRE white space, through
/// `absint()`, each once.
pub(crate) fn wp_parse_id_list(list: &Value) -> Vec<u64> {
    let items: Vec<Value> = match list {
        Value::Array(items) => items
            .iter()
            .filter(|(_, item)| {
                matches!(
                    item,
                    Value::Bool(_) | Value::Int(_) | Value::Float(_) | Value::String(_)
                )
            })
            .map(|(_, item)| item.clone())
            .collect(),
        other => other
            .to_php_string()
            .split(|&b| b == b',' || is_pcre_space(b))
            .filter(|part| !part.is_empty())
            .map(|part| Value::String(part.to_vec()))
            .collect(),
    };
    let mut ids = Vec::new();
    for id in items.iter().map(absint) {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

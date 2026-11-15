// SPDX-License-Identifier: AGPL-3.0-or-later

//! The media widgets, over `WP_Widget_Media::widget()`: the image and
//! gallery widgets.
//!
//! A stored instance is unserialized as it was saved, so its values can be
//! of any type. Where PHP would compare a width or height loosely or
//! multiply it, a value that is not an integer or a numeric string is read
//! through `(int)` here, and a float is truncated; where PHP throws a
//! `TypeError` (an array, or a string that is no number, multiplied in
//! view; an array caption), the image renders without the high priority
//! check and the array as `Array`. In the gallery, an array `itemtag`,
//! `icontag` or `captiontag`, which `tag_escape()` refuses with a
//! `TypeError`, falls back to `dl`, `dt` or `dd`, and an image's height and
//! width are compared as `(float)` for its orientation, where PHP compares
//! strings that are no numbers as strings.

use hyscribe_kses::{esc_attr, esc_html, esc_url};
use hyscribe_model::{Post, PostQuery, StatusFilter};
use hyscribe_phpser::{Key, Value, absint, is_empty_string, isset, php_trim};

use super::{Args, titled, widget_title};
use crate::attachment::{
    ImageOptions, Size, attachment_image, attachment_is_image, attachment_url, metadata,
};
use crate::blocks::is_pcre_space;
use crate::formatting::{CORE_SHORTCODE_TAGS, wptexturize};
use crate::kses::{post_allows, wp_kses_post};
use crate::media::{Filter, Given, Loading};
use crate::post::sanitize_html_class;
use crate::query_loop::{order_by, query_limit};
use crate::site::Site;
use crate::template::Renderer;

/// `get_intermediate_image_sizes()`: core's sizes, and the two it adds with
/// `add_image_size()`.
pub(super) const IMAGE_SIZES: [&str; 6] = [
    "thumbnail",
    "medium",
    "medium_large",
    "large",
    "1536x1536",
    "2048x2048",
];

impl Renderer<'_> {
    /// `WP_Widget_Media_Image::widget()`: nothing without an attachment or
    /// a URL; the wrapper and title alone for an attachment that is no
    /// image and no URL; else the title and the image, linked and
    /// captioned as the instance asks.
    pub(super) fn media_image_widget(&self, args: &Args, instance: &Value) -> Vec<u8> {
        let site = self.context.site;
        // `wp_parse_args()` of the schema's defaults: a key given keeps
        // its value, even null.
        let field = |name: &str, default: Value| instance.get_str(name).cloned().unwrap_or(default);
        let text = |name: &str| field(name, Value::String(Vec::new()));
        let attachment_id = field("attachment_id", Value::Int(0));
        let url = text("url");
        let attachment = attachment_post(site, &attachment_id);
        if attachment.is_none() && url.is_empty() {
            return Vec::new();
        }
        let mut out = args.before_widget.clone();
        out.extend(titled(args, &widget_title(instance, b"")));
        let image = attachment.filter(|post| attachment_is_image(site, post));
        let (width, height) = (
            field("width", Value::Int(0)),
            field("height", Value::Int(0)),
        );
        let caption;
        let caption_width;
        let mut html = match &image {
            Some(post) => {
                caption = match instance.get_str("caption") {
                    None => Value::String(Vec::new()),
                    Some(Value::Null) => Value::String(post.excerpt.clone()),
                    Some(given) if is_empty_string(php_trim(&given.to_php_string())) => {
                        Value::String(Vec::new())
                    }
                    Some(given) => given.clone(),
                };
                let size = field("size", Value::String(b"medium".to_vec()));
                // `in_array( $size, ..., true )`: `custom` is not listed.
                let named = match &size {
                    Value::String(name) => IMAGE_SIZES
                        .into_iter()
                        .chain(["full"])
                        .find(|known| known.as_bytes() == name.as_slice()),
                    _ => None,
                };
                let (size, class) = match named {
                    Some(name) => {
                        caption_width = size_from_meta(site, post.id, name);
                        (Size::Named(name), name.as_bytes().to_vec())
                    }
                    None => {
                        caption_width = width.to_int();
                        let class =
                            [width.to_php_string(), b"x".to_vec(), height.to_php_string()].concat();
                        (Size::from_sides(&width, &height), class)
                    }
                };
                let mut classes =
                    [b"image wp-image-", post.id.to_string().as_bytes(), b" "].concat();
                classes.extend(text("image_classes").to_php_string());
                for prefix in [&b" attachment-"[..], b" size-"] {
                    classes.extend_from_slice(prefix);
                    classes.extend_from_slice(&class);
                }
                let mut attr = vec![
                    ("class", classes),
                    ("style", b"max-width: 100%; height: auto;".to_vec()),
                ];
                let title = text("image_title");
                if !title.is_empty() {
                    attr.push(("title", title.to_php_string()));
                }
                let alt = text("alt");
                if !alt.is_empty() {
                    attr.push(("alt", alt.to_php_string()));
                }
                attachment_image(
                    site,
                    &self.context.loading,
                    post.id,
                    size,
                    &attr,
                    &ImageOptions::default(),
                )
            }
            None => {
                if url.is_empty() {
                    out.extend_from_slice(&args.after_widget);
                    return out;
                }
                caption = text("caption");
                caption_width = width.to_int();
                url_image(
                    &self.context.loading,
                    &url,
                    &text("alt"),
                    &text("image_classes"),
                    &width,
                    &height,
                )
            }
        };
        let link_type = field("link_type", Value::String(b"custom".to_vec()));
        let link_type = match &link_type {
            Value::String(kind) => kind.as_slice(),
            _ => b"",
        };
        let link_url = text("link_url");
        let href = match (link_type, &image) {
            (b"file", Some(post)) => attachment_url(site, post.id).unwrap_or_default(),
            (b"file", None) => url.to_php_string(),
            (b"post", Some(post)) => self.context.permalink(post),
            (b"custom", _) if !link_url.is_empty() => link_url.to_php_string(),
            _ => Vec::new(),
        };
        if !is_empty_string(&href) {
            let mut link = [b"<a href=\"", &esc_url(&href)[..], b"\""].concat();
            for (name, attribute) in [("link_classes", "class"), ("link_rel", "rel")] {
                let value = text(name);
                if !value.is_empty() {
                    link.extend(format!(" {attribute}=\"").into_bytes());
                    link.extend(esc_attr(&value.to_php_string()).into_bytes());
                    link.push(b'"');
                }
            }
            if !text("link_target_blank").is_empty() {
                link.extend_from_slice(b" target=\"_blank\"");
            }
            link.push(b'>');
            link.extend(html);
            link.extend_from_slice(b"</a>");
            html = link;
        }
        if !caption.is_empty() {
            let upload_host = self.context.upload_host();
            html =
                img_caption_shortcode(&upload_host, caption_width, &caption.to_php_string(), html);
        }
        out.extend(html);
        out.extend_from_slice(&args.after_widget);
        out
    }

    /// `WP_Widget_Media_Gallery::widget()`: nothing unless `ids` lists
    /// attachments only (ID 0 is the global post, which `get_post_type( 0 )`
    /// reads), else the title and `gallery_shortcode()` of the instance,
    /// linked as `link_type` asks and in random order for `orderby_random`.
    pub(super) fn media_gallery_widget(&mut self, args: &Args, instance: &Value) -> Vec<u8> {
        let site = self.context.site;
        let ids = instance.get_str("ids").filter(|ids| !ids.is_empty());
        let Some(ids) = ids else {
            return Vec::new();
        };
        // `get_post_type( 0 )` is the global post's.
        let is_attachment = |id: u64| {
            match id {
                0 => self.context.global_post(),
                id => site.post(id),
            }
            .is_some_and(|post| post.post_type == "attachment")
        };
        let ids = wp_parse_id_list(ids);
        if !ids.iter().copied().all(is_attachment) {
            return Vec::new();
        }
        let mut out = args.before_widget.clone();
        out.extend(titled(args, &widget_title(instance, b"")));
        out.extend(self.gallery_shortcode(instance, ids));
        out.extend_from_slice(&args.after_widget);
        out
    }

    /// `gallery_shortcode()` of the widget's instance, merged over its
    /// defaults, with `link` from `link_type` and `ids` parsed, under the
    /// `html5` gallery support block themes have. Not ported: its feed
    /// output (`is_feed()`), where PHP prints each image or link on a line
    /// of its own; a feed gets the page's gallery here.
    fn gallery_shortcode(&mut self, instance: &Value, ids: Vec<u64>) -> Vec<u8> {
        self.gallery_instances += 1;
        let selector = format!("gallery-{}", self.gallery_instances);
        let site = self.context.site;
        let given = |name: &str| instance.get_str(name).cloned();
        let text = |value: &str| Value::String(value.as_bytes().to_vec());
        // `shortcode_atts()`: a key the attributes have keeps its value.
        let orderby = if given("orderby_random").is_some_and(|random| !random.is_empty()) {
            text("rand")
        } else {
            given("orderby")
                .filter(|orderby| !orderby.is_empty())
                .unwrap_or(text("post__in"))
        };
        let att = |name: &str, default: Value| given(name).unwrap_or(default);
        let order = att("order", text("ASC"));
        let id = given("id").map_or_else(
            || self.context.global_post().map_or(0, |post| post.id as i64),
            |id| id.to_int(),
        );
        let tag = |name: &str, default: &str, fallback: &[u8]| {
            let tag = tag_escape(&att(name, text(default)).to_php_string());
            if post_allows(&tag) {
                tag
            } else {
                fallback.to_vec()
            }
        };
        let itemtag = tag("itemtag", "figure", b"dl");
        let icontag = tag("icontag", "div", b"dt");
        let captiontag = tag("captiontag", "figcaption", b"dd");
        let columns = att("columns", Value::Int(3)).to_int();
        let size = att("size", text("thumbnail"));
        let link = given("link_type").unwrap_or(text("post"));
        let link = match &link {
            Value::String(link) => link.as_slice(),
            _ => b"",
        };
        // `get_posts()` asks for as many as it includes. With none (no
        // scalar in an `ids` array, or only commas and white space in a
        // string), `WP_Query` takes no `post__in` and, for a
        // `posts_per_page` of 0, the option's.
        let (include, limit) = if ids.is_empty() {
            let option = site.option("posts_per_page").unwrap_or_default();
            (None, query_limit(Value::String(option).to_int()))
        } else {
            let limit = u32::try_from(ids.len()).unwrap_or(u32::MAX);
            (Some(ids), limit)
        };
        let mut query = PostQuery {
            post_types: vec!["attachment".into()],
            statuses: vec![StatusFilter {
                status: "inherit".into(),
                author: None,
            }],
            mime_types: vec!["image/%".into()],
            limit,
            include,
            ..PostQuery::default()
        };
        // `WP_Query::parse_order()`.
        let ascending =
            matches!(&order, Value::String(order) if order.eq_ignore_ascii_case(b"ASC"));
        query.order = order_by(&orderby, ascending, &query);
        let attachments = site.query(&query);
        if attachments.is_empty() {
            return Vec::new();
        }
        let (size_name, size_class);
        let image_size = match &size {
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
        let none = matches!(&size, Value::String(size) if size == b"none");
        let linked_size = (!size.is_empty() && !none).then_some(image_size);
        let mut output = format!(
            "<div id='{selector}' class='gallery galleryid-{id} gallery-columns-{columns} gallery-size-"
        )
        .into_bytes();
        output.extend(sanitize_html_class(&size_class, b""));
        output.extend_from_slice(b"'>");
        // `wp_get_attachment_image()`'s class names the size by
        // `implode( 'x', $size )` of the array as given.
        let class = matches!(size, Value::Array(_)).then(|| {
            let class = [&b"attachment-"[..], &size_class, b" size-", &size_class].concat();
            ("class", class)
        });
        for attachment in &attachments {
            let excerpt = php_trim(&attachment.excerpt);
            let described = format!("{selector}-{}", attachment.id).into_bytes();
            let mut attr: Vec<(&str, Vec<u8>)> = class.iter().cloned().collect();
            if !is_empty_string(excerpt) {
                attr.push(("aria-describedby", described.clone()));
            }
            let image = match link {
                b"file" => self.attachment_link(attachment, linked_size, false, &attr),
                b"none" => attachment_image(
                    site,
                    &self.context.loading,
                    attachment.id,
                    image_size,
                    &attr,
                    &ImageOptions::default(),
                ),
                _ => self.attachment_link(attachment, linked_size, true, &attr),
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

    /// `wp_get_attachment_link( $id, $size, $permalink, false, false, $attr )`
    /// with `size` none when it is empty or `none`: the image, else the
    /// title, else the file's name, linked to the file or the attachment's
    /// page; `Missing Attachment` when it has no URL.
    fn attachment_link(
        &self,
        attachment: &Post,
        size: Option<Size<'_>>,
        permalink: bool,
        attr: &[(&str, Vec<u8>)],
    ) -> Vec<u8> {
        let site = self.context.site;
        let Some(url) = attachment_url(site, attachment.id) else {
            return b"Missing Attachment".to_vec();
        };
        let url = if permalink {
            self.context.permalink(attachment)
        } else {
            url
        };
        let mut text = size.map_or_else(Vec::new, |size| {
            attachment_image(
                site,
                &self.context.loading,
                attachment.id,
                size,
                attr,
                &ImageOptions::default(),
            )
        });
        if php_trim(&text).is_empty() {
            text = attachment.title.clone();
        }
        if php_trim(&text).is_empty() {
            let file = site
                .post_meta(attachment.id, "_wp_attached_file")
                .unwrap_or_default();
            text = esc_html(&file_name(&file)).into_bytes();
        }
        [b"<a href='", &esc_url(&url)[..], b"'>", &text, b"</a>"].concat()
    }
}

/// `pathinfo( $path, PATHINFO_FILENAME )`: the last part after a `/`,
/// trailing ones aside, up to its last dot. Unlike `wp_basename()`, PHP's
/// `basename()` keeps a backslash.
fn file_name(path: &[u8]) -> Vec<u8> {
    let end = path
        .iter()
        .rposition(|&b| b != b'/')
        .map_or(0, |last| last + 1);
    let path = path.get(..end).unwrap_or_default();
    let start = path
        .iter()
        .rposition(|&b| b == b'/')
        .map_or(0, |slash| slash + 1);
    let name = path.get(start..).unwrap_or_default();
    let dot = name.iter().rposition(|&b| b == b'.').unwrap_or(name.len());
    name.get(..dot).unwrap_or(name).to_vec()
}

/// `tag_escape()`: letters, digits, `-`, `_` and `:`, in lower case.
fn tag_escape(tag: &[u8]) -> Vec<u8> {
    tag.iter()
        .filter(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':'))
        .map(u8::to_ascii_lowercase)
        .collect()
}

/// `wp_parse_id_list()`: an array's scalars, or the parts of anything else
/// read as a string between commas and PCRE white space, through
/// `absint()`, each once.
fn wp_parse_id_list(list: &Value) -> Vec<u64> {
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

/// `get_post( $id )` when it is an attachment, as `has_content()` and
/// `is_attachment_with_mime_type()` look it up: none for an empty ID.
fn attachment_post(site: &dyn Site, id: &Value) -> Option<Post> {
    if id.is_empty() {
        return None;
    }
    let id = u64::try_from(id.to_int()).ok()?;
    site.post(id).filter(|post| post.post_type == "attachment")
}

/// `_wp_get_image_size_from_meta()`'s width, 0 when the size is not stored.
fn size_from_meta(site: &dyn Site, id: u64, size: &str) -> i64 {
    let meta = metadata(site, id).unwrap_or_default();
    let sized = if size == "full" {
        Some(&meta)
    } else {
        meta.get_path(&["sizes", size])
            .filter(|sized| !sized.is_empty())
    };
    sized
        .and_then(|sized| sized.get_str("width"))
        .map_or(0, |width| i64::try_from(absint(width)).unwrap_or(i64::MAX))
}

/// The image widget's `<img>` for a URL with no attachment: a width or
/// height of integer 0 is left empty, and the loading attributes are those
/// of the `widget_media_image` context.
fn url_image(
    loading: &Loading,
    url: &Value,
    alt: &Value,
    classes: &Value,
    width: &Value,
    height: &Value,
) -> Vec<u8> {
    let dimension = |value: &Value| match value {
        Value::Int(0) => Value::String(Vec::new()),
        other => other.clone(),
    };
    let (width, height) = (dimension(width), dimension(height));
    // `isset()` of both, and the product PHP multiplies them to.
    let given = Given {
        width: (!matches!(width, Value::Null)).then(|| width.to_int()),
        height: (!matches!(height, Value::Null)).then(|| height.to_int()),
        ..Given::default()
    };
    let optimization = loading.optimization(true, &given, Filter::None);
    let mut attrs = vec![
        ("class", [&b"image "[..], &classes.to_php_string()].concat()),
        ("src", url.to_php_string()),
        ("alt", alt.to_php_string()),
        ("width", width.to_php_string()),
        ("height", height.to_php_string()),
    ];
    for (name, value) in [
        ("decoding", optimization.decoding),
        ("fetchpriority", optimization.fetchpriority),
        ("loading", optimization.loading),
    ] {
        if let Some(value) = value {
            attrs.push((name, value.as_bytes().to_vec()));
        }
    }
    let mut image = b"<img".to_vec();
    for (name, value) in attrs {
        image.extend(format!(" {name}=\"{}\"", esc_attr(&value)).into_bytes());
    }
    image.extend_from_slice(b" />");
    image
}

/// `img_caption_shortcode( array( 'width' => $width, 'caption' => $caption ),
/// $content )` under the `html5` caption support block themes have: the
/// content as given, as shortcodes are not run.
fn img_caption_shortcode(
    upload_host: &[u8],
    width: i64,
    caption: &[u8],
    content: Vec<u8>,
) -> Vec<u8> {
    let caption = if caption.contains(&b'<') {
        wp_kses_post(caption, upload_host)
    } else {
        caption.to_vec()
    };
    if width < 1 || is_empty_string(&caption) {
        return content;
    }
    [
        format!("<figure style=\"width: {width}px\" class=\"wp-caption alignnone\">").as_bytes(),
        &content,
        b"<figcaption class=\"wp-caption-text\">",
        &caption,
        b"</figcaption></figure>",
    ]
    .concat()
}

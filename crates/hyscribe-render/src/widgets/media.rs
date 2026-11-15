// SPDX-License-Identifier: AGPL-3.0-or-later

//! The media widgets, over `WP_Widget_Media::widget()`: the image widget.
//!
//! A stored instance is unserialized as it was saved, so its values can be
//! of any type. Where PHP would compare a width or height loosely or
//! multiply it, a value that is not an integer or a numeric string is read
//! through `(int)` here, and a float is truncated; where PHP throws a
//! `TypeError` (an array, or a string that is no number, multiplied in
//! view; an array caption), the image renders without the high priority
//! check and the array as `Array`.

use hyscribe_kses::{esc_attr, esc_url};
use hyscribe_model::Post;
use hyscribe_phpser::{Value, absint, is_empty_string, php_trim};

use super::{Args, titled, widget_title};
use crate::attachment::{
    ImageOptions, Size, attachment_image, attachment_is_image, attachment_url, metadata,
};
use crate::kses::wp_kses_post;
use crate::media::{Given, Loading};
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
                        let size = Size::Dimensions {
                            width: width.to_int(),
                            height: height.to_int(),
                            original_ratio: matches!(width, Value::Int(0))
                                || matches!(height, Value::Int(0)),
                        };
                        (size, class)
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
    let optimization = loading.optimization(true, &given, false);
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

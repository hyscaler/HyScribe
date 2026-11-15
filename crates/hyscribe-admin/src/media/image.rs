// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_get_attachment_image( $id, array( 60, 60 ), true, array( 'alt' =>
//! '' ) )`, the media list's thumbnail, and the comments list's at 80 by 60
//! with its alt text: the smallest stored size that fits, or the icon of
//! the file's type.

use hyscribe_kses::{esc_attr, strip_tags};
use hyscribe_model::Post;
use hyscribe_phpser::{Maybe, Value, isset, maybe_unserialize, wp_basename};
use hyscribe_render::attachment::{
    constrain_dimensions, file_url, is_image, matches_ratio, srcset, upload_baseurl,
};
use hyscribe_render::template::replace;

use crate::posts::PostsSite;

/// The list's box, `array( 60, 60 )`.
const LIST_BOX: (i64, i64) = (60, 60);

/// `wp_get_ext_types()`, less the images, which have no icon.
const EXT_TYPES: [(&str, &[&str]); 8] = [
    (
        "audio",
        &[
            "aac", "ac3", "aif", "aiff", "flac", "m3a", "m4a", "m4b", "mka", "mp1", "mp2", "mp3",
            "ogg", "oga", "ram", "wav", "wma",
        ],
    ),
    (
        "video",
        &[
            "3g2", "3gp", "3gpp", "asf", "avi", "divx", "dv", "flv", "m4v", "mkv", "mov", "mp4",
            "mpeg", "mpg", "mpv", "ogm", "ogv", "qt", "rm", "vob", "wmv",
        ],
    ),
    (
        "document",
        &[
            "doc", "docx", "docm", "dotm", "odt", "pages", "pdf", "xps", "oxps", "rtf", "wp",
            "wpd", "psd", "xcf",
        ],
    ),
    (
        "spreadsheet",
        &["numbers", "ods", "xls", "xlsx", "xlsm", "xlsb"],
    ),
    (
        "interactive",
        &[
            "swf", "key", "ppt", "pptx", "pptm", "pps", "ppsx", "ppsm", "sldx", "sldm", "odp",
        ],
    ),
    ("text", &["asc", "csv", "tsv", "txt"]),
    (
        "archive",
        &[
            "bz2", "cab", "dmg", "gz", "rar", "sea", "sit", "sqx", "tar", "tgz", "zip", "7z",
        ],
    ),
    ("code", &["css", "htm", "html", "php", "js"]),
];

/// `wp_get_attachment_metadata()`: the stored value, unserialized.
pub(super) fn metadata(site: &dyn PostsSite, post: &Post) -> Value {
    match maybe_unserialize(
        &site
            .meta(post, "_wp_attachment_metadata")
            .unwrap_or_default(),
    ) {
        Maybe::Value(value) => value,
        _ => Value::Null,
    }
}

/// `wp_get_attachment_url()`, `None` for a post that is not an attachment.
pub(super) fn attachment_url(site: &dyn PostsSite, post: &Post) -> Option<Vec<u8>> {
    if post.post_type != "attachment" {
        return None;
    }
    let file = site.meta(post, "_wp_attached_file").unwrap_or_default();
    let options = |name: &str| site.option(name).unwrap_or_default();
    file_url(&file, || post.guid.clone(), &upload_baseurl(&options))
}

fn dimensions(image: &Value) -> (i64, i64) {
    let get = |key| image.get_str(key).map_or(0, Value::to_int);
    (get("width"), get("height"))
}

/// `image_get_intermediate_size( $id, $size )` of a box: the file and
/// size, constrained to the box, of the smallest stored size of the box's
/// ratio that covers it, or else of the thumbnail if it is that wide.
fn intermediate(meta: &Value, (w, h): (i64, i64)) -> Option<(Vec<u8>, i64, i64)> {
    let sizes = meta.get_str("sizes").filter(|sizes| !sizes.is_empty())?;
    let mut candidates: Vec<(i64, &Value)> = Vec::new();
    for (_, data) in sizes.as_array().unwrap_or_default() {
        let (width, height) = dimensions(data);
        let area = width.saturating_mul(height);
        // The box's own size is the smallest that covers it.
        if width == w && height == h {
            candidates.push((area, data));
            break;
        }
        if width >= w && height >= h && matches_ratio(width, height, w, h) {
            candidates.retain(|(other, _)| *other != area);
            candidates.push((area, data));
        }
    }
    let thumbnail = meta
        .get_path(&["sizes", "thumbnail"])
        .filter(|thumb| !thumb.is_empty() && dimensions(thumb).0 >= w.max(h));
    let data = match candidates.into_iter().min_by_key(|(area, _)| *area) {
        Some((_, data)) => data,
        None => thumbnail?,
    };
    let (width, height) = dimensions(data);
    let (width, height) = constrain_dimensions(width, height, w, h);
    let file = data.get_str("file").map(Value::to_php_string);
    Some((file.unwrap_or_default(), width, height))
}

/// `image_downsize( $id, $size )` of an attachment at `url`, an `image` or
/// not, with metadata `meta`, to a box: the URL, width and height.
fn downsize(
    mut url: Vec<u8>,
    meta: &Value,
    image: bool,
    size: (i64, i64),
) -> Option<(Vec<u8>, i64, i64)> {
    let mut name = wp_basename(&url);
    let (mut width, mut height) = (0, 0);
    if !image {
        let full = meta
            .get_path(&["sizes", "full"])
            .filter(|full| !full.is_empty())?;
        let full_file = full.get_str("file").map(Value::to_php_string);
        let full_file = full_file.unwrap_or_default();
        url = replace(&url, &name, &full_file);
        name = full_file;
        (width, height) = dimensions(full);
    }
    if let Some((sized, w, h)) = intermediate(meta, size) {
        url = replace(&url, &name, &sized);
        (width, height) = (w, h);
    }
    if width == 0 && height == 0 && isset(meta.get_str("width")) && isset(meta.get_str("height")) {
        (width, height) = dimensions(meta);
    }
    let (width, height) = constrain_dimensions(width, height, size.0, size.1);
    Some((url, width, height))
}

/// `wp_mime_type_icon( $id, '.svg' )`'s icon name: the type of the file's
/// extension or of its MIME type that has an icon, else the default one.
fn icon_name(file: &[u8], mime: &str) -> &'static str {
    let name = wp_basename(file);
    let extension = name
        .rsplit(|b| *b == b'.')
        .next()
        .filter(|e| e.len() < name.len())
        .map(|e| String::from_utf8_lossy(e).to_ascii_lowercase())
        .unwrap_or_default();
    let ext_type = EXT_TYPES
        .iter()
        .find(|(_, extensions)| extensions.contains(&extension.as_str()))
        .map(|(group, _)| *group);
    let group = mime.split_once('/').map(|(group, _)| group);
    [ext_type, group]
        .into_iter()
        .flatten()
        .find_map(|candidate| {
            EXT_TYPES
                .iter()
                .map(|(icon, _)| *icon)
                .find(|icon| *icon == candidate)
        })
        .unwrap_or("default")
}

/// `wp_get_attachment_image_src( $id, $size, true )`: the attachment's
/// image for a box, or its type's icon.
pub(super) fn image_src(
    site: &dyn PostsSite,
    post: &Post,
    size: (i64, i64),
) -> (Vec<u8>, i64, i64) {
    let file = site.meta(post, "_wp_attached_file").unwrap_or_default();
    let image = is_image(&file, &post.mime_type);
    let found = attachment_url(site, post)
        .and_then(|url| downsize(url, &metadata(site, post), image, size));
    found.unwrap_or_else(|| {
        let icon = icon_name(&file, &post.mime_type);
        let src = format!("{}/wp-includes/images/media/{icon}.svg", site.site_url());
        (src.into_bytes(), 48, 64)
    })
}

/// The thumbnail of attachment `id`: its image, or its type's icon.
pub(super) fn thumbnail(site: &dyn PostsSite, id: u64) -> String {
    let post = site.post(id).unwrap_or_default();
    boxed(site, &post, LIST_BOX, b"")
}

/// The comments list's thumbnail of attachment `post`, with its alt text.
pub(crate) fn response_thumbnail(site: &dyn PostsSite, post: &Post) -> String {
    let alt = site
        .meta(post, "_wp_attachment_image_alt")
        .unwrap_or_default();
    let alt = strip_tags(&alt);
    boxed(site, post, (80, 60), alt.trim_ascii())
}

/// The `img` of `post` for the box `size`, with `alt`.
fn boxed(site: &dyn PostsSite, post: &Post, size: (i64, i64), alt: &[u8]) -> String {
    let (src, width, height) = image_src(site, post, size);
    let options = |name: &str| site.option(name).unwrap_or_default();
    let class = format!("{}x{}", size.0, size.1);
    let meta = metadata(site, post);
    img(
        &src,
        (width, height),
        &meta,
        &upload_baseurl(&options),
        &class,
        alt,
    )
}

/// The `img` of `src` sized `size`, lazy as every image of the admin is,
/// with a `srcset` of the sizes in `meta` under the uploads' `base` URL.
fn img(src: &[u8], size: (i64, i64), meta: &Value, base: &[u8], class: &str, alt: &[u8]) -> String {
    let (width, height) = size;
    let mut html = format!(
        "<img width=\"{width}\" height=\"{height}\" src=\"{}\" class=\"attachment-{class} size-{class}\" alt=\"{}\" decoding=\"async\" loading=\"lazy\"",
        esc_attr(src),
        esc_attr(alt)
    );
    let size = (width.abs(), height.abs());
    if let Some(set) = meta.as_array().and_then(|_| srcset(size, src, meta, base)) {
        // `wp_img_tag_add_auto_sizes`: lazy images size themselves.
        html.push_str(&format!(
            " srcset=\"{}\" sizes=\"auto, (max-width: {}px) 100vw, {}px\"",
            esc_attr(set.as_bytes()),
            size.0,
            size.0
        ));
    }
    html.push_str(" />");
    html
}

#[cfg(test)]
mod tests;

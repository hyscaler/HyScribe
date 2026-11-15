// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_get_attachment_image()` for a named size or a width and height, and
//! what it builds on: an attachment's URL and stored metadata, the size's
//! dimensions, and `srcset` and `sizes` from the sizes stored.

use hyscribe_kses::{esc_attr, strip_tags};
use hyscribe_phpser::{
    Maybe, Value, float_to_string, is_empty_string, isset, maybe_unserialize, php_trim, wp_basename,
};

use crate::formatting::{contains, dirname, find, untrailingslashit};
use crate::media::{Filter, Given, Loading};
use crate::site::Site;
use crate::template::replace;
use hyscribe_model::Post;

/// The `$size` of `image_downsize()` and `wp_get_attachment_image()`.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Size<'a> {
    /// A registered size's name.
    Named(&'a str),
    /// A width and height, the best stored size for them. `original_ratio`
    /// is PHP's `0 === $size[0] || 0 === $size[1]`: a side that is the
    /// integer 0, where a stored size's ratio is compared with the
    /// original's rather than the size's; another zero (`'0'`, null) is
    /// compared as the size.
    Dimensions {
        width: i64,
        height: i64,
        original_ratio: bool,
    },
}

impl Size<'_> {
    /// `array( $width, $height )`, each read through `(int)`.
    pub(crate) fn from_sides(width: &Value, height: &Value) -> Size<'static> {
        Size::Dimensions {
            width: width.to_int(),
            height: height.to_int(),
            original_ratio: matches!(width, Value::Int(0)) || matches!(height, Value::Int(0)),
        }
    }
}

impl std::fmt::Display for Size<'_> {
    /// The size's class name: its name, or `implode( 'x', $size )` of the
    /// integer sides.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Size::Named(name) => f.write_str(name),
            Size::Dimensions { width, height, .. } => write!(f, "{width}x{height}"),
        }
    }
}

/// What callers add around `wp_get_attachment_image()` with filters.
#[derive(Default)]
pub(crate) struct ImageOptions<'a> {
    /// `get_the_post_thumbnail()`'s `wp-post-image` class.
    pub(crate) post_thumbnail: bool,
    /// A `wp_get_attachment_image_src` filter on the width and height.
    pub(crate) resize: Option<&'a dyn Fn(i64, i64) -> (i64, i64)>,
}

/// `wp_get_attachment_image( $id, $size, false, $attr )`: `attr` overrides
/// or adds attributes, in order; a `loading` or `fetchpriority` in it is
/// read as given, and an empty one left out. A `width`, `height` or
/// `decoding` in it is not ported, as no caller gives one.
pub(crate) fn wp_get_attachment_image(
    site: &dyn Site,
    loading: &Loading,
    id: u64,
    size: &str,
    attr: &[(&str, Vec<u8>)],
    options: &ImageOptions<'_>,
) -> Vec<u8> {
    attachment_image(site, loading, id, Size::Named(size), attr, options)
}

/// `wp_get_attachment_image()` for any `size`.
pub(crate) fn attachment_image(
    site: &dyn Site,
    loading: &Loading,
    id: u64,
    size: Size<'_>,
    attr: &[(&str, Vec<u8>)],
    options: &ImageOptions<'_>,
) -> Vec<u8> {
    let Some((src, width, height)) = downsize(site, id, size) else {
        return Vec::new();
    };
    let (width, height) = match options.resize {
        Some(resize) if width != 0 && height != 0 => resize(width, height),
        _ => (width, height),
    };
    let mut attrs: Vec<(String, Vec<u8>)> = vec![
        ("src".into(), src.clone()),
        (
            "class".into(),
            format!("attachment-{size} size-{size}").into_bytes(),
        ),
        ("alt".into(), attachment_alt(site, id)),
    ];
    for (name, value) in attr {
        set(&mut attrs, name, value.clone());
    }
    let given = Given {
        width: Some(width),
        height: Some(height),
        loading: get(&attrs, "loading"),
        fetchpriority: get(&attrs, "fetchpriority"),
    };
    let optimization = loading.optimization(true, &given, Filter::None);
    let extra = [
        ("decoding", optimization.decoding),
        ("fetchpriority", optimization.fetchpriority),
        ("loading", optimization.loading),
    ];
    for (name, value) in extra {
        if let Some(value) = value {
            set(&mut attrs, name, value.as_bytes().to_vec());
        }
    }
    // A `loading` or `fetchpriority` given as false is left out.
    attrs.retain(|(name, value)| {
        !(matches!(name.as_str(), "loading" | "fetchpriority") && is_empty_string(value))
    });
    if get(&attrs, "srcset").is_none_or(<[u8]>::is_empty) {
        let size = (
            i64::try_from(width.unsigned_abs()).unwrap_or(i64::MAX),
            i64::try_from(height.unsigned_abs()).unwrap_or(i64::MAX),
        );
        let made = metadata(site, id)
            .and_then(|meta| srcset(size, &src, &meta, &upload_baseurl(&site_options(site))));
        // A `srcset` means a width, so `sizes` too.
        if let Some(srcset) = made {
            set(&mut attrs, "srcset", srcset.into_bytes());
            if get(&attrs, "sizes").is_none_or(<[u8]>::is_empty) {
                let sizes = format!("(max-width: {0}px) 100vw, {0}px", size.0);
                set(&mut attrs, "sizes", sizes.into_bytes());
            }
        }
    }
    // `wp_img_tag_add_auto_sizes`: lazy images size themselves.
    let lazy = get(&attrs, "loading") == Some(b"lazy");
    if let Some(sizes) = get(&attrs, "sizes").filter(|_| lazy) {
        let first = sizes.split(|b| *b == b',').next().unwrap_or_default();
        let first = first.trim_ascii().to_ascii_lowercase();
        if first != b"auto" {
            let sizes = [b"auto, ", sizes].concat();
            set(&mut attrs, "sizes", sizes);
        }
    }
    if options.post_thumbnail {
        let class = [get(&attrs, "class").unwrap_or_default(), b" wp-post-image"].concat();
        set(&mut attrs, "class", class);
    }
    // `absint()`, as `image_hwstring()` gets them.
    let (width, height) = (width.unsigned_abs(), height.unsigned_abs());
    let mut html = String::from("<img");
    for (name, value) in [("width", width), ("height", height)] {
        if value != 0 {
            html.push_str(&format!(" {name}=\"{value}\""));
        }
    }
    for (name, value) in attrs.iter().filter(|(n, _)| n != "width" && n != "height") {
        html.push_str(&format!(" {name}=\"{}\"", esc_attr(value)));
    }
    html.push_str(" />");
    html.into_bytes()
}

fn get<'a>(attrs: &'a [(String, Vec<u8>)], name: &str) -> Option<&'a [u8]> {
    attrs
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_slice())
}

/// `$attrs[ $name ] = $value`: in place, or added last.
fn set(attrs: &mut Vec<(String, Vec<u8>)>, name: &str, value: Vec<u8>) {
    match attrs.iter_mut().find(|(n, _)| n == name) {
        Some(found) => found.1 = value,
        None => attrs.push((name.to_string(), value)),
    }
}

/// `get_post_thumbnail_id( $post )`: 0 without a post or thumbnail, as no
/// post has ID 0 or below.
pub(crate) fn post_thumbnail_id(site: &dyn Site, post: Option<u64>) -> u64 {
    post.filter(|id| site.post(*id).is_some())
        .and_then(|id| site.post_meta(id, "_thumbnail_id"))
        .map_or(0, |id| {
            u64::try_from(Value::String(id).to_int()).unwrap_or(0)
        })
}

/// The attachment's alt text, `trim( strip_tags( ... ) )`.
pub(crate) fn attachment_alt(site: &dyn Site, id: u64) -> Vec<u8> {
    let alt = site
        .post_meta(id, "_wp_attachment_image_alt")
        .unwrap_or_default();
    php_trim(&strip_tags(&alt)).to_vec()
}

/// A focal point as `object-position`: each axis `round( $x * 100 )`%.
pub(crate) fn focal_position(point: &Value) -> String {
    let percent =
        |axis| float_to_string((point.get_str(axis).map_or(0.0, Value::to_float) * 100.0).round());
    format!("{}% {}%", percent("x"), percent("y"))
}

/// `wp_get_upload_dir()['baseurl']` from the options `option` reads.
pub fn upload_baseurl(option: &dyn Fn(&str) -> Vec<u8>) -> Vec<u8> {
    let url_path = option("upload_url_path");
    if !is_empty_string(&url_path) {
        return url_path;
    }
    let siteurl = option("siteurl");
    let path = option("upload_path");
    let path = php_trim(&path);
    // A path inside `ABSPATH` is relative; the default and absolute ones
    // sit under `WP_CONTENT_URL`.
    if is_empty_string(path) || path == b"wp-content/uploads" || path.starts_with(b"/") {
        [&siteurl[..], b"/wp-content/uploads"].concat()
    } else {
        [untrailingslashit(&siteurl), b"/", path].concat()
    }
}

/// The site's options, as `upload_baseurl()` reads them.
fn site_options(site: &dyn Site) -> impl Fn(&str) -> Vec<u8> {
    |name| site.option(name).unwrap_or_default()
}

/// `wp_get_attachment_url()`, from `_wp_attached_file`.
pub fn attachment_url(site: &dyn Site, id: u64) -> Option<Vec<u8>> {
    let file = site.post_meta(id, "_wp_attached_file").unwrap_or_default();
    let guid = || site.post(id).map(|post| post.guid).unwrap_or_default();
    file_url(&file, guid, &upload_baseurl(&site_options(site)))
}

/// `wp_get_attachment_url()` of an attachment whose `_wp_attached_file` is
/// `file`, under the uploads' `base` URL; its `guid` without one.
pub fn file_url(file: &[u8], guid: impl FnOnce() -> Vec<u8>, base: &[u8]) -> Option<Vec<u8>> {
    let url = if is_empty_string(file) {
        guid()
    } else if contains(file, b"wp-content/uploads") {
        let directory = [base, b"/", relative_path(file)].concat();
        [untrailingslashit(&directory), b"/", &wp_basename(file)].concat()
    } else {
        [base, b"/", file].concat()
    };
    (!url.is_empty()).then_some(url)
}

/// `wp_get_attachment_metadata()`: the stored value. PHP turns down one
/// that is not an array or is empty; neither has keys for the callers.
pub fn metadata(site: &dyn Site, id: u64) -> Option<Value> {
    let stored = site.post_meta(id, "_wp_attachment_metadata")?;
    match maybe_unserialize(&stored) {
        Maybe::Value(value) => Some(value),
        _ => None,
    }
}

/// `wp_attachment_is( 'image', $post )`: an attached file, and an image
/// type, or an image extension for an imported one.
pub(crate) fn attachment_is_image(site: &dyn Site, post: &Post) -> bool {
    let file = site
        .post_meta(post.id, "_wp_attached_file")
        .unwrap_or_default();
    is_image(&file, &post.mime_type)
}

/// `wp_attachment_is( 'image' )` of an attachment of type `mime` whose
/// `_wp_attached_file` is `file`.
pub fn is_image(file: &[u8], mime: &str) -> bool {
    !is_empty_string(file)
        && (mime.starts_with("image/") || mime == "import" && image_extension(file))
}

/// `image_downsize()` for a named size: the URL, width and height.
pub fn image_downsize(site: &dyn Site, id: u64, size: &str) -> Option<(Vec<u8>, i64, i64)> {
    downsize(site, id, Size::Named(size))
}

/// `image_downsize()` for any `size`.
pub(crate) fn downsize(site: &dyn Site, id: u64, size: Size<'_>) -> Option<(Vec<u8>, i64, i64)> {
    let post = site
        .post(id)
        .filter(|post| post.post_type == "attachment")?;
    let mut url = attachment_url(site, id)?;
    let meta = metadata(site, id).unwrap_or_default();
    let is_image = attachment_is_image(site, &post);
    let mut name = wp_basename(&url);
    let (mut width, mut height) = (0, 0);
    let use_size = |sized: &Value, url: &mut Vec<u8>, name: &mut Vec<u8>| {
        let file = sized
            .get_str("file")
            .map(Value::to_php_string)
            .unwrap_or_default();
        *url = replace(url, name, &file);
        *name = file;
        dimensions(sized)
    };
    if !is_image {
        let full = meta
            .get_path(&["sizes", "full"])
            .filter(|full| !full.is_empty())?;
        (width, height) = use_size(full, &mut url, &mut name);
    }
    match size {
        Size::Named(size) => {
            if let Some(sized) = meta
                .get_path(&["sizes", size])
                .filter(|sized| !sized.is_empty())
            {
                (width, height) = use_size(sized, &mut url, &mut name);
            }
        }
        Size::Dimensions {
            width: max_width,
            height: max_height,
            original_ratio,
        } => {
            if let Some(sized) = best_size(&meta, max_width, max_height, original_ratio) {
                let (w, h) = use_size(sized, &mut url, &mut name);
                (width, height) = constrain_dimensions(w, h, max_width, max_height);
            }
        }
    }
    let isset = |key| isset(meta.get_str(key));
    if width == 0 && height == 0 && isset("width") && isset("height") {
        (width, height) = dimensions(&meta);
    }
    let (width, height) = constrain_for_editor(site, width, height, size);
    Some((url, width, height))
}

/// `image_get_intermediate_size()` for a width and height: the stored size
/// of exactly that size, else the smallest at least as large in the same
/// ratio (or in the original's, for `original_ratio`), else the thumbnail
/// when both fit within its width, as PHP compares them.
fn best_size(meta: &Value, width: i64, height: i64, original_ratio: bool) -> Option<&Value> {
    let sizes = meta.get_str("sizes").filter(|sizes| !sizes.is_empty())?;
    let full = sizes.get_str("full");
    let original = match full {
        Some(full) if !isset(meta.get_str("file")) && isset(Some(full)) => dimensions(full),
        _ => dimensions(meta),
    };
    let mut candidates = std::collections::BTreeMap::new();
    for (_, sized) in sizes.as_array().unwrap_or_default() {
        let (w, h) = dimensions(sized);
        if (w, h) == (width, height) {
            candidates.insert(w.saturating_mul(h), sized);
            break;
        }
        if w >= width && h >= height {
            let same_ratio = if original_ratio {
                matches_ratio(w, h, original.0, original.1)
            } else {
                matches_ratio(w, h, width, height)
            };
            if same_ratio {
                candidates.insert(w.saturating_mul(h), sized);
            }
        }
    }
    if let Some((_, best)) = candidates.pop_first() {
        return Some(best);
    }
    let thumbnail = sizes
        .get_str("thumbnail")
        .filter(|thumbnail| !thumbnail.is_empty())?;
    // PHP checks the height against the thumbnail's width too.
    let thumbnail_width = dimensions(thumbnail).0;
    (width <= thumbnail_width && height <= thumbnail_width).then_some(thumbnail)
}

/// An image's `width` and `height` as integers.
fn dimensions(image: &Value) -> (i64, i64) {
    let get = |key| image.get_str(key).map_or(0, Value::to_int);
    (get("width"), get("height"))
}

/// `image_constrain_size_for_editor()` in the `display` context, with no
/// `$content_width`.
fn constrain_for_editor(site: &dyn Site, width: i64, height: i64, size: Size<'_>) -> (i64, i64) {
    let size = match size {
        Size::Named(size) => size,
        Size::Dimensions {
            width: max_width,
            height: max_height,
            ..
        } => {
            return constrain_dimensions(width, height, max_width, max_height);
        }
    };
    let option = |name: &str| Value::String(site.option(name).unwrap_or_default()).to_int();
    let (max_width, max_height) = match size {
        "thumb" | "thumbnail" => match (option("thumbnail_size_w"), option("thumbnail_size_h")) {
            (0, 0) => (128, 96),
            max => max,
        },
        "medium" | "medium_large" | "large" => (
            option(&format!("{size}_size_w")),
            option(&format!("{size}_size_h")),
        ),
        // The sizes core adds with `add_image_size()`.
        "1536x1536" => (1536, 1536),
        "2048x2048" => (2048, 2048),
        _ => (width, height),
    };
    constrain_dimensions(width, height, max_width, max_height)
}

/// `wp_constrain_dimensions()`.
pub fn constrain_dimensions(
    width: i64,
    height: i64,
    max_width: i64,
    max_height: i64,
) -> (i64, i64) {
    if max_width == 0 && max_height == 0 {
        return (width, height);
    }
    let ratio =
        |current: i64, max: i64| (max > 0 && current > max).then(|| max as f64 / current as f64);
    let (width_ratio, height_ratio) = (ratio(width, max_width), ratio(height, max_height));
    let (w_ratio, h_ratio) = (width_ratio.unwrap_or(1.0), height_ratio.unwrap_or(1.0));
    let (smaller, larger) = (w_ratio.min(h_ratio), w_ratio.max(h_ratio));
    let scaled = |value: i64, ratio: f64| (value as f64 * ratio).round() as i64;
    let ratio = if scaled(width, larger) > max_width || scaled(height, larger) > max_height {
        smaller
    } else {
        larger
    };
    let mut w = scaled(width, ratio).max(1);
    let mut h = scaled(height, ratio).max(1);
    // Round up a dimension one short of its limit.
    if width_ratio.is_some() && w == max_width - 1 {
        w = max_width;
    }
    if height_ratio.is_some() && h == max_height - 1 {
        h = max_height;
    }
    (w, h)
}

/// `wp_image_matches_ratio()`: within a pixel once the larger is scaled to
/// the smaller's width.
pub fn matches_ratio(width: i64, height: i64, other_width: i64, other_height: i64) -> bool {
    let mut pair = [(width, height), (other_width, other_height)];
    pair.sort_unstable();
    let [(small_width, small_height), (large_width, large_height)] = pair;
    let (scaled_width, scaled_height) =
        constrain_dimensions(large_width, large_height, small_width, 0);
    scaled_width.abs_diff(small_width) <= 1 && scaled_height.abs_diff(small_height) <= 1
}

/// `wp_calculate_image_srcset()`, with the default 2048 pixel limit.
pub fn srcset(size: (i64, i64), src: &[u8], meta: &Value, baseurl: &[u8]) -> Option<String> {
    let file = meta
        .get_str("file")
        .map(Value::to_php_string)
        .unwrap_or_default();
    // An unset file reads as empty, and no sizes leave one source, too few.
    let sizes = meta.get_str("sizes").and_then(Value::as_array);
    let sizes = sizes.filter(|_| file.len() >= 4)?;
    let (width, height) = size;
    if width < 1 {
        return None;
    }
    let mut candidates: Vec<(i64, i64, Vec<u8>, Vec<u8>)> = sizes
        .iter()
        .filter_map(|(_, image)| {
            image.as_array()?;
            let value = image
                .get_str("width")
                .map(Value::to_php_string)
                .unwrap_or_default();
            let dimension = |key| image.get_str(key).map_or(0, Value::to_int);
            let file = image
                .get_str("file")
                .map(Value::to_php_string)
                .unwrap_or_default();
            Some((dimension("width"), dimension("height"), file, value))
        })
        .collect();
    // The full size, unless a GIF's thumbnail shows it is animated.
    let gif = meta
        .get_path(&["sizes", "thumbnail", "mime-type"])
        .map(Value::to_php_string);
    if gif.as_deref() != Some(b"image/gif") {
        let dimension = |key| meta.get_str(key).map_or(0, Value::to_int);
        let value = meta
            .get_str("width")
            .map(Value::to_php_string)
            .unwrap_or_default();
        candidates.push((
            dimension("width"),
            dimension("height"),
            wp_basename(&file),
            value,
        ));
    } else if contains(src, &file) {
        return None;
    }
    let dirname = relative_path(&file);
    let dirname = if dirname.is_empty() {
        Vec::new()
    } else {
        [dirname, b"/"].concat()
    };
    let base = [untrailingslashit(baseurl), b"/", &dirname].concat();
    let name = wp_basename(src);
    let edit = edit_hash(&name);
    let mut sources: Vec<(i64, Vec<u8>, Vec<u8>)> = Vec::new();
    let mut matched = false;
    for (image_width, image_height, file, value) in candidates {
        let is_src = !matched && contains(src, &[&dirname[..], &file].concat());
        matched |= is_src;
        // `! strpos()`: a hash at the start counts as missing.
        let edited_away = edit.is_some_and(|hash| find(&file, 0, hash).is_none_or(|at| at == 0));
        if edited_away || (image_width > 2048 && !is_src) {
            continue;
        }
        if matches_ratio(width, height, image_width, image_height) {
            let source = (image_width, [&base[..], &file].concat(), value);
            if is_src {
                sources.retain(|(w, _, _)| *w != image_width);
                sources.insert(0, source);
            } else if let Some(found) = sources.iter_mut().find(|(w, _, _)| *w == image_width) {
                *found = source;
            } else {
                sources.push(source);
            }
        }
    }
    if !matched || sources.len() < 2 {
        return None;
    }
    let listed: Vec<String> = sources
        .iter()
        .map(|(_, url, value)| {
            let url = String::from_utf8_lossy(url).replace(' ', "%20");
            format!("{url} {}w", String::from_utf8_lossy(value))
        })
        .collect();
    Some(listed.join(", "))
}

/// The `-e` and thirteen digits an edited image's file name carries.
pub(crate) fn edit_hash(name: &[u8]) -> Option<&[u8]> {
    name.windows(15)
        .find(|w| w.starts_with(b"-e") && w.iter().skip(2).all(u8::is_ascii_digit))
}

/// `_wp_get_attachment_relative_path()`.
fn relative_path(file: &[u8]) -> &[u8] {
    let dir = dirname(file);
    if dir == b"." {
        return b"";
    }
    match find(dir, 0, b"wp-content/uploads") {
        Some(at) => {
            let rest = dir.get(at + 18..).unwrap_or_default();
            let start = rest.iter().position(|b| *b != b'/').unwrap_or(rest.len());
            rest.get(start..).unwrap_or_default()
        }
        None => dir,
    }
}

/// An image extension `wp_attachment_is( 'image' )` accepts for an
/// imported file: after a dot, in lower case, as `wp_check_filetype()`
/// keeps the case it matched and `in_array()` compares it strictly.
fn image_extension(file: &[u8]) -> bool {
    let extension = file
        .rsplit(|b| *b == b'.')
        .next()
        .filter(|e| e.len() < file.len());
    ["jpg", "jpeg", "jpe", "gif", "png", "webp", "avif", "heic"]
        .iter()
        .any(|e| extension == Some(e.as_bytes()))
}

pub mod prepend;

#[cfg(test)]
mod tests;

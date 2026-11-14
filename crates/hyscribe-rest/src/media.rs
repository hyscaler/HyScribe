// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_Attachments_Controller`: `/wp/v2/media` and its single items,
//! read only, as the posts controller serves them with the attachments'
//! own arguments, fields and links. What PHP reads from the file on disk
//! (a size missing from the metadata, an original image's dimensions) and
//! a video player answer 501.

use std::sync::LazyLock;

use hyscribe_model::Post;
use hyscribe_phpser::{Key, Value, is_empty_string, json_decode, wp_basename};
use hyscribe_render::attachment::prepend::plays_video;
use hyscribe_render::attachment::{
    attachment_url, constrain_dimensions, image_downsize, is_image, metadata,
};
use hyscribe_render::permalinks::{Permalinks, permalink};
use hyscribe_render::rest::Rendering;

use crate::posts::args::id;
use crate::posts::prepare::writes_or_unported;
use crate::posts::{self, Kind};
use crate::request::{Arg, Params, typed};
use crate::response::{RestError, int, list, object, text};
use crate::types::context_arg;
use crate::{Context, Endpoint};

const MIME_TYPES: &[u8] = include_bytes!("../../../wordpress/wp-includes/mime-types.json");

/// The endpoints of the route `regex`, if one of these.
pub(crate) fn endpoints(regex: &str) -> Option<Vec<Endpoint>> {
    match regex {
        "/wp/v2/media" => Some(posts::type_endpoints(Kind::Attachment, false)),
        "/wp/v2/media/(?P<id>[\\d]+)" => {
            let mut endpoints = posts::type_endpoints(Kind::Attachment, true);
            // A single attachment takes no excerpt length or password.
            if let Some(get) = endpoints.first_mut() {
                get.args = vec![id(), context_arg()];
            }
            Some(endpoints)
        }
        _ => None,
    }
}

/// `get_media_types()`: the allowed MIME types by their first part.
fn media_types() -> &'static [(String, Vec<String>)] {
    static TYPES: LazyLock<Vec<(String, Vec<String>)>> = LazyLock::new(|| {
        let allowed = json_decode(MIME_TYPES).unwrap_or_default();
        let mut types: Vec<(String, Vec<String>)> = Vec::new();
        for (_, mime) in allowed.as_array().unwrap_or_default() {
            let mime = String::from_utf8_lossy(&mime.to_php_string()).into_owned();
            let kind = mime.split('/').next().unwrap_or_default().to_string();
            match types.iter_mut().find(|(known, _)| *known == kind) {
                Some((_, mimes)) => mimes.push(mime),
                None => types.push((kind, vec![mime])),
            }
        }
        types
    });
    &TYPES
}

/// The arguments the attachments add to the collection's.
pub(crate) fn collection_args() -> Vec<Arg> {
    let kinds: Vec<String> = media_types()
        .iter()
        .map(|(kind, _)| format!("\"{kind}\""))
        .collect();
    vec![
        typed(
            "media_type",
            &format!(
                r#"{{"default":null,"type":"array","items":{{"type":"string","enum":[{}]}}}}"#,
                kinds.join(",")
            ),
        ),
        typed(
            "mime_type",
            r#"{"default":null,"type":"array","items":{"type":"string"}}"#,
        ),
    ]
}

/// The `post_mime_type` `prepare_items_query()` limits attachments to: the
/// types of each media type asked for, and each MIME type asked for that
/// is allowed.
pub(crate) fn mime_types(params: &Params, kind: Kind) -> Vec<String> {
    if kind != Kind::Attachment {
        return Vec::new();
    }
    let asked = |name| {
        let list = params
            .get(name)
            .and_then(Value::as_array)
            .unwrap_or_default();
        list.iter()
            .map(|(_, item)| String::from_utf8_lossy(&item.to_php_string()).into_owned())
            .collect::<Vec<String>>()
    };
    let group = |kind: &str| {
        let types = media_types().iter();
        types
            .clone()
            .find(|(known, _)| known == kind)
            .map(|(_, mimes)| mimes)
    };
    let mut all: Vec<String> = asked("media_type")
        .iter()
        .filter_map(|kind| group(kind))
        .flatten()
        .cloned()
        .collect();
    for mime in asked("mime_type") {
        let first = mime.split('/').next().unwrap_or_default();
        if group(first).is_some_and(|mimes| mimes.contains(&mime)) {
            all.push(mime);
        }
    }
    let mut unique = Vec::new();
    for mime in all {
        if !unique.contains(&mime) {
            unique.push(mime);
        }
    }
    unique
}

/// The attachments' own part of `create_item_permissions_check()`.
pub(crate) fn create_permission(context: &Context, params: &Params) -> Result<(), RestError> {
    if !context.can("upload_files") {
        return Err(RestError::status(
            "rest_cannot_create",
            "Sorry, you are not allowed to upload media on this site.",
            400,
        ));
    }
    let post = params.get("post").filter(|post| !post.is_empty());
    if post.is_some_and(|post| !context.can_for("edit_post", post.to_int().unsigned_abs())) {
        let message = "Sorry, you are not allowed to upload media to this post.";
        return Err(context.forbidden("rest_cannot_edit", message));
    }
    Ok(())
}

/// `get_post_status()` of an attachment: inheriting its parent's, or
/// published without one (none has ID 0) or with itself as one, or with
/// one in the trash its status before.
pub(crate) fn post_status(context: &Context, post: &Post) -> String {
    let mut seen = vec![post.id];
    let mut post = post.clone();
    while post.status == "inherit" {
        let parent = (!seen.contains(&post.parent))
            .then(|| context.site.post(post.parent))
            .flatten();
        let Some(parent) = parent else {
            return "publish".into();
        };
        if parent.status == "trash" {
            let before = context.site.post_meta(parent.id, "_wp_trash_meta_status");
            let before = before.filter(|status| !is_empty_string(status));
            return before.map_or("publish".into(), |status| {
                String::from_utf8_lossy(&status).into_owned()
            });
        }
        seen.push(parent.id);
        post = parent;
    }
    match post.status.as_str() {
        "private" | "trash" | "auto-draft" => post.status,
        _ if post.post_type == "attachment" => "publish".into(),
        _ => post.status,
    }
}

/// `get_attachment_link()`, with `%postname%` left in the link of a
/// `sample` one.
pub(crate) fn attachment_link(context: &Context, post: &Post, sample: bool) -> Vec<u8> {
    let reads = context.site.reads();
    let links = Permalinks::of(reads);
    let home = String::from_utf8_lossy(&reads.home_url()).into_owned();
    let plain = || format!("{home}/?attachment_id={}", post.id).into_bytes();
    // `wp_force_plain_post_permalink()`.
    let status = post_status(context, post);
    let shown = status == "publish"
        || status == "private" && context.can_for("read_post", post.id)
        || sample;
    let parent = (post.parent != 0)
        .then(|| context.site.post(post.parent))
        .flatten();
    let viewable = |parent: &Post| {
        parent.id != post.id && ["post", "page", "attachment"].contains(&parent.post_type.as_str())
    };
    if !links.pretty() || !shown || post.parent != 0 && !parent.as_ref().is_some_and(viewable) {
        return plain();
    }
    let Some(parent) = parent else {
        if sample {
            return plain();
        }
        let name = String::from_utf8_lossy(&post.name).into_owned();
        return links.trailing(&format!("{home}/{name}")).into_bytes();
    };
    let parent_link = String::from_utf8_lossy(&permalink(reads, &parent)).into_owned();
    if parent_link.contains('?') {
        return plain();
    }
    let name = String::from_utf8_lossy(&post.name).into_owned();
    let numeric = !name.is_empty() && Value::String(post.name.clone()).is_numeric();
    let name = match (sample, numeric || links.uses("%category%")) {
        (true, _) => "%postname%".to_string(),
        (false, true) => format!("attachment/{name}"),
        (false, false) => name,
    };
    let base = parent_link.trim_end_matches(['/', '\\']);
    links.trailing(&format!("{base}/{name}")).into_bytes()
}

/// `wp_get_registered_image_subsizes()`: each size's name, width, height
/// and whether it is cropped, those with a width or height.
pub(crate) fn registered_sizes(context: &Context) -> Vec<(&'static str, i64, i64, bool)> {
    let option = |name: String| Value::String(context.option(&name).unwrap_or_default());
    let defaults = ["thumbnail", "medium", "medium_large", "large"].map(|size| {
        let dimension = |side| option(format!("{size}_size_{side}")).to_int();
        let crop = !option(format!("{size}_crop")).is_empty();
        (size, dimension("w"), dimension("h"), crop)
    });
    // The sizes core adds with `add_image_size()`.
    let added = [
        ("1536x1536", 1536, 1536, false),
        ("2048x2048", 2048, 2048, false),
    ];
    defaults
        .into_iter()
        .chain(added)
        .filter(|(_, width, height, _)| (*width, *height) != (0, 0))
        .collect()
}

/// Whether `image_resize_dimensions()` resizes an image of `width` by
/// `height` to a size.
fn resizes(width: i64, height: i64, (size_width, size_height, crop): (i64, i64, bool)) -> bool {
    // A size larger than the image leaves it as it is, and a registered
    // size has a width or a height.
    if width <= 0 || height <= 0 {
        return false;
    }
    let (new_width, new_height) = if crop {
        let ratio = width as f64 / height as f64;
        let new_width = size_width.min(width);
        let new_height = size_height.min(height);
        match (new_width, new_height) {
            (0, h) => ((h as f64 * ratio).round() as i64, h),
            (w, 0) => (w, (w as f64 / ratio).round() as i64),
            sized => sized,
        }
    } else {
        constrain_dimensions(width, height, size_width, size_height)
    };
    // `wp_fuzzy_number_match()`: the same size is no resize.
    (new_width - width).abs() > 1 || (new_height - height).abs() > 1
}

/// `missing_image_sizes`: the sizes an image could have and its metadata
/// lacks, or for a PDF the fallback sizes it lacks; 501 where PHP reads
/// an original image's size from its file.
fn missing_sizes(
    context: &Context,
    post: &Post,
    image: bool,
    meta: Option<&Value>,
) -> Result<Value, RestError> {
    let names = |names: Vec<&str>| list(names.into_iter().map(text).collect());
    let has = |name: &str| {
        meta.and_then(|meta| meta.get_path(&["sizes", name]))
            .is_some()
    };
    let registered = registered_sizes(context);
    if !image {
        if post.mime_type != "application/pdf" {
            return Ok(names(Vec::new()));
        }
        let fallback = ["thumbnail", "medium", "large"];
        let missing = registered
            .iter()
            .map(|(name, ..)| *name)
            .filter(|name| fallback.contains(name) && !has(name));
        return Ok(names(missing.collect()));
    }
    let Some(meta) = meta else {
        return Ok(names(registered.iter().map(|(name, ..)| *name).collect()));
    };
    if meta
        .get_str("original_image")
        .is_some_and(|original| !original.is_empty())
    {
        return Err(RestError::not_ported());
    }
    let side = |key| meta.get_str(key).map_or(0, Value::to_int);
    let (width, height) = (side("width"), side("height"));
    let missing = registered
        .iter()
        .filter(|(_, w, h, crop)| resizes(width, height, (*w, *h, *crop)))
        .map(|(name, ..)| *name)
        .filter(|name| !has(name));
    Ok(names(missing.collect()))
}

/// `media_details`: the metadata, an empty object without any, each size
/// with its URL and the full size added.
fn media_details(context: &Context, post: &Post, meta: Option<&Value>) -> Value {
    let empty = || Value::Object {
        class: b"stdClass".to_vec(),
        properties: Vec::new(),
    };
    let Some(Value::Array(meta)) = meta.filter(|meta| !meta.is_empty()) else {
        return empty();
    };
    let reads = context.site.reads();
    let mut details = meta.clone();
    let sizes_key = Key::String(b"sizes".to_vec());
    let sizes = details.iter_mut().find(|(key, _)| *key == sizes_key);
    let Some((_, sizes)) = sizes else {
        details.push((sizes_key, empty()));
        return Value::Array(details);
    };
    if sizes.is_empty() {
        *sizes = empty();
        return Value::Array(details);
    }
    if let Value::Array(sizes) = sizes {
        for (name, size) in sizes.iter_mut() {
            if let Value::Array(fields) = size {
                let mime = Key::String(b"mime-type".to_vec());
                if let Some(at) = fields.iter().position(|(key, _)| *key == mime) {
                    let (_, value) = fields.remove(at);
                    fields.push((Key::String(b"mime_type".to_vec()), value));
                }
                let name = String::from_utf8_lossy(&name.to_php_string()).into_owned();
                if let Some((url, ..)) = image_downsize(reads, post.id, &name) {
                    fields.push((Key::String(b"source_url".to_vec()), text(url)));
                }
            }
        }
        if let Some((url, width, height)) = image_downsize(reads, post.id, "full") {
            let full = object([
                ("file", text(wp_basename(&url))),
                ("width", Value::Int(width)),
                ("height", Value::Int(height)),
                ("mime_type", text(post.mime_type.clone())),
                ("source_url", text(url)),
            ]);
            sizes.retain(|(key, _)| key.to_php_string() != b"full");
            sizes.push((Key::String(b"full".to_vec()), full));
        }
    }
    Value::Array(details)
}

/// The fields the attachments controller adds to a post's, those in
/// `fields`, in its order.
pub(crate) fn fields(
    context: &Context,
    fields: &[Vec<u8>],
    post: &Post,
    rendering: &mut Rendering,
) -> Result<Vec<(Key, Value)>, RestError> {
    let wants = |name: &str| fields.iter().any(|field| field == name.as_bytes());
    let reads = context.site.reads();
    // A caption from an empty excerpt renders the content too.
    let renders = wants("description") || wants("caption");
    let logged_in = context.user != 0;
    if renders && (writes_or_unported(&post.content, logged_in) || plays_video(reads, post)) {
        return Err(RestError::not_ported());
    }
    let file = context
        .site
        .post_meta(post.id, "_wp_attached_file")
        .unwrap_or_default();
    let image = is_image(&file, &post.mime_type);
    let meta = metadata(reads, post.id).filter(|meta| matches!(meta, Value::Array(_)));
    let mut data: Vec<(Key, Value)> = Vec::new();
    let mut put = |name: &str, value: Value| data.push((Key::String(name.into()), value));
    if wants("description") {
        let rendered = rendering.the_content(post);
        let parts = [
            ("raw", text(post.content.clone())),
            ("rendered", text(rendered)),
        ];
        put("description", object(parts));
    }
    if wants("caption") {
        let rendered = rendering.the_excerpt(post, 55);
        let parts = [
            ("raw", text(post.excerpt.clone())),
            ("rendered", text(rendered)),
        ];
        put("caption", object(parts));
    }
    if wants("alt_text") {
        let alt = context.site.post_meta(post.id, "_wp_attachment_image_alt");
        put("alt_text", text(alt.unwrap_or_default()));
    }
    if wants("media_type") {
        put("media_type", text(if image { "image" } else { "file" }));
    }
    if wants("mime_type") {
        put("mime_type", text(post.mime_type.clone()));
    }
    if wants("media_details") {
        put("media_details", media_details(context, post, meta.as_ref()));
    }
    if wants("post") {
        let parent = (post.parent != 0).then(|| int(post.parent));
        put("post", parent.unwrap_or(Value::Null));
    }
    if wants("source_url") {
        let url = attachment_url(reads, post.id);
        put("source_url", url.map_or(Value::Bool(false), text));
    }
    if wants("missing_image_sizes") {
        put(
            "missing_image_sizes",
            missing_sizes(context, post, image, meta.as_ref())?,
        );
    }
    let original = meta
        .as_ref()
        .filter(|_| image)
        .and_then(|meta| meta.get_str("original_image"))
        .map(Value::to_php_string)
        .filter(|original| !original.is_empty());
    if wants("filename") {
        let name = original.clone().unwrap_or(file.clone());
        let name = (!is_empty_string(&name)).then(|| text(wp_basename(&name)));
        put("filename", name.unwrap_or(Value::Null));
    }
    if wants("filesize") {
        // A size the metadata lacks PHP reads from the file.
        let size = meta.as_ref().and_then(|meta| meta.get_str("filesize"));
        let size = size.filter(|size| size.is_numeric() && size.to_float() > 0.0);
        let size = size.ok_or_else(RestError::not_ported)?;
        put("filesize", Value::Int(size.to_int()));
    }
    if image {
        if wants("exif_orientation") {
            let orientation = meta
                .as_ref()
                .and_then(|meta| meta.get_path(&["image_meta", "orientation"]))
                .map_or(0, Value::to_int);
            put("exif_orientation", Value::Int(orientation.max(1)));
        }
        if wants("image_output_format") {
            put("image_output_format", Value::Null);
        }
        if wants("image_save_progressive") {
            put("image_save_progressive", Value::Bool(false));
        }
        if wants("image_quality") {
            // `wp_get_image_encode_quality()`: 86 for WebP, else 82.
            let quality = 82 + 4 * i64::from(post.mime_type == "image/webp");
            let quality = object([
                ("default", Value::Int(quality)),
                ("sizes", list(Vec::new())),
            ]);
            put("image_quality", quality);
        }
    }
    Ok(data)
}

/// The `attached-to` link `prepare_links()` adds for a parent.
pub(crate) fn attached_to(
    context: &Context,
    post: &Post,
    response: &mut crate::response::Response,
) {
    let Some(parent) = (post.parent != 0)
        .then(|| context.site.post(post.parent))
        .flatten()
    else {
        return;
    };
    let route = crate::types::items_route(&parent.post_type)
        .map_or_else(|| "/".to_string(), |route| format!("{route}/{}", parent.id));
    response.add_link_with(
        "https://api.w.org/attached-to",
        context.rest_url(&route),
        vec![
            ("embeddable", Value::Bool(true)),
            ("post_type", text(parent.post_type.clone())),
            ("id", int(parent.id)),
        ],
    );
}

#[cfg(test)]
mod tests {
    use super::resizes;

    /// `image_resize_dimensions()` of sizes no fixture registers, a crop of
    /// one side, and of an image without dimensions.
    #[test]
    fn resizes_as_php_does() {
        assert!(resizes(300, 200, (150, 0, true)));
        assert!(resizes(300, 200, (0, 150, true)));
        assert!(!resizes(300, 200, (300, 0, true)));
        assert!(!resizes(300, 200, (0, 200, true)));
        assert!(!resizes(0, 200, (150, 150, true)));
        assert!(!resizes(100, 100, (150, 150, true)));
        assert!(!resizes(301, 200, (300, 300, false)));
        assert!(resizes(150, 400, (150, 150, true)));
        assert!(!resizes(150, 151, (150, 150, true)));
    }
}

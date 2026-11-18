// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_Attachments_Controller`: `/wp/v2/media` and its single items,
//! read only, as the posts controller serves them with the attachments'
//! own arguments, fields and links. What PHP reads from the file on disk
//! (a size missing from the metadata, an original image's dimensions) and
//! a video player answer 501.

use std::sync::LazyLock;

use hyscribe_image::subsizes::{
    SubSize, image_resize_dimensions, wp_get_registered_image_subsizes,
};
use hyscribe_model::Post;
use hyscribe_phpser::{Key, Value, is_empty_string, json_decode, wp_basename};
use hyscribe_render::attachment::prepend::plays_video;
use hyscribe_render::attachment::{attachment_url, image_downsize, is_image, metadata};
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
            for get in endpoints.iter_mut().take(1) {
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

/// The `post_mime_type` `prepare_items_query()` limits attachments to:
/// each media type's types, and each MIME type asked for that is allowed.
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

/// The attachments' own part of `create_item_permissions_check()`, whose
/// `upload_files` is the type's `create_posts` checked already.
pub(crate) fn create_permission(context: &Context, params: &Params) -> Result<(), RestError> {
    let post = params.get("post").filter(|post| !post.is_empty());
    if post.is_some_and(|post| !context.can_for("edit_post", post.to_int().unsigned_abs())) {
        let message = "Sorry, you are not allowed to upload media to this post.";
        return Err(context.forbidden("rest_cannot_edit", message));
    }
    Ok(())
}

/// `get_post_status()` of an attachment: its parent's, or published without
/// one, or a trashed parent's status before.
pub(crate) fn post_status(context: &Context, post: &Post) -> String {
    hyscribe_render::attachment::post_status(context.site.reads(), post)
}

/// `get_attachment_link()`, with `%postname%` left in the link of a
/// `sample` one.
pub(crate) fn attachment_link(context: &Context, post: &Post, sample: bool) -> Vec<u8> {
    let can_read = |id| context.can_for("read_post", id);
    hyscribe_render::attachment::attachment_link(context.site.reads(), post, sample, &can_read)
}

/// `wp_get_registered_image_subsizes()` of the site's options.
pub(crate) fn registered_sizes(context: &Context) -> Vec<SubSize> {
    wp_get_registered_image_subsizes(&|name| context.option(name).unwrap_or_default())
}

/// `missing_image_sizes`: the sizes an image, or a PDF's fallbacks, could
/// have and lack; 501 where PHP reads an original image's file.
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
            .map(|size| size.name.as_str())
            .filter(|name| fallback.contains(name) && !has(name));
        return Ok(names(missing.collect()));
    }
    let Some(meta) = meta else {
        return Ok(names(
            registered.iter().map(|size| size.name.as_str()).collect(),
        ));
    };
    if meta
        .get_str("original_image")
        .is_some_and(|original| !original.is_empty())
    {
        return Err(RestError::not_ported());
    }
    let side = |key| u32::try_from(meta.get_str(key).map_or(0, Value::to_int)).unwrap_or(0);
    let full = (side("width"), side("height"));
    let missing = registered
        .iter()
        .filter(|size| {
            image_resize_dimensions(full, (size.width, size.height), &size.crop).is_some()
        })
        .map(|size| size.name.as_str())
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
    // A size's MIME type renamed and its URL added, and the full size.
    let mime = Key::String(b"mime-type".to_vec());
    let mut named: Vec<(Key, Value)> = sizes
        .as_array()
        .unwrap_or_default()
        .iter()
        .map(|(name, size)| {
            let mut fields = size.as_array().unwrap_or_default().to_vec();
            let at = fields.iter().position(|(key, _)| *key == mime);
            let renamed = at.map(|at| fields.remove(at).1);
            fields.extend(renamed.map(|value| (Key::String(b"mime_type".to_vec()), value)));
            let label = String::from_utf8_lossy(&name.to_php_string()).into_owned();
            let url = image_downsize(reads, post.id, &label);
            fields.extend(url.map(|(url, ..)| (Key::String(b"source_url".to_vec()), text(url))));
            (name.clone(), Value::Array(fields))
        })
        .collect();
    let full = image_downsize(reads, post.id, "full").map(|(url, width, height)| {
        object([
            ("file", text(wp_basename(&url))),
            ("width", Value::Int(width)),
            ("height", Value::Int(height)),
            ("mime_type", text(post.mime_type.clone())),
            ("source_url", text(url)),
        ])
    });
    named.retain(|(key, _)| full.is_none() || key.to_php_string() != b"full");
    named.extend(full.map(|full| (Key::String(b"full".to_vec()), full)));
    *sizes = Value::Array(named);
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
    let parent = (post.parent != 0).then(|| context.site.post(post.parent));
    let Some(parent) = parent.flatten() else {
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

// SPDX-License-Identifier: AGPL-3.0-or-later

//! The media library's reads over `admin-ajax.php` (MVP 6.4): the grid's
//! and the media modal's `query-attachments`, `get-attachment`, and
//! `wp_prepare_attachment_for_js()`, which they and the uploads answer
//! with. What PHP reads in a way not ported answers 501.

use axum::http::{HeaderValue, StatusCode};
use axum::response::Response;
use hyscribe_admin::actions::Asked;
use hyscribe_admin::assets::Urls;
use hyscribe_admin::media::{attachments_query, human_readable_duration, icon_name, size_format};
use hyscribe_admin::menu::MenuUser;
use hyscribe_admin::page::PageSite;
use hyscribe_admin::posts::PostsSite;
use hyscribe_auth::caps::CapabilityData;
use hyscribe_model::Post;
use hyscribe_phpser::{Key, Maybe, Value, absint, json_encode, maybe_unserialize, wp_basename};
use hyscribe_render::attachment::{
    attachment_link, attachment_url, image_constrain_size_for_editor, image_downsize, metadata,
};
use hyscribe_render::date::{Zone, format, stored_unix};
use hyscribe_render::formatting::html_entity_decode;

use super::{Admin, JSON, ajax_text, not_ported};

/// `wp_send_json_error()` without data.
const ERROR: &[u8] = b"{\"success\":false}";

/// The answer to the media read `asked` names, once `admin-ajax.php`'s
/// checks pass; `None` for another action.
pub(super) fn read(admin: &Admin, asked: &Asked) -> Option<Response> {
    let action = asked.request("action").and_then(Value::as_bytes)?;
    let answer = match action {
        b"query-attachments" => query_attachments(admin, asked),
        b"get-attachment" => get_attachment(admin, asked),
        _ => return None,
    };
    Some(answer.unwrap_or_else(not_ported))
}

/// `wp_send_json_success( $data )`.
fn success(data: Value) -> Option<Vec<u8>> {
    let json = object(vec![("success", Value::Bool(true)), ("data", data)]);
    json_encode(&json)
}

fn json(body: Vec<u8>) -> Response {
    ajax_text(StatusCode::OK, JSON, body)
}

/// `wp_ajax_get_attachment()`.
fn get_attachment(admin: &Admin, asked: &Asked) -> Option<Response> {
    let id = asked.request("id").map_or(0, absint);
    let post = (id != 0).then(|| admin.full_post(id)).flatten();
    let post = post.filter(|post| post.post_type == "attachment" && admin.can("upload_files"));
    let Some(post) = post else {
        return Some(json(ERROR.to_vec()));
    };
    Some(json(success(prepare_for_js(admin, &post)?)?))
}

/// `wp_ajax_query_attachments()`: the attachments, and their count and
/// pages in `X-WP-Total` and `X-WP-TotalPages`.
fn query_attachments(admin: &Admin, asked: &Asked) -> Option<Response> {
    if !admin.can("upload_files") {
        return Some(json(ERROR.to_vec()));
    }
    let query = match asked.request("query") {
        Some(query @ Value::Array(_)) => query.clone(),
        _ => Value::Array(Vec::new()),
    };
    let media_trash = admin.site.constants.media_trash;
    let (query, per_page) = attachments_query(admin, &query, media_trash)?;
    let query = hyscribe_model::hooks::apply_filters("ajax_query_attachments_args", query, &[]);
    let (posts, total) = admin.list_posts(&query);
    let posts = posts
        .iter()
        .zip(0..)
        .map(|(post, n)| Some((Key::Int(n), prepare_for_js(admin, post)?)))
        .collect::<Option<Vec<_>>>()?;
    let total = i64::try_from(total).ok()?;
    let pages = match per_page {
        0 => 0,
        // `(int) ceil( $total / $per_page )`, which -1 makes negative.
        per_page => -(-total).div_euclid(per_page),
    };
    let mut response = json(success(Value::Array(posts))?);
    let headers = response.headers_mut();
    headers.insert("x-wp-total", HeaderValue::from(total));
    headers.insert("x-wp-totalpages", HeaderValue::from(pages));
    Some(response)
}

/// A PHP array with string keys.
fn object(entries: Vec<(&str, Value)>) -> Value {
    let entries = entries
        .into_iter()
        .map(|(key, value)| (Key::String(key.into()), value));
    Value::Array(entries.collect())
}

fn text(bytes: impl Into<Vec<u8>>) -> Value {
    Value::String(bytes.into())
}

/// `landscape` or `portrait`.
fn orientation(width: &Value, height: &Value) -> Value {
    let portrait = height.to_float() > width.to_float();
    text(if portrait { "portrait" } else { "landscape" })
}

/// `get_edit_post_link( $post, 'raw' )`: null for a user who may not edit
/// it; `None` for a post type whose screen is not `post.php`.
fn edit_link(admin: &Admin, post: &Post) -> Option<Value> {
    if !["post", "page", "attachment", "wp_block"].contains(&post.post_type.as_str()) {
        return None;
    }
    Some(match admin.can_for("edit_post", post.id) {
        true => text(admin.admin_url(&format!("post.php?post={}&action=edit", post.id))),
        false => Value::Null,
    })
}

/// `get_edit_user_link( $id )`: the profile for the user's own, empty for
/// a user who may not edit them.
fn edit_user_link(admin: &Admin, id: u64) -> String {
    match admin.can_for("edit_user", id) {
        false => String::new(),
        true if id == admin.user.id => admin.admin_url("profile.php"),
        true => admin.admin_url(&format!("user-edit.php?user_id={id}")),
    }
}

/// `get_media_states()` of a theme without custom headers or backgrounds.
fn media_states(admin: &Admin, post: &Post) -> Vec<&'static str> {
    let option = |name: &str| PostsSite::option(admin, name).unwrap_or_default();
    let id = post.id.cast_signed();
    let mut states = Vec::new();
    if Value::String(option("site_icon")).to_int() == id {
        states.push("Site Icon");
    }
    let stylesheet = String::from_utf8_lossy(&option("stylesheet")).into_owned();
    let logo = match maybe_unserialize(&option(&format!("theme_mods_{stylesheet}"))) {
        Maybe::Value(mods) => mods.get_str("custom_logo").map_or(0, Value::to_int),
        _ => 0,
    };
    if logo == id {
        states.push("Logo");
    }
    hyscribe_model::hooks::apply_filters("display_media_states", states, &[post])
}

/// The size in bytes of the attachment's metadata, else of its file when
/// it can be read; `None` for a file outside the uploads folder.
fn file_size(admin: &Admin, meta: &Value, file: &[u8]) -> Option<i64> {
    let stored = meta.get_str("filesize").filter(|size| size.is_numeric());
    if let Some(size) = stored.map(Value::to_int).filter(|size| *size > 0) {
        return Some(size);
    }
    if file.is_empty() {
        return Some(0);
    }
    let size = admin.site.uploads.as_ref()?.file_size(file)?;
    Some(size.map_or(0, |size| i64::try_from(size).unwrap_or(i64::MAX)))
}

/// `wp_get_attachment_image_src( $id, $size )` of a song's or a video's
/// featured image: its `src`, `width` and `height`.
fn featured_image(admin: &Admin, id: u64, size: &str) -> Option<Value> {
    let (src, width, height) = image_downsize(&admin.reads.reads, id, size)?;
    Some(object(vec![
        ("src", text(hyscribe_kses::esc_url_raw(&src))),
        ("width", Value::Int(width)),
        ("height", Value::Int(height)),
    ]))
}

/// `wp_prepare_attachment_for_js( $post )` for the current user; `None`
/// for what is not ported.
pub(super) fn prepare_for_js(admin: &Admin, post: &Post) -> Option<Value> {
    let reads = &admin.reads.reads;
    let id = post.id;
    let meta = metadata(reads, id).filter(|meta| !meta.is_empty());
    let meta_of = |key: &str| meta.as_ref().and_then(|meta| meta.get_str(key));
    let (kind, subtype) = post
        .mime_type
        .split_once('/')
        .unwrap_or((&post.mime_type, ""));
    let url = attachment_url(reads, id).unwrap_or_default();
    let base_url = hyscribe_render::template::replace(&url, &wp_basename(&url), b"");
    let file = PostsSite::meta(admin, post, "_wp_attached_file").unwrap_or_default();
    let can_read = |id| admin.can_for("read_post", id);
    let link = attachment_link(reads, post, false, &can_read);
    let alt = PostsSite::meta(admin, post, "_wp_attachment_image_alt");
    let unix = |date: &str| Value::Int(stored_unix(date, Zone::Utc).saturating_mul(1000));
    let icon = format!(
        "{}/wp-includes/images/media/{}.svg",
        admin.site_url(),
        icon_name(&file, &post.mime_type)
    );
    let mut data = vec![
        ("id", Value::Int(id.cast_signed())),
        ("title", text(post.title.clone())),
        ("filename", text(wp_basename(&file))),
        ("url", text(hyscribe_kses::esc_url_raw(&url))),
        ("link", text(link)),
        ("alt", text(alt.unwrap_or_default())),
        ("author", text(post.author.to_string())),
        ("description", text(post.content.clone())),
        ("caption", text(post.excerpt.clone())),
        ("name", text(post.name.clone())),
        ("status", text(post.status.clone())),
        ("uploadedTo", Value::Int(post.parent.cast_signed())),
        ("date", unix(&post.date_gmt)),
        ("modified", unix(&post.modified_gmt)),
        ("menuOrder", Value::Int(post.menu_order)),
        ("mime", text(post.mime_type.clone())),
        ("type", text(kind)),
        ("subtype", text(subtype)),
        ("icon", text(icon.clone())),
        (
            "dateFormatted",
            text(format("F j, Y", &post.date, admin.site.zone())),
        ),
    ];
    let editable = admin.can_for("edit_post", id);
    let nonce = |action: String| text(PageSite::nonce(admin, &action));
    let maybe_nonce = |can: bool, action: String| can.then(|| nonce(action));
    data.push((
        "nonces",
        object(vec![
            (
                "update",
                maybe_nonce(editable, format!("update-post_{id}")).unwrap_or(Value::Bool(false)),
            ),
            (
                "delete",
                maybe_nonce(
                    admin.can_for("delete_post", id),
                    format!("delete-post_{id}"),
                )
                .unwrap_or(Value::Bool(false)),
            ),
            (
                "edit",
                maybe_nonce(editable, format!("image_editor-{id}")).unwrap_or(Value::Bool(false)),
            ),
        ]),
    ));
    let edit = edit_link(admin, post)?;
    data.push(("editLink", if editable { edit } else { Value::Bool(false) }));
    data.push(("meta", Value::Bool(false)));
    match PostsSite::userdata(admin, post.author) {
        Some(user) => {
            let name = match user.display_name.is_empty() {
                true => CapabilityData::user_meta(admin, user.id, "nickname").unwrap_or_default(),
                false => user.display_name.clone(),
            };
            data.push(("authorName", text(html_entity_decode(&name))));
            data.push(("authorLink", text(edit_user_link(admin, user.id))));
        }
        None => data.push(("authorName", text("(no author)"))),
    }
    let parent = (post.parent != 0)
        .then(|| admin.full_post(post.parent))
        .flatten()
        .filter(|parent| admin.can_for("read_post", parent.id));
    if let Some(parent) = parent {
        let title = match parent.title.is_empty() {
            true => b"(no title)".to_vec(),
            false => parent.title.clone(),
        };
        data.push(("uploadedToTitle", text(title)));
        data.push(("uploadedToLink", edit_link(admin, &parent)?));
    }
    let empty = Value::Null;
    let bytes = file_size(admin, meta.as_ref().unwrap_or(&empty), &file)?;
    if bytes != 0 {
        data.push(("filesizeInBytes", Value::Int(bytes)));
        data.push(("filesizeHumanReadable", text(size_format(bytes))));
    }
    let context = PostsSite::meta(admin, post, "_wp_attachment_context");
    data.push(("context", text(context.unwrap_or_default())));
    let mut data: Vec<(Key, Value)> = data
        .into_iter()
        .map(|(key, value)| (Key::String(key.into()), value))
        .collect();
    let sized = meta_of("sizes").is_some_and(|sizes| !sizes.is_empty());
    if let Some(meta) = meta.as_ref().filter(|_| kind == "image" || sized) {
        image_fields(admin, &mut data, (meta, kind == "image"), (&url, &base_url));
    }
    if kind == "video" {
        for key in ["width", "height"] {
            if let Some(side) = meta_of(key).filter(|side| !matches!(side, Value::Null)) {
                put(&mut data, key, Value::Int(side.to_int()));
            }
        }
    }
    let player = matches!(kind, "audio" | "video");
    if let Some(meta) = meta.as_ref().filter(|_| player) {
        player_fields(admin, &mut data, post, meta, icon)?;
    }
    data.push((
        Key::String(b"compat".to_vec()),
        object(vec![("item", text("")), ("meta", text(""))]),
    ));
    let states = media_states(admin, post);
    if !states.is_empty() {
        data.push((
            Key::String(b"mediaStates".to_vec()),
            text(states.join(", ")),
        ));
    }
    let data = Value::Array(data);
    Some(hyscribe_model::hooks::apply_filters(
        "wp_prepare_attachment_for_js",
        data,
        &[post, &meta],
    ))
}

/// A song's or a video's length, tags and featured image, or its icon.
fn player_fields(
    admin: &Admin,
    data: &mut Vec<(Key, Value)>,
    post: &Post,
    meta: &Value,
    icon: String,
) -> Option<()> {
    if let Some(length) = meta.get_str("length_formatted") {
        put(data, "fileLength", length.clone());
        // False where the length is not `[H:]M:S`.
        let readable = human_readable_duration(length);
        let readable = match readable.is_empty() {
            true => Value::Bool(false),
            false => text(readable),
        };
        put(data, "fileLengthHumanReadable", readable);
    }
    let tags = ["artist", "album", "bitrate", "bitrate_mode"].map(|key| {
        let value = meta.get_str(key).filter(|value| !value.is_empty());
        (key, value.cloned().unwrap_or(Value::Bool(false)))
    });
    put(data, "meta", object(tags.to_vec()));
    let thumbnail = PostsSite::meta(admin, post, "_thumbnail_id");
    let thumbnail = Value::String(thumbnail.unwrap_or_default()).to_int();
    if thumbnail == 0 {
        let icon = object(vec![
            ("src", text(icon)),
            ("width", Value::Int(48)),
            ("height", Value::Int(64)),
        ]);
        put(data, "image", icon.clone());
        put(data, "thumb", icon);
        return Some(());
    }
    let thumbnail = u64::try_from(thumbnail).ok()?;
    for (key, size) in [("image", "full"), ("thumb", "thumbnail")] {
        if let Some(image) = featured_image(admin, thumbnail, size) {
            put(data, key, image);
        }
    }
    Some(())
}

/// `$response[ $key ] = $value`: in place when the key is there.
fn put(data: &mut Vec<(Key, Value)>, key: &str, value: Value) {
    let key = Key::String(key.into());
    match data.iter_mut().find(|(existing, _)| *existing == key) {
        Some((_, existing)) => *existing = value,
        None => data.push((key, value)),
    }
}

/// The sizes of an attachment with metadata `meta`, an image or not
/// (`image`), at `url` under `base`: its sizes, and for an image the
/// original's URL and name and its full size.
fn image_fields(
    admin: &Admin,
    data: &mut Vec<(Key, Value)>,
    (meta, image): (&Value, bool),
    (url, base): (&[u8], &[u8]),
) {
    let mut push = |key: &str, value: Value| put(data, key, value);
    let mut sizes = Vec::new();
    let reads = &admin.reads.reads;
    let sized_url = |file: Option<&Value>| {
        let file = file.map(Value::to_php_string).unwrap_or_default();
        text(hyscribe_kses::esc_url_raw(&[base, &file[..]].concat()))
    };
    for name in ["thumbnail", "medium", "large"] {
        let Some(size) = meta.get_path(&["sizes", name]) else {
            continue;
        };
        let side = |key| size.get_str(key).map_or(0, Value::to_int);
        let (width, height) =
            image_constrain_size_for_editor(reads, (side("width"), side("height")), name);
        let (width, height) = (Value::Int(width), Value::Int(height));
        let entry = object(vec![
            ("height", height.clone()),
            ("width", width.clone()),
            ("url", sized_url(size.get_str("file"))),
            ("orientation", orientation(&width, &height)),
        ]);
        sizes.push((Key::String(name.into()), entry));
    }
    let full = if image {
        let original = meta
            .get_str("original_image")
            .filter(|name| !name.is_empty());
        if let Some(original) = original.map(Value::to_php_string) {
            let original_url = [base, &original[..]].concat();
            push(
                "originalImageURL",
                text(hyscribe_kses::esc_url_raw(&original_url)),
            );
            push("originalImageName", text(wp_basename(&original)));
        }
        let mut full = vec![("url", text(hyscribe_kses::esc_url_raw(url)))];
        if let (Some(height), Some(width)) = (meta.get_str("height"), meta.get_str("width")) {
            full.push(("height", height.clone()));
            full.push(("width", width.clone()));
            full.push(("orientation", orientation(width, height)));
        }
        // array_merge(): `url` is there already, the rest comes after.
        for (key, value) in full.iter().skip(1) {
            push(key, value.clone());
        }
        Some(object(full))
    } else {
        let full = meta
            .get_path(&["sizes", "full"])
            .filter(|full| full.get_str("file").is_some_and(|file| !file.is_empty()));
        full.and_then(|full| {
            let (width, height) = (full.get_str("width")?, full.get_str("height")?);
            Some(object(vec![
                ("url", sized_url(full.get_str("file"))),
                ("height", height.clone()),
                ("width", width.clone()),
                ("orientation", orientation(width, height)),
            ]))
        })
    };
    sizes.extend(full.map(|full| (Key::String(b"full".to_vec()), full)));
    push("sizes", Value::Array(sizes));
}

#[cfg(test)]
mod tests;

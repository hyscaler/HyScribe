// SPDX-License-Identifier: AGPL-3.0-or-later

//! hs-admin's uploads: `async-upload.php`, which the media modal's
//! uploader (`upload-attachment`) and Add New Media's Plupload (`short`)
//! post to, and `media-new.php`'s browser uploader form, with their nonce
//! and capability checks and answers as PHP gives them, and
//! `wp_prepare_attachment_for_js()` of an image just uploaded. What else
//! they take (a `fetch` of a media item, Plupload's long answer,
//! `post_data`) answers 501.

use axum::http::{StatusCode, header};
use axum::response::Response;
use hyscribe_admin::actions::{ActionSite, Asked};
use hyscribe_admin::assets::Urls;
use hyscribe_admin::login::Site as LoginSite;
use hyscribe_admin::menu::MenuUser;
use hyscribe_admin::page::PageSite;
use hyscribe_admin::posts::PostsSite;
use hyscribe_auth::caps::CapabilityData;
use hyscribe_kses::esc_html;
use hyscribe_phpser::{Key, Value, absint, json_encode, json_encode_localized, wp_basename};
use hyscribe_render::attachment::{
    attachment_link, attachment_url, image_constrain_size_for_editor, metadata,
};
use hyscribe_render::date::{Zone, format, stored_unix};
use hyscribe_render::formatting::html_entity_decode;
use hyscribe_write::posts::Context;

use super::{Admin, died, not_ported, redirect, refused, screen_headers};
use crate::login::{nocache, response};
use crate::uploads::attach::upload_time;
use crate::uploads::{Refused, Sent, Who};

/// The screens that take an upload.
pub(super) const SCREENS: [&str; 2] = ["async-upload", "media-new"];

/// `async-upload.php`'s content type, which its errors keep.
const PLAIN: &str = "text/plain; charset=UTF-8";

/// A text answer as `async-upload.php` sends it.
fn plain(status: StatusCode, body: Vec<u8>, id: Option<u64>) -> Response {
    let mut headers = nocache();
    headers.extend(id.map(|id| {
        let name = header::HeaderName::from_static(crate::uploads::ATTACHMENT_ID);
        (name, id.to_string())
    }));
    headers.push((header::CONTENT_TYPE, PLAIN.into()));
    response(status, headers, body)
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

/// `upload-attachment`'s refusal: `{"success":false,"data":{...}}`.
fn failed(message: &str, file: &[u8]) -> Response {
    let data = object(vec![
        ("message", text(message)),
        ("filename", text(esc_html(file))),
    ]);
    let json = object(vec![("success", Value::Bool(false)), ("data", data)]);
    plain(StatusCode::OK, json_encode(&json).unwrap_or_default(), None)
}

/// The upload of the field `async-upload`, if one was sent.
fn sent(files: &[(Vec<u8>, Sent)]) -> Option<Sent> {
    let found = files.iter().find(|(field, _)| field == b"async-upload");
    found.map(|(_, sent)| sent.clone())
}

/// `uniqid()`: the time in seconds and microseconds, in hexadecimal.
fn uniqid() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{:08x}{:05x}", now.as_secs(), now.subsec_micros())
}

/// The answer to an upload on `screen` once its user is logged in.
pub(super) fn upload(
    admin: &Admin,
    screen: &str,
    asked: &Asked,
    files: &[(Vec<u8>, Sent)],
) -> Response {
    let ajax = asked.request("action").and_then(Value::as_bytes) == Some(b"upload-attachment");
    match screen {
        "async-upload" if ajax => upload_attachment(admin, asked, files),
        "async-upload" => async_upload(admin, asked, files),
        _ => media_new(admin, asked, files),
    }
}

/// `wp_ajax_upload_attachment()`.
fn upload_attachment(admin: &Admin, asked: &Asked, files: &[(Vec<u8>, Sent)]) -> Response {
    let nonce = asked
        .request("_ajax_nonce")
        .or_else(|| asked.request("_wpnonce"));
    let verified =
        nonce.is_some_and(|nonce| admin.verify_nonce(&nonce.to_php_string(), "media-form"));
    if !verified {
        return plain(StatusCode::FORBIDDEN, b"-1".to_vec(), None);
    }
    let Some(sent) = sent(files) else {
        return not_ported();
    };
    if !admin.can("upload_files") {
        return failed("Sorry, you are not allowed to upload files.", &sent.name);
    }
    let post_id = asked.request("post_id").map(Value::to_int);
    if post_id.is_some_and(|id| !admin.current_user_can("edit_post", &[Value::Int(id)])) {
        return failed(
            "Sorry, you are not allowed to attach files to this post.",
            &sent.name,
        );
    }
    if asked
        .request("post_data")
        .is_some_and(|data| !data.is_empty())
    {
        return not_ported();
    }
    let name = sent.name.clone();
    let post_id = post_id.map_or(0, i64::unsigned_abs);
    match media_handle_upload(admin, sent, post_id) {
        Ok(id) => {
            let data = prepare_for_js(admin, id);
            let json = object(vec![("success", Value::Bool(true)), ("data", data)]);
            plain(
                StatusCode::OK,
                json_encode(&json).unwrap_or_default(),
                Some(id),
            )
        }
        Err(Refused::Message(message)) => failed(&message, &name),
        Err(refused) => answered(admin, refused),
    }
}

/// The answer to what stops an upload but its own error message: 501, or
/// the error page of a write or file that failed.
fn answered(admin: &Admin, refused: Refused) -> Response {
    match refused {
        Refused::Failed(err) => crate::html(crate::database_error(admin.site, &err)),
        _ => not_ported(),
    }
}

/// The post an upload is attached to: `post_id`, if the user may edit it.
fn parent(admin: &Admin, asked: &Asked) -> u64 {
    let id = asked.request("post_id").map_or(0, absint);
    let usable = id != 0 && admin.full_post(id).is_some() && admin.can_for("edit_post", id);
    if usable { id } else { 0 }
}

/// `async-upload.php` without `upload-attachment`: Plupload's short answer.
fn async_upload(admin: &Admin, asked: &Asked, files: &[(Vec<u8>, Sent)]) -> Response {
    if !admin.can("upload_files") {
        return died(
            admin,
            StatusCode::INTERNAL_SERVER_ERROR,
            "Sorry, you are not allowed to upload files.",
        );
    }
    // A media item's form, and the long answer, are not ported.
    let fetch = asked
        .request("attachment_id")
        .is_some_and(|id| id.to_int() != 0)
        && asked
            .request("fetch")
            .is_some_and(|fetch| !fetch.is_empty());
    let short = asked
        .request("short")
        .is_some_and(|short| !short.is_empty());
    if fetch || !short {
        return not_ported();
    }
    if let Err(refusal) = hyscribe_admin::actions::referer(admin, asked, "media-form") {
        return refused(admin.reads.public(), refusal);
    }
    let post_id = parent(admin, asked);
    let Some(sent) = sent(files) else {
        return not_ported();
    };
    let name = sent.name.clone();
    match media_handle_upload(admin, sent, post_id) {
        Ok(id) => plain(StatusCode::OK, id.to_string().into_bytes(), Some(id)),
        Err(Refused::Message(message)) => {
            let (button, description) = (
                format!("dismiss-{}", uniqid()),
                format!("error-description-{}", uniqid()),
            );
            let file = esc_html(&name);
            let notice = format!(
                "<div id=\"{description}\" class=\"notice error-div error\"><button type=\"button\" id=\"{button}\" class=\"dismiss button-link\" aria-describedby=\"{description}\">Dismiss</button> <strong>&#8220;{file}&#8221; has failed to upload.</strong><br />{}</div>",
                esc_html(message.as_bytes())
            );
            let speak = [&name[..], b" has failed to upload."].concat();
            let speak = json_encode_localized(&text(speak)).unwrap_or_default();
            let script = format!(
                "<script>_.delay(function() {{wp.a11y.speak({});}}, 1500);jQuery( 'button#{button}' ).on( 'click', function() {{jQuery(this).parents('div.media-item').slideUp(200, function(){{jQuery(this).remove();wp.a11y.speak( wp.i18n.__( 'Error dismissed.' ) );jQuery( '#plupload-browse-button' ).trigger( 'focus' );}})}});</script>\n",
                String::from_utf8_lossy(&speak)
            );
            plain(StatusCode::OK, (notice + &script).into_bytes(), None)
        }
        Err(refused) => answered(admin, refused),
    }
}

/// `media-new.php`'s form post: the browser uploader's upload, then the
/// media library.
fn media_new(admin: &Admin, asked: &Asked, files: &[(Vec<u8>, Sent)]) -> Response {
    // `admin.php` refuses the screen first.
    if !admin.can("upload_files") {
        return died(
            admin,
            StatusCode::FORBIDDEN,
            "Sorry, you are not allowed to access this page.",
        );
    }
    let post_id = parent(admin, asked);
    if asked.post.is_empty() {
        return not_ported();
    }
    let library = || redirect(screen_headers(), admin.admin_url("upload.php").as_bytes());
    if asked.post("html-upload").is_none() || files.is_empty() {
        return library();
    }
    if let Err(refusal) = hyscribe_admin::actions::referer(admin, asked, "media-form") {
        return refused(admin.reads.public(), refusal);
    }
    let Some(sent) = sent(files) else {
        return not_ported();
    };
    match media_handle_upload(admin, sent, post_id) {
        Ok(id) => {
            let mut answer = library();
            let name = header::HeaderName::from_static(crate::uploads::ATTACHMENT_ID);
            answer.headers_mut().insert(name, id.into());
            answer
        }
        Err(Refused::Message(message)) => died(admin, StatusCode::INTERNAL_SERVER_ERROR, &message),
        Err(refused) => answered(admin, refused),
    }
}

/// The posts' context of `admin`'s writes.
fn context(admin: &Admin, unfiltered_html: bool) -> Context {
    Context {
        user: admin.user.id,
        can_publish: false,
        home: admin.site.home.as_bytes().to_vec(),
        upload_host: hyscribe_render::link::host(admin.site.raw_option("siteurl")),
        unfiltered_html,
        constants: admin.site.constants,
        now: admin.reads.now,
    }
}

/// `media_handle_upload( 'async-upload', $post_id )` of `sent`: the
/// attachment's ID, refused without a writer or uploads folder.
fn media_handle_upload(admin: &Admin, sent: Sent, post_id: u64) -> Result<u64, Refused> {
    let (Some(writer), Some(uploads)) = (&admin.site.writer, &admin.site.uploads) else {
        return Err(Refused::Unported);
    };
    let who = Who {
        unfiltered_html: admin.can("unfiltered_html"),
        unfiltered_upload: admin.can("unfiltered_upload"),
    };
    let parent = (post_id != 0).then(|| admin.full_post(post_id)).flatten();
    let time = upload_time(admin.site, parent, admin.reads.now);
    let prepared = uploads.media_handle_upload(admin.site, sent, post_id, (&time, who))?;
    let ctx = context(admin, who.unfiltered_html);
    uploads.completed(admin.site, writer, &ctx, &prepared)
}

/// `wp_prepare_attachment_for_js( $id )` of an image the current user has
/// just uploaded: their own, which every role that uploads may edit and
/// delete, attached to a post they may edit, if any. No audio or video, and
/// no compat fields, which only plugins add.
fn prepare_for_js(admin: &Admin, id: u64) -> Value {
    let reads = &admin.reads.reads;
    let post = admin.full_post(id).unwrap_or_default();
    let meta = metadata(reads, id).filter(|meta| matches!(meta, Value::Array(_)));
    let (kind, subtype) = post
        .mime_type
        .split_once('/')
        .unwrap_or((&post.mime_type, ""));
    let url = attachment_url(reads, id).unwrap_or_default();
    let base_url = hyscribe_render::template::replace(&url, &wp_basename(&url), b"");
    let file = PostsSite::meta(admin, &post, "_wp_attached_file").unwrap_or_default();
    let can_read = |id| admin.can_for("read_post", id);
    let link = attachment_link(reads, &post, false, &can_read);
    let edit_link = |id| text(admin.admin_url(&format!("post.php?post={id}&action=edit")));
    let alt = PostsSite::meta(admin, &post, "_wp_attachment_image_alt");
    let unix = |date: &str| Value::Int(stored_unix(date, Zone::Utc) * 1000);
    let nonce = |action: String| text(PageSite::nonce(admin, &action));
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
        ("icon", {
            let icon = hyscribe_admin::media::icon_name(&file, &post.mime_type);
            text(format!(
                "{}/wp-includes/images/media/{icon}.svg",
                admin.site_url()
            ))
        }),
        (
            "dateFormatted",
            text(format("F j, Y", &post.date, admin.site.zone())),
        ),
        (
            "nonces",
            object(vec![
                ("update", nonce(format!("update-post_{id}"))),
                ("delete", nonce(format!("delete-post_{id}"))),
                ("edit", nonce(format!("image_editor-{id}"))),
            ]),
        ),
        ("editLink", edit_link(id)),
        ("meta", Value::Bool(false)),
    ];
    // `get_edit_user_link()` of the user's own is their profile.
    let name = match admin.user.display_name.is_empty() {
        true => CapabilityData::user_meta(admin, admin.user.id, "nickname").unwrap_or_default(),
        false => admin.user.display_name.clone(),
    };
    data.push(("authorName", text(html_entity_decode(&name))));
    data.push(("authorLink", text(admin.admin_url("profile.php"))));
    let parent = (post.parent != 0)
        .then(|| admin.full_post(post.parent))
        .flatten();
    if let Some(parent) = parent {
        let title = match parent.title.is_empty() {
            true => b"(no title)".to_vec(),
            false => parent.title.clone(),
        };
        data.push(("uploadedToTitle", text(title)));
        data.push(("uploadedToLink", edit_link(parent.id)));
    }
    let bytes = meta
        .as_ref()
        .and_then(|meta| meta.get_str("filesize"))
        .filter(|size| size.is_numeric() && size.to_int() > 0)
        .map(Value::to_int);
    if let Some(bytes) = bytes {
        data.push(("filesizeInBytes", Value::Int(bytes)));
        data.push((
            "filesizeHumanReadable",
            text(hyscribe_admin::media::size_format(bytes)),
        ));
    }
    let context = PostsSite::meta(admin, &post, "_wp_attachment_context");
    data.push(("context", text(context.unwrap_or_default())));
    let mut data: Vec<(Key, Value)> = data
        .into_iter()
        .map(|(key, value)| (Key::String(key.into()), value))
        .collect();
    if let Some(meta) = meta.filter(|_| kind == "image") {
        image_fields(admin, &mut data, (&meta, &url, &base_url));
    }
    data.push((
        Key::String(b"compat".to_vec()),
        object(vec![("item", text("")), ("meta", text(""))]),
    ));
    Value::Array(data)
}

/// `landscape` or `portrait`.
fn orientation(width: i64, height: i64) -> Value {
    text(if height > width {
        "portrait"
    } else {
        "landscape"
    })
}

/// An image's own fields of `wp_prepare_attachment_for_js()`: the
/// original's URL and name, its full size and its sizes.
fn image_fields(
    admin: &Admin,
    data: &mut Vec<(Key, Value)>,
    (meta, url, base): (&Value, &[u8], &[u8]),
) {
    let mut push = |key: &str, value: Value| data.push((Key::String(key.into()), value));
    let mut sizes = Vec::new();
    for name in ["thumbnail", "medium", "large"] {
        let Some(size) = meta.get_path(&["sizes", name]) else {
            continue;
        };
        let side = |key| size.get_str(key).map_or(0, Value::to_int);
        let reads = &admin.reads.reads;
        let (width, height) =
            image_constrain_size_for_editor(reads, (side("width"), side("height")), name);
        let file = size
            .get_str("file")
            .map(Value::to_php_string)
            .unwrap_or_default();
        let entry = object(vec![
            ("height", Value::Int(height)),
            ("width", Value::Int(width)),
            (
                "url",
                text(hyscribe_kses::esc_url_raw(&[base, &file[..]].concat())),
            ),
            ("orientation", orientation(width, height)),
        ]);
        sizes.push((Key::String(name.into()), entry));
    }
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
    let side = |key| meta.get_str(key).map(Value::to_int);
    let mut full = vec![("url", text(hyscribe_kses::esc_url_raw(url)))];
    if let (Some(width), Some(height)) = (side("width"), side("height")) {
        full.push(("height", Value::Int(height)));
        full.push(("width", Value::Int(width)));
        full.push(("orientation", orientation(width, height)));
    }
    // array_merge(): `url` is there already, the rest comes after.
    for (key, value) in full.iter().skip(1) {
        push(key, value.clone());
    }
    sizes.push((Key::String(b"full".to_vec()), object(full)));
    push("sizes", Value::Array(sizes));
}

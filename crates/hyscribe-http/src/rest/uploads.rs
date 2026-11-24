// SPDX-License-Identifier: AGPL-3.0-or-later

//! The media collection's upload, `create_item()` of `/wp/v2/media`: the
//! file sent as the body with its `Content-Type` and `Content-Disposition`
//! (`upload_from_data()`), or as a multipart form's `file`
//! (`upload_from_file()`), then the attachment, its sizes and metadata,
//! and the answer the attachments controller prepares. A `Content-MD5`
//! answers 501, and so does an image type no editor here is known to take.

use axum::body::Bytes;
use axum::http::{HeaderMap, HeaderName, HeaderValue};
use axum::response::Response;
use hyscribe_phpser::is_empty_string;
use hyscribe_rest::{Outcome, Pending, Request, Upload};
use hyscribe_write::Error;
use tokio::runtime::Handle;

use super::sent;
use super::writes::{Asker, context, finished, read_only};
use crate::Site;
use crate::uploads::attach::upload_time;
use crate::uploads::{Refused, Sent, Who};

/// The image types the permission check lets through: those the oracle's
/// editor takes, and SVG, which no editor resizes.
const IMAGE_TYPES: [&str; 9] = [
    "image/jpeg",
    "image/png",
    "image/gif",
    "image/webp",
    "image/avif",
    "image/heic",
    "image/heif",
    "image/bmp",
    "image/svg+xml",
];

/// A refusal as the controller gives it, 400.
fn invalid(code: &'static str, message: &str) -> Option<Outcome> {
    Some(Outcome::Failed(code, message.into(), 400))
}

/// The upload's file and whether it is a form's, or the refusal of what it
/// was sent with, `None` for what is not ported.
pub(super) fn file(
    headers: &HeaderMap,
    body: &Bytes,
    files: &[(Vec<u8>, Sent)],
    max: u64,
) -> Result<(Sent, bool), Option<Outcome>> {
    let header = |name: &str| headers.get(name).map(HeaderValue::as_bytes);
    if header("content-md5").is_some() {
        return Err(None);
    }
    if !files.is_empty() {
        let found = files.iter().find(|(field, _)| field == b"file");
        let (_, sent) = found.ok_or(None)?;
        let kind = String::from_utf8_lossy(&sent.kind).into_owned();
        if kind.starts_with("image/") && !IMAGE_TYPES.contains(&kind.as_str()) {
            return Err(None);
        }
        return Ok((sent.clone(), true));
    }
    if body.is_empty() {
        return Err(invalid("rest_upload_no_data", "No data supplied."));
    }
    let Some(kind) = header("content-type").filter(|kind| !is_empty_string(kind)) else {
        return Err(invalid(
            "rest_upload_no_content_type",
            "No Content-Type supplied.",
        ));
    };
    let disposition = header("content-disposition").filter(|value| !is_empty_string(value));
    let Some(disposition) = disposition else {
        return Err(invalid(
            "rest_upload_no_content_disposition",
            "No Content-Disposition supplied.",
        ));
    };
    let Some(name) = crate::multipart::disposition_filename(disposition) else {
        return Err(invalid(
            "rest_upload_invalid_disposition",
            "Invalid Content-Disposition supplied. Content-Disposition needs to be formatted as `attachment; filename=\"image.png\"` or similar.",
        ));
    };
    Ok((Sent::new(name, kind.to_vec(), body.clone(), max), false))
}

/// The answer to the upload `pending` plans, `upload` its fields, of a
/// request with `headers`, `body` and `files`, refused without a writer.
pub(super) fn answer(
    site: &Site,
    request: &Request,
    (pending, upload): (Pending, &Upload),
    asker: Asker,
    (headers, body, files): (&HeaderMap, &Bytes, &[(Vec<u8>, Sent)]),
    unfiltered_upload: bool,
) -> Response {
    let Some(writer) = &site.writer else {
        return sent(read_only());
    };
    let Some(uploads) = &site.uploads else {
        return sent(hyscribe_rest::Served::not_ported());
    };
    let now = asker.2;
    let uploaded = file(headers, body, files, uploads.max_size()).map(|(sent, form)| {
        let who = Who {
            unfiltered_html: pending.plan.unfiltered_html,
            unfiltered_upload,
        };
        // A database that fails here fails the insert after it.
        let parent =
            (upload.post != 0).then(|| Handle::current().block_on(site.db.post(upload.post)));
        let parent = parent.and_then(Result::ok).flatten();
        let time = upload_time(site, parent, now);
        let prepared = uploads.rest_upload(site, (sent, form), upload, (&time, who));
        let code = match form {
            true => "rest_upload_unknown_error",
            false => "rest_upload_sideload_error",
        };
        let prepared = match prepared {
            Err(Refused::Message(message)) => return Ok(Outcome::Failed(code, message, 500)),
            prepared => prepared?,
        };
        let ctx = context(site, &pending.plan, now);
        uploads
            .completed(site, writer, &ctx, &prepared)
            .map(Outcome::Written)
    });
    let outcome = match uploaded {
        Ok(Ok(outcome)) | Err(Some(outcome)) => Ok(outcome),
        Ok(Err(Refused::Failed(err))) => return crate::html(crate::database_error(site, &err)),
        Ok(Err(_)) | Err(None) => Err(Error::Unported("an upload")),
    };
    let id = match &outcome {
        Ok(Outcome::Written(id)) => Some(*id),
        _ => None,
    };
    let mut answer = finished(site, writer, request, pending, outcome, asker);
    if let Some(id) = id.filter(|_| answer.status().is_success()) {
        let name = HeaderName::from_static(crate::uploads::ATTACHMENT_ID);
        answer.headers_mut().insert(name, HeaderValue::from(id));
    }
    answer
}

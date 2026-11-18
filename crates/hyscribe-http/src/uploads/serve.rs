// SPDX-License-Identifier: AGPL-3.0-or-later

//! `/wp-content/uploads/...` from the uploads folder, as Apache serves a
//! site's uploads: the file's bytes, never run, with `Last-Modified` and an
//! `ETag` from its size and time, and conditional requests answered. A
//! path that is not a plain file inside the folder (`..`, a hidden name, a
//! link leading out) is not found. Images, audio, video, PDFs and plain
//! text are shown in the browser with their type; any other file, HTML and
//! SVG among them, is sent as a download, so nothing uploaded runs as the
//! site's own page.

use std::time::UNIX_EPOCH;

use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::Response;
use hyscribe_image::upload::wp_check_filetype;
use hyscribe_phpser::urldecode;

use crate::Site;

/// The URL path the uploads folder is served at.
const PREFIX: &str = "/wp-content/uploads/";

/// The type a file is shown with, `None` for one only downloaded.
fn shown(name: &str) -> Option<&'static str> {
    let (_, kind) = wp_check_filetype(name, false)?;
    let shown = ["image/", "audio/", "video/"]
        .iter()
        .any(|group| kind.starts_with(group))
        && kind != "image/svg+xml";
    (shown || kind == "application/pdf" || kind == "text/plain").then_some(kind)
}

/// The upload at the URL path `path`, answering `headers`' preconditions
/// at `now`; `None` for a path outside the uploads or a file not there.
pub(crate) fn serve(site: &Site, path: &str, headers: &HeaderMap, now: u64) -> Option<Response> {
    let uploads = site.uploads.as_ref()?;
    let relative = path.strip_prefix(PREFIX)?;
    // Apache decodes the path, a `+` staying a `+`.
    let decoded = urldecode(relative.replace('+', "%2B").as_bytes());
    let decoded = String::from_utf8(decoded).ok()?;
    if !decoded.split('/').all(super::stays) {
        return None;
    }
    let dir = uploads.dir.canonicalize().ok()?;
    let file = dir.join(&decoded).canonicalize().ok()?;
    let meta = std::fs::metadata(&file)
        .ok()
        .filter(|_| file.starts_with(&dir))?;
    let bytes = std::fs::read(&file).ok().filter(|_| meta.is_file())?;
    let since = meta
        .modified()
        .ok()
        .and_then(|at| at.duration_since(UNIX_EPOCH).ok());
    let modified = since.unwrap_or_default().as_secs();
    let etag = format!("\"{:x}-{:x}\"", meta.len(), modified);
    let name = decoded.rsplit('/').next().unwrap_or_default();
    let kind = shown(name);
    let response = (kind.unwrap_or("application/octet-stream"), bytes);
    let mut response = crate::files::conditional(headers, response, (modified, etag), now);
    if kind.is_none() {
        let download = HeaderValue::from_static("attachment");
        response
            .headers_mut()
            .insert(header::CONTENT_DISPOSITION, download);
    }
    Some(response)
}

#[cfg(test)]
mod tests;

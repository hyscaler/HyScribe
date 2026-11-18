// SPDX-License-Identifier: AGPL-3.0-or-later

//! Uploads (MVP 6.2): `_wp_handle_upload()`, the attachment
//! `media_handle_upload()` and the REST API insert, and the sizes and
//! metadata of `wp_generate_attachment_metadata()`, made on the image
//! worker pool (ADR 0015). A file is written under a temporary name and
//! then linked to its own, so a crash leaves no half file, and never
//! outside the uploads folder. A type PHP checks with `fileinfo` (any file
//! but an image) is not ported, answered 501 before anything is written.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::body::Bytes;
use hyscribe_image::codec::Limits;
use hyscribe_image::hooks::Hooks;
use hyscribe_image::pool::Pool;
use hyscribe_image::subsizes::{Generated, Upload, wp_generate_attachment_metadata};
use hyscribe_image::upload::{wp_check_filetype_and_ext, wp_unique_filename};
use hyscribe_phpser::{Value, is_empty_string};

use crate::Site;

pub(crate) mod attach;
pub(crate) mod serve;

/// `UPLOAD_ERR_INI_SIZE`'s message: the file is over `UPLOAD_MAX_SIZE`.
const TOO_LARGE: &str = "The uploaded file exceeds the upload_max_filesize directive in php.ini.";

/// The header that names a new attachment, so a client can resume its sizes.
pub(crate) const ATTACHMENT_ID: &str = "x-wp-upload-attachment-id";

/// `UPLOAD_ERR_NO_FILE`'s message: a file field sent without a file.
const NO_FILE: &str = "No file was uploaded.";

/// `_wp_handle_upload()`'s refusal of an empty file.
const EMPTY: &str = "File is empty. Please upload something more substantial. This error could also be caused by uploads being disabled in your php.ini file or by post_max_size being defined as smaller than upload_max_filesize in php.ini.";

/// `_wp_handle_upload()`'s refusal of a type not allowed.
const NOT_ALLOWED: &str = "Sorry, you are not allowed to upload this file type.";

/// WordPress's upload hook points under their names, plain calls until the
/// extension runtime (docs/EXTENSIONS.md); each default changes nothing.
/// The image hooks are `hyscribe-image`'s.
pub trait UploadHooks: Hooks + Send + Sync {
    /// `wp_handle_upload_prefilter`: the file as `$_FILES` gives it; an
    /// `error` refuses it.
    fn wp_handle_upload_prefilter(&self, file: Sent) -> Sent {
        file
    }

    /// `wp_handle_sideload_prefilter`, for a file sent as the body.
    fn wp_handle_sideload_prefilter(&self, file: Sent) -> Sent {
        file
    }

    /// `wp_handle_upload`: the file written, its URL and type, and the
    /// context, `upload` or `sideload`. The file stays where it was written.
    fn wp_handle_upload(&self, upload: Handled, _context: &str) -> Handled {
        upload
    }

    /// `add_attachment`: an attachment was inserted.
    fn add_attachment(&self, _id: u64) {}
}

/// No hook attached.
pub struct NoHooks;

impl Hooks for NoHooks {}

impl UploadHooks for NoHooks {}

/// The uploads folder and what uploads are held to.
pub struct Uploads {
    dir: PathBuf,
    max_size: u64,
    limits: Limits,
    pool: Pool,
    hooks: Arc<dyn UploadHooks>,
}

impl std::fmt::Debug for Uploads {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Uploads").field("dir", &self.dir).finish()
    }
}

impl Uploads {
    /// Uploads into `dir`, of at most `max_size` bytes, their images
    /// decoded within `limits` by a worker for each CPU.
    pub fn new(dir: PathBuf, max_size: u64, limits: hyscribe_config::ImageLimits) -> Uploads {
        let workers = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
        Uploads {
            dir,
            max_size,
            limits: Limits {
                bytes: limits.bytes,
                width: limits.width,
                height: limits.height,
                area: limits.area,
                memory: limits.memory,
            },
            pool: Pool::new(workers, workers * 4),
            hooks: Arc::new(NoHooks),
        }
    }

    /// `UPLOAD_MAX_SIZE`.
    pub(crate) fn max_size(&self) -> u64 {
        self.max_size
    }

    /// The uploads folder's path of `file`, an attachment's
    /// `_wp_attached_file`: `None` for one that leaves the folder, as an
    /// absolute path or `..` would.
    pub(crate) fn path(&self, file: &[u8]) -> Option<PathBuf> {
        use std::os::unix::ffi::OsStrExt as _;
        let relative = Path::new(std::ffi::OsStr::from_bytes(file));
        let inside = relative
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)));
        inside.then(|| self.dir.join(relative))
    }

    /// `wp_filesize()` of `file` when `is_readable()`, else none; `None`
    /// for a file outside the folder.
    pub(crate) fn file_size(&self, file: &[u8]) -> Option<Option<u64>> {
        let path = self.path(file)?;
        Some(std::fs::metadata(path).ok().map(|meta| meta.len()))
    }

    /// `wp_delete_file_from_directory()` of each file within its folder:
    /// deleted when both resolve inside the uploads folder, the file
    /// below the folder.
    pub(crate) fn delete(&self, files: &[(Vec<u8>, Vec<u8>)]) {
        for (file, folder) in files {
            let (Some(path), Some(folder)) = (self.path(file), self.path(folder)) else {
                continue;
            };
            let real = |path: &Path| std::fs::canonicalize(path).ok();
            let (Some(real_file), Some(real_folder)) = (real(&path), real(&folder)) else {
                continue;
            };
            if real_file != real_folder && real_file.starts_with(&real_folder) {
                let _ = std::fs::remove_file(path);
            }
        }
    }

    /// The uploads with `hooks` attached, as a test attaches them until
    /// the extension runtime does.
    #[cfg(test)]
    pub(crate) fn with_hooks(self, hooks: Arc<dyn UploadHooks>) -> Uploads {
        Uploads { hooks, ..self }
    }
}

/// An uploaded file as `$_FILES` describes it.
#[derive(Clone, Debug)]
pub struct Sent {
    /// `name`, as sent.
    pub name: Vec<u8>,
    /// `type`, as sent.
    pub kind: Vec<u8>,
    /// `error`: why the file did not arrive, as its message.
    pub error: Option<String>,
    /// The file's bytes.
    pub bytes: Bytes,
}

impl Sent {
    /// The file `name` of `kind` with `bytes`, as PHP takes it: no file
    /// without a name, and none over `max` bytes, as over
    /// `upload_max_filesize`.
    pub(crate) fn new(name: Vec<u8>, kind: Vec<u8>, bytes: Bytes, max: u64) -> Sent {
        let error = match name.is_empty() {
            true => Some(NO_FILE),
            false => (bytes.len() as u64 > max).then_some(TOO_LARGE),
        };
        Sent {
            name,
            kind,
            bytes: if error.is_some() { Bytes::new() } else { bytes },
            error: error.map(String::from),
        }
    }
}

/// What `_wp_handle_upload()` gives: the file written, its URL and type.
#[derive(Clone, Debug)]
pub struct Handled {
    /// `file`: its path within the uploads folder.
    pub file: String,
    /// `url`.
    pub url: Vec<u8>,
    /// `type`.
    pub mime: String,
    /// The name the file was sent as, with the extension its type gives.
    pub name: Vec<u8>,
    /// The file's bytes.
    pub bytes: Bytes,
}

/// Why an upload stops.
#[derive(Debug, PartialEq)]
pub(crate) enum Refused {
    /// The error message PHP gives.
    Message(String),
    /// What is not ported: 501, before anything is written.
    Unported,
    /// A write or file that failed, the error page's text.
    Failed(String),
}

impl From<hyscribe_write::Error> for Refused {
    fn from(err: hyscribe_write::Error) -> Refused {
        match err {
            hyscribe_write::Error::Unported(_) | hyscribe_write::Error::Refused(_) => {
                Refused::Unported
            }
            err => Refused::Failed(format!("{err:?}")),
        }
    }
}

/// Who uploads: what the type check allows them.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Who {
    /// `current_user_can( 'unfiltered_html' )`.
    pub unfiltered_html: bool,
    /// `current_user_can( 'unfiltered_upload' )`.
    pub unfiltered_upload: bool,
}

/// Whether a request is one that takes an upload: hs-admin's upload
/// screens, and the REST API's media collection.
pub(crate) fn takes_upload(uri: &axum::http::Uri) -> bool {
    let path = uri.path();
    let route = hyscribe_phpser::parse_str(uri.query().unwrap_or_default().as_bytes());
    let route = route.get_str("rest_route").and_then(Value::as_bytes);
    ["/hs-admin/async-upload", "/hs-admin/media-new"].contains(&path)
        || path.ends_with("/wp-json/wp/v2/media")
        || route == Some(b"/wp/v2/media")
}

/// `$_FILES`: each file sent, by its field.
pub(crate) type Files = Vec<(Vec<u8>, Sent)>;

/// `$_POST` and `$_FILES` of a `multipart/form-data` body, by field, a
/// file over `max` bytes refused as PHP refuses one over
/// `upload_max_filesize`; `None` for another body, or one it cannot read.
pub(crate) fn form(
    headers: &axum::http::HeaderMap,
    body: &Bytes,
    max: u64,
) -> Option<(Value, Files)> {
    let kind = headers.get(axum::http::header::CONTENT_TYPE)?.as_bytes();
    let form = crate::multipart::read(body, crate::multipart::boundary(kind)?)?;
    let files = form.files.iter().map(|file| {
        let (name, kind) = (file.name.clone(), file.kind.clone());
        let sent = Sent::new(name, kind, body.slice_ref(file.bytes), max);
        (file.field.clone(), sent)
    });
    Some((form.post(), files.collect()))
}

/// A file name that stays in its folder: not empty, no separator or NUL,
/// and no leading dot, so never `.`, `..` or a temporary file.
fn stays(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && !name.contains(['/', '\\', '\0'])
}

/// The names of the files in `folder`.
fn names(folder: &Path) -> std::io::Result<Vec<String>> {
    std::fs::read_dir(folder)?
        .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
        .collect()
}

/// `folder/name` with `bytes`, written under a temporary name first: linked
/// to a name not taken, or renamed over one with `replace`.
fn place(folder: &Path, name: &str, bytes: &[u8], replace: bool) -> std::io::Result<()> {
    static TEMPORARY: AtomicU64 = AtomicU64::new(0);
    if !stays(name) {
        return Err(std::io::ErrorKind::InvalidInput.into());
    }
    let n = TEMPORARY.fetch_add(1, Ordering::Relaxed);
    let temporary = folder.join(format!(".hyscribe-upload-{}-{n}", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    let placed = written.and_then(|()| match replace {
        true => std::fs::rename(&temporary, folder.join(name)),
        false => std::fs::hard_link(&temporary, folder.join(name)),
    });
    // Renamed, the temporary name is gone already.
    let _ = std::fs::remove_file(&temporary);
    placed
}

/// `wp_upload_dir()`'s folder for `subdir`, made if missing: `None` when
/// it cannot be made or leads outside `base`.
fn folder(base: &Path, subdir: &str) -> Option<PathBuf> {
    let folder = base.join(subdir.trim_start_matches('/'));
    std::fs::create_dir_all(&folder).ok()?;
    match (base.canonicalize(), folder.canonicalize()) {
        (Ok(base), Ok(folder)) if folder.starts_with(&base) => Some(folder),
        _ => None,
    }
}

impl Uploads {
    /// `_wp_handle_upload( $file, $overrides, $time )` of `sent` by `who`
    /// for a post dated `time` (`current_time( 'mysql' )` without one):
    /// the file written, once its prefilter, error, size and type pass.
    pub(crate) fn handle(
        &self,
        site: &Site,
        sent: Sent,
        (time, who): (&str, Who),
        sideload: bool,
    ) -> Result<Handled, Refused> {
        let sent = match sideload {
            true => self.hooks.wp_handle_sideload_prefilter(sent),
            false => self.hooks.wp_handle_upload_prefilter(sent),
        };
        if let Some(error) = sent.error {
            return Err(Refused::Message(error));
        }
        if sent.bytes.is_empty() {
            return Err(Refused::Message(EMPTY.into()));
        }
        let name = String::from_utf8_lossy(&sent.name).into_owned();
        let checked = wp_check_filetype_and_ext(&sent.bytes, &name, who.unfiltered_html)
            .map_err(|_| Refused::Unported)?;
        let name = checked
            .proper_filename
            .map_or(sent.name, String::into_bytes);
        let (_, mime) = match checked.ext_type {
            Some(found) => found,
            // A type PHP lets through by the browser's word alone.
            None if who.unfiltered_upload => return Err(Refused::Unported),
            None => return Err(Refused::Message(NOT_ALLOWED.into())),
        };
        // Only the default folder and URL are ported.
        let option = |name: &str| site.raw_option(name);
        let path = hyscribe_phpser::php_trim(option("upload_path"));
        let default = is_empty_string(path) || path == b"wp-content/uploads";
        if !default || !is_empty_string(option("upload_url_path")) {
            return Err(Refused::Unported);
        }
        let subdir = match is_empty_string(option("uploads_use_yearmonth_folders")) {
            true => String::new(),
            false => {
                let part = |range| time.get(range).unwrap_or_default();
                format!("/{}/{}", part(0..4), part(5..7))
            }
        };
        let shown = format!("wp-content/uploads{subdir}");
        let Some(folder) = folder(&self.dir, &subdir) else {
            return Err(Refused::Message(format!(
                "Unable to create directory {shown}. Is its parent directory writable by the server?"
            )));
        };
        let moved = names(&folder).and_then(|files| {
            let unique = wp_unique_filename(&name, &files, who.unfiltered_html);
            place(&folder, &unique, &sent.bytes, false).map(|()| unique)
        });
        let Ok(unique) = moved else {
            return Err(Refused::Message(format!(
                "The uploaded file could not be moved to {shown}."
            )));
        };
        let base =
            hyscribe_render::attachment::upload_baseurl(&|name| site.raw_option(name).to_vec());
        let file = format!("{}/{unique}", subdir.trim_start_matches('/'));
        let file = file.trim_start_matches('/').to_string();
        let handled = Handled {
            url: [&base[..], b"/", file.as_bytes()].concat(),
            file: file.clone(),
            mime: mime.into(),
            name,
            bytes: sent.bytes,
        };
        let context = if sideload { "sideload" } else { "upload" };
        let filtered = self.hooks.wp_handle_upload(handled, context);
        Ok(Handled { file, ..filtered })
    }

    /// Deletes an upload `handle()` wrote, when what follows stops it.
    pub(crate) fn discard(&self, handled: &Handled) {
        let _ = std::fs::remove_file(self.dir.join(&handled.file));
    }

    /// `wp_generate_attachment_metadata( $id, $file )` of the attachment
    /// `id`, made on a worker, its files written beside the upload.
    pub(crate) fn generate(
        &self,
        site: &Site,
        handled: &Handled,
        id: u64,
    ) -> std::io::Result<Generated> {
        let sizes = hyscribe_image::subsizes::wp_get_registered_image_subsizes(&|name| {
            site.raw_option(name).to_vec()
        });
        let host = hyscribe_render::link::host(site.raw_option("siteurl"));
        let (bytes, file, mime) = (
            handled.bytes.clone(),
            handled.file.clone(),
            handled.mime.clone(),
        );
        let (limits, hooks) = (self.limits, Arc::clone(&self.hooks));
        let generated = self.pool.run(move || {
            let upload = Upload {
                bytes: &bytes,
                file: &file,
                attachment: id,
            };
            let hooks: &dyn Hooks = &*hooks;
            wp_generate_attachment_metadata(upload, &mime, sizes, &limits, hooks, &host)
        });
        // A job the pool drops, or metadata not ported, which the routes
        // read before the insert, leaves the attachment without metadata.
        let Some(Ok(generated)) = generated else {
            return Err(std::io::Error::other("no metadata was made"));
        };
        let folder = self.dir.join(&handled.file);
        let folder = folder.parent().unwrap_or(&self.dir);
        for saved in &generated.files {
            let name = saved.path.rsplit('/').next().unwrap_or_default();
            place(folder, name, &saved.bytes, true)?;
        }
        Ok(generated)
    }

    /// `add_attachment`, once the attachment's action is committed.
    pub(crate) fn added(&self, id: u64) {
        self.hooks.add_attachment(id);
    }
}

#[cfg(test)]
mod tests;

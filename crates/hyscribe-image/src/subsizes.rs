// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_generate_attachment_metadata()` for an image: the `-scaled` or
//! `-rotated` copy, the registered sizes (`wp_create_image_subsizes()`,
//! `_wp_make_subsizes()`) and the metadata WordPress stores, with
//! `image_resize_dimensions()` and `wp_constrain_dimensions()` behind
//! them. An image no editor here opens (AVIF, BMP, HEIC, or over a limit)
//! keeps its width and height and gets no sizes, as PHP keeps an image its
//! editor fails on.

use hyscribe_phpser::{Key, Value, lookup};
use hyscribe_render::attachment::constrain_dimensions;

use crate::codec::{Error, Limits};
use crate::editor::{Editor, Saved};
use crate::getid3::{wp_read_audio_metadata, wp_read_video_metadata};
use crate::header::{ImageType, getimagesize};
use crate::hooks::Hooks;
use crate::meta::{Unported, wp_read_image_metadata};

/// Where a cropped size is taken from: `false`, or `true` and the
/// `array( $x, $y )` positions (`left`, `center`, `right`; `top`,
/// `center`, `bottom`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Crop {
    /// No crop.
    No,
    /// A crop at these positions.
    At(String, String),
}

impl Crop {
    /// `true`: centred.
    pub fn centre() -> Crop {
        Crop::At("center".into(), "center".into())
    }
}

/// One of `wp_get_registered_image_subsizes()`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubSize {
    /// The size's name.
    pub name: String,
    /// The width, 0 for any.
    pub width: u32,
    /// The height, 0 for any.
    pub height: u32,
    /// The crop.
    pub crop: Crop,
}

/// `wp_get_registered_image_subsizes()` of a site with `option` and
/// WordPress's own two added sizes, `1536x1536` and `2048x2048`.
pub fn wp_get_registered_image_subsizes(option: &dyn Fn(&str) -> Vec<u8>) -> Vec<SubSize> {
    let int = |name: String| {
        let text = String::from_utf8_lossy(&option(&name)).into_owned();
        let digits: String = text
            .trim_start()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        digits.parse::<u32>().unwrap_or(0)
    };
    let mut sizes = Vec::new();
    for name in ["thumbnail", "medium", "medium_large", "large"] {
        let (width, height) = (int(format!("{name}_size_w")), int(format!("{name}_size_h")));
        let crop = option(&format!("{name}_crop"));
        let crop = if crop.is_empty() || crop == b"0" {
            Crop::No
        } else {
            Crop::centre()
        };
        if width != 0 || height != 0 {
            sizes.push(SubSize {
                name: name.into(),
                width,
                height,
                crop,
            });
        }
    }
    for side in [1536, 2048] {
        sizes.push(SubSize {
            name: format!("{side}x{side}"),
            width: side,
            height: side,
            crop: Crop::No,
        });
    }
    sizes
}

/// `(int) round()` as PHP rounds, half away from zero.
fn round(value: f64) -> u32 {
    value.round() as u32
}

/// `image_resize_dimensions()`: `[ 0, 0, $s_x, $s_y, $new_w, $new_h,
/// $crop_w, $crop_h ]`, `None` for `false`.
pub fn image_resize_dimensions(
    (orig_w, orig_h): (u32, u32),
    (dest_w, dest_h): (u32, u32),
    crop: &Crop,
) -> Option<[u32; 8]> {
    if orig_w == 0 || orig_h == 0 || (dest_w == 0 && dest_h == 0) {
        return None;
    }
    // PHP's refusal of a size larger than the image is left to the check of
    // the same size below, which gives the same answer.
    let (s_x, s_y, new_w, new_h, crop_w, crop_h) = match crop {
        Crop::At(x, y) => {
            let aspect = f64::from(orig_w) / f64::from(orig_h);
            let mut new_w = dest_w.min(orig_w);
            let mut new_h = dest_h.min(orig_h);
            if new_w == 0 {
                new_w = round(f64::from(new_h) * aspect);
            }
            if new_h == 0 {
                new_h = round(f64::from(new_w) / aspect);
            }
            let ratio =
                (f64::from(new_w) / f64::from(orig_w)).max(f64::from(new_h) / f64::from(orig_h));
            let crop_w = round(f64::from(new_w) / ratio);
            let crop_h = round(f64::from(new_h) / ratio);
            let place = |at: &str, start: &str, end: &str, orig: u32, crop: u32| match at {
                _ if at == start => 0,
                _ if at == end => orig.saturating_sub(crop),
                _ => orig.saturating_sub(crop) / 2,
            };
            (
                place(x, "left", "right", orig_w, crop_w),
                place(y, "top", "bottom", orig_h, crop_h),
                new_w,
                new_h,
                crop_w,
                crop_h,
            )
        }
        Crop::No => {
            let (new_w, new_h) =
                constrain_dimensions(orig_w.into(), orig_h.into(), dest_w.into(), dest_h.into());
            let (new_w, new_h) = (new_w as u32, new_h as u32);
            (0, 0, new_w, new_h, orig_w, orig_h)
        }
    };
    // Within a pixel of the original is the original.
    if new_w.abs_diff(orig_w) <= 1 && new_h.abs_diff(orig_h) <= 1 {
        return None;
    }
    Some([0, 0, s_x, s_y, new_w, new_h, crop_w, crop_h])
}

/// An upload: its bytes, its path within the uploads folder and its
/// attachment.
#[derive(Clone, Copy, Debug)]
pub struct Upload<'a> {
    /// The file's bytes.
    pub bytes: &'a [u8],
    /// `2026/11/name.jpg`.
    pub file: &'a str,
    /// The attachment's ID.
    pub attachment: u64,
}

/// What `wp_generate_attachment_metadata()` comes to.
#[derive(Clone, Debug, PartialEq)]
pub struct Generated {
    /// `_wp_attachment_metadata`.
    pub meta: Value,
    /// The files made, each with its path within the uploads folder.
    pub files: Vec<Saved>,
    /// `_wp_attached_file` where it changes to a `-scaled` or `-rotated`
    /// copy.
    pub attached_file: Option<String>,
    /// Why the image got no sizes, for the log.
    pub unedited: Option<Error>,
}

fn key(name: &str) -> Key {
    Key::String(name.into())
}

fn int(n: impl Into<i64>) -> Value {
    Value::Int(n.into())
}

fn text(text: &str) -> Value {
    Value::String(text.as_bytes().to_vec())
}

/// `_wp_attachment_metadata` as it is made.
struct Meta {
    width: u32,
    height: u32,
    file: String,
    filesize: usize,
    sizes: Vec<(Key, Value)>,
    image_meta: Vec<(Key, Value)>,
    original_image: Option<String>,
}

impl Meta {
    /// The array WordPress stores, its keys in WordPress's order.
    fn value(&self) -> Value {
        let mut entries = vec![
            (key("width"), int(self.width)),
            (key("height"), int(self.height)),
            (key("file"), text(&self.file)),
            (key("filesize"), int(self.filesize as i64)),
            (key("sizes"), Value::Array(self.sizes.clone())),
            (key("image_meta"), Value::Array(self.image_meta.clone())),
        ];
        entries.extend(
            self.original_image
                .as_deref()
                .map(|name| (key("original_image"), text(name))),
        );
        Value::Array(entries)
    }
}

/// A size's entry in `sizes`.
fn size_meta(saved: &Saved) -> Value {
    Value::Array(vec![
        (key("file"), text(&saved.file)),
        (key("width"), int(saved.width)),
        (key("height"), int(saved.height)),
        (key("mime-type"), text(saved.mime)),
        (key("filesize"), int(saved.bytes.len() as i64)),
    ])
}

/// What WordPress keeps of an image: the metadata and files made so far.
struct Making<'a> {
    upload: Upload<'a>,
    limits: &'a Limits,
    hooks: &'a dyn Hooks,
    meta: Meta,
    files: Vec<Saved>,
    unedited: Option<Error>,
}

impl<'a> Making<'a> {
    fn editor(&mut self) -> Option<Editor<'a>> {
        Editor::load(self.upload.bytes, self.upload.file, self.limits, self.hooks)
            .map_err(|err| self.unedited = Some(err))
            .ok()
    }

    /// `_wp_image_meta_replace_original()` with the copy `saved`, and the
    /// stored EXIF orientation set to 1 if it was `turned`.
    fn replace_original(&mut self, saved: Saved, turned: bool) {
        let original = self.upload.file.rsplit('/').next().unwrap_or_default();
        self.meta.width = saved.width;
        self.meta.height = saved.height;
        self.meta.file.clone_from(&saved.path);
        self.meta.filesize = saved.bytes.len();
        self.meta.original_image = Some(original.into());
        self.files.push(saved);
        // Turned, the stored orientation is one of 2 to 8: never empty.
        let set = |(k, _): &(Key, Value)| *k == key("orientation");
        if let Some((_, orientation)) = self
            .meta
            .image_meta
            .iter_mut()
            .find(|entry| turned && set(entry))
        {
            *orientation = int(1);
        }
    }

    /// `_wp_make_subsizes()`: the sizes made from the original, turned as
    /// its EXIF says, in WordPress's order of priority.
    fn make_subsizes(&mut self, sizes: Vec<SubSize>, orientation: Option<u32>) {
        let priority = ["medium", "large", "thumbnail", "medium_large"];
        let mut ordered: Vec<SubSize> = priority
            .iter()
            .filter_map(|name| sizes.iter().find(|s| s.name == *name).cloned())
            .collect();
        ordered.extend(
            sizes
                .into_iter()
                .filter(|s| !priority.contains(&s.name.as_str())),
        );
        let Some(mut editor) = ordered.first().and_then(|_| self.editor()) else {
            return;
        };
        editor.maybe_exif_rotate(orientation);
        for size in ordered {
            let Some(saved) = editor.make_subsize(&size).and_then(Result::ok) else {
                continue;
            };
            match self
                .meta
                .sizes
                .iter_mut()
                .find(|(k, _)| *k == key(&size.name))
            {
                Some((_, old)) => *old = size_meta(&saved),
                None => self.meta.sizes.push((key(&size.name), size_meta(&saved))),
            }
            self.files.push(saved);
        }
    }
}

/// `wp_generate_attachment_metadata()` of an upload of `mime`: for an
/// image, the copies and sizes made and the metadata; for any other type
/// (a TIFF too, which no editor displays) its `filesize` alone. `sizes`
/// are the registered sizes; `upload_host` is the uploads URL's host, for
/// `wp_kses_post()`.
pub fn wp_generate_attachment_metadata(
    upload: Upload<'_>,
    mime: &str,
    sizes: Vec<SubSize>,
    limits: &Limits,
    hooks: &dyn Hooks,
    upload_host: &[u8],
) -> Result<Generated, Unported> {
    let size = getimagesize(upload.bytes);
    // file_is_displayable_image(): BMP, ICO and the editors' types.
    let displayable = size.as_ref().is_some_and(|s| {
        !matches!(
            s.kind,
            ImageType::TiffIi | ImageType::TiffMm | ImageType::Heif
        )
    });
    let (Some(size), true) = (
        size,
        mime == "image/heic" || (mime.starts_with("image/") && displayable),
    ) else {
        // A video's or audio file's, which getID3 reads, then the size where
        // it gives none.
        let format = |entries: &[(Key, Value)]| {
            lookup(entries, "fileformat")
                .map(|v| String::from_utf8_lossy(&v.to_php_string()).into_owned())
        };
        let filtered = if mime.starts_with("video/") {
            let read = wp_read_video_metadata(upload.bytes)?;
            let file_format = format(&read);
            hooks.wp_read_video_metadata(Value::Array(read), file_format.as_deref())
        } else if mime.starts_with("audio/") {
            let read = wp_read_audio_metadata(upload.bytes)?;
            let file_format = format(&read);
            hooks.wp_read_audio_metadata(Value::Array(read), file_format.as_deref())
        } else {
            Value::Array(Vec::new())
        };
        // A filter that gives anything else leaves no metadata, as PHP does.
        let mut entries = match filtered {
            Value::Array(entries) => entries,
            _ => Vec::new(),
        };
        if lookup(&entries, "filesize").is_none() {
            entries.push((key("filesize"), int(upload.bytes.len() as i64)));
        }
        let meta = Value::Array(entries);
        let meta = hooks.wp_generate_attachment_metadata(meta, upload.attachment, "create");
        return Ok(Generated {
            meta,
            files: Vec::new(),
            attached_file: None,
            unedited: None,
        });
    };
    let meta = Meta {
        width: size.width,
        height: size.height,
        file: upload.file.into(),
        filesize: upload.bytes.len(),
        sizes: Vec::new(),
        image_meta: wp_read_image_metadata(upload.bytes, &size, upload_host)?,
        original_image: None,
    };
    let mut making = Making {
        upload,
        limits,
        hooks,
        meta,
        files: Vec::new(),
        unedited: None,
    };
    // What maybe_exif_rotate() reads: a JPEG's EXIF orientation, as stored.
    let stored = making
        .meta
        .image_meta
        .iter()
        .find(|(k, _)| *k == key("orientation"));
    let orientation = stored.and_then(|(_, v)| {
        String::from_utf8_lossy(v.as_bytes().unwrap_or_default())
            .parse()
            .ok()
    });
    let orientation = orientation.filter(|_| size.kind == ImageType::Jpeg);
    let threshold = hooks.big_image_size_threshold(
        2560,
        (size.width, size.height),
        upload.file,
        upload.attachment,
    );
    let scale_down =
        threshold != 0 && (i64::from(size.width) > threshold || i64::from(size.height) > threshold);
    let (suffix, wanted) = if scale_down {
        ("scaled", true)
    } else {
        ("rotated", orientation.is_some_and(|o| o != 1))
    };
    if wanted {
        let Some(mut editor) = making.editor() else {
            return Ok(making.finish());
        };
        let side = u32::try_from(threshold).unwrap_or(0);
        let resized = !scale_down || editor.resize(side, side, &Crop::No);
        // Scaled, it is saved whether or not it turns; else only if it does.
        let turned = resized && editor.maybe_exif_rotate(orientation);
        if resized
            && (scale_down || turned)
            && let Ok(saved) = editor.save(Some(editor.generate_filename(Some(suffix))))
        {
            making.replace_original(saved, turned);
        }
    }
    let sizes =
        hooks.intermediate_image_sizes_advanced(sizes, &making.meta.value(), upload.attachment);
    making.make_subsizes(sizes, orientation);
    Ok(making.finish())
}

impl Making<'_> {
    fn finish(self) -> Generated {
        let attached_file = self
            .meta
            .original_image
            .as_ref()
            .map(|_| self.meta.file.clone());
        let meta = self.hooks.wp_generate_attachment_metadata(
            self.meta.value(),
            self.upload.attachment,
            "create",
        );
        Generated {
            meta,
            files: self.files,
            attached_file,
            unedited: self.unedited,
        }
    }
}

#[cfg(test)]
mod tests;

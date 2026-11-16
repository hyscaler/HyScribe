// SPDX-License-Identifier: AGPL-3.0-or-later

//! The image editor, shaped like `WP_Image_Editor_Imagick` (ADR 0015):
//! loaded from a file's bytes, then `resize()`, `crop()`, `rotate()`,
//! `flip()`, `maybe_exif_rotate()`, `save()` and `make_subsize()` as
//! WordPress calls them. Files are named, not written: a save gives the
//! bytes and the path within the uploads folder, for the caller to write.

use crate::codec::{self, Error, Kept, Limits};
use crate::header::image_type;
use crate::hooks::Hooks;
use crate::pixels::Pixels;
use crate::subsizes::{Crop, SubSize, image_resize_dimensions};

/// A file the editor made: `_save()`'s answer, with its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved {
    /// `path`: within the uploads folder.
    pub path: String,
    /// `file`: the base name, after `image_make_intermediate_size`.
    pub file: String,
    /// `width`.
    pub width: u32,
    /// `height`.
    pub height: u32,
    /// `mime-type`.
    pub mime: &'static str,
    /// The file's bytes, whose length is `filesize`.
    pub bytes: Vec<u8>,
}

/// An image being edited.
pub struct Editor<'a> {
    image: Pixels,
    kept: Kept,
    /// The file's path within the uploads folder.
    file: String,
    mime: &'static str,
    quality: u8,
    hooks: &'a dyn Hooks,
}

/// `$dir/$name` of a path, the name without its extension, and the
/// extension.
fn parts(path: &str) -> (&str, &str, &str) {
    let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    (dir, stem, ext)
}

impl<'a> Editor<'a> {
    /// `wp_get_image_editor( $file )`: the first frame of `bytes`, the
    /// file at `file` within the uploads folder, held to `limits`.
    pub fn load(
        bytes: &[u8],
        file: &str,
        limits: &Limits,
        hooks: &'a dyn Hooks,
    ) -> Result<Editor<'a>, Error> {
        let kind = image_type(bytes).ok_or(Error::Undecodable)?;
        let decoded = codec::decode(bytes, kind, limits)?;
        let mut editor = Editor {
            image: decoded.pixels,
            kept: decoded.kept,
            file: file.into(),
            mime: kind.mime(),
            quality: 82,
            hooks,
        };
        editor.set_quality(editor.size());
        Ok(editor)
    }

    /// `get_size()`.
    pub fn size(&self) -> (u32, u32) {
        (self.image.width, self.image.height)
    }

    /// `set_quality( null, $dims )`: the default, 82, through
    /// `wp_editor_set_quality` and, for a JPEG, `jpeg_quality`; outside 0
    /// to 100 the default, and 0 is 1.
    fn set_quality(&mut self, size: (u32, u32)) {
        let mut quality = self.hooks.wp_editor_set_quality(82, self.mime, size);
        if self.mime == "image/jpeg" {
            quality = self.hooks.jpeg_quality(quality, "image_resize");
        }
        self.quality = u8::try_from(quality)
            .ok()
            .filter(|&q| q <= 100)
            .unwrap_or(82)
            .max(1);
    }

    /// `thumbnail_image()`: point sampled to 5 times a big reduction, measured
    /// against `size`, then the triangle filter (a JPEG's unsharp mask changes no pixel).
    fn thumbnail(&mut self, (width, height): (u32, u32), size: (u32, u32)) {
        let ratio = f64::from(width) / f64::from(size.0) * (f64::from(height) / f64::from(size.1));
        if ratio < 0.111 && width > 25 && height > 25 {
            self.image = self.image.sample(width * 5, height * 5);
        }
        self.image = self.image.resize(width, height);
    }

    /// `resize( $max_w, $max_h, $crop )`; `false` where
    /// `image_resize_dimensions()` gives no size.
    pub fn resize(&mut self, max_w: u32, max_h: u32, crop: &Crop) -> bool {
        let size = self.size();
        if size == (max_w, max_h) {
            return true;
        }
        let Some(dims) = image_resize_dimensions(size, (max_w, max_h), crop) else {
            return false;
        };
        let [_, _, src_x, src_y, dst_w, dst_h, src_w, src_h] = dims;
        if *crop != Crop::No {
            self.crop((src_x, src_y, src_w, src_h), (dst_w, dst_h));
        } else {
            self.set_quality((dst_w, dst_h));
            self.thumbnail((dst_w, dst_h), size);
        }
        true
    }

    /// `crop( $src_x, $src_y, $src_w, $src_h, $dst_w, $dst_h )`.
    pub fn crop(&mut self, (x, y, width, height): (u32, u32, u32, u32), target: (u32, u32)) {
        let size = self.size();
        self.image = self.image.crop(x, y, width, height);
        let target = (
            if target.0 == 0 { width } else { target.0 },
            if target.1 == 0 { height } else { target.1 },
        );
        self.thumbnail(target, size);
    }

    /// `rotate( $angle )`, counter-clockwise in quarter turns.
    pub fn rotate(&mut self, angle: u32) {
        let quarters = 4 - angle / 90 % 4;
        self.image = self.image.rotate(quarters as u8);
    }

    /// `flip( $horz, $vert )`: `$horz` upside down, `$vert` left to right.
    pub fn flip(&mut self, horz: bool, vert: bool) {
        if horz {
            self.image = self.image.flip();
        }
        if vert {
            self.image = self.image.flop();
        }
    }

    /// `maybe_exif_rotate()` for a JPEG's EXIF `orientation`: whether the
    /// image was turned or mirrored.
    pub fn maybe_exif_rotate(&mut self, orientation: Option<u32>) -> bool {
        match orientation.filter(|_| self.mime == "image/jpeg") {
            Some(2) => self.flip(false, true),
            Some(3) => self.flip(true, true),
            Some(4) => self.flip(true, false),
            Some(5) => {
                self.rotate(90);
                self.flip(true, false);
            }
            Some(6) => self.rotate(270),
            Some(7) => {
                self.rotate(90);
                self.flip(false, true);
            }
            Some(8) => self.rotate(90),
            _ => return false,
        }
        true
    }

    /// `generate_filename( $suffix )`: the file's name with `-$suffix`,
    /// the size's `-WxH` without one, and the extension in lower case.
    pub fn generate_filename(&self, suffix: Option<&str>) -> String {
        let (dir, stem, ext) = parts(&self.file);
        let (width, height) = self.size();
        let suffix = suffix.map_or(format!("-{width}x{height}"), |s| match s {
            "" => String::new(),
            s => format!("-{s}"),
        });
        let dir = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        format!("{dir}{stem}{suffix}.{}", ext.to_lowercase())
    }

    /// `save( $filename )`, or `_save()` at the generated name.
    pub fn save(&self, path: Option<String>) -> Result<Saved, Error> {
        let path = path.unwrap_or_else(|| self.generate_filename(None));
        let bytes = codec::encode(&self.image, self.mime, self.quality, &self.kept)?;
        let file = self.hooks.image_make_intermediate_size(path.clone());
        let file = file.rsplit('/').next().unwrap_or_default().to_string();
        let (width, height) = self.size();
        Ok(Saved {
            path,
            file,
            width,
            height,
            mime: self.mime,
            bytes,
        })
    }

    /// `make_subsize( $size_data )`: the size saved from the image as it
    /// is, which stays as it was (its quality does not, as in PHP); `None`
    /// where it is the image's own size or no size comes of it.
    pub fn make_subsize(&mut self, size: &SubSize) -> Option<Result<Saved, Error>> {
        if self.size() == (size.width, size.height) {
            return None;
        }
        let image = self.image.clone();
        let saved = self
            .resize(size.width, size.height, &size.crop)
            .then(|| self.save(None));
        self.image = image;
        saved
    }
}

#[cfg(test)]
mod tests;

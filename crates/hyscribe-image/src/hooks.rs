// SPDX-License-Identifier: AGPL-3.0-or-later

//! WordPress's image hook points under their names and with their
//! arguments (ADR 0015), plain calls until the extension runtime: each
//! default returns its value unchanged, as WordPress does with nothing
//! attached.

use hyscribe_phpser::Value;

use crate::subsizes::SubSize;

/// The filters `wp_create_image_subsizes()` and the editor apply.
pub trait Hooks: Sync {
    /// `big_image_size_threshold`: the threshold, the image's width and
    /// height, its file and the attachment; 0 turns scaling off.
    fn big_image_size_threshold(
        &self,
        threshold: i64,
        _size: (u32, u32),
        _file: &str,
        _attachment: u64,
    ) -> i64 {
        threshold
    }

    /// `intermediate_image_sizes_advanced`: the sizes to make, the
    /// metadata so far and the attachment.
    fn intermediate_image_sizes_advanced(
        &self,
        sizes: Vec<SubSize>,
        _meta: &Value,
        _attachment: u64,
    ) -> Vec<SubSize> {
        sizes
    }

    /// `wp_editor_set_quality`: the quality, the output MIME type and the
    /// size it is for.
    fn wp_editor_set_quality(&self, quality: i64, _mime: &str, _size: (u32, u32)) -> i64 {
        quality
    }

    /// `jpeg_quality`: a JPEG's quality and the context, `image_resize`.
    fn jpeg_quality(&self, quality: i64, _context: &str) -> i64 {
        quality
    }

    /// `image_make_intermediate_size`: the path of a size just saved,
    /// whose base name the metadata keeps.
    fn image_make_intermediate_size(&self, file: String) -> String {
        file
    }

    /// `wp_read_audio_metadata`: the metadata read from an audio file and
    /// the file format getID3 names (`None` for none); getID3's raw data
    /// is not given.
    fn wp_read_audio_metadata(&self, meta: Value, _file_format: Option<&str>) -> Value {
        meta
    }

    /// `wp_read_video_metadata`, as [`Hooks::wp_read_audio_metadata`] for
    /// a video file.
    fn wp_read_video_metadata(&self, meta: Value, _file_format: Option<&str>) -> Value {
        meta
    }

    /// `wp_generate_attachment_metadata`: the metadata, the attachment
    /// and the context, `create`.
    fn wp_generate_attachment_metadata(
        &self,
        meta: Value,
        _attachment: u64,
        _context: &str,
    ) -> Value {
        meta
    }
}

/// Nothing attached.
pub struct NoHooks;

impl Hooks for NoHooks {}

#[cfg(test)]
mod tests;

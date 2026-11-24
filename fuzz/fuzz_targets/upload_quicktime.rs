// SPDX-License-Identifier: AGPL-3.0-or-later

//! The QuickTime and MP4 reader takes any atoms: given an `ftyp` atom first,
//! so that every input reaches it, it never panics or reads outside the
//! bytes, and what it reads has the file's size and type and a cover picture
//! of the three types the image reader takes.

#![no_main]

use hyscribe_image::getid3::{wp_read_audio_metadata, wp_read_video_metadata};
use hyscribe_phpser::{Value, lookup};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let file = [b"\0\0\0\x0cftypisom", data].concat();
    let known = [
        b"image/gif".to_vec(),
        b"image/jpeg".to_vec(),
        b"image/png".to_vec(),
    ];
    for meta in [
        wp_read_video_metadata(&file),
        wp_read_audio_metadata(&file, b""),
    ]
    .into_iter()
    .flatten()
    {
        let size = lookup(&meta, "filesize");
        assert_eq!(size, Some(&Value::Int(file.len() as i64)));
        let mime = lookup(&meta, "mime_type").map(Value::to_php_string);
        assert!(
            mime.is_some_and(|mime| mime.starts_with(b"video/") || mime.starts_with(b"audio/"))
        );
        if let Some(Value::Array(image)) = lookup(&meta, "image") {
            let mime = lookup(image, "mime").map(Value::to_php_string);
            assert!(mime.is_some_and(|mime| known.contains(&mime)));
        }
    }
});

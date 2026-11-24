// SPDX-License-Identifier: AGPL-3.0-or-later

//! The audio and video metadata readers take any bytes: they never panic
//! or read outside them, a file they read has the size of the input, and a
//! cover picture is one of the three types the image reader takes.

#![no_main]

use hyscribe_image::getid3::{wp_read_audio_metadata, wp_read_video_metadata};
use hyscribe_phpser::{Value, lookup};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(meta) = wp_read_audio_metadata(data, b"example.test") {
        let size = lookup(&meta, "filesize");
        assert_eq!(size, Some(&Value::Int(data.len() as i64)));
        if let Some(Value::Array(image)) = lookup(&meta, "image") {
            let mime = lookup(image, "mime").map(Value::to_php_string);
            let known = [
                b"image/gif".to_vec(),
                b"image/jpeg".to_vec(),
                b"image/png".to_vec(),
            ];
            assert!(mime.is_some_and(|mime| known.contains(&mime)));
        }
    }
    if let Ok(meta) = wp_read_video_metadata(data) {
        assert!(lookup(&meta, "filesize").is_some());
    }
});

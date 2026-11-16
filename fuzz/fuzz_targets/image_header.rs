// SPDX-License-Identifier: AGPL-3.0-or-later

//! The image header reader takes any bytes: a size it reads has a type the
//! signature agrees with, and an `APP13` it hands on lies within the input.

#![no_main]

use hyscribe_image::header::{getimagesize, image_mime, image_type};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mime = image_mime(data);
    if let Some(kind) = image_type(data) {
        assert_eq!(mime, Some(kind.mime()));
    }
    if let Some(size) = getimagesize(data) {
        assert!(size.width != 0 || size.height != 0);
        assert!(mime.is_some());
        if let Some(app13) = size.app13 {
            assert!(app13.len() <= data.len());
        }
    }
});

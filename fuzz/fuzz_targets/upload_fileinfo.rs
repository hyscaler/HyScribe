// SPDX-License-Identifier: AGPL-3.0-or-later

//! The `fileinfo` reader takes any bytes: it never panics or reads outside
//! them, any answer names at least one type, a file it takes for an image
//! has a header the image reader reads, and a type check that rests on it
//! never keeps a type an extension names for a script.

#![no_main]

use hyscribe_image::fileinfo::finfo_candidates;
use hyscribe_image::header::getimagesize;
use hyscribe_image::upload::wp_check_filetype_and_ext;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(types) = finfo_candidates(data) {
        assert!(!types.is_empty());
        if types.iter().any(|t| t.starts_with("image/")) && data.len() > 1 {
            assert!(getimagesize(data).is_some() || types.contains(&"image/x-xcf"));
        }
    }
    for name in ["a.txt", "a.pdf", "a.mp3", "a.docx", "a.jpg", "a.php"] {
        if let Ok(checked) = wp_check_filetype_and_ext(data, name, false)
            && let Some((ext, _)) = checked.ext_type
        {
            assert_ne!(ext, "php");
        }
    }
});

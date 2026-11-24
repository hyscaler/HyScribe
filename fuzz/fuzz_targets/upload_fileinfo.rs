// SPDX-License-Identifier: AGPL-3.0-or-later

//! The `fileinfo` reader takes any bytes, and a Compound File Binary file
//! altered by them: it never panics or reads outside them, any answer
//! names at least one type, a file it takes for an image has a header the
//! image reader reads, and a type check that rests on it never keeps a
//! type an extension names for a script.

#![no_main]

use hyscribe_image::fileinfo::finfo_candidates;
use hyscribe_image::header::getimagesize;
use hyscribe_image::upload::wp_check_filetype_and_ext;
use libfuzzer_sys::fuzz_target;

/// A Compound File Binary file, whose bytes the input overwrites from a
/// place it names, so the fuzzer reaches the tables and streams behind the
/// header.
const COMPOUND: &[u8] = include_bytes!("../../crates/hyscribe-image/tests/corpus/sample.xls");

fuzz_target!(|data: &[u8]| {
    if let Some((place, patch)) = data.split_first_chunk::<2>() {
        let at = usize::from(u16::from_le_bytes(*place)) % COMPOUND.len();
        let mut file = COMPOUND.to_vec();
        let end = (at + patch.len()).min(file.len());
        if let Some(window) = file.get_mut(at..end) {
            window.copy_from_slice(patch.get(..end - at).unwrap_or_default());
        }
        if let Ok(types) = finfo_candidates(&file) {
            assert!(!types.is_empty());
        }
    }
    if let Ok(types) = finfo_candidates(data) {
        assert!(!types.is_empty());
        if types.iter().any(|t| t.starts_with("image/")) && data.len() > 1 {
            assert!(getimagesize(data).is_some() || types.contains(&"image/x-xcf"));
        }
    }
    for name in ["a.txt", "a.pdf", "a.mp3", "a.docx", "a.doc", "a.xls", "a.ppt", "a.jpg", "a.php"] {
        if let Ok(checked) = wp_check_filetype_and_ext(data, name, false)
            && let Some((ext, _)) = checked.ext_type
        {
            assert_ne!(ext, "php");
        }
    }
});

// SPDX-License-Identifier: AGPL-3.0-or-later

//! Any upload name becomes a file name that stays in its folder: no
//! separator, no NUL, no leading dot, and never `.` or `..`; made unique,
//! it is none of the files already there.

#![no_main]

use hyscribe_image::upload::{sanitize_file_name, wp_unique_filename};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let name = sanitize_file_name(data, false);
    assert!(!name.contains(['/', '\\', '\0']));
    assert!(!name.starts_with('.'));
    let files = vec![name.clone(), format!("{name}-1")];
    let unique = wp_unique_filename(data, &files, false);
    assert!(!unique.contains(['/', '\\', '\0']) && !unique.starts_with('.'));
    assert!(!files.contains(&unique));
});

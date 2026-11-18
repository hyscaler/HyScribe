// SPDX-License-Identifier: AGPL-3.0-or-later

//! An upload's type check never panics, under any name and bytes; a type
//! it allows ends the name it checked, never a script's, and a renamed
//! file keeps its stem.

#![no_main]

use hyscribe_image::upload::{wp_check_filetype, wp_check_filetype_and_ext};
use libfuzzer_sys::fuzz_target;

const SCRIPTS: [&str; 6] = ["php", "phtml", "phar", "pht", "cgi", "pl"];

/// `name` ends in `.ext`, any case.
fn ends(name: &str, ext: &str) -> bool {
    name.len() > ext.len()
        && name
            .get(name.len() - ext.len() - 1..)
            .is_some_and(|tail| tail.eq_ignore_ascii_case(&format!(".{ext}")))
}

fuzz_target!(|data: &[u8]| {
    let split = usize::from(data.first().copied().unwrap_or(0)).min(data.len());
    let (name, bytes) = data.split_at(split);
    let name = String::from_utf8_lossy(name);
    let unfiltered = bytes.first().is_some_and(|b| b & 1 == 1);
    if let Some((ext, _)) = wp_check_filetype(&name, unfiltered) {
        assert!(ends(&name, &ext));
        assert!(!SCRIPTS.contains(&ext.to_ascii_lowercase().as_str()));
    }
    if let Ok(checked) = wp_check_filetype_and_ext(bytes, &name, unfiltered) {
        if let Some((ext, _)) = &checked.ext_type {
            assert!(ends(
                checked.proper_filename.as_deref().unwrap_or(&name),
                ext
            ));
        }
        if let Some(renamed) = checked.proper_filename {
            let stem = name.rsplit_once('.').map_or(&*name, |(stem, _)| stem);
            assert!(renamed.starts_with(stem));
        }
    }
});

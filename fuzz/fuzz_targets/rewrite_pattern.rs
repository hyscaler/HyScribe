// SPDX-License-Identifier: AGPL-3.0-or-later

//! The rewrite rules' regex engine parses any pattern and matches any path
//! without panicking, and a match's captures lie inside the path.

#![no_main]

use hyscribe_http::fuzzing::Pattern;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&split, rest)) = data.split_first() else {
        return;
    };
    let (regex, path) = rest.split_at(usize::from(split).min(rest.len()));
    let Some(pattern) = std::str::from_utf8(regex).ok().and_then(Pattern::new) else {
        return;
    };
    if let Some(captures) = pattern.captures(path) {
        assert_eq!(captures.first().copied().flatten().map(|(start, _)| start), Some(0));
        for (start, end) in captures.into_iter().flatten() {
            assert!(start <= end && end <= path.len());
        }
    }
});

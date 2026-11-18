// SPDX-License-Identifier: AGPL-3.0-or-later

//! The upload routes' readers never panic: any `Content-Type` gives a
//! boundary or none, any body a form or none, whose files are slices of
//! the body named without a path, and any `Content-Disposition` a file
//! name or none.

#![no_main]

use hyscribe_http::fuzzing::{disposition_filename, multipart_boundary, multipart_form};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let split = usize::from(data.first().copied().unwrap_or(0)).min(data.len());
    let (kind, body) = data.split_at(split);
    let _ = disposition_filename(data);
    let boundary = multipart_boundary(kind).unwrap_or(b"fuzz");
    if let Some(form) = multipart_form(body, boundary) {
        assert!(form.files.len() <= 20);
        for file in &form.files {
            assert!(!file.name.contains(&b'/') && !file.name.contains(&b'\\'));
            assert!(file.bytes.len() <= body.len());
        }
        let _ = form.post();
    }
});

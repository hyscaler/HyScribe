// SPDX-License-Identifier: AGPL-3.0-or-later

//! A fetched page of any bytes, from any URL, gives the link preview's
//! title, icon, description and image without panicking, and in time
//! linear in its size: the page is what another site sends.

#![no_main]

use hyscribe_rest::fuzzing::details;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let split = data.iter().position(|&b| b == 0).unwrap_or(0);
    let (url, page) = data.split_at(split);
    let [title, icon, description, image] = details(page, url);
    // Nothing read is longer than the page and URL it came from, but for
    // the scheme and host the icon and image are made absolute on.
    for read in [&title, &description] {
        assert!(read.len() <= page.len() * 4);
    }
    for read in [&icon, &image] {
        assert!(read.len() <= page.len() + 2 * url.len() + 8);
    }
});

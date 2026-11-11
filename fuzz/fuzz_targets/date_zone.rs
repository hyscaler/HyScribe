// SPDX-License-Identifier: AGPL-3.0-or-later

//! TZif data and its POSIX TZ footer parse, and resolve times, without
//! panicking.

#![no_main]

use hyscribe_render::date::zone::Tz;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Some(tz) = Tz::parse(data) {
        // Years 1 to 9999, the range stored dates hold.
        for time in [-62_135_596_800, -1, 0, 1_782_900_000, 253_402_300_799] {
            let _ = tz.period(time);
            let _ = tz.resolve(time);
        }
    }
});

// SPDX-License-Identifier: AGPL-3.0-or-later

//! A feed or oEmbed answer of any bytes, from another site, reads to its
//! end or an error without panicking, in steps linear in its size: no
//! entity expands to more than one character.

#![no_main]

use hyscribe_render::xml::{Event, Parser};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    for html in [false, true] {
        let mut parser = Parser::new(data, html);
        let mut steps = 0;
        let mut text = 0;
        while let Ok(event) = parser.read() {
            steps += 1;
            assert!(steps <= data.len() + 1);
            match event {
                Event::Eof => break,
                Event::Text(chunk) => text += chunk.len(),
                _ => {}
            }
            assert!(text <= data.len() * 4);
        }
    }
});

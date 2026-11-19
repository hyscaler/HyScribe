// SPDX-License-Identifier: AGPL-3.0-or-later

//! The answers of the new site's install never panic, and a site address that
//! passes holds nothing that needs escaping where it reaches an option or a
//! post: only `http` or `https`, a host, a port and a path of URL characters.

#![no_main]

use hyscribe_write::install::{Answers, Plan, site_address};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if let Some(address) = site_address(text) {
        assert!(address.starts_with("http://") || address.starts_with("https://"));
        assert!(!address.ends_with('/'));
        let safe = |b: u8| b.is_ascii_alphanumeric() || b":/.-_~[]".contains(&b);
        assert!(address.bytes().all(safe));
        assert!(!address.contains("//") || address.matches("//").count() == 1);
    }
    // The six answers, split at NUL bytes.
    let mut fields = text.split('\0');
    let mut next = || fields.next().unwrap_or_default();
    let (title, login, password, repeat, email, home) =
        (next(), next(), next(), next(), next(), next());
    let answers = Answers {
        title,
        login,
        password,
        repeat,
        email,
        public: data.len() % 2 == 0,
        home,
    };
    let _ = Plan::new(&answers, 1_700_000_000);
});

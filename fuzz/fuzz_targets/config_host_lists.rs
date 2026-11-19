// SPDX-License-Identifier: AGPL-3.0-or-later

//! The host allowlist keys (`EMBED_ALLOWED_HOSTS`, `RSS_ALLOWED_HOSTS`)
//! never panic, and what they accept is plain lower case host names.

#![no_main]

use std::collections::HashMap;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    for key in ["EMBED_ALLOWED_HOSTS", "RSS_ALLOWED_HOSTS"] {
        let values: HashMap<String, String> = [
            ("DB_NAME", "wordpress"),
            ("DB_USER", "wordpress"),
            (key, text),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        if let Ok(config) = hyscribe_config::from_map(&values) {
            let hosts = config
                .embed_allowed
                .hosts
                .iter()
                .chain(&config.rss_allowed_hosts);
            for host in hosts {
                assert!(host.bytes().all(|b| b.is_ascii_lowercase()
                    || b.is_ascii_digit()
                    || matches!(b, b'-' | b'.' | b'_')));
            }
        }
    }
});

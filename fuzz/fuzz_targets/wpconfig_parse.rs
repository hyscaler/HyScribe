// SPDX-License-Identifier: AGPL-3.0-or-later

//! The wp-config.php reader never panics, DB_HOST splitting is total, and
//! the setup wizard's import and `.env` rendering of what it read never
//! panic either.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(config) = hyscribe_config::wpconfig::parse(data) {
        for constant in &config.constants {
            if let Some(host) = constant.value.as_str() {
                let _ = hyscribe_config::wpconfig::parse_db_host(host);
            }
        }
        let import = hyscribe_config::setup::import(&config);
        let _ = hyscribe_config::setup::render(&import.values);
    }
});

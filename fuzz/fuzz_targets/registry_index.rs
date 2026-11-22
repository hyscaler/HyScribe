// SPDX-License-Identifier: AGPL-3.0-or-later

//! Reading the registry's documents (ADR 0024), the key list, the index
//! and the kept state, never panics, and what they read keeps their bounds.

#![no_main]

use hyscribe_ext::origin::{Origin, hex64};
use hyscribe_registry::index::Index;
use hyscribe_registry::kept::State;
use hyscribe_registry::keys::KeyList;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    if let Some(list) = KeyList::parse(bytes) {
        assert!(list.keys.iter().all(|key| key.not_before <= key.not_after));
    }
    if let Some(index) = Index::parse(bytes) {
        for revoked in &index.revoked {
            assert!(hex64(&revoked.sha256));
        }
    }
    if let Some(state) = State::parse(bytes) {
        assert_eq!(State::parse(state.text().as_bytes()), Some(state));
    }
    if let Some(origin) = Origin::parse(bytes) {
        assert_eq!(Origin::parse(origin.text().as_bytes()), Some(origin));
    }
});

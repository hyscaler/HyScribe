// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_ext::signature::PublicKey;
use hyscribe_testkit::minisign::{Key, key_list, sign_keys};

use crate::keys::KeyList;

/// The test clock.
pub(crate) const NOW: u64 = 1_800_000_000;

/// Three test roots, keys 1 to 3.
pub(crate) fn test_roots() -> Vec<PublicKey> {
    (1..=3)
        .map(|seed| PublicKey::parse(&Key(seed).public()).unwrap())
        .collect()
}

/// `text` with the first member `name` renamed, so it is missing and unknown.
pub(crate) fn without(text: &str, name: &str) -> String {
    text.replacen(&format!("\"{name}\":"), "\"gone\":", 1)
}

#[test]
fn the_placeholder_roots_verify_nothing() {
    let list = key_list(1, NOW + 1, &[], &[]);
    let signatures = sign_keys(&list, 1, NOW + 1, &[&Key(1), &Key(2), &Key(3)]);
    let signatures: Vec<Vec<u8>> = signatures.into_iter().map(String::into_bytes).collect();
    assert!(KeyList::verify(list.as_bytes(), &signatures, &test_roots(), NOW).is_ok());
    let roots: Vec<PublicKey> = super::ROOTS
        .into_iter()
        .filter_map(PublicKey::parse)
        .collect();
    assert!(KeyList::verify(list.as_bytes(), &signatures, &roots, NOW).is_err());
}

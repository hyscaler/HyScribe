// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::minisign::{Key, key_list};

use super::*;
use crate::tests::NOW;

const EXPIRES: u64 = NOW + 7 * 86_400;
const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn keys() -> KeyList {
    let keys = [
        (&Key(10), r#""index""#, NOW - 10, NOW + 10),
        (&Key(11), r#""packages""#, NOW - 10, NOW + 10),
    ];
    KeyList::parse(key_list(1, NOW + 100, &keys, &[]).as_bytes()).unwrap()
}

fn index(serial: u64) -> String {
    format!(
        r#"{{"serial":{serial},"expires":{EXPIRES},"extensions":[{{"id":"contact-form-7","kind":"plugin","latest":{{"0.1":"0.1.0","0.2":"0.2.1"}},"sha256":"{HASH}","size":2048}},{{"id":"a-theme","kind":"theme","latest":{{}},"sha256":"{HASH}","size":1}}],"revoked":[{{"id":"contact-form-7","version":"0.1.0","sha256":"{HASH}","reason":"CVE-2026-0001: fixed in 0.2.1"}}]}}"#
    )
}

fn sign(text: &str, key: u8, serial: u64) -> Vec<u8> {
    let comment = format!("hyscribe-index serial={serial} expires={EXPIRES}");
    Key(key).sign(text.as_bytes(), &comment).into_bytes()
}

#[test]
fn verifies_an_index_signed_by_an_index_key() {
    let text = index(9);
    let index = Index::verify(text.as_bytes(), &sign(&text, 10, 9), &keys(), NOW).unwrap();
    assert_eq!((index.serial, index.expires), (9, EXPIRES));
    let reason = "CVE-2026-0001: fixed in 0.2.1";
    assert_eq!(index.revoked("contact-form-7", "0.1.0", HASH), Some(reason));
    assert_eq!(index.revoked("contact-form-7", "0.2.1", HASH), None);
    assert_eq!(index.revoked("contact-form-8", "0.1.0", HASH), None);
    assert_eq!(
        index.revoked("contact-form-7", "0.1.0", &HASH.replace('0', "1")),
        None
    );
}

#[test]
fn refuses_a_signature_it_cannot_trust() {
    let text = index(9);
    let verify = |signature: &[u8], now| Index::verify(text.as_bytes(), signature, &keys(), now);
    let packages = format!("key {} may not sign index", Key(11).id());
    assert_eq!(verify(&sign(&text, 11, 9), NOW).unwrap_err(), packages);
    let unknown = format!("key {} is not in the key list", Key(12).id());
    assert_eq!(verify(&sign(&text, 12, 9), NOW).unwrap_err(), unknown);
    let not = "index.json's signature does not verify";
    assert_eq!(verify(&sign(&text, 10, 8), NOW).unwrap_err(), not);
    assert_eq!(verify(&sign(&index(8), 10, 9), NOW).unwrap_err(), not);
    let late = format!("key {} could not sign at {}", Key(10).id(), NOW + 11);
    assert_eq!(verify(&sign(&text, 10, 9), NOW + 11).unwrap_err(), late);
    let unread = "index.json: a signature file has four lines";
    assert_eq!(verify(b"", NOW).unwrap_err(), unread);
    let unreadable = Index::verify(b"[]", &sign(&text, 10, 9), &keys(), NOW);
    assert_eq!(unreadable.unwrap_err(), "index.json does not read");
}

#[test]
fn refuses_an_expired_or_oversized_index() {
    let text = index(9);
    let mut keys = keys();
    keys.keys[0].not_after = EXPIRES;
    let verify = |text: &str, now| Index::verify(text.as_bytes(), &sign(text, 10, 9), &keys, now);
    assert!(verify(&text, EXPIRES - 1).is_ok());
    let expired = format!("index.json expired at {EXPIRES}");
    assert_eq!(verify(&text, EXPIRES).unwrap_err(), expired);
    let full = format!("{text}{}", " ".repeat(MAX_BYTES as usize - text.len()));
    assert!(verify(&full, NOW).is_ok());
    let over = format!("{full} ");
    assert_eq!(
        verify(&over, NOW).unwrap_err(),
        "index.json is larger than 4194304 bytes"
    );
}

#[test]
fn refuses_members_out_of_their_bounds() {
    let good = index(9);
    assert!(Index::parse(good.as_bytes()).is_some());
    let refused = |text: &str| assert_eq!(Index::parse(text.as_bytes()), None, "{text}");
    refused(&good.replace("\"serial\":9", "\"serial\":9.5"));
    refused(&good.replace("\"serial\":9", "\"serial\":9,\"x\":1"));
    refused(&good.replace("\"a-theme\"", "\"contact-form-7\""));
    refused(&good.replace("\"a-theme\"", "\"A-theme\""));
    refused(&good.replace("\"theme\"", "\"block\""));
    refused(&good.replace("\"theme\"", "1"));
    refused(&good.replace("\"0.2\":", "\"0 2\":"));
    refused(&good.replace("\"0.2\":", "\"2\":"));
    refused(&good.replace("\"0.2.1\"}", "\"0.2 1\"}"));
    refused(&good.replace("\"0.2.1\"}", "1}"));
    refused(&good.replace("\"size\":1}", "\"size\":0}"));
    refused(&good.replace("\"size\":2048", &format!("\"size\":{}", MAX_DOCUMENT + 1)));
    refused(&good.replace("\"size\":1}", "\"size\":1,\"x\":1}"));
    refused(&good.replacen(HASH, &HASH.to_uppercase(), 1));
    refused(&good.replace("\"version\":\"0.1.0\"", "\"version\":\"0.1 0\""));
    refused(&good.replace("\"reason\":\"CVE", "\"reason\":\"\\nCVE"));
    refused(&good.replace(",\"reason\":\"CVE-2026-0001: fixed in 0.2.1\"", ""));
    refused(&good.replace(
        "\"id\":\"contact-form-7\",\"version",
        "\"id\":\"Contact\",\"version",
    ));
    refused(&good.replace(
        &format!("\"sha256\":\"{HASH}\",\"reason"),
        "\"sha256\":\"x\",\"reason",
    ));
    refused(&good.replace("0.2.1\"}]", "0.2.1\",\"x\":1}]"));
    for member in ["expires", "extensions", "revoked", "latest"] {
        refused(&crate::tests::without(&good, member));
    }
    refused(&good.replace("\"extensions\":[{", "\"extensions\":[1,{"));
    refused(&good.replace("\"revoked\":[{", "\"revoked\":[1,{"));
    refused("1");
    let size = good.replace("\"size\":2048", &format!("\"size\":{MAX_DOCUMENT}"));
    assert!(Index::parse(size.as_bytes()).is_some());
}

// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::minisign::{Key, key_list, sign_keys};

use super::*;
use crate::tests::{NOW, test_roots, without};

const EXPIRES: u64 = NOW + 1_000;

/// A key list with key 10 for both uses and key 11 for packages.
fn list(serial: u64, revoked: &[&str]) -> String {
    let keys = [
        (&Key(10), r#""index","packages""#, NOW - 10, NOW + 10),
        (&Key(11), r#""packages""#, NOW - YEAR, NOW),
    ];
    key_list(serial, EXPIRES, &keys, revoked)
}

fn signed(text: &str, roots: &[u8]) -> Vec<Vec<u8>> {
    let roots: Vec<Key> = roots.iter().map(|&seed| Key(seed)).collect();
    let roots: Vec<&Key> = roots.iter().collect();
    let signed = sign_keys(text, 4, EXPIRES, &roots);
    signed.into_iter().map(String::into_bytes).collect()
}

fn verify(text: &str, signatures: &[Vec<u8>], now: u64) -> Result<KeyList, String> {
    KeyList::verify(text.as_bytes(), signatures, &test_roots(), now)
}

#[test]
fn a_year_is_a_leap_years_seconds() {
    assert_eq!(YEAR, 31_622_400);
}

#[test]
fn verifies_two_of_three_roots() {
    let text = list(4, &[]);
    let list = verify(&text, &signed(&text, &[1, 3]), NOW).unwrap();
    assert_eq!((list.serial, list.expires), (4, EXPIRES));
    assert_eq!(list.keys.len(), 2);
    assert_eq!(list.keys[0].uses, [Use::Index, Use::Packages]);
    assert_eq!(list.keys[1].uses, [Use::Packages]);
    assert_eq!(list.keys[1].not_before, NOW - YEAR);
    assert_eq!(list.keys[1].key.id(), Key(11).id());
    assert!(verify(&text, &signed(&text, &[3, 2, 1]), NOW).is_ok());
    let two = "keys.json is not signed by two of the root keys";
    assert_eq!(verify(&text, &signed(&text, &[2]), NOW).unwrap_err(), two);
    assert_eq!(
        verify(&text, &signed(&text, &[2, 2]), NOW).unwrap_err(),
        two
    );
    assert_eq!(
        verify(&text, &signed(&text, &[2, 10]), NOW).unwrap_err(),
        two
    );
    let mut bad = signed(&text, &[1, 2]);
    bad[1] = b"not a signature".to_vec();
    assert_eq!(verify(&text, &bad, NOW).unwrap_err(), two);
    let other = sign_keys(&text, 5, EXPIRES, &[&Key(2)])
        .remove(0)
        .into_bytes();
    let moved = [signed(&text, &[1]), vec![other]].concat();
    assert_eq!(
        verify(&text, &moved, NOW).unwrap_err(),
        two,
        "another serial's comment"
    );
    let changed = text.replace("\"serial\":4", "\"serial\": 4");
    assert_eq!(
        verify(&changed, &signed(&text, &[1, 2]), NOW).unwrap_err(),
        two
    );
    let none = KeyList::verify(text.as_bytes(), &signed(&text, &[1, 2]), &[], NOW);
    assert_eq!(none.unwrap_err(), two, "the placeholder roots");
}

#[test]
fn refuses_an_expired_or_unreadable_list() {
    let text = list(4, &[]);
    let signatures = signed(&text, &[1, 2]);
    assert!(verify(&text, &signatures, EXPIRES - 1).is_ok());
    let expired = format!("keys.json expired at {EXPIRES}");
    assert_eq!(verify(&text, &signatures, EXPIRES).unwrap_err(), expired);
    assert_eq!(
        verify("{}", &signatures, NOW).unwrap_err(),
        "keys.json does not read"
    );
}

#[test]
fn refuses_a_list_expiring_more_than_13_months_ahead() {
    let far = |expires: u64| {
        let text = key_list(4, expires, &[], &[]);
        let roots: Vec<Key> = [1, 2].map(Key).into();
        let signatures = sign_keys(&text, 4, expires, &[&roots[0], &roots[1]]);
        let signatures: Vec<Vec<u8>> = signatures.into_iter().map(String::into_bytes).collect();
        verify(&text, &signatures, NOW)
    };
    assert!(far(NOW + 34_819_200).is_ok());
    let refused = far(NOW + 34_819_201).unwrap_err();
    assert_eq!(
        refused,
        format!(
            "keys.json expires more than 13 months ahead, at {}",
            NOW + 34_819_201
        )
    );
}

#[test]
fn reads_at_most_64_kibibytes() {
    let text = list(4, &[]);
    let full = format!("{text}{}", " ".repeat(MAX_BYTES as usize - text.len()));
    assert!(verify(&full, &signed(&full, &[1, 2]), NOW).is_ok());
    let over = format!("{full} ");
    let refused = verify(&over, &signed(&over, &[1, 2]), NOW).unwrap_err();
    assert_eq!(refused, "keys.json is larger than 65536 bytes");
}

#[test]
fn refuses_members_out_of_their_bounds() {
    let good = list(4, &[&Key(12).id()]);
    assert_eq!(
        KeyList::parse(good.as_bytes()).unwrap().revoked,
        [Key(12).id()]
    );
    let refused = |text: &str| assert_eq!(KeyList::parse(text.as_bytes()), None, "{text}");
    refused(&good.replace("\"serial\":4", "\"serial\":-4"));
    refused(&good.replace("\"serial\":4", "\"serial\":\"4\""));
    refused(&good.replace("\"serial\":4,", ""));
    refused(&good.replace("\"serial\":4", "\"serial\":4,\"more\":1"));
    refused(&good.replace(&Key(12).id(), "0c0c0c0c0c0c0c0c"));
    refused(&good.replace(&format!("\"{}\"", Key(12).id()), "12"));
    refused(&good.replace("\"packages\"]", "\"releases\"]"));
    refused(&good.replace("\"packages\"]", "1]"));
    refused(
        &good
            .replace("\"keys\":[", "\"keys\":{\"a\":")
            .replace("}],\"revoked", "}},\"revoked"),
    );
    refused(&good.replace(
        &format!("\"id\":\"{}\"", Key(10).id()),
        &format!("\"id\":\"{}\"", Key(11).id()),
    ));
    refused(&good.replace(&Key(10).public(), "RWQ"));
    refused(&good.replace(
        "\"not_after\":1800000010",
        "\"not_after\":1800000010,\"x\":0",
    ));
    refused(&good.replace("\"not_after\":1800000010", "\"not_after\":1799999989"));
    refused(&good.replace(
        &format!("\"not_before\":{}", NOW - YEAR),
        &format!("\"not_before\":{}", NOW - YEAR - 1),
    ));
    refused(&good.replace(
        &format!("{}\",\"public_key\":\"{}", Key(11).id(), Key(11).public()),
        &format!("{}\",\"public_key\":\"{}", Key(10).id(), Key(10).public()),
    ));
    for member in [
        "expires",
        "keys",
        "revoked",
        "id",
        "public_key",
        "may_sign",
        "not_before",
        "not_after",
    ] {
        refused(&without(&good, member));
    }
    refused(&good.replace("\"keys\":[{", "\"keys\":[1,{"));
    refused("[1]");
    refused("1");
    refused("{");
}

#[test]
fn finds_a_key_allowed_to_sign() {
    let text = list(4, &[&Key(11).id()]);
    let keys = KeyList::parse(text.as_bytes()).unwrap();
    let ten = keys.key(&Key(10).id(), Use::Index, NOW - 10).unwrap();
    assert_eq!(ten.id(), Key(10).id());
    assert!(keys.key(&Key(10).id(), Use::Packages, NOW + 10).is_ok());
    let id = Key(10).id();
    let outside = format!("key {id} could not sign at {}", NOW + 11);
    assert_eq!(keys.key(&id, Use::Index, NOW + 11).unwrap_err(), outside);
    let before = format!("key {id} could not sign at {}", NOW - 11);
    assert_eq!(keys.key(&id, Use::Index, NOW - 11).unwrap_err(), before);
    let eleven = Key(11).id();
    assert_eq!(
        keys.key(&eleven, Use::Packages, NOW).unwrap_err(),
        format!("key {eleven} is revoked")
    );
    let unlisted = KeyList::parse(list(4, &[]).as_bytes()).unwrap();
    assert_eq!(
        unlisted.key(&eleven, Use::Index, NOW).unwrap_err(),
        format!("key {eleven} may not sign index")
    );
    let twelve = Key(12).id();
    assert_eq!(
        keys.key(&twelve, Use::Index, NOW).unwrap_err(),
        format!("key {twelve} is not in the key list")
    );
}

// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::minisign::Key;

use super::*;

/// minisign's own prehashed signature of `test` (minisign-verify's tests).
const KEY: &str = "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
const SIGNED: &str = "RUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=";
const GLOBAL: &str =
    "y/rUw2y8/hOUYjZU71eHp/Wo1KZ40fGy2VJEDl34XMJM+TX48Ss/17u3IvIfbVR1FkZZSNCisQbuQY+bHwhEBg==";
const COMMENT: &str = "timestamp:1556193335\tfile:test";

fn file(untrusted: &str, first: &str, trusted: &str, global: &str) -> String {
    format!("untrusted comment: {untrusted}\n{first}\ntrusted comment: {trusted}\n{global}\n")
}

/// The vector's signature as read, its trusted comment holding a tab,
/// which minisign's default comment has and the reader refuses.
fn minisign_vector() -> Signature {
    let first: [u8; 74] = decoded(SIGNED).unwrap();
    Signature {
        key_id: first[2..10].try_into().unwrap(),
        signature: first[10..].try_into().unwrap(),
        trusted: COMMENT.into(),
        global: decoded(GLOBAL).unwrap(),
    }
}

#[test]
fn verifies_minisigns_own_prehashed_signature() {
    let key = PublicKey::parse(KEY).unwrap();
    assert_eq!(key.id(), "E7620F1842B4E81F");
    let signature = minisign_vector();
    assert_eq!(signature.key_id(), "E7620F1842B4E81F");
    assert!(signature.verify(&key, &prehash(b"test")));
    assert!(!signature.verify(&key, &prehash(b"tesT")));
    let mut moved = signature.clone();
    moved.trusted.push('x');
    assert!(
        !moved.verify(&key, &prehash(b"test")),
        "the global signature"
    );
    let mut other = signature.clone();
    other.key_id[0] ^= 1;
    assert!(!other.verify(&key, &prehash(b"test")), "another key id");
    let refused = file("c", SIGNED, COMMENT, GLOBAL);
    assert_eq!(
        Signature::parse(refused.as_bytes()),
        Err("the third line is not a trusted comment")
    );
}

#[test]
fn reads_and_verifies_a_signature_file() {
    let (key, other) = (Key(7), Key(8));
    let text = key.sign(b"package", "hyscribe-package id=a");
    let signature = Signature::parse(text.as_bytes()).unwrap();
    assert_eq!(signature.key_id(), key.id());
    assert_eq!(signature.key_id(), "0707070707070707");
    assert_eq!(signature.trusted_comment(), "hyscribe-package id=a");
    let public = PublicKey::parse(&key.public()).unwrap();
    assert_eq!(public.id(), key.id());
    assert!(signature.verify(&public, &prehash(b"package")));
    assert!(!signature.verify(&public, &prehash(b"packagf")));
    let stranger = PublicKey::parse(&other.public()).unwrap();
    assert!(!signature.verify(&stranger, &prehash(b"package")));
    let unterminated = text.strip_suffix('\n').unwrap();
    assert_eq!(Signature::parse(unterminated.as_bytes()), Ok(signature));
}

#[test]
fn reads_at_most_four_kibibytes() {
    let text = Key(7).sign(b"p", "t");
    let pad = "x".repeat(4096 - text.len());
    let full = text.replacen("comment: ", &format!("comment: {pad}"), 1);
    assert_eq!(full.len(), 4096);
    assert!(Signature::parse(full.as_bytes()).is_ok());
    let over = full.replacen("comment: ", "comment: x", 1);
    assert_eq!(
        Signature::parse(over.as_bytes()),
        Err("a signature file is at most 4 KiB")
    );
}

#[test]
fn refuses_what_is_not_four_strict_lines() {
    let text = Key(7).sign(b"p", "t");
    let lines: Vec<&str> = text.lines().collect();
    let (first, global) = (lines[1], lines[3]);
    let refused = |text: &str| Signature::parse(text.as_bytes()).unwrap_err();
    assert_eq!(
        Signature::parse(b"\xff"),
        Err("a signature file is ASCII text")
    );
    let four = "a signature file has four lines";
    assert_eq!(refused(&lines[..3].join("\n")), four);
    assert_eq!(refused(&format!("{text}\n")), four);
    assert_eq!(refused(&format!("{text}x\n")), four);
    let untrusted = "the first line is not an untrusted comment";
    assert_eq!(refused(&text.replace('\n', "\r\n")), untrusted);
    assert_eq!(
        refused(&text.replacen("untrusted", "Untrusted", 1)),
        untrusted
    );
    assert_eq!(refused(&file("a\tb", first, "t", global)), untrusted);
    assert_eq!(refused(&file("a\u{7f}", first, "t", global)), untrusted);
    assert!(Signature::parse(file("", first, "t", global).as_bytes()).is_ok());
    let trusted = "the third line is not a trusted comment";
    assert_eq!(
        refused(&text.replacen("\ntrusted", "\nTrusted", 1)),
        trusted
    );
    assert_eq!(refused(&file("a", first, "t\r", global)), trusted);
    assert_eq!(refused(&file("a", first, "t\u{7f}", global)), trusted);
    let line = "the signature line does not read";
    assert_eq!(
        refused(&file("a", first.get(1..).unwrap(), "t", global)),
        line
    );
    assert_eq!(
        refused(&file("a", &format!("{first}AAAA"), "t", global)),
        line
    );
    let lax = first.replace('=', "");
    assert_eq!(refused(&file("a", &lax, "t", global)), line);
    let globals = "the global signature line does not read";
    assert_eq!(
        refused(&file("a", first, "t", global.get(4..).unwrap())),
        globals
    );
    assert_eq!(refused(&file("a", first, "t", first)), globals);
}

#[test]
fn refuses_the_legacy_form_and_other_algorithms() {
    let text = Key(7).sign(b"p", "t");
    let lines: Vec<&str> = text.lines().collect();
    let with = |algorithm: &[u8; 2]| {
        let mut first = STANDARD.decode(lines[1]).unwrap();
        first[..2].copy_from_slice(algorithm);
        let first = STANDARD.encode(first);
        Signature::parse(file("a", &first, "t", lines[3]).as_bytes())
    };
    assert_eq!(
        with(b"Ed"),
        Err("a signature of the whole file (minisign's legacy form) is refused")
    );
    assert_eq!(with(b"EE"), Err("not a minisign Ed25519 signature"));
    assert!(with(b"ED").is_ok());
}

#[test]
fn refuses_public_keys_of_another_form() {
    let mut bytes = STANDARD.decode(KEY).unwrap();
    assert_eq!(PublicKey::parse(&STANDARD.encode(&bytes[..41])), None);
    assert_eq!(PublicKey::parse(KEY.get(1..).unwrap()), None);
    assert_eq!(PublicKey::parse(&format!(" {KEY}")), None);
    bytes[1] = b'D';
    assert_eq!(PublicKey::parse(&STANDARD.encode(&bytes)), None);
    bytes[1] = b'd';
    assert_eq!(
        PublicKey::parse(&STANDARD.encode(&bytes)),
        PublicKey::parse(KEY)
    );
}


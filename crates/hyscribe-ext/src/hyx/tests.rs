// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::hyx::zip;

use super::*;

const MANIFEST: &[u8] = include_bytes!("../../guest/hyscribe.ext.toml");

fn package(extra: &[(&str, &[u8], bool)]) -> Vec<u8> {
    let mut entries = vec![
        (manifest::FILE, MANIFEST, true),
        (COMPONENT, b"\0asm".as_slice(), false),
    ];
    entries.extend_from_slice(extra);
    zip(&entries)
}

fn refused(bytes: &[u8]) -> String {
    Hyx::read(bytes).expect_err("refused")
}

/// Where the central directory starts.
fn central(bytes: &[u8]) -> usize {
    let at = bytes.len() - 6;
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
}

fn patched(mut bytes: Vec<u8>, at: usize, with: &[u8]) -> Vec<u8> {
    bytes[at..at + with.len()].copy_from_slice(with);
    bytes
}

/// A one file package whose first central entry has `with` at `field`.
fn central_field(field: usize, with: &[u8]) -> String {
    let bytes = zip(&[("LICENSE", b"text", false)]);
    let at = central(&bytes) + field;
    refused(&patched(bytes, at, with))
}

#[test]
fn reads_a_package() {
    let css = b"body { color: red; } ".repeat(40);
    let bytes = package(&[
        ("public/", b"", false),
        ("public/css/a.css", &css, true),
        ("LICENSE", b"Apache-2.0 OR MIT", false),
        ("languages/", b"", false),
    ]);
    let hyx = Hyx::read(&bytes).unwrap();
    assert_eq!(hyx.manifest.id, "hyscribe-test");
    let names: Vec<&str> = hyx.files.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [manifest::FILE, COMPONENT, "public/css/a.css", "LICENSE"]
    );
    assert_eq!(hyx.files[2].1, css);
    let theme =
        b"[extension]\nkind = 'theme'\nid = 't'\nversion = '1'\nsdk = '0.3'\nlicense = 'MIT'\n";
    let hyx = Hyx::read(&zip(&[(manifest::FILE, theme, false)])).unwrap();
    assert_eq!(hyx.files.len(), 1);
}

#[test]
fn refuses_what_is_not_a_package() {
    assert!(refused(&vec![0; (MAX_TOTAL + 1) as usize]).contains("at most"));
    assert!(refused(&vec![0; MAX_TOTAL as usize]).contains("not a zip archive"));
    assert!(refused(b"PK").contains("not a zip archive"));
    let good = package(&[]);
    let end = good.len() - 22;
    let cases: [(usize, &[u8], &str); 6] = [
        (end, b"PK\x05\x07", "or one with a comment"),
        (end + 20, b"\x01\x00", "or one with a comment"),
        (end + 4, b"\x01\x00", "several disks"),
        (end + 6, b"\x01\x00", "several disks"),
        (end + 8, b"\x03\x00", "several disks"),
        (
            end + 16,
            b"\x00\x00\x00\x00",
            "is not where the archive says",
        ),
    ];
    for (at, with, why) in cases {
        let error = refused(&patched(good.clone(), at, with));
        assert!(error.contains(why), "{at}: {error}");
    }
    let many = patched(good.clone(), end + 8, &10_001u16.to_le_bytes());
    let many = patched(many, end + 10, &10_001u16.to_le_bytes());
    assert!(refused(&many).contains("at most 10000 entries"));
    let most = patched(good.clone(), end + 8, &10_000u16.to_le_bytes());
    let most = patched(most, end + 10, &10_000u16.to_le_bytes());
    assert!(!refused(&most).contains("entries"));
    let fewer = patched(good.clone(), end + 8, &[1, 0]);
    let fewer = patched(fewer, end + 10, &[1, 0]);
    assert!(refused(&fewer).contains("holds more than its entries"));
    let over = patched(good, end + 8, &[3, 0]);
    let over = patched(over, end + 10, &[3, 0]);
    assert!(refused(&over).contains("central directory entry is broken"));
}

#[test]
fn refuses_names_a_package_may_not_hold() {
    for name in [
        "../x",
        "/LICENSE",
        "public/../LICENSE",
        "public//a",
        "public/./a",
        "public\\a",
        "c:/LICENSE",
        "other.txt",
        "public",
        "LICENSE/a",
        "faults.txt",
        "public/a\u{7}",
        "/",
    ] {
        let error = refused(&package(&[(name, b"x", false)]));
        assert!(
            error.contains("is not a name a package may hold"),
            "{name}: {error}"
        );
    }
    let long = format!("public/{}", "a".repeat(1_020));
    assert!(refused(&package(&[(&long, b"x", false)])).contains("may hold"));
    let error = refused(&package(&[
        ("public/A", b"a", false),
        ("public/a", b"b", false),
    ]));
    assert!(error.contains("public/a is named twice"));
    let error = refused(&package(&[
        ("public/a", b"a", false),
        ("public/a/b", b"b", false),
    ]));
    assert!(error.contains("public/a is a file and a folder"));
    let error = refused(&zip(&[(COMPONENT, b"\0asm", false)]));
    assert!(error.contains("has a hyscribe.ext.toml"));
    let error = refused(&zip(&[(manifest::FILE, MANIFEST, false)]));
    assert!(error.contains("a plugin package has an extension.wasm"));
    let error = refused(&zip(&[(manifest::FILE, b"[x]\n", false)]));
    assert!(error.contains("no table [x]"));
    let mut bytes = zip(&[("LICENSE", b"text", false)]);
    let at = central(&bytes) + 46;
    bytes[at] = 0xff;
    assert!(refused(&bytes).contains("not UTF-8"));
}

#[test]
fn refuses_entries_it_cannot_trust() {
    let cases: [(usize, &[u8], &str); 9] = [
        (0, b"PK\x01\x03", "central directory entry is broken"),
        (28, b"\xff\x00", "runs past the directory"),
        (8, b"\x01\x08", "is encrypted"),
        (8, b"\x40\x08", "is encrypted"),
        (10, b"\x0c\x00", "neither stored nor deflated"),
        (40, b"\xff\xa1", "a link or a special file"),
        (34, b"\x01\x00", "on another disk"),
        (24, b"\x05\x00\x00\x00", "does not have the size it says"),
        (24, b"\x01\x00\x00\x04", "is larger than"),
    ];
    for (field, with, why) in cases {
        let error = central_field(field, with);
        assert!(error.contains(why), "{field}: {error}");
    }
    // Not made on Unix, the mode is not read.
    let bytes = zip(&[("LICENSE", b"text", false)]);
    let at = central(&bytes);
    let other = patched(patched(bytes, at + 5, &[0]), at + 40, b"\xff\xa1");
    assert_eq!(
        Hyx::read(&other).unwrap_err(),
        "a package has a hyscribe.ext.toml"
    );
    // At the limits, an entry is read on: here to its local header.
    let at_most = [
        ((64u32 << 20).to_le_bytes(), 1u32 << 20, "larger than"),
        (100u32.to_le_bytes(), 1, "the size it says"),
    ];
    for (size, stored, not) in at_most {
        let bytes = zip(&[("LICENSE", b"x", false)]);
        let at = central(&bytes);
        let bytes = patched(patched(bytes, at + 10, &[8, 0]), at + 24, &size);
        let bytes = patched(bytes, at + 20, &stored.to_le_bytes());
        let error = refused(&bytes);
        assert!(
            !error.contains(not) && error.contains("local header"),
            "{error}"
        );
    }
    let error = refused(&package(&[("public/", b"x", false)]));
    assert!(error.contains("public/ does not have the size it says"));
    let zeros = vec![0; 200_000];
    let error = refused(&package(&[("LICENSE", &zeros, true)]));
    assert!(error.contains("LICENSE does not have the size it says"));
}

#[test]
fn refuses_more_than_the_total() {
    let names: Vec<String> = (0..5).map(|n| format!("public/{n}")).collect();
    let entries: Vec<(&str, &[u8], bool)> = names
        .iter()
        .map(|n| (n.as_str(), b"x".as_slice(), false))
        .collect();
    let mut bytes = zip(&entries);
    let mut at = central(&bytes);
    for name in &names {
        bytes = patched(bytes, at + 10, &[8, 0]);
        bytes = patched(bytes, at + 20, &(1u32 << 20).to_le_bytes());
        bytes = patched(bytes, at + 24, &(60u32 << 20).to_le_bytes());
        at += 46 + name.len();
    }
    assert!(refused(&bytes).contains("holds at most 268435456 bytes"));
    // Exactly the most a package may hold is read on.
    let names: Vec<String> = (0..4).map(|n| format!("public/{n}")).collect();
    let entries: Vec<(&str, &[u8], bool)> = names
        .iter()
        .map(|n| (n.as_str(), b"x".as_slice(), false))
        .collect();
    let mut bytes = zip(&entries);
    let mut at = central(&bytes);
    for name in &names {
        bytes = patched(bytes, at + 10, &[8, 0]);
        bytes = patched(bytes, at + 20, &(1u32 << 20).to_le_bytes());
        bytes = patched(bytes, at + 24, &(64u32 << 20).to_le_bytes());
        at += 46 + name.len();
    }
    assert!(refused(&bytes).contains("local header"));
}

#[test]
fn checks_each_entry_against_its_local_header() {
    let bytes = zip(&[(manifest::FILE, MANIFEST, false)]);
    for (at, with) in [
        (0, b"PK\x03\x05".as_slice()),
        (6, b"\x00\x00"),
        (8, b"\x08\x00"),
        (30, b"x"),
    ] {
        let error = refused(&patched(bytes.clone(), at, with));
        assert!(
            error.contains("does not match its local header"),
            "{at}: {error}"
        );
    }
    let error = refused(&patched(bytes.clone(), 26, b"\xff\xff"));
    assert!(error.contains("does not match its local header"));
    let local = central(&bytes) + 42;
    let error = refused(&patched(bytes.clone(), local, &u32::MAX.to_le_bytes()));
    assert!(error.contains("does not match its local header"));
    let error = refused(&patched(bytes.clone(), central(&bytes) + 16, b"\0\0\0\0"));
    assert!(error.contains("size or CRC"));
    let deflated = zip(&[(manifest::FILE, MANIFEST, true)]);
    let data = 30 + manifest::FILE.len();
    let error = refused(&patched(deflated.clone(), data, b"\xff\xff\xff"));
    assert!(error.contains("does not inflate to the size it says"));
    let longer = (MANIFEST.len() as u32 + 1).to_le_bytes();
    let at = central(&deflated) + 24;
    let error = refused(&patched(deflated, at, &longer));
    assert!(error.contains("size or CRC"), "{error}");
}

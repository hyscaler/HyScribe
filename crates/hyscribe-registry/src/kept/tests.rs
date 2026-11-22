// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fs;
use std::os::unix::fs::PermissionsExt;

use hyscribe_testkit::minisign::{Key, key_list, sign_keys};

use super::*;
use crate::tests::{NOW, test_roots};

/// Keeps a key list of `serial` signed by roots 1 and 2 (and by key 9 in
/// the third place, which counts for nothing), with key 10 for the index.
fn keep_keys(folder: &Path, serial: u64) {
    let keys = [(&Key(10), r#""index""#, NOW - 10, NOW + 10)];
    let list = key_list(serial, NOW + 100, &keys, &[]);
    fs::write(folder.join("keys.json"), &list).unwrap();
    let signatures = sign_keys(&list, serial, NOW + 100, &[&Key(1), &Key(2), &Key(9)]);
    for (n, signature) in signatures.iter().enumerate() {
        fs::write(
            folder.join(format!("keys.json.{}.minisig", n + 1)),
            signature,
        )
        .unwrap();
    }
}

fn keep_index(folder: &Path, serial: u64) {
    let index = format!(
        r#"{{"serial":{serial},"expires":{},"extensions":[],"revoked":[]}}"#,
        NOW + 5
    );
    fs::write(folder.join("index.json"), &index).unwrap();
    let comment = format!("hyscribe-index serial={serial} expires={}", NOW + 5);
    fs::write(
        folder.join("index.json.minisig"),
        Key(10).sign(index.as_bytes(), &comment),
    )
    .unwrap();
}

fn kept_state(folder: &Path) -> String {
    fs::read_to_string(folder.join("state")).unwrap()
}

#[test]
fn reads_nothing_from_an_empty_folder() {
    let folder = tempfile::tempdir().unwrap();
    assert_eq!(keys(folder.path(), &test_roots(), NOW), Ok(None));
    let list = KeyList::parse(key_list(1, NOW + 1, &[], &[]).as_bytes()).unwrap();
    assert_eq!(index(folder.path(), &list, NOW), Ok(None));
    assert!(!folder.path().join("state").exists());
}

#[test]
fn keeps_the_highest_serials() {
    let folder = tempfile::tempdir().unwrap();
    let folder = folder.path();
    keep_keys(folder, 3);
    let list = keys(folder, &test_roots(), NOW).unwrap().unwrap();
    assert_eq!(list.serial, 3);
    assert_eq!(kept_state(folder), "keys 3\nindex 0\n");
    keep_index(folder, 5);
    assert_eq!(index(folder, &list, NOW).unwrap().unwrap().serial, 5);
    assert_eq!(kept_state(folder), "keys 3\nindex 5\n");
    keep_keys(folder, 4);
    assert_eq!(keys(folder, &test_roots(), NOW).unwrap().unwrap().serial, 4);
    assert_eq!(kept_state(folder), "keys 4\nindex 5\n");
    keep_keys(folder, 2);
    let older = "registry/keys.json: it is older than one already accepted (serial 2 < 4)";
    assert_eq!(keys(folder, &test_roots(), NOW), Err(older.into()));
    keep_index(folder, 4);
    let older = "registry/index.json: it is older than one already accepted (serial 4 < 5)";
    assert_eq!(index(folder, &list, NOW), Err(older.into()));
    assert_eq!(kept_state(folder), "keys 4\nindex 5\n");
}

#[test]
fn reads_the_same_serial_without_writing() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path();
    keep_keys(path, 3);
    keep_index(path, 5);
    let list = keys(path, &test_roots(), NOW).unwrap().unwrap();
    index(path, &list, NOW).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o555)).unwrap();
    let again = (keys(path, &test_roots(), NOW), index(path, &list, NOW));
    writable(path);
    assert!(again.0.unwrap().is_some() && again.1.unwrap().is_some());
}

fn writable(path: &Path) {
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn refuses_what_does_not_verify_or_read() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path();
    keep_keys(path, 3);
    let refused = keys(path, &[], NOW).unwrap_err();
    assert_eq!(refused, "keys.json is not signed by two of the root keys");
    let list = keys(path, &test_roots(), NOW).unwrap().unwrap();
    keep_index(path, 5);
    fs::remove_file(path.join("index.json.minisig")).unwrap();
    let alone = "registry/index.json has no index.json.minisig beside it";
    assert_eq!(index(path, &list, NOW), Err(alone.into()));
    keep_index(path, 5);
    assert!(index(path, &list, NOW + 5).unwrap_err().contains("expired"));
    fs::write(path.join("state"), "keys 3\n").unwrap();
    let state = "registry/state does not read";
    let unread = keys(path, &test_roots(), NOW).unwrap_err();
    assert_eq!(unread, format!("registry/keys.json: {state}"));
    let unread = index(path, &list, NOW).unwrap_err();
    assert_eq!(unread, format!("registry/index.json: {state}"));
    fs::remove_file(path.join("keys.json.2.minisig")).unwrap();
    fs::create_dir(path.join("keys.json.2.minisig")).unwrap();
    let unread = keys(path, &test_roots(), NOW).unwrap_err();
    assert!(
        unread.starts_with("registry/keys.json.2.minisig cannot be read: "),
        "{unread}"
    );
    fs::write(path.join("state"), "keys 1\nindex 1\n").unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o555)).unwrap();
    let unwritten = index(path, &list, NOW);
    writable(path);
    let unwritten = unwritten.unwrap_err();
    let expected = "registry/index.json: registry/state cannot be written: ";
    assert!(unwritten.starts_with(expected), "{unwritten}");
}

#[test]
fn reports_files_that_cannot_be_read() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path();
    keep_keys(path, 3);
    let list = keys(path, &test_roots(), NOW).unwrap().unwrap();
    keep_index(path, 5);
    let unread = |name: &str, read: &dyn Fn() -> String| {
        let file = path.join(name);
        let kept = fs::read(&file).unwrap();
        fs::remove_file(&file).unwrap();
        fs::create_dir(&file).unwrap();
        let error = read();
        fs::remove_dir(&file).unwrap();
        fs::write(&file, kept).unwrap();
        let expected = format!("registry/{name} cannot be read: ");
        assert!(error.contains(&expected), "{error}");
    };
    unread("state", &|| keys(path, &test_roots(), NOW).unwrap_err());
    unread("keys.json", &|| keys(path, &test_roots(), NOW).unwrap_err());
    unread("index.json", &|| index(path, &list, NOW).unwrap_err());
    unread("index.json.minisig", &|| {
        index(path, &list, NOW).unwrap_err()
    });
}

#[test]
fn reads_and_writes_the_state() {
    let state = State {
        keys: 7,
        index: 18_446_744_073_709_551_615,
    };
    assert_eq!(State::parse(state.text().as_bytes()), Some(state));
    for refused in [
        "keys 1\nindex 2",
        "keys 1\nindex 2\n\n",
        "index 1\nkeys 2\n",
        "keys  1\nindex 2\n",
        "keys\nindex 2\n",
        "keys 1x\nindex 2\n",
        "keys -1\nindex 2\n",
        "keys 1\nindex 18446744073709551616\n",
        "keys 000000000000000000001\nindex 2\n",
        "keys1\nindex 2\n",
        "\u{ff}",
    ] {
        assert_eq!(State::parse(refused.as_bytes()), None, "{refused:?}");
    }
    assert_eq!(State::parse(b"\xff\n"), None);
    let padded = State::parse(b"keys 00000000000000000001\nindex 2\n");
    assert_eq!(padded, Some(State { keys: 1, index: 2 }));
}

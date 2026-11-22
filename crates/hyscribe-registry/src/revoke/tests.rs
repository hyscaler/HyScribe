// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fs;
use std::os::unix::fs::PermissionsExt;

use hyscribe_ext::origin::Origin;
use hyscribe_testkit::minisign::{Key, key_list};

use super::*;
use crate::tests::NOW;

const MANIFEST: &str = include_str!("../../../hyscribe-ext/guest/hyscribe.ext.toml");
const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// A plugin `id` whose `origin.txt` says `origin`, none for no file.
fn plugin(plugins: &Path, id: &str, origin: Option<String>) {
    let folder = plugins.join(id);
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("hyscribe.ext.toml"),
        MANIFEST.replace("hyscribe-test", id),
    )
    .unwrap();
    if let Some(origin) = origin {
        fs::write(folder.join(origin::FILE), origin).unwrap();
    }
}

fn origin_of(plugins: &Path, id: &str) -> Origin {
    origin::read(&plugins.join(id)).unwrap().unwrap()
}

fn keys() -> KeyList {
    let keys = [(&Key(10), r#""packages""#, 0, 1)];
    let list = key_list(1, NOW + 1, &keys, &[&Key(11).id()]);
    KeyList::parse(list.as_bytes()).unwrap()
}

fn index() -> Index {
    let text = format!(
        r#"{{"serial":1,"expires":{},"extensions":[],"revoked":[{{"id":"a","version":"0.1.0","sha256":"{HASH}","reason":"CVE-1"}}]}}"#,
        NOW + 1
    );
    Index::parse(text.as_bytes()).unwrap()
}

#[test]
fn marks_what_the_registry_revokes_only() {
    let dir = tempfile::tempdir().unwrap();
    let plugins = dir.path();
    let (ten, eleven) = (Key(10).id(), Key(11).id());
    plugin(plugins, "a", Some(format!("signed {ten} {HASH}\n")));
    plugin(plugins, "b", Some(format!("signed {eleven} {HASH}\n")));
    plugin(plugins, "c", Some(format!("trusted {eleven} {HASH}\n")));
    plugin(plugins, "d", Some(format!("unsigned {HASH}\n")));
    let earlier = format!("signed {eleven} {HASH}\nrevoked 5 earlier\n");
    plugin(plugins, "e", Some(earlier.clone()));
    plugin(plugins, "f", None);
    plugin(plugins, "g", Some("nonsense".into()));
    plugin(plugins, "h", Some(format!("signed {ten} {HASH}\n")));
    let marked = revoke(plugins, &keys(), Some(&index()), NOW).unwrap();
    let key = format!("its signing key {eleven} was revoked");
    assert_eq!(marked, ["a: CVE-1".to_string(), format!("b: {key}")]);
    assert_eq!(origin_of(plugins, "a").revoked, Some((NOW, "CVE-1".into())));
    assert_eq!(origin_of(plugins, "b").revoked, Some((NOW, key)));
    for id in ["c", "d", "h"] {
        assert_eq!(origin_of(plugins, id).revoked, None, "{id}");
    }
    assert_eq!(
        fs::read_to_string(plugins.join("e").join(origin::FILE)).unwrap(),
        earlier
    );
    assert!(!plugins.join("f").join(origin::FILE).exists());
    assert_eq!(
        revoke(plugins, &keys(), Some(&index()), NOW + 1),
        Ok(vec![])
    );
}

#[test]
fn needs_the_index_for_a_revoked_version() {
    let dir = tempfile::tempdir().unwrap();
    let plugins = dir.path();
    plugin(
        plugins,
        "a",
        Some(format!("signed {} {HASH}\n", Key(10).id())),
    );
    assert_eq!(revoke(plugins, &keys(), None, NOW), Ok(vec![]));
    let folder = plugins.join("a");
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o555)).unwrap();
    let refused = revoke(plugins, &keys(), Some(&index()), NOW);
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).unwrap();
    let refused = refused.unwrap_err();
    assert!(refused.starts_with("cannot mark a revoked: "), "{refused}");
}

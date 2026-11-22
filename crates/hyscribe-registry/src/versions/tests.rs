// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn document() -> String {
    format!(
        r#"{{"id":"contact-form-7","versions":[{{"version":"0.1.0","sdk":"0.1","requires_hyscribe":"0.1.0","replaces":"contact-form-7/wp-contact-form-7.php","replaces_versions":">=5.9","size":2048,"sha256":"{HASH}","released":1795161600,"page":"extensions/contact-form-7/","revoked":"CVE-2026-0001: fixed in 0.2.1"}},{{"version":"0.2.1","sdk":"0.2","requires_hyscribe":"12.0.345","replaces":null,"replaces_versions":null,"size":{MAX_PACKAGE},"sha256":"{HASH}","released":1795161600,"page":"x","revoked":null}}]}}"#
    )
}

fn entry(text: &str) -> Entry {
    Entry {
        id: "contact-form-7".into(),
        kind: hyscribe_ext::manifest::Kind::Plugin,
        latest: Vec::new(),
        sha256: sha256(text.as_bytes()),
        size: text.len() as u64,
    }
}

#[test]
fn reads_the_versions_oldest_first() {
    let text = document();
    let versions = Versions::verify(text.as_bytes(), &entry(&text)).unwrap();
    assert_eq!(versions.id, "contact-form-7");
    let first = Version {
        version: "0.1.0".into(),
        sdk: "0.1".into(),
        requires_hyscribe: [0, 1, 0],
        replaces: Some("contact-form-7/wp-contact-form-7.php".into()),
        replaces_versions: Some(">=5.9".into()),
        size: 2048,
        sha256: HASH.into(),
        released: 1_795_161_600,
        page: "extensions/contact-form-7/".into(),
        revoked: Some("CVE-2026-0001: fixed in 0.2.1".into()),
    };
    assert_eq!(versions.versions[0], first);
    let second = &versions.versions[1];
    assert_eq!(second.requires_hyscribe, [12, 0, 345]);
    assert_eq!(
        (second.replaces.as_ref(), second.revoked.as_ref()),
        (None, None)
    );
    assert_eq!(second.size, MAX_PACKAGE);
    let empty = r#"{"id":"a","versions":[]}"#;
    assert_eq!(Versions::parse(empty.as_bytes()).unwrap().versions, []);
}

#[test]
fn refuses_a_document_the_index_does_not_name() {
    let text = document();
    let name = "extensions/contact-form-7.json";
    let mut other = entry(&text);
    other.size += 1;
    let not = format!("{name} is not the one the index names");
    assert_eq!(Versions::verify(text.as_bytes(), &other).unwrap_err(), not);
    let mut other = entry(&text);
    other.sha256 = HASH.into();
    assert_eq!(Versions::verify(text.as_bytes(), &other).unwrap_err(), not);
    let unread = "{}";
    let refused = Versions::verify(unread.as_bytes(), &entry(unread)).unwrap_err();
    assert_eq!(refused, format!("{name} does not read"));
    let moved = text.replace(
        "\"contact-form-7\",\"versions",
        "\"contact-form-8\",\"versions",
    );
    let refused = Versions::verify(moved.as_bytes(), &entry(&moved)).unwrap_err();
    assert_eq!(refused, format!("{name} is of contact-form-8"));
}

#[test]
fn refuses_members_out_of_their_bounds() {
    let good = document();
    assert!(Versions::parse(good.as_bytes()).is_some());
    let refused = |text: &str| {
        assert_ne!(text, good);
        assert_eq!(Versions::parse(text.as_bytes()), None, "{text}");
    };
    refused(&good.replace("\"contact-form-7\",\"versions", "\"Contact\",\"versions"));
    refused(&good.replace("\"versions\":[", "\"x\":1,\"versions\":["));
    refused(&good.replace("\"versions\":[{", "\"versions\":[1,{"));
    refused(&good.replace("\"0.2.1\"", "\"0.1.0\""));
    refused(&good.replace(":1795161600,\"page\":\"x\"", ":1795161599,\"page\":\"x\""));
    refused(&good.replace("\"version\":\"0.1.0\"", "\"version\":\"0 1\""));
    refused(&good.replace("\"sdk\":\"0.1\"", "\"sdk\":\"\""));
    for release in [
        "0.1",
        "0.1.0.0",
        "0.1.",
        "0.+1.0",
        "0.1.a",
        "99999999999999999999.0.0",
    ] {
        refused(&good.replace("\"12.0.345\"", &format!("\"{release}\"")));
    }
    refused(&good.replace("\"12.0.345\"", "12"));
    refused(&good.replace("\">=5.9\"", "\"\""));
    refused(&good.replace("\">=5.9\"", "1"));
    refused(&good.replace("\">=5.9\"", "\"a\\nb\""));
    refused(&good.replace("\">=5.9\"", &format!("\"{}\"", "a".repeat(MAX_TEXT + 1))));
    refused(&good.replace("/wp-", "/\\u00e9"));
    refused(&good.replace("\"size\":2048", "\"size\":0"));
    refused(&good.replace(
        &format!("\"size\":{MAX_PACKAGE}"),
        &format!("\"size\":{}", MAX_PACKAGE + 1),
    ));
    refused(&good.replacen(HASH, "x", 1));
    refused(&good.replace(
        "\"released\":1795161600,\"page\":\"e",
        "\"released\":-1,\"page\":\"e",
    ));
    refused(&good.replace("\"revoked\":null", "\"revoked\":\"\""));
    refused(&good.replace("\"revoked\":null", "\"revoked\":null,\"x\":1"));
    refused("1");
    for member in ["versions", "sdk", "replaces", "page", "revoked"] {
        refused(&crate::tests::without(&good, member));
    }
    let long = good.replace("\">=5.9\"", &format!("\"{}\"", "a".repeat(MAX_TEXT)));
    assert!(Versions::parse(long.as_bytes()).is_some());
}

#[test]
fn a_page_is_a_path_under_the_base() {
    for page in ["x", "a/b", "a/b/", "a-b_c.d~e/f.html"] {
        assert!(path(page), "{page}");
    }
    let long = "a".repeat(MAX_TEXT);
    assert!(path(&long));
    for page in [
        "",
        "/",
        "/a",
        "a//b",
        "a/../b",
        "..",
        "./a",
        "a?b",
        "a b",
        "https://x",
    ] {
        assert!(!path(page), "{page}");
    }
    assert!(!path(&format!("{long}a")));
}

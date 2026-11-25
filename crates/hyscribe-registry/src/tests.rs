// SPDX-License-Identifier: AGPL-3.0-or-later

use std::net::SocketAddr;
use std::sync::Arc;

use hyscribe_ext::signature::{PublicKey, sha256};
use hyscribe_fetch::{Boxed, Client, Net};
use hyscribe_testkit::minisign::{Key, key_list, sign_keys};
use hyscribe_testkit::registry::Served;
use tokio::net::TcpStream;

use crate::keys::KeyList;

/// The test extension, built from source as `hyscribe-ext`'s tests build it.
#[allow(dead_code, reason = "only its component is used here")]
#[path = "../../hyscribe-ext/tests/guest/mod.rs"]
mod guest;

/// The test clock.
pub(crate) const NOW: u64 = 1_800_000_000;

/// The test registry's base URL.
pub(crate) const BASE: &str = "https://pages.test/";

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

/// Every name resolves to a public address, every connection goes to the
/// test registry.
struct Local(SocketAddr);

impl Net for Local {
    fn resolve<'a>(&'a self, _: &'a str, port: u16) -> Boxed<'a, Vec<SocketAddr>> {
        let address = SocketAddr::new([93, 184, 215, 14].into(), port);
        Box::pin(async move { Ok(vec![address]) })
    }

    fn dial(&self, _: SocketAddr) -> Boxed<'_, TcpStream> {
        Box::pin(TcpStream::connect(self.0))
    }
}

/// A client reaching `served` for every host, trusting its certificate.
pub(crate) fn client(served: &Served) -> Client {
    let local = Arc::new(Local(served.address));
    Client::new(hyscribe_testkit::registry::roots(), local, Arc::new(()))
}

/// The test extension's package at `version`, granted the option `more`
/// too unless it is empty.
pub(crate) fn package(version: &str, more: &str) -> Vec<u8> {
    if more == "broken" {
        return b"not a package".to_vec();
    }
    if more == "renamed" {
        let manifest = include_str!("../../hyscribe-ext/guest/hyscribe.ext.toml")
            .replace("id = \"hyscribe-test\"", "id = \"renamed\"")
            .replace("\"hyscribe-test/", "\"renamed/");
        return hyscribe_testkit::hyx::zip(&[
            ("hyscribe.ext.toml", manifest.as_bytes(), true),
            ("extension.wasm", guest::component(), true),
        ]);
    }
    let more = if more.is_empty() {
        String::new()
    } else {
        format!("\"{more}\", ")
    };
    let manifest = include_str!("../../hyscribe-ext/guest/hyscribe.ext.toml")
        .replace("version = \"0.1.0\"", &format!("version = \"{version}\""))
        .replace("options = [\"ext\",", &format!("options = [{more}\"ext\","));
    hyscribe_testkit::hyx::zip(&[
        ("hyscribe.ext.toml", manifest.as_bytes(), true),
        ("extension.wasm", guest::component(), true),
    ])
}

/// A version the test registry publishes.
#[derive(Clone, Copy)]
pub(crate) struct Release {
    pub(crate) version: &'static str,
    pub(crate) sdk: &'static str,
    pub(crate) requires: &'static str,
    pub(crate) revoked: Option<&'static str>,
    /// An option its manifest grants beside the test extension's own, if
    /// not empty; its manifest names SDK 0.4 whatever `sdk` says.
    pub(crate) option: &'static str,
}

/// A release of SDK 0.4 for any HyScribe.
pub(crate) const fn release(version: &'static str) -> Release {
    Release {
        version,
        sdk: "0.4",
        requires: "0.1.0",
        revoked: None,
        option: "",
    }
}

/// The test registry at `serial`: keys 1 and 2 sign the key list, key 10
/// the index, key 11 the packages; the index lists `hyscribe-test` with
/// `releases` and revokes the versions of `revoked`, and a theme.
pub(crate) fn publish(
    served: &Served,
    serial: u64,
    releases: &[Release],
    revoked: &[(&str, &str)],
) {
    let keys = [
        (&Key(10), r#""index""#, NOW - 10, NOW + 10),
        (&Key(11), r#""packages""#, NOW - 10, NOW + 10),
    ];
    let list = key_list(serial, NOW + 100, &keys, &[]);
    for (n, signature) in sign_keys(&list, serial, NOW + 100, &[&Key(1), &Key(2)])
        .into_iter()
        .enumerate()
    {
        served.put(&format!("v1/keys.json.{}.minisig", n + 1), signature);
    }
    served.put("v1/keys.json", list);
    let mut entries = Vec::new();
    let mut hashes = Vec::new();
    for release in releases {
        let bytes = package(release.version, release.option);
        let sha = sha256(&bytes);
        let name = format!(
            "v1/packages/hyscribe-test/hyscribe-test-{}.hyx",
            release.version
        );
        let comment = format!(
            "hyscribe-package id=hyscribe-test version={} sdk={} sha256={sha} signed={NOW}",
            release.version, release.sdk
        );
        served.put(&format!("{name}.minisig"), Key(11).sign(&bytes, &comment));
        let revoked = release
            .revoked
            .map_or("null".into(), |r| format!("\"{r}\""));
        entries.push(format!(
            r#"{{"version":"{}","sdk":"{}","requires_hyscribe":"{}","replaces":"hyscribe-test/hyscribe-test.php","replaces_versions":">=0.1","size":{},"sha256":"{sha}","released":{NOW},"page":"extensions/hyscribe-test/","revoked":{revoked}}}"#,
            release.version,
            release.sdk,
            release.requires,
            bytes.len()
        ));
        hashes.push((release.version, sha));
        served.put(&name, bytes);
    }
    let document = format!(
        r#"{{"id":"hyscribe-test","versions":[{}]}}"#,
        entries.join(",")
    );
    let revoked: Vec<String> = revoked
        .iter()
        .map(|(version, reason)| {
            let sha = hashes.iter().find(|(v, _)| v == version).unwrap().1.clone();
            format!(r#"{{"id":"hyscribe-test","version":"{version}","sha256":"{sha}","reason":"{reason}"}}"#)
        })
        .collect();
    let latest: Vec<String> = releases
        .iter()
        .map(|r| format!(r#""{}":"{}""#, r.sdk, r.version))
        .collect();
    let index = format!(
        r#"{{"serial":{serial},"expires":{},"extensions":[{{"id":"hyscribe-test","kind":"plugin","latest":{{{}}},"sha256":"{}","size":{}}},{{"id":"a-theme","kind":"theme","latest":{{}},"sha256":"{}","size":1}}],"revoked":[{}]}}"#,
        NOW + 5,
        latest.join(","),
        sha256(document.as_bytes()),
        document.len(),
        sha256(b"x"),
        revoked.join(",")
    );
    let comment = format!("hyscribe-index serial={serial} expires={}", NOW + 5);
    served.put(
        "v1/index.json.minisig",
        Key(10).sign(index.as_bytes(), &comment),
    );
    served.put("v1/index.json", index);
    served.put("v1/extensions/hyscribe-test.json", document);
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

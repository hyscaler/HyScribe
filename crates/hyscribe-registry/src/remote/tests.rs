// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fs;

use hyscribe_testkit::registry::Served;

use super::*;
use crate::tests::{BASE, NOW, client, publish, release, test_roots};

#[test]
fn a_registry_url_is_https_and_ends_in_a_slash() {
    let served = Served::start().unwrap();
    let client = client(&served);
    assert!(Remote::new(&client, BASE).is_ok());
    for base in [
        "http://pages.test/",
        "https://pages.test/v1",
        "https://pages.test:22/",
        "x",
    ] {
        let refused = Remote::new(&client, base).err().unwrap();
        assert_eq!(refused, format!("{base} is not an https: registry URL"));
    }
}

#[tokio::test]
async fn refreshes_and_keeps_what_verifies() {
    let served = Served::start().unwrap();
    publish(&served, 4, &[release("0.1.0")], &[]);
    let client = client(&served);
    let remote = Remote::new(&client, BASE).unwrap();
    let folder = tempfile::tempdir().unwrap();
    let registry = folder.path().join("registry");
    fs::create_dir_all(&registry).unwrap();
    fs::write(registry.join("keys.json.3.minisig"), "old").unwrap();
    let (keys, index) = remote.refresh(&registry, &test_roots(), NOW).await.unwrap();
    assert_eq!((keys.serial, index.serial), (4, 4));
    // Nothing is sent but what the request needs.
    let head = format!(
        "GET /v1/keys.json HTTP/1.1\r\nhost: pages.test\r\nuser-agent: HyScribe/{}\r\naccept-encoding: identity\r\nconnection: close\r\n\r\n",
        env!("CARGO_PKG_VERSION")
    );
    assert_eq!(served.heads()[0], head);
    assert!(!registry.join("keys.json.3.minisig").exists());
    let kept = kept::keys(&registry, &test_roots(), NOW).unwrap().unwrap();
    assert_eq!(kept, keys);
    assert_eq!(
        kept::index(&registry, &kept, NOW).unwrap(),
        Some(index.clone())
    );
    let entry = index.extension("hyscribe-test").unwrap();
    assert_eq!(remote.versions(entry).await.unwrap().versions.len(), 1);
    // Each file must be there, as large as it may be, and verify.
    served.put("v1/index.json.minisig", "x");
    let refused = remote.refresh(&registry, &test_roots(), NOW).await;
    assert_eq!(
        refused.unwrap_err(),
        "index.json: a signature file has four lines"
    );
    served.remove("v1/index.json");
    let refused = remote.refresh(&registry, &test_roots(), NOW).await;
    assert_eq!(
        refused.unwrap_err(),
        "https://pages.test/v1/index.json is not there"
    );
    served.put("v1/keys.json", vec![b' '; keys::MAX_BYTES as usize + 1]);
    let refused = remote.refresh(&registry, &test_roots(), NOW).await;
    assert_eq!(
        refused.unwrap_err(),
        "https://pages.test/v1/keys.json is larger than 65536 bytes"
    );
    served.put("v1/keys.json", "{}");
    let refused = remote.refresh(&registry, &test_roots(), NOW).await;
    assert_eq!(refused.unwrap_err(), "keys.json does not read");
    served.remove("v1/extensions/hyscribe-test.json");
    let missing = remote.versions(entry).await.unwrap_err();
    assert_eq!(
        missing,
        "https://pages.test/v1/extensions/hyscribe-test.json is not there"
    );
}

#[tokio::test]
async fn reports_what_it_cannot_fetch() {
    let served = Served::start().unwrap();
    let client = client(&served);
    // Port 9 of the test site's address answers nothing the client takes.
    let remote = Remote::new(&client, "https://two.test/").unwrap();
    let folder = tempfile::tempdir().unwrap();
    let refused = remote.refresh(folder.path(), &test_roots(), NOW).await;
    assert_eq!(
        refused.unwrap_err(),
        "cannot fetch https://two.test/v1/keys.json: Tls"
    );
    // An answer other than 200 or 404.
    let remote = Remote::new(&client, BASE).unwrap();
    let failing = "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n";
    served.put("v1/keys.json", failing);
    let refused = remote.refresh(folder.path(), &test_roots(), NOW).await;
    assert_eq!(
        refused.unwrap_err(),
        "https://pages.test/v1/keys.json answered 500"
    );
    served.put("v1/keys.json", "");
    assert_eq!(remote.get("v1/keys.json", 0).await, Ok(Some(Vec::new())));
}

#[tokio::test]
async fn downloads_a_package_and_its_signature() {
    let served = Served::start().unwrap();
    publish(&served, 1, &[release("0.1.0")], &[]);
    let client = client(&served);
    let remote = Remote::new(&client, BASE).unwrap();
    let folder = tempfile::tempdir().unwrap();
    let registry = folder.path().join("registry");
    let (_, index) = remote.refresh(&registry, &test_roots(), NOW).await.unwrap();
    let versions = remote
        .versions(index.extension("hyscribe-test").unwrap())
        .await
        .unwrap();
    let version = versions.versions[0].clone();
    let packages = folder.path().join("packages");
    let (bytes, signature) = remote
        .package("hyscribe-test", &version, &packages)
        .await
        .unwrap();
    assert_eq!(hyscribe_ext::signature::sha256(&bytes), version.sha256);
    assert!(signature.starts_with(b"untrusted comment: "));
    let agent = format!("\r\nuser-agent: HyScribe/{}\r\n", env!("CARGO_PKG_VERSION"));
    assert!(served.heads().iter().all(|head| head.contains(&agent)));
    assert_eq!(fs::read_dir(&packages).unwrap().count(), 0);
    // Another package of the same size.
    let mut other = version.clone();
    other.sha256 = "0".repeat(64);
    let refused = remote.package("hyscribe-test", &other, &packages).await;
    assert_eq!(
        refused.unwrap_err(),
        "hyscribe-test-0.1.0.hyx is not the package its version document names"
    );
    let mut larger = version.clone();
    larger.size += 1;
    let refused = remote.package("hyscribe-test", &larger, &packages).await;
    let url = "https://pages.test/v1/packages/hyscribe-test/hyscribe-test-0.1.0.hyx";
    assert_eq!(refused.unwrap_err(), format!("cannot download {url}: Size"));
    served.remove("v1/packages/hyscribe-test/hyscribe-test-0.1.0.hyx.minisig");
    let refused = remote.package("hyscribe-test", &version, &packages).await;
    assert_eq!(refused.unwrap_err(), format!("{url}.minisig is not there"));
    let file = folder.path().join("file");
    fs::write(&file, "").unwrap();
    let refused = remote.package("hyscribe-test", &version, &file).await;
    assert!(
        refused
            .unwrap_err()
            .starts_with(&format!("cannot write {}: ", file.display()))
    );
}

#[tokio::test]
async fn stays_on_the_registry_s_host_and_needs_each_file() {
    let served = Served::start().unwrap();
    publish(&served, 1, &[release("0.1.0")], &[]);
    let client = client(&served);
    let remote = Remote::new(&client, BASE).unwrap();
    let folder = tempfile::tempdir().unwrap();
    let roots = test_roots();
    let refresh = || remote.refresh(folder.path(), &roots, NOW);
    served.remove("v1/index.json.minisig");
    let missing = refresh().await.unwrap_err();
    assert_eq!(
        missing,
        "https://pages.test/v1/index.json.minisig is not there"
    );
    served.put(
        "v1/keys.json.2.minisig",
        "HTTP/1.1 503 Busy\r\nContent-Length: 0\r\n\r\n",
    );
    let busy = refresh().await.unwrap_err();
    assert_eq!(
        busy,
        "https://pages.test/v1/keys.json.2.minisig answered 503"
    );
    let away = "HTTP/1.1 302 Found\r\nLocation: https://two.test/keys.json\r\n\r\n";
    served.put("v1/keys.json", away);
    let stopped = refresh().await.unwrap_err();
    assert_eq!(
        stopped,
        "cannot fetch https://pages.test/v1/keys.json: Disallowed"
    );
}

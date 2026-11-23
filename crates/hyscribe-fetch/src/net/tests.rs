// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

fn tls() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/tls")
}

#[test]
fn roots_come_from_the_bundle_else_the_folders() {
    let bundle = roots(Some(tls().join("ca.pem")), vec![tls()]);
    assert_eq!(bundle.len(), 1);
    // Each file of the folders: the CA and the leaf; the key holds none.
    let folders = roots(None, vec![tls(), tls().join("missing")]);
    assert_eq!(folders.len(), 2);
    assert!(roots(Some(tls().join("missing.pem")), Vec::new()).is_empty());
    assert!(roots(None, Vec::new()).is_empty());
    assert!(!host_roots().is_empty());
    assert_eq!(host_certificates().len(), host_roots().len());
}

#[test]
fn a_certificate_that_is_no_trust_anchor_is_left_out() {
    let dir = std::env::temp_dir().join(format!("hyscribe-fetch-roots-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let junk = dir.join("junk.pem");
    let pem = "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n";
    std::fs::write(&junk, pem).unwrap();
    assert!(roots(Some(junk), Vec::new()).is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn the_system_resolves_and_connects() {
    let site = hyscribe_testkit::pages::page_server().unwrap();
    let addresses = System.resolve("127.0.0.1", 8080).await.unwrap();
    assert_eq!(addresses, vec!["127.0.0.1:8080".parse().unwrap()]);
    assert!(System.resolve("bad host name", 80).await.is_err());
    assert!(System.dial(site).await.is_ok());
}

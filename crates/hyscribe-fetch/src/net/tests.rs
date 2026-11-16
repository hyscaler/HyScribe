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
}

#[tokio::test]
async fn the_system_resolves_and_connects() {
    let site = hyscribe_testkit::pages::page_server().unwrap();
    let addresses = System.resolve("127.0.0.1", 8080).await.unwrap();
    assert_eq!(addresses, vec!["127.0.0.1:8080".parse().unwrap()]);
    assert!(System.resolve("bad host name", 80).await.is_err());
    assert!(System.dial(site).await.is_ok());
}

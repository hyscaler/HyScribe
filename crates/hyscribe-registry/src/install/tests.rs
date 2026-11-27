// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fs;

use hyscribe_ext::origin::Origin;
use hyscribe_testkit::registry::Served;

use super::*;
use crate::tests::{BASE, NOW, Release, client, package, publish, release, test_roots};

/// A site in a fresh folder, and the registry it fetches from.
struct Test {
    folder: tempfile::TempDir,
    served: Served,
    client: hyscribe_fetch::Client,
    roots: Vec<PublicKey>,
}

impl Test {
    fn new(releases: &[Release], revoked: &[(&str, &str)]) -> Test {
        let served = Served::start().unwrap();
        publish(&served, 1, releases, revoked);
        Test {
            folder: tempfile::tempdir().unwrap(),
            client: client(&served),
            served,
            roots: test_roots(),
        }
    }

    fn site(&self) -> Site<'_> {
        Site {
            home: self.folder.path(),
            roots: &self.roots,
            now: NOW,
        }
    }

    async fn install(
        &self,
        name: &str,
        version: Option<&str>,
        allow: Allow,
    ) -> Result<String, String> {
        let remote = Remote::new(&self.client, BASE).unwrap();
        install(&remote, &self.site(), (name, version), allow).await
    }

    async fn update(&self, which: Update<'_>, allow: Allow) -> Result<String, String> {
        let remote = Remote::new(&self.client, BASE).unwrap();
        match update(&remote, &self.site(), which, allow).await? {
            (text, false) => Ok(text),
            (text, true) => Err(text),
        }
    }

    fn origin(&self) -> Origin {
        let folder = self.folder.path().join("plugins/hyscribe-test");
        origin::read(&folder).unwrap().unwrap()
    }

    fn version(&self) -> String {
        let plugins = self.folder.path().join("plugins");
        installed::list(&plugins)[0].manifest.version.clone()
    }
}

const NEWER: Release = Release {
    requires: "99.0.0",
    ..release("0.3.0")
};

const ASKS: &str = "Options: ext, ext_active, ext_bytes, ext_fail, ext_hooks, ext_title";

#[tokio::test]
async fn installs_the_newest_version_that_fits() {
    let test = Test::new(&[release("0.1.0"), release("0.2.0"), NEWER], &[]);
    let done = test
        .install("hyscribe-test", None, Allow::default())
        .await
        .unwrap();
    assert!(done.starts_with(&format!(
        "Installed HyScribe Test 0.2.0 into plugins/hyscribe-test/ from the registry, signed by its key {}, standing in for hyscribe-test/hyscribe-test.php. It asks for: {ASKS}; ",
        hyscribe_testkit::minisign::Key(11).id()
    )), "{done}");
    assert!(done.contains("Sensitive hooks: map_meta_cap, rest_pre_dispatch, rest_pre_insert_post; limits of 50 ms a call, 250 ms a request and 16 MiB. Its page: https://pages.test/extensions/hyscribe-test/. It runs once activated from the Plugins screen.\n"), "{done}");
    let origin = test.origin();
    assert_eq!(origin.signer.unwrap().0, Signer::Registry);
    assert_eq!(origin.sha256, signature::sha256(&package("0.2.0", "")));
    let home = test.folder.path();
    assert!(
        home.join("packages/hyscribe-test-0.2.0.hyx.minisig")
            .is_file()
    );
    assert!(!home.join("packages/.download-hyscribe-test-0.2.0").exists());
    assert_eq!(
        fs::read_to_string(home.join("registry/state")).unwrap(),
        "keys 1\nindex 1\n"
    );
    // The same version again installs it again; an older one only when asked.
    assert!(
        test.install("hyscribe-test", Some("0.2.0"), Allow::default())
            .await
            .is_ok()
    );
    let older = test
        .install("hyscribe-test", Some("0.1.0"), Allow::default())
        .await;
    assert_eq!(
        older.unwrap_err(),
        "hyscribe-test 0.2.0 is installed and 0.1.0 is not newer; --downgrade installs it"
    );
    let downgrade = Allow {
        downgrade: true,
        ..Allow::default()
    };
    assert!(
        test.install("hyscribe-test", Some("0.1.0"), downgrade)
            .await
            .is_ok()
    );
    assert_eq!(test.version(), "0.1.0");
}

#[tokio::test]
async fn says_why_no_version_fits() {
    let unlinked = Release {
        sdk: "0.9",
        ..release("0.4.0")
    };
    let revoked = Release {
        revoked: Some("CVE-2026-0002"),
        ..release("0.1.0")
    };
    let test = Test::new(
        &[revoked, release("0.2.0"), NEWER, unlinked],
        &[("0.2.0", "CVE-2026-0003")],
    );
    let allow = Allow::default();
    let asked = |version| test.install("hyscribe-test", Some(version), allow);
    assert_eq!(
        asked("0.1.0").await.unwrap_err(),
        "hyscribe-test 0.1.0 is revoked: CVE-2026-0002"
    );
    assert_eq!(
        asked("0.2.0").await.unwrap_err(),
        "hyscribe-test 0.2.0 is revoked: CVE-2026-0003"
    );
    assert_eq!(
        asked("0.3.0").await.unwrap_err(),
        "hyscribe-test 0.3.0 needs HyScribe 99.0.0 or newer"
    );
    assert_eq!(
        asked("0.4.0").await.unwrap_err(),
        "hyscribe-test 0.4.0 needs SDK 0.9, which this HyScribe does not link"
    );
    assert_eq!(
        asked("0.5.0").await.unwrap_err(),
        "the registry has no version 0.5.0 of hyscribe-test"
    );
    let newest = test.install("hyscribe-test", None, allow).await;
    assert_eq!(
        newest.unwrap_err(),
        "hyscribe-test 0.4.0 needs SDK 0.9, which this HyScribe does not link"
    );
    let theme = test.install("a-theme", None, allow).await;
    assert_eq!(
        theme.unwrap_err(),
        "a-theme is not a plugin in the registry"
    );
    let none = test.install("contact-form-7", None, allow).await;
    assert_eq!(
        none.unwrap_err(),
        "contact-form-7 is not a plugin in the registry"
    );
    let empty = Test::new(&[], &[]);
    let nothing = empty.install("hyscribe-test", None, allow).await;
    assert_eq!(
        nothing.unwrap_err(),
        "the registry has no version of hyscribe-test"
    );
}

#[tokio::test]
async fn refuses_a_package_its_document_does_not_describe() {
    // Listed as SDK 0.2, its manifest says 0.4.
    let wrong = Release {
        sdk: "0.2",
        ..release("0.1.0")
    };
    let test = Test::new(&[wrong], &[]);
    let refused = test.install("hyscribe-test", None, Allow::default()).await;
    assert_eq!(
        refused.unwrap_err(),
        "hyscribe-test 0.1.0's manifest is not what its version document says"
    );
    // Listed as hyscribe-test, its manifest names another plugin.
    let renamed = Release {
        option: "renamed",
        ..release("0.1.0")
    };
    let test = Test::new(&[renamed], &[]);
    let refused = test.install("hyscribe-test", None, Allow::default()).await;
    assert_eq!(
        refused.unwrap_err(),
        "hyscribe-test 0.1.0's manifest is not what its version document says"
    );
    // A package signature that names no package.
    let test = Test::new(&[release("0.1.0")], &[]);
    let path = "v1/packages/hyscribe-test/hyscribe-test-0.1.0.hyx";
    let bytes = package("0.1.0", "");
    test.served.put(
        &format!("{path}.minisig"),
        hyscribe_testkit::minisign::Key(11).sign(&bytes, "x"),
    );
    let refused = test.install("hyscribe-test", None, Allow::default()).await;
    assert_eq!(
        refused.unwrap_err(),
        "its signature's trusted comment names no package"
    );
    test.served.put(&format!("{path}.minisig"), "x");
    let refused = test.install("hyscribe-test", None, Allow::default()).await;
    assert_eq!(
        refused.unwrap_err(),
        "its signature: a signature file has four lines"
    );
    // Signed by key 10, which signs the index only.
    let comment = format!(
        "hyscribe-package id=hyscribe-test version=0.1.0 sdk=0.5 sha256={} signed={NOW}",
        signature::sha256(&bytes)
    );
    let index_key = hyscribe_testkit::minisign::Key(10);
    test.served
        .put(&format!("{path}.minisig"), index_key.sign(&bytes, &comment));
    let refused = test.install("hyscribe-test", None, Allow::default()).await;
    assert_eq!(
        refused.unwrap_err(),
        format!("key {} may not sign packages", index_key.id())
    );
    // Signed by key 11 for another package.
    let other = package("0.1.0", "other");
    test.served.put(
        &format!("{path}.minisig"),
        hyscribe_testkit::minisign::Key(11).sign(&other, &comment),
    );
    let refused = test.install("hyscribe-test", None, Allow::default()).await;
    assert!(
        refused
            .unwrap_err()
            .starts_with("its signature does not verify")
    );
    assert!(!test.folder.path().join("plugins/hyscribe-test").exists());
}

#[tokio::test]
async fn new_grants_need_allowing() {
    let wider = Release {
        option: "ext_more",
        ..release("0.2.0")
    };
    let test = Test::new(&[release("0.1.0"), wider], &[]);
    let allow = Allow::default();
    test.install("hyscribe-test", Some("0.1.0"), allow)
        .await
        .unwrap();
    // Not yet activated: nothing approved, nothing refused.
    let plugins = test.folder.path().join("plugins");
    let found = installed::list(&plugins).remove(0);
    installed::approve(&found).unwrap();
    let refused = test.install("hyscribe-test", Some("0.2.0"), allow).await;
    assert_eq!(
        refused.unwrap_err(),
        "hyscribe-test 0.2.0 asks for other grants or limits than those approved (Options: ext_more); --allow-new-grants installs it, and it runs again once activated from the Plugins screen"
    );
    let allowed = Allow {
        new_grants: true,
        ..allow
    };
    test.install("hyscribe-test", Some("0.2.0"), allowed)
        .await
        .unwrap();
    // Back to fewer grants than the approved 0.2.0.
    let found = installed::list(&plugins).remove(0);
    installed::approve(&found).unwrap();
    let downgrade = Allow {
        downgrade: true,
        ..allow
    };
    let refused = test
        .install("hyscribe-test", Some("0.1.0"), downgrade)
        .await;
    assert!(refused.unwrap_err().contains("than those approved (fewer)"));
}

#[tokio::test]
async fn a_local_install_is_replaced_only_when_asked() {
    let test = Test::new(&[release("0.1.0"), release("0.2.0")], &[]);
    let plugins = test.folder.path().join("plugins");
    let packages = test.folder.path().join("packages");
    let bytes = package("0.2.0", "");
    installed::install((&plugins, &packages), &bytes, Vouch::Unsigned).unwrap();
    let allow = Allow::default();
    let refused = test.install("hyscribe-test", Some("0.1.0"), allow).await;
    assert_eq!(
        refused.unwrap_err(),
        "hyscribe-test was installed from a local file, not from the registry; --replace-local replaces it with the registry's"
    );
    let updated = test
        .update(Update::Ids(&["hyscribe-test".into()]), allow)
        .await;
    assert_eq!(
        updated.unwrap_err(),
        "error: hyscribe-test was installed from a local file, so the registry offers it no update; `hyscribe plugin install hyscribe-test --replace-local` replaces it with the registry's\n"
    );
    let all = test.update(Update::All, allow).await.unwrap();
    assert_eq!(all, "No plugin installed from the registry.\n");
    let check = test.update(Update::Check, allow).await.unwrap();
    assert_eq!(check, "Every plugin from the registry is up to date.\n");
    // Replacing a local build is no downgrade, whatever its version.
    let replace = Allow {
        replace_local: true,
        ..allow
    };
    test.install("hyscribe-test", Some("0.1.0"), replace)
        .await
        .unwrap();
    assert_eq!(test.origin().signer.unwrap().0, Signer::Registry);
}

#[tokio::test]
async fn updates_check_and_install_the_newest() {
    let unlinked = Release {
        sdk: "0.9",
        ..release("0.4.0")
    };
    let test = Test::new(&[release("0.1.0"), release("0.2.0"), unlinked], &[]);
    let allow = Allow::default();
    test.install("hyscribe-test", Some("0.1.0"), allow)
        .await
        .unwrap();
    let check = test.update(Update::Check, allow).await.unwrap();
    assert_eq!(
        check,
        "hyscribe-test 0.1.0 is installed; 0.2.0 is available (SDK 0.5): `hyscribe plugin update hyscribe-test`\n"
    );
    let ids = ["hyscribe-test".to_string(), "other".to_string()];
    let updated = test.update(Update::Ids(&ids), allow).await.unwrap_err();
    assert!(
        updated.starts_with("Installed HyScribe Test 0.2.0"),
        "{updated}"
    );
    assert!(
        updated.ends_with("\nerror: other is not installed\n"),
        "{updated}"
    );
    assert_eq!(test.version(), "0.2.0");
    let again = test.update(Update::All, allow).await.unwrap();
    assert_eq!(again, "hyscribe-test 0.2.0 is up to date.\n");
    let check = test.update(Update::Check, allow).await.unwrap();
    assert_eq!(check, "Every plugin from the registry is up to date.\n");
}

/// The kept index offers a newer version to a plugin the registry signed
/// only, never to one an operator's key signed.
#[tokio::test]
async fn offers_go_to_the_registry_s_plugins_only() {
    let test = Test::new(&[release("0.1.0"), release("0.2.0")], &[]);
    test.install("hyscribe-test", Some("0.1.0"), Allow::default())
        .await
        .unwrap();
    let registry = test.folder.path().join("registry");
    let index = crate::kept::shown(&registry, &test.roots, NOW);
    let index = index.unwrap().unwrap();
    let plugins = installed::list(&test.folder.path().join("plugins"));
    let offered = offer(&plugins[0], &index);
    assert_eq!(offered, Some(&("0.5".to_string(), "0.2.0".to_string())));
    let mut origin = test.origin();
    origin.signer = origin.signer.map(|(_, key)| (Signer::Trusted, key));
    origin::write(&plugins[0].folder, &origin).unwrap();
    assert_eq!(offer(&plugins[0], &index), None);
}

#[tokio::test]
async fn a_refreshed_index_revokes_what_it_lists() {
    let test = Test::new(&[release("0.1.0"), release("0.2.0")], &[]);
    let allow = Allow::default();
    test.install("hyscribe-test", Some("0.2.0"), allow)
        .await
        .unwrap();
    publish(
        &test.served,
        2,
        &[release("0.1.0"), release("0.2.0")],
        &[("0.2.0", "CVE-2026-0004")],
    );
    let check = test.update(Update::Check, allow).await.unwrap();
    assert_eq!(
        check,
        "Revoked hyscribe-test: CVE-2026-0004\nhyscribe-test 0.2.0 is revoked: CVE-2026-0004\n"
    );
    assert_eq!(test.origin().revoked, Some((NOW, "CVE-2026-0004".into())));
    // The only way on is an older version, which needs --downgrade.
    let stuck = test.update(Update::All, allow).await.unwrap_err();
    assert_eq!(
        stuck,
        "error: hyscribe-test 0.2.0 is installed and 0.1.0 is not newer; --downgrade installs it\n"
    );
    let downgrade = Allow {
        downgrade: true,
        ..allow
    };
    let done = test.update(Update::All, downgrade).await.unwrap();
    assert!(done.starts_with("Installed HyScribe Test 0.1.0"), "{done}");
    assert_eq!(test.origin().revoked, None);
    // A registry going back is refused before anything changes.
    publish(&test.served, 1, &[release("0.1.0")], &[]);
    let back = test.update(Update::Check, allow).await.unwrap_err();
    assert!(
        back.starts_with("the registry's files are older than those kept"),
        "{back}"
    );
}

#[tokio::test]
async fn an_installed_version_the_document_does_not_list_is_no_newer() {
    let test = Test::new(&[release("0.1.0"), release("0.2.0")], &[]);
    let allow = Allow::default();
    test.install("hyscribe-test", Some("0.2.0"), allow)
        .await
        .unwrap();
    publish(&test.served, 2, &[release("0.1.0")], &[]);
    let refused = test.update(Update::All, allow).await.unwrap_err();
    assert_eq!(
        refused,
        "error: hyscribe-test 0.2.0 is installed and 0.1.0 is not newer; --downgrade installs it\n"
    );
}

#[tokio::test]
async fn reports_each_step_that_fails() {
    let broken = Release {
        option: "broken",
        ..release("0.2.0")
    };
    let test = Test::new(&[release("0.1.0"), broken], &[]);
    let allow = Allow::default();
    let refused = test
        .install("hyscribe-test", None, allow)
        .await
        .unwrap_err();
    assert_eq!(refused, "not a zip archive");
    assert!(!test.folder.path().join("plugins/hyscribe-test").exists());
    // A folder plugins/ cannot be made in.
    fs::write(test.folder.path().join("plugins"), "").unwrap();
    let refused = test.install("hyscribe-test", Some("0.1.0"), allow).await;
    assert!(refused.unwrap_err().starts_with("cannot install into"));
    fs::remove_file(test.folder.path().join("plugins")).unwrap();
    let path = "v1/packages/hyscribe-test/hyscribe-test-0.1.0.hyx";
    test.served.remove(path);
    let refused = test.install("hyscribe-test", Some("0.1.0"), allow).await;
    assert!(refused.unwrap_err().starts_with("cannot download"));
    test.served.remove("v1/extensions/hyscribe-test.json");
    let refused = test.install("hyscribe-test", None, allow).await;
    assert!(
        refused
            .unwrap_err()
            .ends_with("hyscribe-test.json is not there")
    );
    test.served.remove("v1/keys.json");
    let refused = test.install("hyscribe-test", None, allow).await;
    assert_eq!(
        refused.unwrap_err(),
        "https://pages.test/v1/keys.json is not there"
    );
}

#[tokio::test]
async fn a_revocation_it_cannot_write_stops_the_command() {
    let test = Test::new(&[release("0.1.0")], &[]);
    let allow = Allow::default();
    test.install("hyscribe-test", None, allow).await.unwrap();
    publish(
        &test.served,
        2,
        &[release("0.1.0")],
        &[("0.1.0", "CVE-2026-0005")],
    );
    let folder = test.folder.path().join("plugins/hyscribe-test");
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o555)).unwrap();
    let refused = test.update(Update::Check, allow).await;
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        refused
            .unwrap_err()
            .starts_with("cannot mark hyscribe-test revoked")
    );
}

#[tokio::test]
async fn a_version_no_longer_revoked_is_installed_again() {
    let test = Test::new(&[release("0.1.0")], &[]);
    let allow = Allow::default();
    test.install("hyscribe-test", None, allow).await.unwrap();
    publish(
        &test.served,
        2,
        &[release("0.1.0")],
        &[("0.1.0", "CVE-2026-0006")],
    );
    test.update(Update::Check, allow).await.unwrap();
    assert!(test.origin().revoked.is_some());
    publish(&test.served, 3, &[release("0.1.0")], &[]);
    let done = test.update(Update::All, allow).await.unwrap();
    assert!(done.starts_with("Installed HyScribe Test 0.1.0"), "{done}");
    assert_eq!(test.origin().revoked, None);
}

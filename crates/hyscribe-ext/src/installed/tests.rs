// SPDX-License-Identifier: AGPL-3.0-or-later

use std::os::unix::fs::PermissionsExt;

use hyscribe_testkit::hyx::zip;

use super::*;
use crate::tests::component;

const MANIFEST: &str = include_str!("../../guest/hyscribe.ext.toml");

fn package(manifest: &str) -> Vec<u8> {
    zip(&[
        (manifest::FILE, manifest.as_bytes(), true),
        (COMPONENT, component(), true),
        ("public/", b"", false),
        ("public/css/a.css", b"a {}", false),
    ])
}

fn folders(dir: &Path) -> (PathBuf, PathBuf) {
    (dir.join("plugins"), dir.join("packages"))
}

#[test]
fn installs_lists_and_approves() {
    let dir = tempfile::tempdir().unwrap();
    let (plugins, packages) = folders(dir.path());
    assert_eq!(list(&plugins), []);
    let manifest = install((&plugins, &packages), &package(MANIFEST)).unwrap();
    assert_eq!(manifest.id, "hyscribe-test");
    let folder = plugins.join("hyscribe-test");
    assert_eq!(fs::read(folder.join("public/css/a.css")).unwrap(), b"a {}");
    assert_eq!(fs::read(folder.join(COMPONENT)).unwrap(), component());
    assert!(packages.join("hyscribe-test-0.1.0.hyx").is_file());
    let mode = fs::metadata(folder.join(manifest::FILE))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o644);
    let installed = list(&plugins);
    assert_eq!(installed.len(), 1);
    let found = find(&plugins, "hyscribe-test/hyscribe-test.php").unwrap();
    assert_eq!(found, installed[0]);
    assert_eq!(found.manifest, manifest);
    assert!(!found.approved && !found.disabled);
    approve(&found).unwrap();
    let approved = find(&plugins, "hyscribe-test/hyscribe-test.php").unwrap();
    assert!(approved.approved);
    // An update keeps the approval file, and asks again for more.
    let more = MANIFEST
        .replace("version = \"0.1.0\"", "version = \"0.2.0\"")
        .replace(
            "[rest]\nnamespaces = [\"ext/v1\"]",
            "[rest]\nnamespaces = [\"ext/v1\", \"ext/v2\"]",
        );
    faults::write(&folder, &crate::faults::Record::default()).unwrap();
    install((&plugins, &packages), &package(&more)).unwrap();
    let updated = find(&plugins, "hyscribe-test/hyscribe-test.php").unwrap();
    assert_eq!(updated.manifest.version, "0.2.0");
    assert!(!updated.approved && updated.disabled);
    assert!(folder.join(APPROVED).is_file() && folder.join(faults::FILE).is_file());
    revoke(&updated).unwrap();
    revoke(&updated).unwrap();
    assert!(!folder.join(APPROVED).exists());
    let leftovers: Vec<_> = fs::read_dir(&plugins)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(leftovers, ["hyscribe-test"]);
}

#[test]
fn lists_only_plugins_in_their_own_folders() {
    let dir = tempfile::tempdir().unwrap();
    let plugins = dir.path();
    let write = |folder: &str, text: &str| {
        fs::create_dir_all(plugins.join(folder)).unwrap();
        fs::write(plugins.join(folder).join(manifest::FILE), text).unwrap();
    };
    write("hyscribe-test", MANIFEST);
    write(
        "other",
        &MANIFEST.replace("hyscribe-test/hyscribe-test.php", "other/x.php"),
    );
    write("broken", "[x]");
    write("Upper", MANIFEST);
    let theme = "[extension]\nkind = 'theme'\nid = 'theme'\nversion = '1'\nsdk = '0.1'\nlicense = 'MIT'\nname = 'T'\n";
    write("theme", theme);
    fs::write(plugins.join("loose.txt"), "").unwrap();
    fs::create_dir(plugins.join("empty")).unwrap();
    let names: Vec<String> = list(plugins).into_iter().map(|i| i.manifest.id).collect();
    assert_eq!(names, ["hyscribe-test"]);
    assert_eq!(find(plugins, "hyscribe-test/other.php"), None);
    assert_eq!(find(plugins, "hello.php"), None);
    assert_eq!(find(plugins, "../x/hyscribe-test.php"), None);
    fs::write(plugins.join("hyscribe-test").join(faults::FILE), "nonsense").unwrap();
    fs::write(plugins.join("hyscribe-test").join(APPROVED), "[x]").unwrap();
    let found = find(plugins, "hyscribe-test/hyscribe-test.php").unwrap();
    assert!(found.disabled && !found.approved);
}

#[test]
fn refuses_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (plugins, packages) = folders(dir.path());
    let theme = "[extension]\nkind = 'theme'\nid = 't'\nversion = '1'\nsdk = '0.1'\nlicense = 'MIT'\nname = 'T'\n";
    let refusals = [
        (b"not a zip".to_vec(), "not a zip archive"),
        (
            zip(&[(manifest::FILE, theme.as_bytes(), false)]),
            "not a plugin package",
        ),
        (
            zip(&[
                (manifest::FILE, MANIFEST.as_bytes(), false),
                (COMPONENT, b"\0asm", false),
            ]),
            "the component does not load",
        ),
        (
            package(&MANIFEST.replace("[rest]\nnamespaces = [\"ext/v1\"]\n", "")),
            "the component does not load",
        ),
    ];
    for (bytes, why) in refusals {
        let error = install((&plugins, &packages), &bytes).unwrap_err();
        assert!(error.contains(why), "{error}");
    }
    assert_eq!(fs::read_dir(&plugins).unwrap().count(), 0);
    assert!(!packages.exists());
    fs::remove_dir(&plugins).unwrap();
    // A packages folder that is a file, then a plugins folder that is one.
    fs::write(&packages, "").unwrap();
    let error = install((&plugins, &packages), &package(MANIFEST)).unwrap_err();
    assert!(error.contains("cannot install into"), "{error}");
    assert_eq!(fs::read_dir(&plugins).unwrap().count(), 0);
    fs::remove_file(&packages).unwrap();
    fs::remove_dir(&plugins).unwrap();
    fs::write(&plugins, "").unwrap();
    let error = install((&plugins, &packages), &package(MANIFEST)).unwrap_err();
    assert!(error.contains("cannot install into"), "{error}");
}

#[test]
fn an_approval_needs_its_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let installed = Installed {
        manifest: Manifest::parse(MANIFEST.as_bytes()).unwrap(),
        folder: dir.path().join("gone"),
        approved: false,
        disabled: false,
    };
    assert!(approve(&installed).is_err());
    fs::create_dir(&installed.folder).unwrap();
    fs::create_dir(installed.folder.join(APPROVED)).unwrap();
    assert!(revoke(&installed).is_err());
    fs::write(installed.folder.join(manifest::FILE), MANIFEST).unwrap();
    assert!(approve(&installed).is_err());
}

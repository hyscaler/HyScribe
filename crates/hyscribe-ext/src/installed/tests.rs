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
    // A manifest one byte larger than a manifest may be.
    let large = MANIFEST.replace("hyscribe-test", "large");
    let large = format!("{large}{}", "#".repeat(65_537 - large.len()));
    write("large", &large);
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

/// A plugin named `id` as `install` leaves it: its folder, and its package
/// kept as `<id>-<version>.hyx`, which is read for the id it names.
fn kept(dir: &Path, id: &str, version: &str) -> PathBuf {
    let (plugins, packages) = folders(dir);
    let text = MANIFEST
        .replace("hyscribe-test", id)
        .replace("0.1.0", version);
    fs::create_dir_all(plugins.join(id).join("public")).unwrap();
    fs::write(plugins.join(id).join(manifest::FILE), &text).unwrap();
    fs::write(plugins.join(id).join("public/a.css"), "a {}").unwrap();
    fs::create_dir_all(&packages).unwrap();
    let file = packages.join(format!("{id}-{version}.hyx"));
    let entries = [
        (manifest::FILE, text.as_bytes(), false),
        (COMPONENT, b"\0asm".as_slice(), false),
    ];
    fs::write(&file, zip(&entries)).unwrap();
    file
}

fn idle() -> Result<Vec<String>, String> {
    Ok(vec!["other/x.php".into(), "hyscribe-test-more".into()])
}

#[test]
fn deletes_a_plugin_and_its_packages_only() {
    let dir = tempfile::tempdir().unwrap();
    let (plugins, packages) = folders(dir.path());
    let first = kept(dir.path(), "hyscribe-test", "0.1.0");
    let second = kept(dir.path(), "hyscribe-test", "0.2.0");
    let more = kept(dir.path(), "hyscribe-test-more", "0.1.0");
    fs::write(plugins.join("hyscribe-test").join(APPROVED), "x").unwrap();
    fs::write(plugins.join("hyscribe-test").join(faults::FILE), "x").unwrap();
    // Not a package, and a name that is not a package's.
    fs::write(packages.join("hyscribe-test-9.hyx"), "not a package").unwrap();
    // Names that are not `<id>-<version>.hyx` are not read for their id.
    let same = fs::read(&first).unwrap();
    fs::write(packages.join("hyscribe-test-9.hyx.new"), &same).unwrap();
    fs::write(packages.join("stray.hyx"), &same).unwrap();
    let deleted = delete((&plugins, &packages), "hyscribe-test", &idle).unwrap();
    assert_eq!(
        deleted.packages,
        ["hyscribe-test-0.1.0.hyx", "hyscribe-test-0.2.0.hyx"]
    );
    assert!(!plugins.join("hyscribe-test").exists());
    assert!(!first.exists() && !second.exists());
    assert!(more.is_file() && plugins.join("hyscribe-test-more").is_dir());
    assert!(packages.join("hyscribe-test-9.hyx").is_file());
    assert!(packages.join("hyscribe-test-9.hyx.new").is_file());
    assert!(packages.join("stray.hyx").is_file());
    let names: Vec<_> = fs::read_dir(&plugins)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, ["hyscribe-test-more"]);
    let again = delete((&plugins, &packages), "hyscribe-test", &idle).unwrap_err();
    assert_eq!(again, "hyscribe-test is not installed");
}

#[test]
fn refuses_an_active_plugin_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (plugins, packages) = folders(dir.path());
    let package = kept(dir.path(), "hyscribe-test", "0.1.0");
    let active = || Ok(vec!["hyscribe-test/hyscribe-test.php".to_string()]);
    let error = delete((&plugins, &packages), "hyscribe-test", &active).unwrap_err();
    assert!(error.contains("is active: deactivate it"), "{error}");
    assert!(plugins.join("hyscribe-test/public/a.css").is_file() && package.is_file());
    let unreadable = || Err("cannot read active_plugins".to_string());
    let error = delete((&plugins, &packages), "hyscribe-test", &unreadable).unwrap_err();
    assert_eq!(error, "cannot read active_plugins");
    assert!(plugins.join("hyscribe-test").is_dir());
}

#[test]
fn checks_the_id_before_building_a_path() {
    let dir = tempfile::tempdir().unwrap();
    let (plugins, packages) = folders(dir.path());
    kept(dir.path(), "hyscribe-test", "0.1.0");
    let never = || -> Result<Vec<String>, String> { panic!("asked for an invalid id") };
    for id in ["", "../plugins/hyscribe-test", "Hyscribe-Test", "a/b", ".."] {
        let error = delete((&plugins, &packages), id, &never).unwrap_err();
        assert!(error.contains("is not a plugin id"), "{error}");
    }
    let error = delete((&plugins, &packages), "nothing", &never).unwrap_err();
    assert_eq!(error, "nothing is not installed");
    assert!(plugins.join("hyscribe-test").is_dir());
}

#[test]
fn removes_links_and_never_follows_them() {
    let dir = tempfile::tempdir().unwrap();
    let (plugins, packages) = folders(dir.path());
    let outside = dir.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("keep"), "x").unwrap();
    let real = kept(dir.path(), "hyscribe-test", "0.1.0");
    // The package kept is a link to a package elsewhere, and a link in the
    // plugin's folder leads out of it.
    let elsewhere = outside.join("package.hyx");
    fs::rename(&real, &elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, &real).unwrap();
    std::os::unix::fs::symlink(&outside, plugins.join("hyscribe-test/escape")).unwrap();
    delete((&plugins, &packages), "hyscribe-test", &idle).unwrap();
    assert!(!plugins.join("hyscribe-test").exists() && real.symlink_metadata().is_err());
    assert!(elsewhere.is_file());
    assert_eq!(fs::read(outside.join("keep")).unwrap(), b"x");
    // A link standing for the whole plugin's folder is removed itself.
    std::os::unix::fs::symlink(&outside, plugins.join("linked")).unwrap();
    let done = delete((&plugins, &packages), "linked", &idle).unwrap();
    assert!(done.packages.is_empty() && !plugins.join("linked").exists());
    assert_eq!(fs::read(outside.join("keep")).unwrap(), b"x");
}

#[test]
fn finishes_what_a_crash_left_and_reports_a_failure() {
    let dir = tempfile::tempdir().unwrap();
    let (plugins, packages) = folders(dir.path());
    let package = kept(dir.path(), "hyscribe-test", "0.1.0");
    // A folder renamed aside, with nothing else left to delete.
    let aside = plugins.join(".hyscribe-test.deleting");
    fs::rename(plugins.join("hyscribe-test"), &aside).unwrap();
    fs::remove_file(&package).unwrap();
    let done = delete((&plugins, &packages), "hyscribe-test", &idle).unwrap();
    assert!(done.packages.is_empty());
    assert_eq!(fs::read_dir(&plugins).unwrap().count(), 0);
    // Folders that cannot be changed keep the plugin whole and its packages.
    let package = kept(dir.path(), "hyscribe-test", "0.1.0");
    let mode = |folder: &Path, mode| {
        fs::set_permissions(folder, fs::Permissions::from_mode(mode)).unwrap();
    };
    mode(&plugins, 0o555);
    mode(&packages, 0o555);
    let error = delete((&plugins, &packages), "hyscribe-test", &idle).unwrap_err();
    assert!(
        error.starts_with("could not fully delete hyscribe-test"),
        "{error}"
    );
    assert!(plugins.join("hyscribe-test/public/a.css").is_file() && package.is_file());
    mode(&plugins, 0o755);
    mode(&packages, 0o755);
    delete((&plugins, &packages), "hyscribe-test", &idle).unwrap();
    assert!(!package.exists());
}

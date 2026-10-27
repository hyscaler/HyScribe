// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use std::os::unix::fs::PermissionsExt;

struct Output {
    code: u8,
    out: String,
    err: String,
}

fn run_with(args: &[&str], env: &[(&str, &str)], home: &std::path::Path) -> Output {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run(Context {
        args: std::iter::once("hyscribe")
            .chain(args.iter().copied())
            .map(OsString::from)
            .collect(),
        env: env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        home: home.to_path_buf(),
        out: &mut out,
        err: &mut err,
    });
    Output {
        code,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

/// A writer that always fails, like a closed terminal.
struct Closed;

impl Write for Closed {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("closed"))
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn version_names_license_and_source() {
    let home = tempfile::tempdir().unwrap();
    let output = run_with(&["version"], &[], home.path());
    assert_eq!(output.code, 0);
    assert_eq!(
        output.out,
        format!(
            "hyscribe {}\nLicense: AGPL-3.0-or-later\nSource: {SOURCE_URL}\n",
            env!("CARGO_PKG_VERSION")
        )
    );
}

#[test]
fn usage_errors_go_to_stderr_and_help_to_stdout() {
    let home = tempfile::tempdir().unwrap();
    let output = run_with(&["frobnicate"], &[], home.path());
    assert_eq!(output.code, 2);
    assert!(output.err.contains("unrecognized subcommand"));
    let output = run_with(&["--help"], &[], home.path());
    assert_eq!(output.code, 0);
    assert!(output.out.contains("Usage: hyscribe"));
}

#[test]
fn config_check_without_config_reports_setup_mode() {
    let home = tempfile::tempdir().unwrap();
    let output = run_with(&["config", "check"], &[], home.path());
    assert_eq!(output.code, 0);
    assert_eq!(
        output.out,
        format!(
            "No configuration in {}: HyScribe will start in setup mode.\n",
            home.path().display()
        )
    );
}

#[test]
fn config_check_describes_without_secrets() {
    let home = tempfile::tempdir().unwrap();
    let env = [
        ("DB_NAME", "wordpress"),
        ("DB_USER", "wp"),
        ("DB_PASSWORD", "hunter2"),
        ("AUTH_KEY", "k3y"),
    ];
    let output = run_with(&["config", "check"], &env, home.path());
    assert_eq!(output.code, 0);
    assert_eq!(
        output.out,
        "Configuration OK\n\
         Database: wp@localhost:3306/wordpress (password set, prefix wp_, charset utf8mb4)\n\
         Listen: 127.0.0.1:8080\n\
         Mode: shadow (read only)\n\
         Uploads: hyscribe/uploads\n\
         Keys and salts: 1 of 8 set\n"
    );
    assert!(!output.out.contains("hunter2") && !output.out.contains("k3y"));
}

#[test]
fn config_check_reads_dir_option() {
    let home = tempfile::tempdir().unwrap();
    let site = tempfile::tempdir().unwrap();
    let path = site.path().join(".env");
    std::fs::write(
        &path,
        "DB_NAME=blog\nDB_USER=wp\nMODE=primary\nBEHIND_PROXY=true\nUPLOADS_DIR=/srv/uploads\n",
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let dir = site.path().to_str().unwrap();
    let output = run_with(&["config", "check", "--dir", dir], &[], home.path());
    assert_eq!(output.code, 0, "{}", output.err);
    assert!(output.out.contains("password not set"));
    assert!(
        output
            .out
            .contains("Listen: 127.0.0.1:8080 behind a proxy\n")
    );
    assert!(output.out.contains("Mode: primary\n"));
    assert!(output.out.contains("Uploads: /srv/uploads\n"));
}

#[test]
fn config_check_reports_errors() {
    let home = tempfile::tempdir().unwrap();
    let output = run_with(&["config", "check"], &[("DB_NAME", "x")], home.path());
    assert_eq!(output.code, 1);
    assert_eq!(output.err, "error: DB_USER is not set\n");
}

#[test]
fn closed_stdout_is_an_error() {
    let home = tempfile::tempdir().unwrap();
    for args in [&["version"][..], &["config", "check"][..]] {
        let mut err = Vec::new();
        let code = run(Context {
            args: std::iter::once("hyscribe")
                .chain(args.iter().copied())
                .map(OsString::from)
                .collect(),
            env: HashMap::new(),
            home: home.path().to_path_buf(),
            out: &mut Closed,
            err: &mut err,
        });
        assert_eq!(code, 1);
        assert_eq!(String::from_utf8(err).unwrap(), "error: closed\n");
    }
    let mut closed = Closed;
    assert!(closed.flush().is_ok());
}

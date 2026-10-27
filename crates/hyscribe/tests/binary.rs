// SPDX-License-Identifier: AGPL-3.0-or-later

//! The real binary, run as a process.

// Tests fail by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::process::Command;

/// The binary with an empty environment, except the variable coverage runs
/// use to collect profiles from child processes.
fn clean_hyscribe() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_hyscribe"));
    command.env_clear();
    if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
        command.env("LLVM_PROFILE_FILE", profile);
    }
    command
}

#[test]
fn version_runs_as_a_process() {
    let output = Command::new(env!("CARGO_BIN_EXE_hyscribe"))
        .arg("version")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("AGPL-3.0-or-later")
    );
}

#[test]
fn config_check_uses_the_environment() {
    let dir = tempfile::tempdir().unwrap();
    let output = clean_hyscribe()
        .args(["config", "check", "--dir"])
        .arg(dir.path())
        .env("DB_NAME", "wordpress")
        .env("DB_USER", "wp")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .starts_with("Configuration OK")
    );
}

#[test]
fn non_utf8_environment_is_ignored() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let dir = tempfile::tempdir().unwrap();
    let output = clean_hyscribe()
        .args(["config", "check", "--dir"])
        .arg(dir.path())
        .env("DB_NAME", OsStr::from_bytes(b"\xff"))
        .env(OsStr::from_bytes(b"\xfe"), "x")
        .output()
        .unwrap();
    // DB_NAME is dropped, so there is no configuration at all.
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("setup mode")
    );
}

#[test]
fn errors_set_the_exit_code() {
    let output = Command::new(env!("CARGO_BIN_EXE_hyscribe"))
        .arg("no-such-command")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}

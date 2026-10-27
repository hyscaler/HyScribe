// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use std::os::unix::fs::PermissionsExt;

/// Any unprivileged user id.
const USER: u32 = 1000;

struct Output {
    code: u8,
    out: String,
    err: String,
}

fn run_with(args: &[&str], env: &[(&str, &str)], home: &std::path::Path) -> Output {
    let env = env
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    run_as(
        args,
        env,
        home,
        USER,
        (PathBuf::from("/bin/false"), Vec::new()),
    )
}

fn run_as(
    args: &[&str],
    env: HashMap<String, String>,
    home: &std::path::Path,
    euid: u32,
    server: (PathBuf, Vec<OsString>),
) -> Output {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run(Context {
        args: std::iter::once("hyscribe")
            .chain(args.iter().copied())
            .map(OsString::from)
            .collect(),
        env,
        home: home.to_path_buf(),
        euid,
        server,
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
            euid: USER,
            server: (PathBuf::from("/bin/false"), Vec::new()),
            out: &mut Closed,
            err: &mut err,
        });
        assert_eq!(code, 1);
        assert_eq!(String::from_utf8(err).unwrap(), "error: closed\n");
    }
    let mut closed = Closed;
    assert!(closed.flush().is_ok());
}

const CHILD_HOME: &str = "HYSCRIBE_TEST_CHILD_HOME";

/// When started by [`child_server`], this test binary runs `hyscribe start`
/// in process. Run normally, it does nothing.
#[test]
fn child_server_main() {
    let Ok(home) = std::env::var(CHILD_HOME) else {
        return;
    };
    // Real standard output, which is the log, so lines appear while it runs.
    let code = run(Context {
        args: ["hyscribe", "start"].map(OsString::from).to_vec(),
        env: std::env::vars().collect(),
        home: PathBuf::from(home),
        euid: USER,
        server: (PathBuf::from("/bin/false"), Vec::new()),
        out: &mut std::io::stdout(),
        err: &mut std::io::stderr(),
    });
    assert_eq!(code, 0);
}

/// The server command `service start` runs in tests: this test binary.
fn child_server() -> (PathBuf, Vec<OsString>) {
    (
        std::env::current_exe().unwrap(),
        ["--exact", "tests::child_server_main", "--nocapture", "-q"]
            .map(OsString::from)
            .to_vec(),
    )
}

/// A site configured through the environment, for `home`.
fn site_env(home: &std::path::Path) -> HashMap<String, String> {
    let mut env: HashMap<String, String> = [
        ("DB_NAME", "wordpress"),
        ("DB_USER", "wp"),
        (CHILD_HOME, home.to_str().unwrap()),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    if let Ok(profile) = std::env::var("LLVM_PROFILE_FILE") {
        env.insert("LLVM_PROFILE_FILE".to_string(), profile);
    }
    env
}

fn service(args: &[&str], home: &std::path::Path) -> Output {
    let args: Vec<&str> = std::iter::once("service")
        .chain(args.iter().copied())
        .collect();
    run_as(&args, site_env(home), home, USER, child_server())
}

fn pid_of(text: &str) -> u32 {
    let start = text.find("(pid ").unwrap() + 5;
    let rest = text.get(start..).unwrap();
    rest.get(..rest.find(')').unwrap())
        .unwrap()
        .parse()
        .unwrap()
}

#[test]
fn service_lifecycle() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    let log = home.join("logs/hyscribe.log");

    let started = service(&["start"], home);
    assert_eq!(started.code, 0, "{}", started.err);
    let pid = pid_of(&started.out);
    assert_eq!(
        started.out,
        format!("started (pid {pid})\nlogs: {}\n", log.display())
    );
    let again = service(&["start"], home);
    assert!(
        again
            .out
            .starts_with(&format!("already running (pid {pid})\n"))
    );

    // The server's first line reaches the log once it is up.
    let running = format!(
        "hyscribe {} running (pid {pid}) in shadow (read only) mode, configured for http://127.0.0.1:8080",
        env!("CARGO_PKG_VERSION")
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !std::fs::read_to_string(&log).unwrap().contains(&running) {
        assert!(std::time::Instant::now() < deadline, "server never logged");
        std::thread::sleep(hyscribe_service::POLL);
    }
    let status = service(&["status"], home);
    assert_eq!(status.code, 0);
    assert!(
        status
            .out
            .starts_with(&format!("running (pid {pid})\nlast log line: "))
    );
    let logged = service(&["log", "-n", "50"], home);
    assert!(logged.out.contains(&running), "{}", logged.out);

    let follower = {
        let home = home.to_path_buf();
        std::thread::spawn(move || service(&["log", "--follow"], &home))
    };
    let stopped = service(&["stop"], home);
    assert_eq!(stopped.out, format!("stopped (pid {pid})\n"));
    let followed = follower.join().unwrap();
    assert_eq!(followed.code, 0, "{}", followed.err);
    assert!(
        followed.out.contains("stopped on signal 15"),
        "{}",
        followed.out
    );

    let restarted = service(&["restart"], home);
    assert_eq!(restarted.code, 0, "{}", restarted.err);
    assert!(restarted.out.starts_with("not running\nstarted (pid "));
    let new_pid = pid_of(restarted.out.strip_prefix("not running\n").unwrap());
    let stopped = service(&["stop"], home);
    assert_eq!(stopped.out, format!("stopped (pid {new_pid})\n"));

    let status = service(&["status"], home);
    assert_eq!(status.code, EXIT_NOT_RUNNING);
    // The last line is the test harness's summary; the real binary's is
    // "stopped on signal 15", checked in tests/binary.rs.
    assert!(status.out.starts_with("not running\nlast log line: "));
}

#[test]
fn servers_refuse_root() {
    let home = tempfile::tempdir().unwrap();
    // No configuration: if the root check were missing, these would fail
    // fast on that instead of starting a server.
    for args in [
        &["start"][..],
        &["service", "start"],
        &["service", "restart"],
    ] {
        let output = run_as(args, HashMap::new(), home.path(), 0, child_server());
        assert_eq!(output.code, 1);
        assert_eq!(
            output.err,
            "error: refusing to run as root; run HyScribe as an unprivileged user\n"
        );
    }
}

#[test]
fn servers_need_a_configuration() {
    let home = tempfile::tempdir().unwrap();
    let expected = format!("error: {NO_CONFIG}\n");
    let output = run_with(&["start"], &[], home.path());
    assert_eq!(output.err, expected);
    let output = run_with(&[], &[], home.path());
    assert_eq!(output.err, expected);
    let output = run_with(&["service", "start"], &[], home.path());
    assert_eq!(output.err, expected);
    // Restart stops (nothing runs), then cannot start.
    let output = run_with(&["service", "restart"], &[], home.path());
    assert_eq!(output.err, expected);
    // Configuration errors are reported before anything starts.
    let output = run_with(&["start"], &[("DB_NAME", "x")], home.path());
    assert_eq!(output.err, "error: DB_USER is not set\n");
}

#[test]
fn start_refuses_a_second_server() {
    let home = tempfile::tempdir().unwrap();
    let _lock = hyscribe_service::hold(&Paths::new(home.path())).unwrap();
    let output = run_as(
        &["start"],
        site_env(home.path()),
        home.path(),
        USER,
        child_server(),
    );
    assert_eq!(
        output.err,
        format!("error: already running (pid {})\n", std::process::id())
    );
}

#[test]
fn service_start_reports_a_failing_server() {
    let home = tempfile::tempdir().unwrap();
    let output = run_with(
        &["service", "start"],
        &[("DB_NAME", "x"), ("DB_USER", "y")],
        home.path(),
    );
    assert_eq!(output.code, 1);
    assert_eq!(
        output.err,
        "error: the server exited with code 1 while starting\n"
    );
}

#[test]
fn service_commands_report_unreadable_files() {
    let home = tempfile::tempdir().unwrap();
    let paths = Paths::new(home.path());
    std::fs::create_dir_all(paths.logs_dir()).unwrap();
    std::fs::write(paths.log_file(), "").unwrap();
    std::fs::set_permissions(paths.log_file(), std::fs::Permissions::from_mode(0o000)).unwrap();
    for args in [&["status"][..], &["log"]] {
        let output = service(args, home.path());
        assert!(
            output.err.starts_with("error: cannot read "),
            "{}",
            output.err
        );
    }

    std::fs::create_dir_all(paths.run_dir()).unwrap();
    std::fs::write(paths.pid_file(), "").unwrap();
    std::fs::set_permissions(paths.pid_file(), std::fs::Permissions::from_mode(0o000)).unwrap();
    for args in [&["status"][..], &["stop"], &["restart"]] {
        let output = service(args, home.path());
        assert!(
            output.err.starts_with("error: cannot open "),
            "{}",
            output.err
        );
    }
}

#[test]
fn log_follow_reports_problems() {
    let home = tempfile::tempdir().unwrap();
    let output = service(&["log", "-f"], home.path());
    assert!(
        output.err.starts_with("error: cannot open "),
        "{}",
        output.err
    );

    let paths = Paths::new(home.path());
    std::fs::create_dir_all(paths.logs_dir()).unwrap();
    std::fs::write(paths.log_file(), "a line\n").unwrap();
    let mut err = Vec::new();
    let code = run(Context {
        args: ["hyscribe", "service", "log", "-f"]
            .map(OsString::from)
            .to_vec(),
        env: HashMap::new(),
        home: home.path().to_path_buf(),
        euid: USER,
        server: child_server(),
        out: &mut Closed,
        err: &mut err,
    });
    assert_eq!(code, 1);
    assert_eq!(
        String::from_utf8(err).unwrap(),
        "error: cannot write the log: closed\n"
    );
}

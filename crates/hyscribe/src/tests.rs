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
        input: &mut std::io::empty(),
        terminal: None,
        screen: false,
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
fn the_proxies_are_trusted_only_behind_a_proxy() {
    let values = |extra: &[(&str, &str)]| {
        let mut map: std::collections::HashMap<String, String> =
            [("DB_NAME", "b"), ("DB_USER", "w")]
                .into_iter()
                .chain(extra.iter().copied())
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
        map.entry("PORT".into()).or_insert("8080".into());
        hyscribe_config::from_map(&map).unwrap()
    };
    assert_eq!(super::trusted_proxies(&values(&[])), None);
    let behind = values(&[("BEHIND_PROXY", "true"), ("TRUSTED_PROXIES", "10.0.0.0/8")]);
    let list = super::trusted_proxies(&behind).unwrap();
    assert_eq!(list, behind.trusted_proxies);
    assert_eq!(list.len(), 1);
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
            input: &mut std::io::empty(),
            terminal: None,
            screen: false,
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
        input: &mut std::io::empty(),
        terminal: None,
        screen: false,
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

/// The port the test site in `home` listens on: a free one, picked once.
fn site_port(home: &std::path::Path) -> u16 {
    let file = home.join("test-port");
    if let Ok(port) = std::fs::read_to_string(&file) {
        return port.parse().unwrap();
    }
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    std::fs::write(&file, port.to_string()).unwrap();
    port
}

/// The default install fixture as environment variables.
fn fixture_db() -> Vec<(&'static str, String)> {
    let db = hyscribe_testkit::fixture_database();
    vec![
        ("DB_HOST", db.host),
        ("DB_PORT", db.port.to_string()),
        ("DB_NAME", db.name),
        ("DB_USER", db.user),
        ("DB_PASSWORD", db.password.expose().to_string()),
    ]
}

/// A site configured through the environment, for `home`.
fn site_env(home: &std::path::Path) -> HashMap<String, String> {
    let mut env: HashMap<String, String> = fixture_db()
        .into_iter()
        .chain([
            ("PORT", site_port(home).to_string()),
            (CHILD_HOME, home.to_str().unwrap().to_string()),
        ])
        .map(|(k, v)| (k.to_string(), v))
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

    // `service start` returns once the server listens, its lines logged.
    let running = format!(
        "hyscribe {} running (pid {pid}) in shadow (read only) mode, listening on http://127.0.0.1:{}",
        env!("CARGO_PKG_VERSION"),
        site_port(home)
    );
    assert!(std::fs::read_to_string(&log).unwrap().contains(&running));
    // The default install is served from the fixture database.
    let page = http_get(site_port(home), "/?p=1");
    assert!(page.starts_with("HTTP/1.1 200 OK"), "{page}");
    assert!(page.contains("Hello world!"), "{page}");
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
        &["setup"],
    ] {
        let output = run_as(args, HashMap::new(), home.path(), 0, child_server());
        assert_eq!(output.code, 1);
        assert_eq!(
            output.err,
            "error: refusing to run as root; run HyScribe as an unprivileged user\n"
        );
    }
}

/// With no configuration the server starts in setup mode, on the address
/// the environment gives (the wizard itself is tested in `setup::web`).
#[test]
fn servers_without_a_configuration_start_in_setup_mode() {
    let home = tempfile::tempdir().unwrap();
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port().to_string();
    for args in [&["start"][..], &[]] {
        let output = run_with(args, &[("PORT", &port)], home.path());
        assert_eq!(output.code, 1);
        assert!(
            output
                .err
                .starts_with(&format!("error: cannot listen on 127.0.0.1:{port}: ")),
            "{}",
            output.err
        );
    }
    // Configuration errors are reported before anything starts.
    let output = run_with(&["start"], &[("DB_NAME", "x")], home.path());
    assert_eq!(output.err, "error: DB_USER is not set\n");
    let output = run_with(&["service", "restart"], &[("DB_NAME", "x")], home.path());
    assert_eq!(output.err, "error: DB_USER is not set\n");
}

/// An open pseudo terminal's answering end.
fn pty() -> std::fs::File {
    use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
    let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).unwrap();
    grantpt(&master).unwrap();
    unlockpt(&master).unwrap();
    let name = ptsname(&master, Vec::new()).unwrap();
    // The master stays open as long as the test runs.
    std::mem::forget(master);
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(name.to_str().unwrap())
        .unwrap()
}

/// At a terminal, a server without a configuration asks where to set up.
#[test]
fn a_terminal_is_asked_where_to_set_up() {
    use std::os::fd::AsFd;
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("hyscribe");
    let wordpress = dir.path().join("site");
    std::fs::create_dir(&home).unwrap();
    std::fs::create_dir_all(wordpress.join("wp-content/uploads")).unwrap();
    std::fs::write(wordpress.join("wp-config.php"), setup::tests::wp_config("")).unwrap();
    let env = HashMap::new();
    let server = child_server();
    let site = Site {
        paths: Paths::new(&home),
        env: &env,
        euid: USER,
        server: &server,
        screen: false,
    };
    let tty = pty();
    let ask = |answers: &str, tty: Option<std::os::fd::BorrowedFd<'_>>| {
        let mut out = Vec::new();
        let mut term = setup::Terminal {
            input: &mut std::io::Cursor::new(answers.as_bytes().to_vec()),
            out: &mut out,
            tty,
        };
        let config = server_config(&site, &mut term).map(|config| config.is_some());
        (config, String::from_utf8(out).unwrap())
    };
    let question =
        "? No .env found. Set up in the browser or in the terminal? [browser/terminal]: ";
    let (config, out) = ask("maybe\nbrowser\n", Some(tty.as_fd()));
    assert_eq!(config, Ok(false));
    assert_eq!(
        out,
        format!("{question}  Answer browser or terminal.\n{question}")
    );
    let (config, _) = ask("\n", Some(tty.as_fd()));
    assert_eq!(config, Ok(false));
    let (config, _) = ask("x\ny\nz\nterminal\n", Some(tty.as_fd()));
    assert_eq!(
        config,
        Err("setup stopped: neither browser nor terminal was chosen".into())
    );
    // Not a terminal: setup mode, without a question.
    let null = std::fs::File::open("/dev/null").unwrap();
    assert_eq!(
        ask("terminal\n", Some(null.as_fd())),
        (Ok(false), String::new())
    );
    assert_eq!(ask("terminal\n", None), (Ok(false), String::new()));
    // The answers can end, or the terminal close, on the way.
    let ended = Err("setup stopped: the input ended before every question was answered".into());
    assert_eq!(ask("", Some(tty.as_fd())).0, ended);
    assert_eq!(ask("terminal\n", Some(tty.as_fd())).0, ended);
    let mut term = setup::Terminal {
        input: &mut std::io::Cursor::new(b"maybe\n".to_vec()),
        out: &mut setup::tests::FailAfter(1),
        tty: Some(tty.as_fd()),
    };
    assert_eq!(server_config(&site, &mut term), Err("closed".into()));
    // An environment the wizard's .env does not override still counts.
    let bogus = HashMap::from([("MODE".to_string(), "bogus".to_string())]);
    let mut out = Vec::new();
    let answers = format!("terminal\n{}", setup::tests::DEFAULTS);
    let mut term = setup::Terminal {
        input: &mut std::io::Cursor::new(answers.into_bytes()),
        out: &mut out,
        tty: Some(tty.as_fd()),
    };
    let overridden = Site {
        paths: Paths::new(&home),
        env: &bogus,
        euid: USER,
        server: &server,
        screen: false,
    };
    assert_eq!(
        server_config(&overridden, &mut term),
        Err("MODE is invalid: must be shadow or primary".into())
    );
    std::fs::remove_file(home.join(".env")).unwrap();
    let answers = format!("Terminal\n{}", setup::tests::DEFAULTS);
    let (config, out) = ask(&answers, Some(tty.as_fd()));
    assert_eq!(config, Ok(true), "{out}");
    let wrote = format!("Wrote {} (mode 600)\n", home.join(".env").display());
    assert!(out.ends_with(&wrote), "{out}");
    // With .env, nothing is asked.
    assert_eq!(ask("", Some(tty.as_fd())), (Ok(true), String::new()));
}

/// The wizard reads standard input (its answers are tested in
/// `setup::tests`).
#[test]
fn setup_reads_standard_input() {
    let home = tempfile::tempdir().unwrap();
    let output = run_with(&["setup"], &[], home.path());
    assert_eq!(output.code, 1);
    assert!(output.out.starts_with("? Path to the WordPress folder"));
    assert_eq!(
        output.err,
        "error: setup stopped: the input ended before every question was answered\n"
    );
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
    // The real server's reason, from the log.
    let mut env = site_env(home.path());
    env.insert("DB_PORT".into(), "1".into());
    let output = run_as(
        &["service", "start"],
        env,
        home.path(),
        USER,
        child_server(),
    );
    assert_eq!(output.code, 1);
    assert!(
        output
            .err
            .starts_with("error: the server exited with code ")
            && output
                .err
                .contains("\n  error: cannot connect to the database: "),
        "{}",
        output.err
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
        input: &mut std::io::empty(),
        terminal: None,
        screen: false,
        out: &mut Closed,
        err: &mut err,
    });
    assert_eq!(code, 1);
    assert_eq!(
        String::from_utf8(err).unwrap(),
        "error: cannot write the log: closed\n"
    );
}

/// A plain HTTP/1.1 GET to the local server, returning the raw response.
fn http_get(port: u16, path: &str) -> String {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

#[test]
fn start_reports_what_stops_it_serving() {
    let start = |extra: &[(&str, &str)], home: &std::path::Path| {
        let mut env = site_env(home);
        env.extend(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())));
        run_as(&["start"], env, home, USER, child_server())
    };
    let home = tempfile::tempdir().unwrap();
    let output = start(&[("DB_PORT", "1")], home.path());
    assert!(
        output
            .err
            .starts_with("error: cannot connect to the database: "),
        "{}",
        output.err
    );
    // Reads as the read only user, writes as a main user it cannot be.
    let read_user = [
        ("MODE", "primary"),
        ("DB_READ_USER", "wordpress_read"),
        ("DB_READ_PASSWORD", "reference-only"),
        ("DB_PASSWORD", "wrong"),
    ];
    let output = start(&read_user, home.path());
    assert!(
        output.err.starts_with("error: cannot open the writer: "),
        "{}",
        output.err
    );
    let output = start(&[("DB_PREFIX", "missing_")], home.path());
    assert!(
        output
            .err
            .starts_with("error: cannot read the site options: "),
        "{}",
        output.err
    );
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port().to_string();
    let output = start(&[("PORT", &port)], home.path());
    assert!(
        output
            .err
            .starts_with(&format!("error: cannot listen on 127.0.0.1:{port}: ")),
        "{}",
        output.err
    );
}

/// Stops the service in its folder when dropped, so a failing test leaves
/// no server writing to the scratch database.
struct Stopping<'a>(&'a std::path::Path);

impl Drop for Stopping<'_> {
    fn drop(&mut self) {
        service(&["stop"], self.0);
    }
}

/// In primary mode the login screen lets people log in: it has no read
/// only notice. The cron task runs the default install's due events.
#[test]
fn primary_mode_serves_a_login_screen() {
    let _saves = SAVES.lock().unwrap_or_else(|e| e.into_inner());
    saves_with(&[]);
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    let mut env = site_env(home);
    env.insert("MODE".into(), "primary".into());
    env.insert("DB_NAME".into(), "wordpress_saves".into());
    let run = |command: &str| {
        run_as(
            &["service", command],
            env.clone(),
            home,
            USER,
            child_server(),
        )
    };
    let started = run("start");
    let _stop = Stopping(home);
    assert_eq!(started.code, 0, "{}", started.err);
    let log = home.join("logs/hyscribe.log");
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !std::fs::read_to_string(&log)
        .unwrap_or_default()
        .contains("cron: wp_privacy_personal_data_cleanup_requests ran")
    {
        assert!(std::time::Instant::now() < deadline, "server never logged");
        std::thread::sleep(hyscribe_service::POLL);
    }
    let page = http_get(site_port(home), "/hs-login");
    assert!(page.starts_with("HTTP/1.1 200 OK"), "{page}");
    assert!(!page.contains("read only"), "{page}");
    assert_eq!(run("stop").code, 0);
}

/// Primary mode opens the writer and logs what it refuses; shadow mode
/// opens none.
#[test]
fn the_writer_s_refusals_are_logged() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let database = hyscribe_testkit::fixture_database();
    let mut out = Vec::new();
    runtime.block_on(async {
        let shadow = open_writer(&database, Mode::Shadow, &mut out).await;
        assert!(shadow.unwrap().is_none());
        let primary = open_writer(&database, Mode::Primary, &mut out).await;
        assert!(primary.unwrap().is_some());
        assert!(out.is_empty());
        let missing = hyscribe_config::Database {
            prefix: hyscribe_config::Prefix::new("missing_").unwrap(),
            ..database.clone()
        };
        assert!(open_writer(&missing, Mode::Primary, &mut out).await.is_ok());
        let unreachable = hyscribe_config::Database {
            port: 1,
            ..database.clone()
        };
        let err = open_writer(&unreachable, Mode::Primary, &mut out).await;
        assert!(err.err().unwrap().starts_with("cannot open the writer: "));
    });
    let out = String::from_utf8(out).unwrap();
    assert!(out.starts_with("writes refused: "), "{out}");
    assert!(out.contains("missing_options"), "{out}");
    assert_eq!(out.lines().count(), 1, "{out}");
}

/// `wordpress_saves`, the scratch database, is changed one test at a time.
pub(crate) static SAVES: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Loads the default install into `wordpress_saves` and runs `statements`.
pub(crate) fn saves_with(statements: &[String]) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        use sqlx::Connection as _;
        let db = hyscribe_config::Database {
            name: "wordpress_saves".into(),
            ..hyscribe_testkit::fixture_database()
        };
        hyscribe_testkit::writes::load_default_install(&db)
            .await
            .unwrap();
        let options = sqlx::mysql::MySqlConnectOptions::new()
            .host(&db.host)
            .port(db.port)
            .database(&db.name)
            .username(&db.user)
            .password(db.password.expose());
        let mut conn = sqlx::MySqlConnection::connect_with(&options).await.unwrap();
        for statement in statements {
            sqlx::raw_sql(sqlx::AssertSqlSafe(statement.clone()))
                .execute(&mut conn)
                .await
                .unwrap();
        }
    });
}

/// `hyscribe cron run` runs the due events once in primary mode, with the
/// configuration's checks.
#[test]
fn cron_run_runs_the_due_events_once() {
    let _saves = SAVES.lock().unwrap_or_else(|e| e.into_inner());
    saves_with(&[]);
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    let cron = |extra: &[(&str, &str)]| {
        let mut env = site_env(home);
        env.insert("DB_NAME".into(), "wordpress_saves".into());
        env.extend(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())));
        run_as(&["cron", "run"], env, home, USER, child_server())
    };
    let shadow = cron(&[]);
    assert_eq!(
        shadow.err,
        "error: cron: off in shadow mode, which writes nothing\n"
    );
    assert_eq!(shadow.code, 1);
    let primary = [
        ("MODE", "primary"),
        ("DISABLE_WP_CRON", "true"),
        ("ALTERNATE_WP_CRON", "1"),
    ];
    let ran = cron(&primary);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(
        ran.out,
        "cron: ALTERNATE_WP_CRON is ignored; HyScribe makes no loopback request\n\
         cron: wp_privacy_delete_old_export_files ran\n\
         cron: wp_privacy_personal_data_cleanup_requests ran\n"
    );
    assert_eq!(cron(&primary[..1]).out, "");
    let unreachable = cron(&[("MODE", "primary"), ("DB_PORT", "1")]);
    assert!(
        unreachable
            .err
            .starts_with("error: cannot open the writer: "),
        "{}",
        unreachable.err
    );
    let root = run_as(&["cron", "run"], site_env(home), home, 0, child_server());
    assert!(
        root.err.starts_with("error: refusing to run as root"),
        "{}",
        root.err
    );
    let site = tempfile::tempdir().unwrap();
    std::fs::write(site.path().join("object-cache.php"), "<?php").unwrap();
    let uploads = site.path().join("uploads");
    let cached = cron(&[
        ("MODE", "primary"),
        ("UPLOADS_DIR", uploads.to_str().unwrap()),
    ]);
    assert!(
        cached.err.starts_with("error: cron: off, since "),
        "{}",
        cached.err
    );
    let none = run_with(&["cron", "run"], &[], home);
    assert_eq!(none.err, "error: no configuration: run ./hyscribe setup\n");
    let invalid = run_with(&["cron", "run"], &[("DB_NAME", "x")], home);
    assert_eq!(invalid.err, "error: DB_USER is not set\n");
}

#[test]
fn the_start_log_says_whether_fetches_are_on() {
    assert_eq!(outbound_line(None), "outbound fetches off");
    assert_eq!(outbound_line(Some(&outbound())), "outbound fetches on");
    let net = std::sync::Arc::new(hyscribe_fetch::System);
    let bare = hyscribe_fetch::Client::new(
        hyscribe_fetch::RootCertStore::empty(),
        net,
        std::sync::Arc::new(()),
    );
    assert!(outbound_line(Some(&bare)).ends_with("no certificate store was found"));
}

/// A raw HTTP/1.1 request to the local server, returning the raw response.
fn http(port: u16, request: &str) -> String {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

/// `method` of `path` with `cookie` and a form body.
fn http_form(port: u16, method: &str, path: &str, cookie: &str, form: &str) -> String {
    http(
        port,
        &format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\
             Cookie: {cookie}\r\nContent-Type: application/x-www-form-urlencoded\r\n\
             Content-Length: {}\r\n\r\n{form}",
            form.len()
        ),
    )
}

/// The text after the last `start` in `text`, up to `end`.
fn between<'a>(text: &'a str, start: &str, end: char) -> &'a str {
    let at = text.rfind(start).unwrap() + start.len();
    let rest = text.get(at..).unwrap();
    rest.get(..rest.find(end).unwrap()).unwrap()
}

/// Waits until the log in `home` holds `text`, and returns the log.
fn logged(home: &std::path::Path, text: &str) -> String {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let log = std::fs::read_to_string(home.join("logs/hyscribe.log")).unwrap_or_default();
        if log.contains(text) {
            return log;
        }
        assert!(std::time::Instant::now() < deadline, "never logged {text}");
        std::thread::sleep(hyscribe_service::POLL);
    }
}

/// `service start` without `.env` serves the browser wizard, which writes
/// it; the same process then serves the site.
#[test]
fn setup_mode_hands_over_to_the_site() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("hyscribe");
    let wordpress = dir.path().join("site");
    std::fs::create_dir(&home).unwrap();
    std::fs::create_dir_all(wordpress.join("wp-content/uploads")).unwrap();
    let wp_config = setup::tests::wp_config("");
    std::fs::write(wordpress.join("wp-config.php"), wp_config).unwrap();
    let port = site_port(&home);
    let env: HashMap<String, String> = site_env(&home)
        .into_iter()
        .filter(|(key, _)| !key.starts_with("DB_"))
        .collect();
    let service = |command: &str, env: &HashMap<String, String>| {
        let args = ["service", command];
        run_as(&args, env.clone(), &home, USER, child_server())
    };
    let secret_file = home.join("logs/setup-secret");
    let setup = format!(
        "Setup mode: the address to open and the one-time setup secret are in {} until setup \
         mode ends\n",
        secret_file.display()
    );
    let setup = setup.as_str();
    let fresh = || std::fs::remove_file(home.join("logs/hyscribe.log")).unwrap();
    // Written, `.env` still loses to an environment it does not override.
    let mut bogus = env.clone();
    bogus.insert("MODE".into(), "bogus".into());
    let started = service("start", &bogus);
    assert!(started.out.ends_with(setup), "{}", started.err);
    through_the_wizard(&home, port, &wordpress);
    logged(&home, "error: MODE is invalid: must be shadow or primary");
    std::fs::remove_file(home.join(".env")).unwrap();
    fresh();
    assert!(!secret_file.exists());
    // Stopped in setup mode, as any server is. A terminal is shown the
    // secret, which is in the file until the stop and never in the log. A
    // file a killed server left does not stop it.
    std::fs::write(&secret_file, "stale\n").unwrap();
    let mut out = Vec::new();
    let code = run(Context {
        args: ["hyscribe", "service", "start"]
            .map(OsString::from)
            .to_vec(),
        env: env.clone(),
        home: home.clone(),
        euid: USER,
        server: child_server(),
        input: &mut std::io::empty(),
        terminal: None,
        screen: true,
        out: &mut out,
        err: &mut std::io::sink(),
    });
    assert_eq!(code, 0);
    let out = String::from_utf8(out).unwrap();
    let open = std::fs::read_to_string(&secret_file).unwrap();
    assert!(out.ends_with(&format!("Setup mode: {open}")), "{out}");
    let secret = between(&open, "setup secret ", '\n');
    assert_eq!(service("stop", &env).code, 0);
    let log = logged(&home, "stopped on signal 15");
    assert!(log.contains("setup secret in "), "{log}");
    assert!(!log.contains(secret), "{log}");
    assert!(!secret_file.exists());
    fresh();

    let started = service("start", &env);
    assert!(started.out.ends_with(setup), "{}", started.err);
    through_the_wizard(&home, port, &wordpress);
    logged(
        &home,
        &format!("in shadow (read only) mode, listening on http://127.0.0.1:{port}"),
    );
    assert!(!secret_file.exists());
    let page = http_get(port, "/?p=1");
    assert!(page.contains("Hello world!"), "{page}");
    assert_eq!(service("stop", &env).code, 0);
}

/// Signs in to setup mode's pages in `home`, listening on `port`, with the
/// secret it gave: the session cookie and the token of the forms after it.
fn wizard_session(home: &std::path::Path, port: u16) -> (String, String) {
    let open = std::fs::read_to_string(home.join("logs/setup-secret")).unwrap();
    let secret = open
        .strip_prefix(&format!(
            "Open http://127.0.0.1:{port}/installation/step-1 and enter the one-time setup secret "
        ))
        .unwrap()
        .trim_end();
    let page = http_get(port, "/installation/step-1");
    assert!(page.starts_with("HTTP/1.1 200 OK"), "{page}");
    let token = between(&page, "name=\"_csrf\" value=\"", '"');
    let form = format!("hyscribe_setup_form={token}");
    let sent = format!("_csrf={token}&secret={secret}");
    let page = http_form(port, "POST", "/installation/step-1", &form, &sent);
    assert!(page.starts_with("HTTP/1.1 303 See Other"), "{page}");
    let session = format!("hyscribe_setup={}", between(&page, "hyscribe_setup=", ';'));
    let page = http_form(port, "GET", "/installation/step-2", &session, "");
    let csrf = between(&page, "name=\"_csrf\" value=\"", '"').to_string();
    (session, csrf)
}

/// Answers setup mode's pages in `home`, listening on `port`, for the site
/// in `wordpress`.
fn through_the_wizard(home: &std::path::Path, port: u16, wordpress: &std::path::Path) {
    let (session, csrf) = wizard_session(home, port);
    let sent = format!("_csrf={csrf}&path={}", wordpress.display());
    let page = http_form(port, "POST", "/installation/step-2", &session, &sent);
    assert!(page.contains("location: /installation/step-3"), "{page}");
    let sent = format!("_csrf={csrf}&MODE=shadow&PORT={port}");
    let page = http_form(port, "POST", "/installation/step-3", &session, &sent);
    assert!(page.contains("<h1>HyScribe is set up</h1>"), "{page}");
}

/// Drops every table of `wordpress_saves`, the scratch database.
pub(crate) fn empty_saves() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        use sqlx::{Connection as _, Row as _};
        let mut conn = saves_connection().await;
        let sql = "SELECT TABLE_NAME AS name FROM information_schema.TABLES \
                   WHERE TABLE_SCHEMA = DATABASE()";
        for row in sqlx::raw_sql(sql).fetch_all(&mut conn).await.unwrap() {
            let name = String::from_utf8(row.get::<Vec<u8>, _>("name")).unwrap();
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP TABLE {name}")))
                .execute(&mut conn)
                .await
                .unwrap();
        }
        conn.close().await.unwrap();
    });
}

async fn saves_connection() -> sqlx::MySqlConnection {
    use sqlx::Connection as _;
    let db = hyscribe_testkit::fixture_database();
    let options = sqlx::mysql::MySqlConnectOptions::new()
        .host(&db.host)
        .port(db.port)
        .database("wordpress_saves")
        .username(&db.user)
        .password(db.password.expose())
        .charset("utf8mb4");
    let mut conn = sqlx::MySqlConnection::connect_with(&options).await.unwrap();
    // A table a test left locked fails the test, not waits for ever.
    let wait = sqlx::raw_sql("SET SESSION lock_wait_timeout = 30");
    wait.execute(&mut conn).await.unwrap();
    conn
}

/// The first column of the first row `sql` gives over `wordpress_saves`.
pub(crate) async fn saves_value(sql: &str) -> Option<String> {
    use sqlx::Row as _;
    let mut conn = saves_connection().await;
    let rows = sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_all(&mut conn)
        .await
        .unwrap();
    let bytes = rows.first()?.try_get_unchecked::<Option<Vec<u8>>, _>(0);
    bytes
        .unwrap()
        .map(|bytes| String::from_utf8(bytes).unwrap())
}

/// [`saves_value`] outside a runtime.
pub(crate) fn saves_scalar(sql: &str) -> Option<String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(saves_value(sql))
}

/// GOALS.md S4: the wizard goes from an empty database to a running site in
/// under five minutes. Setup mode installs a new site through its pages, and
/// the same process then answers its home page with 200.
#[test]
fn the_new_site_wizard_reaches_a_home_page_within_s4_s_time() {
    let _saves = SAVES.lock().unwrap_or_else(|e| e.into_inner());
    empty_saves();
    let started = std::time::Instant::now();
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("hyscribe");
    std::fs::create_dir(&home).unwrap();
    let port = site_port(&home);
    let env: HashMap<String, String> = site_env(&home)
        .into_iter()
        .filter(|(key, _)| !key.starts_with("DB_"))
        .collect();
    let service = |command: &str| {
        let args = ["service", command];
        run_as(&args, env.clone(), &home, USER, child_server())
    };
    let begun = service("start");
    assert_eq!(begun.code, 0, "{}", begun.err);
    let (session, csrf) = wizard_session(&home, port);
    let page = http_form(
        port,
        "POST",
        "/installation/step-2",
        &session,
        &format!("_csrf={csrf}&new=1"),
    );
    assert!(page.contains("location: /installation/step-3"), "{page}");
    let page = http_form(port, "GET", "/installation/step-3", &session, "");
    assert!(page.contains("name=\"weblog_title\""), "{page}");
    assert!(!page.contains("Shadow mode"), "{page}");
    let db = hyscribe_testkit::fixture_database();
    let fields = [
        ("_csrf", csrf.as_str()),
        ("DB_NAME", "wordpress_saves"),
        ("DB_USER", db.user.as_str()),
        ("DB_PASSWORD", db.password.expose()),
        ("DB_HOST", db.host.as_str()),
        ("DB_PORT", &db.port.to_string()),
        ("weblog_title", "A New Site"),
        ("user_name", "owner"),
        ("admin_password", "correct horse battery"),
        ("admin_password2", "correct horse battery"),
        ("admin_email", "owner@example.org"),
        ("site_address", &format!("http://127.0.0.1:{port}")),
        ("PORT", &port.to_string()),
    ];
    let sent: Vec<String> = fields
        .iter()
        .map(|(name, value)| {
            let value = hyscribe_phpser::urlencode(value.as_bytes());
            format!("{name}={}", String::from_utf8(value).unwrap())
        })
        .collect();
    let page = http_form(
        port,
        "POST",
        "/installation/step-3",
        &session,
        &sent.join("&"),
    );
    assert!(page.contains("<h1>HyScribe is set up</h1>"), "{page}");
    assert!(
        page.contains("tables and first content in wordpress_saves"),
        "{page}"
    );
    assert!(!page.contains("shown once"), "{page}");
    logged(
        &home,
        &format!("in primary mode, listening on http://127.0.0.1:{port}"),
    );
    let page = http_get(port, "/");
    assert!(page.starts_with("HTTP/1.1 200 OK"), "{page}");
    assert!(page.contains("A New Site"), "{page}");
    assert!(page.contains("Hello world!"), "{page}");
    // The first post is at its pretty URL, which answers too.
    let prefix = format!("href=\"http://127.0.0.1:{port}/");
    let link = page
        .split(&prefix)
        .skip(1)
        .find_map(|rest| {
            rest.split('"')
                .next()
                .filter(|p| p.ends_with("/hello-world/"))
        })
        .unwrap_or_else(|| panic!("no pretty link to the first post in {page}"));
    let parts: Vec<&str> = link.trim_end_matches('/').split('/').collect();
    let digits = |part: &str, n: usize| part.len() == n && part.bytes().all(|b| b.is_ascii_digit());
    assert!(
        matches!(parts[..], [y, m, d, "hello-world"] if digits(y, 4) && digits(m, 2) && digits(d, 2)),
        "{link}"
    );
    let post = http_get(port, &format!("/{link}"));
    assert!(post.starts_with("HTTP/1.1 200 OK"), "{post}");
    assert!(post.contains("Welcome to WordPress."), "{post}");
    let took = started.elapsed();
    assert!(took < Duration::from_secs(5 * 60), "{took:?}");
    assert_eq!(service("stop").code, 0);
    empty_saves();
}

/// Uploads go to `UPLOADS_DIR`, else to `uploads/` in the installation's folder.
#[test]
fn uploads_go_to_their_folder() {
    let keys = |extra: &[(&str, &str)]| {
        let mut keys = vec![("DB_NAME", "wordpress"), ("DB_USER", "wp")];
        keys.extend_from_slice(extra);
        let keys = keys.iter().map(|(k, v)| (k.to_string(), v.to_string()));
        hyscribe_config::from_map(&keys.collect()).unwrap()
    };
    let home = std::path::Path::new("/opt/hyscribe");
    let new = uploads_dir(&keys(&[]), home);
    assert_eq!(new, home.join("uploads"));
    let existing = uploads_dir(&keys(&[("UPLOADS_DIR", "/srv/site/uploads")]), home);
    assert_eq!(existing, std::path::Path::new("/srv/site/uploads"));
}

/// The test extension, built from source as `hyscribe-ext`'s tests build it.
#[allow(dead_code, reason = "only its component is used here")]
#[path = "../../hyscribe-ext/tests/guest/mod.rs"]
mod guest;

/// `plugin install` unpacks a local package into `plugins/`, and refuses
/// root, a file it cannot read and a package it does not take.
#[test]
fn plugin_install() {
    let home = tempfile::tempdir().unwrap();
    let manifest = include_bytes!("../../hyscribe-ext/guest/hyscribe.ext.toml");
    let package = hyscribe_testkit::hyx::zip(&[
        ("hyscribe.ext.toml", manifest, true),
        ("extension.wasm", guest::component(), true),
    ]);
    let file = home.path().join("hyscribe-test-0.1.0.hyx");
    std::fs::write(&file, package).unwrap();
    let path = file.to_str().unwrap();
    let unsigned = run_with(&["plugin", "install", path], &[], home.path());
    assert_eq!(unsigned.code, 1);
    let why = format!(
        "error: no signature {path}.minisig beside it; a package without a signature this site trusts installs only with --unsigned\n"
    );
    assert_eq!(unsigned.err, why);
    let output = run_with(&["plugin", "install", "--unsigned", path], &[], home.path());
    assert_eq!((output.code, output.err.as_str()), (0, ""));
    assert_eq!(
        output.out,
        format!(
            "Installed HyScribe Test 0.1.0 into plugins/hyscribe-test/, standing in for hyscribe-test/hyscribe-test.php, unsigned (no signature {path}.minisig beside it). Activate it from the Plugins screen.\n"
        )
    );
    assert!(
        home.path()
            .join("plugins/hyscribe-test/extension.wasm")
            .is_file()
    );
    assert!(
        home.path()
            .join("packages/hyscribe-test-0.1.0.hyx")
            .is_file()
    );
    let root = run_as(
        &["plugin", "install", path],
        HashMap::new(),
        home.path(),
        0,
        child_server(),
    );
    assert_eq!(root.code, 1);
    assert!(root.err.contains("refusing to run as root"));
    let gone = home.path().join("gone.hyx");
    let missing = run_with(
        &["plugin", "install", gone.to_str().unwrap()],
        &[],
        home.path(),
    );
    assert_eq!(missing.code, 1);
    assert!(
        missing.err.starts_with("error: cannot read "),
        "{}",
        missing.err
    );
    // One byte more than a package may hold is read, and refused.
    std::fs::File::create(&file)
        .unwrap()
        .set_len(hyscribe_ext::hyx::MAX_TOTAL + 1)
        .unwrap();
    let large = run_with(&["plugin", "install", "--unsigned", path], &[], home.path());
    assert!(large.err.contains("a package is at most"), "{}", large.err);
    std::fs::write(&file, "not a package").unwrap();
    let refused = run_with(&["plugin", "install", "--unsigned", path], &[], home.path());
    assert_eq!(
        (refused.code, refused.err.as_str()),
        (1, "error: not a zip archive\n")
    );
}

/// `plugin install` takes a package whose signature beside it verifies
/// with a key `EXTENSION_TRUSTED_KEYS` lists, or with a key of the kept
/// registry key list, applying the kept index's revocations; anything else
/// needs `--unsigned` (ADR 0024).
#[test]
fn plugin_install_signatures() {
    use hyscribe_testkit::minisign::{Key, key_list, sign_keys};
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    let manifest = include_bytes!("../../hyscribe-ext/guest/hyscribe.ext.toml");
    let package = hyscribe_testkit::hyx::zip(&[
        ("hyscribe.ext.toml", manifest, true),
        ("extension.wasm", guest::component(), true),
    ]);
    let sha = hyscribe_ext::signature::sha256(&package);
    let file = home.join("hyscribe-test-0.1.0.hyx");
    std::fs::write(&file, &package).unwrap();
    let path = file.to_str().unwrap();
    let minisig = home.join("hyscribe-test-0.1.0.hyx.minisig");
    let claim = format!(
        "hyscribe-package id=hyscribe-test version=0.1.0 sdk=0.3 sha256={sha} signed=1800000000"
    );
    let sign = |seed: u8, comment: &str| {
        std::fs::write(&minisig, Key(seed).sign(&package, comment)).unwrap();
    };
    let install = |env: &[(&str, &str)], unsigned: bool| {
        let args: &[&str] = if unsigned {
            &["plugin", "install", "--unsigned", path]
        } else {
            &["plugin", "install", path]
        };
        let output = run_with(args, env, home);
        if output.code == 0 {
            output.out
        } else {
            output.err
        }
    };
    let origin = || std::fs::read_to_string(home.join("plugins/hyscribe-test/origin.txt")).unwrap();
    let tail = "; a package without a signature this site trusts installs only with --unsigned\n";
    // An operator's key.
    sign(7, &claim);
    let keys = format!(" , {}, {}", Key(5).public(), Key(7).public());
    let site = |keys: &'static str| {
        [
            ("DB_NAME", "x"),
            ("DB_USER", "x"),
            ("EXTENSION_TRUSTED_KEYS", keys),
        ]
    };
    let keys: &'static str = Box::leak(keys.into_boxed_str());
    let trusted = install(&site(keys), false);
    let by = format!(
        "signed by key {}, which EXTENSION_TRUSTED_KEYS trusts",
        Key(7).id()
    );
    assert!(trusted.contains(&by), "{trusted}");
    assert_eq!(origin(), format!("trusted {} {sha}\n", Key(7).id()));
    let bad = install(&site("RWQx"), false);
    assert_eq!(
        bad,
        format!("error: EXTENSION_TRUSTED_KEYS: RWQx is not a minisign public key{tail}")
    );
    let other = claim.replace(&sha, &"0".repeat(64));
    sign(7, &other);
    let moved = install(&site(keys), false);
    assert_eq!(
        moved,
        format!("error: its signature is of another package{tail}")
    );
    let anyway = install(&site(keys), true);
    let broken = install(&[("DB_NAME", "x")], false);
    assert_eq!(broken, format!("error: DB_USER is not set{tail}"));
    assert!(
        anyway.contains("unsigned (its signature is of another package)"),
        "{anyway}"
    );
    assert_eq!(origin(), format!("unsigned {sha}\n"));
    // A key of the registry's key list, kept in registry/.
    sign(10, &claim);
    let none = install(&[], false);
    assert!(
        none.starts_with("error: no key list is kept in registry/"),
        "{none}"
    );
    let now = crate::signed::now();
    let registry = home.join("registry");
    std::fs::create_dir(&registry).unwrap();
    let online = [
        (
            &Key(10),
            r#""packages""#,
            1_800_000_000 - 100,
            1_800_000_000 + 100,
        ),
        (&Key(11), r#""index""#, now - 1_000, now + 1_000),
    ];
    let list = key_list(1, now + 1_000, &online, &[]);
    std::fs::write(registry.join("keys.json"), &list).unwrap();
    for (n, signature) in sign_keys(&list, 1, now + 1_000, &[&Key(1), &Key(3)])
        .iter()
        .enumerate()
    {
        std::fs::write(
            registry.join(format!("keys.json.{}.minisig", n + 1)),
            signature,
        )
        .unwrap();
    }
    let signed = install(&[], false);
    let by = format!("signed by the registry's key {}", Key(10).id());
    assert!(signed.contains(&by), "{signed}");
    assert_eq!(origin(), format!("signed {} {sha}\n", Key(10).id()));
    sign(10, &claim.replace("signed=1800000000", "signed=1800000101"));
    let late = install(&[], false);
    let window = format!(
        "error: key {} could not sign at 1800000101{tail}",
        Key(10).id()
    );
    assert_eq!(late, window);
    sign(10, "timestamp:1");
    let unnamed = install(&[], false);
    assert_eq!(
        unnamed,
        format!("error: its signature's trusted comment names no package{tail}")
    );
    // The index revokes the version: the installed plugin, and the install.
    let index = format!(
        r#"{{"serial":1,"expires":{},"extensions":[],"revoked":[{{"id":"hyscribe-test","version":"0.1.0","sha256":"{sha}","reason":"CVE-1"}}]}}"#,
        now + 1_000
    );
    std::fs::write(registry.join("index.json"), &index).unwrap();
    let comment = format!("hyscribe-index serial=1 expires={}", now + 1_000);
    std::fs::write(registry.join("index.json.minisig"), "x").unwrap();
    sign(10, &claim);
    let bad_index = install(&[], false);
    let expected = "error: index.json: a signature file has four lines";
    assert!(bad_index.starts_with(expected), "{bad_index}");
    std::fs::write(
        registry.join("index.json.minisig"),
        Key(11).sign(index.as_bytes(), &comment),
    )
    .unwrap();
    let plugin = home.join("plugins/hyscribe-test");
    let mode = |mode| {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&plugin, std::fs::Permissions::from_mode(mode)).unwrap();
    };
    mode(0o555);
    let unmarked = install(&[], false);
    mode(0o755);
    assert!(
        unmarked.starts_with("error: cannot mark hyscribe-test revoked: "),
        "{unmarked}"
    );
    let revoked = install(&[], false);
    assert_eq!(
        revoked,
        format!("error: hyscribe-test 0.1.0 is revoked: CVE-1{tail}")
    );
    let marked = origin();
    assert!(marked.starts_with(&format!("signed {} {sha}\nrevoked ", Key(10).id())));
    assert!(marked.ends_with(" CVE-1\n"), "{marked}");
    // A key list that does not verify, and signature files that do not read.
    std::fs::write(registry.join("keys.json.1.minisig"), "x").unwrap();
    let unsigned_list = install(&[], false);
    let expected = format!("error: keys.json is not signed by two of the root keys{tail}");
    assert_eq!(unsigned_list, expected);
    std::fs::write(&minisig, "x").unwrap();
    let unread = install(&[], false);
    assert_eq!(
        unread,
        format!("error: {path}.minisig: a signature file has four lines{tail}")
    );
    std::fs::remove_file(&minisig).unwrap();
    std::fs::create_dir(&minisig).unwrap();
    let folder = install(&[], false);
    assert!(
        folder.starts_with(&format!("error: cannot read {path}.minisig: ")),
        "{folder}"
    );
}

/// `plugin delete` removes an installed plugin that `active_plugins` does
/// not name, and otherwise changes nothing.
#[test]
fn plugin_delete() {
    let _saves = SAVES.lock().unwrap_or_else(|e| e.into_inner());
    let active = |value: &str| {
        let value = value.replace('\'', "''");
        saves_with(&[format!(
            "UPDATE wp_options SET option_value = '{value}' WHERE option_name = 'active_plugins'"
        )]);
    };
    active("a:0:{}");
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    let manifest = include_bytes!("../../hyscribe-ext/guest/hyscribe.ext.toml");
    let package = hyscribe_testkit::hyx::zip(&[
        ("hyscribe.ext.toml", manifest, true),
        ("extension.wasm", guest::component(), true),
    ]);
    let file = home.join("hyscribe-test-0.1.0.hyx");
    std::fs::write(&file, package).unwrap();
    let path = file.to_str().unwrap();
    let delete = |id: &str, extra: &[(&str, &str)]| {
        let mut env = site_env(home);
        env.insert("DB_NAME".into(), "wordpress_saves".into());
        env.extend(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())));
        run_as(&["plugin", "delete", id], env, home, USER, child_server())
    };
    let install = || {
        let output = run_with(&["plugin", "install", "--unsigned", path], &[], home);
        assert_eq!(output.code, 0);
    };
    let folder = home.join("plugins/hyscribe-test");
    let kept = home.join("packages/hyscribe-test-0.1.0.hyx");
    install();
    let untouched = |why: &str| {
        assert!(folder.is_dir() && kept.is_file(), "{why}");
    };
    let refused = |id: &str, extra: &[(&str, &str)], message: &str| {
        let output = delete(id, extra);
        assert_eq!(output.code, 1, "{message}");
        assert!(output.err.contains(message), "{}", output.err);
    };
    let root = run_as(
        &["plugin", "delete", "hyscribe-test"],
        site_env(home),
        home,
        0,
        child_server(),
    );
    assert!(root.err.contains("refusing to run as root"), "{}", root.err);
    refused("../x", &[], "is not a plugin id");
    refused("other", &[], "other is not installed");
    untouched("refusals before the database");
    let none = run_with(&["plugin", "delete", "hyscribe-test"], &[], home);
    assert_eq!(none.err, "error: no configuration: run ./hyscribe setup\n");
    refused(
        "hyscribe-test",
        &[("DB_PORT", "1")],
        "cannot read active_plugins",
    );
    untouched("no configuration, no database");
    active("a:1:{i:0;s:31:\"hyscribe-test/hyscribe-test.php\";}");
    refused("hyscribe-test", &[], "is active: deactivate it");
    untouched("active");
    active("not a list");
    refused("hyscribe-test", &[], "active_plugins is not a list");
    untouched("active_plugins unreadable");
    active("a:2:{i:0;i:5;i:1;s:11:\"other/x.php\";}");
    let done = delete("hyscribe-test", &[]);
    assert_eq!((done.code, done.err.as_str()), (0, ""));
    assert_eq!(
        done.out,
        "Deleted plugins/hyscribe-test/ and 1 kept package(s).\n"
    );
    assert!(!folder.exists() && !kept.exists());
    // An option that is not there names no plugin.
    saves_with(&["DELETE FROM wp_options WHERE option_name = 'active_plugins'".to_string()]);
    install();
    assert_eq!(delete("hyscribe-test", &[]).code, 0);
    assert!(!folder.exists() && !kept.exists());
}

/// `plugin install <name>` and `plugin update` need `EXTENSION_REGISTRY`,
/// whose host must pass the outbound client's checks (ADR 0024).
#[test]
fn plugin_install_by_name_needs_a_registry() {
    let home = tempfile::tempdir().unwrap();
    let site = [("DB_NAME", "x"), ("DB_USER", "x")];
    let unset = run_with(&["plugin", "install", "hyscribe-test"], &site, home.path());
    assert_eq!(
        (unset.code, unset.err.as_str()),
        (
            1,
            "error: EXTENSION_REGISTRY is not set: the hosted registry has no address yet, so set it to a registry's https: URL\n"
        )
    );
    let local = [
        ("DB_NAME", "x"),
        ("DB_USER", "x"),
        ("EXTENSION_REGISTRY", "https://127.0.0.1/"),
    ];
    for args in [
        &["plugin", "install", "hyscribe-test@0.1.0", "--downgrade"][..],
        &["plugin", "update", "--all", "--allow-new-grants"],
        &["plugin", "update", "--check"],
        &["plugin", "update", "a", "b"],
    ] {
        let refused = run_with(args, &local, home.path());
        let why = "error: https://127.0.0.1/ is not an https: registry URL\n";
        assert_eq!((refused.code, refused.err.as_str()), (1, why), "{args:?}");
    }
    let unset = run_with(&["plugin", "update", "--all"], &[], home.path());
    let why = "error: no configuration: run ./hyscribe setup\n";
    assert_eq!((unset.code, unset.err.as_str()), (1, why));
    let which = run_with(&["plugin", "update"], &local, home.path());
    assert_eq!(which.code, 2);
    let both = run_with(&["plugin", "update", "--check", "a"], &local, home.path());
    assert_eq!(both.code, 2);
}

/// Resolves every name to a public address and connects to the test
/// registry.
struct Local(std::net::SocketAddr);

impl hyscribe_fetch::Net for Local {
    fn resolve<'a>(
        &'a self,
        _: &'a str,
        port: u16,
    ) -> hyscribe_fetch::Boxed<'a, Vec<std::net::SocketAddr>> {
        let address = std::net::SocketAddr::new([93, 184, 215, 14].into(), port);
        Box::pin(async move { Ok(vec![address]) })
    }

    fn dial(&self, _: std::net::SocketAddr) -> hyscribe_fetch::Boxed<'_, tokio::net::TcpStream> {
        Box::pin(tokio::net::TcpStream::connect(self.0))
    }
}

/// The registry commands over a test registry listing no plugin, signed
/// by the test roots this build trusts.
#[test]
fn registry_commands_reach_the_registry() {
    use hyscribe_registry::install::{Allow, Update};
    use hyscribe_testkit::minisign::{Key, key_list, sign_keys};
    use registry::Asked;

    let served = hyscribe_testkit::registry::Served::start().unwrap();
    let now = signed::now();
    let keys = [(&Key(10), r#""index""#, now - 100, now + 1000)];
    let list = key_list(1, now + 1000, &keys, &[]);
    for (n, signature) in sign_keys(&list, 1, now + 1000, &[&Key(1), &Key(3)])
        .into_iter()
        .enumerate()
    {
        served.put(&format!("v1/keys.json.{}.minisig", n + 1), signature);
    }
    served.put("v1/keys.json", list);
    let index = format!(
        r#"{{"serial":1,"expires":{},"extensions":[],"revoked":[]}}"#,
        now + 1000
    );
    let comment = format!("hyscribe-index serial=1 expires={}", now + 1000);
    served.put(
        "v1/index.json.minisig",
        Key(10).sign(index.as_bytes(), &comment),
    );
    served.put("v1/index.json", index);
    let net = std::sync::Arc::new(Local(served.address));
    let roots = hyscribe_testkit::registry::roots();
    let client = hyscribe_fetch::Client::new(roots, net, std::sync::Arc::new(()));
    let home = tempfile::tempdir().unwrap();
    let env = [
        ("DB_NAME", "x"),
        ("DB_USER", "x"),
        ("EXTENSION_REGISTRY", "https://pages.test/"),
    ]
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .into();
    let run = |asked| registry::command(home.path(), &env, &client, asked, Allow::default());
    let reply = |asked| run(asked).map(|reply| (reply.text, reply.code));
    let check = reply(Asked::Update(Update::Check)).unwrap();
    assert_eq!(
        check,
        ("Every plugin from the registry is up to date.\n".into(), 0)
    );
    let all = reply(Asked::Update(Update::All)).unwrap();
    assert_eq!(all, ("No plugin installed from the registry.\n".into(), 0));
    let ids = ["a".to_string()];
    let missing = reply(Asked::Update(Update::Ids(&ids))).unwrap();
    assert_eq!(missing, ("error: a is not installed\n".into(), 1));
    for name in ["nothing", "nothing@1.0"] {
        let refused = reply(Asked::Install(name)).unwrap_err();
        assert_eq!(refused, "nothing is not a plugin in the registry");
    }
    for name in ["Bad", "bad@1 0", "@1"] {
        let refused = reply(Asked::Install(name)).unwrap_err();
        assert!(
            refused.starts_with(&format!("{name:?} is neither")),
            "{refused}"
        );
    }
    assert!(registry::is_file("a/b") && registry::is_file("a.hyx"));
    assert!(!registry::is_file("a"));
    served.remove("v1/keys.json");
    let gone = reply(Asked::Update(Update::Check)).unwrap_err();
    assert_eq!(gone, "https://pages.test/v1/keys.json is not there");
}

/// `update`'s and `install`'s flags reach the registry commands as given.
#[test]
fn registry_flags_are_read() {
    use hyscribe_registry::install::Update;
    let ids = ["a".to_string()];
    assert!(matches!(which(true, false, &ids), Update::Check));
    assert!(matches!(which(false, true, &ids), Update::All));
    assert!(matches!(which(false, false, &ids), Update::Ids([id]) if id == "a"));
    let args = [
        "hyscribe",
        "plugin",
        "install",
        "a",
        "--downgrade",
        "--allow-new-grants",
        "--replace-local",
    ];
    let Some(Command::Plugin {
        command:
            PluginCommand::Install {
                allow,
                replace_local,
                ..
            },
    }) = Cli::try_parse_from(args).unwrap().command
    else {
        panic!("not an install");
    };
    let read = allow.allow(replace_local);
    assert!(read.downgrade && read.new_grants && read.replace_local);
    let none = AllowArgs {
        downgrade: false,
        allow_new_grants: false,
    }
    .allow(false);
    assert!(!(none.downgrade || none.new_grants || none.replace_local));
}

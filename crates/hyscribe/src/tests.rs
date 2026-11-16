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
            input: &mut std::io::empty(),
            terminal: None,
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

    // The server's first line reaches the log once it is up.
    let running = format!(
        "hyscribe {} running (pid {pid}) in shadow (read only) mode, listening on http://127.0.0.1:{}",
        env!("CARGO_PKG_VERSION"),
        site_port(home)
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !std::fs::read_to_string(&log).unwrap().contains(&running) {
        assert!(std::time::Instant::now() < deadline, "server never logged");
        std::thread::sleep(hyscribe_service::POLL);
    }
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
}

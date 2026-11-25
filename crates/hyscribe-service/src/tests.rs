// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::sync::mpsc;

const SHORT: Duration = Duration::from_millis(300);
const LONG: Duration = Duration::from_secs(30);
const HOME_VAR: &str = "HYSCRIBE_FAKE_SERVER_HOME";
const IGNORE_VAR: &str = "HYSCRIBE_FAKE_SERVER_IGNORE";
const DEAF_VAR: &str = "HYSCRIBE_FAKE_SERVER_DEAF";

/// When started by [`fake_spawn`], this test binary acts as the server:
/// it catches signals, holds the pid file, says it listens (unless
/// [`DEAF_VAR`] is set) and waits. Run normally, it does nothing.
#[test]
fn fake_server() {
    let Ok(home) = std::env::var(HOME_VAR) else {
        return;
    };
    let mut ignore: u32 = std::env::var(IGNORE_VAR).unwrap().parse().unwrap();
    let (mut shutdown, lock) = claim(&Paths::new(home)).unwrap();
    if std::env::var_os(DEAF_VAR).is_none() {
        lock.listening();
    }
    println!("ready");
    loop {
        let signal = shutdown.wait();
        if ignore == 0 {
            break;
        }
        ignore -= 1;
        println!("ignored {signal}");
    }
    println!("stopping");
    drop(lock);
}

/// This test binary as a server that ignores the first `ignore` signals.
fn fake_spawn(home: &Path, ignore: u32) -> Spawn {
    let mut env = vec![
        (HOME_VAR.to_string(), home.display().to_string()),
        (IGNORE_VAR.to_string(), ignore.to_string()),
    ];
    // Coverage runs collect profiles from child processes through this.
    if let Ok(profile) = std::env::var("LLVM_PROFILE_FILE") {
        env.push(("LLVM_PROFILE_FILE".to_string(), profile));
    }
    Spawn {
        program: std::env::current_exe().unwrap(),
        args: ["--exact", "tests::fake_server", "--nocapture", "-q"]
            .map(OsString::from)
            .to_vec(),
        env,
    }
}

fn program(path: &str, args: &[&str]) -> Spawn {
    Spawn {
        program: PathBuf::from(path),
        args: args.iter().map(OsString::from).collect(),
        env: vec![("PATH".to_string(), "/usr/bin:/bin".to_string())],
    }
}

/// Locks the pid file the way a server would, with `contents` in it.
fn fake_lock(paths: &Paths, contents: &str) -> File {
    std::fs::create_dir_all(paths.run_dir()).unwrap();
    std::fs::write(paths.pid_file(), contents).unwrap();
    let file = File::open(paths.pid_file()).unwrap();
    file.try_lock().unwrap();
    file
}

fn wait_for_log(paths: &Paths, line: &str) {
    let deadline = Instant::now() + LONG;
    while !tail(&paths.log_file(), 100)
        .unwrap()
        .iter()
        .any(|l| l == line)
    {
        assert!(Instant::now() < deadline, "no {line:?} in the log");
        sleep(POLL);
    }
}

/// Waits for the lock to be free. Tests in this binary spawn processes, and a
/// child keeps a copy of every open file, lock included, until it execs.
fn wait_for_stopped(paths: &Paths) {
    let deadline = Instant::now() + LONG;
    while status(paths).unwrap() != Status::Stopped {
        assert!(Instant::now() < deadline, "still locked");
        sleep(POLL);
    }
}

fn chmod(path: &Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// A writer that always fails, like a closed terminal.
struct Closed;

impl Write for Closed {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("closed"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::other("closed"))
    }
}

#[test]
fn paths_follow_the_folder_layout() {
    let paths = Paths::new("/opt/hyscribe");
    assert_eq!(paths.home(), Path::new("/opt/hyscribe"));
    assert_eq!(
        paths.pid_file(),
        Path::new("/opt/hyscribe/run/hyscribe.pid")
    );
    assert_eq!(
        paths.log_file(),
        Path::new("/opt/hyscribe/logs/hyscribe.log")
    );
    assert_eq!(
        paths.setup_secret_file(),
        Path::new("/opt/hyscribe/logs/setup-secret")
    );
}

#[test]
fn lifecycle() {
    let home = tempfile::tempdir().unwrap();
    let paths = Paths::new(home.path());
    assert_eq!(status(&paths).unwrap(), Status::Stopped);

    let pid = match start(&paths, &fake_spawn(home.path(), 0), LONG).unwrap() {
        Started::Now(pid) => pid,
        other => panic!("{other:?}"),
    };
    assert_eq!(status(&paths).unwrap(), Status::Running(pid));
    assert_eq!(
        start(&paths, &fake_spawn(home.path(), 0), LONG).unwrap(),
        Started::Already(Status::Running(pid))
    );
    wait_for_log(&paths, "ready");

    let (sender, receiver) = mpsc::channel();
    let follower = {
        let paths = paths.clone();
        std::thread::spawn(move || {
            let mut out = Vec::new();
            // The tail covers lines written before following started.
            follow(&paths, 100, &mut out, POLL).unwrap();
            sender.send(out).unwrap();
        })
    };
    assert_eq!(stop(&paths, LONG).unwrap(), Stopped::Stopped(pid));
    follower.join().unwrap();
    let followed = String::from_utf8(receiver.recv().unwrap()).unwrap();
    assert!(followed.contains("stopping"), "{followed:?}");

    assert_eq!(stop(&paths, LONG).unwrap(), Stopped::NotRunning);
    assert_eq!(status(&paths).unwrap(), Status::Stopped);
    // The pid file is emptied on a clean exit.
    assert_eq!(std::fs::read_to_string(paths.pid_file()).unwrap(), "");

    let modes = |path: PathBuf| std::fs::metadata(path).unwrap().mode() & 0o777;
    assert_eq!(modes(paths.run_dir()), 0o700);
    assert_eq!(modes(paths.logs_dir()), 0o700);
    assert_eq!(modes(paths.pid_file()), 0o600);
    assert_eq!(modes(paths.log_file()), 0o600);
}

#[test]
fn a_server_that_ignores_one_signal_stops_on_the_next() {
    let home = tempfile::tempdir().unwrap();
    let paths = Paths::new(home.path());
    let Started::Now(pid) = start(&paths, &fake_spawn(home.path(), 1), LONG).unwrap() else {
        panic!("not started");
    };
    let target = rustix::process::Pid::from_raw(i32::try_from(pid).unwrap()).unwrap();
    rustix::process::kill_process(target, rustix::process::Signal::TERM).unwrap();
    wait_for_log(&paths, &format!("ignored {}", signal_hook::consts::SIGTERM));
    assert_eq!(stop(&paths, LONG).unwrap(), Stopped::Stopped(pid));
}

#[test]
fn a_server_that_ignores_sigterm_is_killed() {
    let home = tempfile::tempdir().unwrap();
    let paths = Paths::new(home.path());
    let Started::Now(pid) = start(&paths, &fake_spawn(home.path(), u32::MAX), LONG).unwrap() else {
        panic!("not started");
    };
    wait_for_log(&paths, "ready");
    assert_eq!(stop(&paths, SHORT).unwrap(), Stopped::Killed(pid));
    assert_eq!(status(&paths).unwrap(), Status::Stopped);
}

#[test]
fn hold_is_exclusive() {
    let home = tempfile::tempdir().unwrap();
    let paths = Paths::new(home.path());
    let lock = hold(&paths).unwrap();
    assert!(format!("{lock:?}").starts_with("PidLock"));
    let pid = std::process::id();
    assert_eq!(status(&paths).unwrap(), Status::Starting(Some(pid)));
    assert!(matches!(hold(&paths), Err(Error::AlreadyRunning(Some(p))) if p == pid));
    lock.listening();
    lock.listening();
    assert_eq!(
        std::fs::read_to_string(paths.pid_file()).unwrap(),
        format!("{pid}\nlistening\n")
    );
    assert_eq!(status(&paths).unwrap(), Status::Running(pid));
    assert!(matches!(hold(&paths), Err(Error::AlreadyRunning(Some(p))) if p == pid));
    drop(lock);
    wait_for_stopped(&paths);
    // A stale file from a crash does not block the next server.
    std::fs::write(paths.pid_file(), "12345\n").unwrap();
    assert_eq!(status(&paths).unwrap(), Status::Stopped);
    drop(hold(&paths).unwrap());
}

#[test]
fn a_starting_server_has_no_pid_yet() {
    let home = tempfile::tempdir().unwrap();
    let paths = Paths::new(home.path());
    let lock = fake_lock(&paths, "");
    assert_eq!(status(&paths).unwrap(), Status::Starting(None));
    assert!(matches!(hold(&paths), Err(Error::AlreadyRunning(None))));
    assert!(matches!(stop(&paths, SHORT), Err(Error::Starting)));
    assert_eq!(
        start(&paths, &program("/bin/true", &[]), SHORT).unwrap(),
        Started::Already(Status::Starting(None))
    );
    // A second line other than `listening` is still starting.
    drop(lock);
    let _lock = fake_lock(&paths, "7\nconnecting\n");
    assert_eq!(status(&paths).unwrap(), Status::Starting(Some(7)));
}

/// A server that holds the lock but never listens is reported with its
/// log and stopped, and `stop` stops one while it starts. The wait leaves a
/// loaded machine time to start the fake server and log `ready`.
#[test]
fn a_server_that_never_listens_times_out() {
    let home = tempfile::tempdir().unwrap();
    let paths = Paths::new(home.path());
    let mut deaf = fake_spawn(home.path(), 0);
    deaf.env.push((DEAF_VAR.to_string(), "1".to_string()));
    let err = start(&paths, &deaf, Duration::from_secs(10)).unwrap_err();
    // The test harness prints its own lines first.
    assert!(
        matches!(&err, Error::Timeout { tail } if tail.last().unwrap() == "ready"),
        "{err}"
    );
    let text = err.to_string();
    assert!(
        text.starts_with("the server was not listening in time and was stopped; last log lines:\n")
            && text.ends_with("\n  ready"),
        "{text}"
    );
    wait_for_stopped(&paths);

    let mut spawned = Command::new(&deaf.program)
        .args(&deaf.args)
        .envs(deaf.env.iter().map(|(k, v)| (k, v)))
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + LONG;
    while status(&paths).unwrap() != Status::Starting(Some(spawned.id())) {
        assert!(Instant::now() < deadline, "never held the lock");
        sleep(POLL);
    }
    assert_eq!(
        start(&paths, &deaf, SHORT).unwrap(),
        Started::Already(Status::Starting(Some(spawned.id())))
    );
    assert_eq!(stop(&paths, LONG).unwrap(), Stopped::Stopped(spawned.id()));
    spawned.wait().unwrap();
}

#[test]
fn stop_refuses_an_impossible_pid() {
    let home = tempfile::tempdir().unwrap();
    let paths = Paths::new(home.path());
    let _lock = fake_lock(&paths, "0\n");
    let err = stop(&paths, SHORT).unwrap_err();
    assert_eq!(err.to_string(), "cannot signal pid 0: invalid data");
}

#[test]
fn stop_reports_a_lock_holder_that_never_exits() {
    let home = tempfile::tempdir().unwrap();
    let paths = Paths::new(home.path());
    // The pid of a process that has exited, so the signals find nobody,
    // while this test keeps the lock.
    let mut child = Command::new("/bin/true").spawn().unwrap();
    let gone = child.id();
    child.wait().unwrap();
    let _lock = fake_lock(&paths, &format!("{gone}\n"));
    let err = stop(&paths, SHORT).unwrap_err();
    assert!(matches!(err, Error::StillRunning(p) if p == gone));
    assert_eq!(
        err.to_string(),
        format!("pid {gone} is still running after SIGKILL")
    );
}

#[test]
fn stop_reports_signal_errors() {
    // As root this would signal init; the binary refuses root anyway.
    assert_ne!(effective_uid(), 0, "run the tests as a normal user");
    let home = tempfile::tempdir().unwrap();
    let paths = Paths::new(home.path());
    let _lock = fake_lock(&paths, "1\n");
    let err = stop(&paths, SHORT).unwrap_err();
    assert!(
        err.to_string().starts_with("cannot signal pid 1: "),
        "{err}"
    );
}

#[test]
fn start_reports_servers_that_fail() {
    let home = tempfile::tempdir().unwrap();
    let paths = Paths::new(home.path());

    let err = start(&paths, &program("/no/such/program", &[]), LONG).unwrap_err();
    assert!(err.to_string().starts_with("cannot run /no/such/program: "));

    let err = start(
        &paths,
        &program("/bin/sh", &["-c", "echo boom; exit 3"]),
        LONG,
    )
    .unwrap_err();
    assert!(matches!(&err, Error::ExitedEarly { code: Some(3), tail } if tail == &["boom"]));
    assert_eq!(
        err.to_string(),
        "the server exited with code 3 while starting; last log lines:\n  boom"
    );

    std::fs::remove_file(paths.log_file()).unwrap();
    let err = start(&paths, &program("/bin/sh", &["-c", "kill -9 $$"]), LONG).unwrap_err();
    assert_eq!(
        err.to_string(),
        "the server exited with a signal while starting"
    );

    let err = start(&paths, &program("/bin/sleep", &["30"]), SHORT).unwrap_err();
    assert!(matches!(&err, Error::Timeout { tail } if tail.is_empty()));
    assert_eq!(
        err.to_string(),
        "the server was not listening in time and was stopped"
    );
}

#[test]
fn start_reports_unusable_folders() {
    let home = tempfile::tempdir().unwrap();
    let paths = Paths::new(home.path());
    std::fs::write(paths.logs_dir(), "").unwrap();
    let err = start(&paths, &program("/bin/true", &[]), SHORT).unwrap_err();
    assert!(err.to_string().starts_with("cannot create "), "{err}");

    std::fs::remove_file(paths.logs_dir()).unwrap();
    std::fs::create_dir_all(paths.log_file()).unwrap();
    let err = start(&paths, &program("/bin/true", &[]), SHORT).unwrap_err();
    assert!(err.to_string().starts_with("cannot open "), "{err}");

    std::fs::remove_dir(paths.log_file()).unwrap();
    std::fs::create_dir_all(paths.home().join("logs/hyscribe.log.1/full")).unwrap();
    std::fs::write(paths.log_file(), vec![b'x'; 16]).unwrap();
    // Rotation is only reached for a log over the size limit, so check it
    // through `rotate` directly.
    let err = rotate(&paths.log_file(), 1, 1).unwrap_err();
    assert!(err.to_string().starts_with("cannot rotate "), "{err}");
}

#[test]
fn status_and_hold_report_unreadable_files() {
    let home = tempfile::tempdir().unwrap();
    let paths = Paths::new(home.path());
    std::fs::create_dir_all(paths.run_dir()).unwrap();
    std::fs::write(paths.pid_file(), "").unwrap();
    chmod(&paths.pid_file(), 0o000);
    assert!(
        status(&paths)
            .unwrap_err()
            .to_string()
            .starts_with("cannot open ")
    );
    assert!(
        hold(&paths)
            .unwrap_err()
            .to_string()
            .starts_with("cannot open ")
    );

    let file_home = home.path().join("file");
    std::fs::write(&file_home, "").unwrap();
    let err = hold(&Paths::new(&file_home)).unwrap_err();
    assert!(err.to_string().starts_with("cannot create "), "{err}");
}

#[test]
fn tail_returns_the_last_lines() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("log");
    assert!(tail(&log, 5).unwrap().is_empty());
    std::fs::write(&log, "one\ntwo\nthree\n").unwrap();
    assert_eq!(tail(&log, 2).unwrap(), ["two", "three"]);
    assert_eq!(tail(&log, 9).unwrap(), ["one", "two", "three"]);
    chmod(&log, 0o000);
    assert!(
        tail(&log, 2)
            .unwrap_err()
            .to_string()
            .starts_with("cannot read ")
    );
}

#[test]
fn follow_reports_problems() {
    let home = tempfile::tempdir().unwrap();
    let paths = Paths::new(home.path());
    let err = follow(&paths, 5, &mut Vec::new(), POLL).unwrap_err();
    assert!(err.to_string().starts_with("cannot open "), "{err}");

    std::fs::create_dir_all(paths.logs_dir()).unwrap();
    std::fs::write(paths.log_file(), "older\nold\n").unwrap();
    // Nothing runs, so it prints the last line and returns, promptly: a
    // follow that never notices the stop must fail here, not hang.
    let (sender, receiver) = mpsc::channel();
    let follower = paths.clone();
    std::thread::spawn(move || {
        let mut out = Vec::new();
        follow(&follower, 1, &mut out, POLL).unwrap();
        sender.send(out).unwrap();
    });
    assert_eq!(receiver.recv_timeout(LONG).unwrap(), b"old\n");
    let err = follow(&paths, 1, &mut Closed, POLL).unwrap_err();
    assert_eq!(err.to_string(), "cannot write the log: closed");
    // Nothing to print first: the failure comes from the follow loop.
    let err = follow(&paths, 0, &mut Closed, POLL).unwrap_err();
    assert_eq!(err.to_string(), "cannot write the log: closed");
}

#[test]
fn rotate_keeps_numbered_copies() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("hyscribe.log");
    rotate(&log, 1, 3).unwrap();
    std::fs::write(&log, "new").unwrap();
    rotate(&log, 100, 3).unwrap();
    assert!(log.exists());

    std::fs::write(dir.path().join("hyscribe.log.1"), "one").unwrap();
    std::fs::write(dir.path().join("hyscribe.log.3"), "dropped").unwrap();
    rotate(&log, 3, 3).unwrap();
    let read = |name: &str| std::fs::read_to_string(dir.path().join(name)).unwrap();
    assert!(!log.exists());
    assert_eq!(read("hyscribe.log.1"), "new");
    assert_eq!(read("hyscribe.log.2"), "one");
    // log.2 did not exist, so log.3 is untouched.
    assert_eq!(read("hyscribe.log.3"), "dropped");
}

#[test]
fn rotate_reports_errors() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("hyscribe.log");
    std::fs::write(&log, "data").unwrap();
    std::fs::create_dir_all(dir.path().join("hyscribe.log.2/full")).unwrap();
    std::fs::write(dir.path().join("hyscribe.log.1"), "one").unwrap();
    let err = rotate(&log, 1, 2).unwrap_err();
    assert!(err.to_string().starts_with("cannot rotate "), "{err}");

    let locked = dir.path().join("locked");
    std::fs::create_dir(&locked).unwrap();
    chmod(&locked, 0o000);
    let err = rotate(&locked.join("hyscribe.log"), 1, 3).unwrap_err();
    chmod(&locked, 0o700);
    assert!(err.to_string().starts_with("cannot check "), "{err}");
}

#[test]
fn messages_read_well() {
    assert_eq!(Status::Running(7).to_string(), "running (pid 7)");
    assert_eq!(Status::Starting(Some(7)).to_string(), "starting (pid 7)");
    assert_eq!(Status::Starting(None).to_string(), "starting");
    assert_eq!(Status::Stopped.to_string(), "not running");
    assert_eq!(Started::Now(7).to_string(), "started (pid 7)");
    assert_eq!(
        Started::Already(Status::Running(7)).to_string(),
        "already running (pid 7)"
    );
    assert_eq!(
        Started::Already(Status::Starting(None)).to_string(),
        "already starting"
    );
    assert_eq!(Stopped::NotRunning.to_string(), "not running");
    assert_eq!(Stopped::Stopped(7).to_string(), "stopped (pid 7)");
    assert_eq!(
        Stopped::Killed(7).to_string(),
        "killed (pid 7) after it ignored SIGTERM"
    );
    assert_eq!(
        Error::AlreadyRunning(Some(7)).to_string(),
        "already running (pid 7)"
    );
    assert_eq!(
        Error::AlreadyRunning(None).to_string(),
        "another server is starting; try again in a moment"
    );
    assert_eq!(
        Error::Starting.to_string(),
        Error::AlreadyRunning(None).to_string()
    );
    let err: Box<dyn std::error::Error> = Box::new(Error::Timeout { tail: Vec::new() });
    assert!(err.source().is_none());
    assert_eq!(format!("{:?}", listen().unwrap()), "Shutdown");
}

#[test]
fn effective_uid_is_this_user() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        effective_uid(),
        std::fs::metadata(dir.path()).unwrap().uid()
    );
}

fn assert_starts(err: Error, prefix: &str) {
    assert!(err.to_string().starts_with(prefix), "{err}");
}

#[test]
fn status_reports_an_unreadable_holder() {
    let home = tempfile::tempdir().unwrap();
    let paths = Paths::new(home.path());
    // A directory can be locked but not read.
    std::fs::create_dir_all(paths.pid_file()).unwrap();
    let dir = File::open(paths.pid_file()).unwrap();
    dir.try_lock().unwrap();
    assert_starts(status(&paths).unwrap_err(), "cannot read ");
}

#[test]
fn start_and_stop_report_an_unreadable_pid_file() {
    let home = tempfile::tempdir().unwrap();
    let paths = Paths::new(home.path());
    std::fs::create_dir_all(paths.run_dir()).unwrap();
    std::fs::write(paths.pid_file(), "").unwrap();
    chmod(&paths.pid_file(), 0o000);
    assert_starts(
        start(&paths, &program("/bin/true", &[]), SHORT).unwrap_err(),
        "cannot open ",
    );
    assert_starts(stop(&paths, SHORT).unwrap_err(), "cannot open ");
}

#[test]
fn start_reports_an_unreadable_logs_folder() {
    let home = tempfile::tempdir().unwrap();
    let paths = Paths::new(home.path());
    std::fs::create_dir_all(paths.logs_dir()).unwrap();
    chmod(&paths.logs_dir(), 0o000);
    let err = start(&paths, &program("/bin/true", &[]), SHORT).unwrap_err();
    chmod(&paths.logs_dir(), 0o700);
    assert_starts(err, "cannot check ");
}

#[test]
fn start_reports_problems_while_waiting() {
    let home = tempfile::tempdir().unwrap();
    let paths = Paths::new(home.path());
    let hide_log = "chmod 000 logs/hyscribe.log; exit 1";
    assert_starts(
        start(&paths, &program("/bin/sh", &["-c", hide_log]), LONG).unwrap_err(),
        "cannot read ",
    );
    std::fs::remove_file(paths.log_file()).unwrap();

    // The child makes the pid file unreadable and keeps running; start
    // reports it and does not leave the child behind.
    let hide_pid = "mkdir -p run && : > run/hyscribe.pid && chmod 000 run/hyscribe.pid \
                    && echo $$ > child.pid && exec sleep 30";
    assert_starts(
        start(&paths, &program("/bin/sh", &["-c", hide_pid]), LONG).unwrap_err(),
        "cannot open ",
    );
    let child = std::fs::read_to_string(home.path().join("child.pid")).unwrap();
    let child = rustix::process::Pid::from_raw(child.trim().parse().unwrap()).unwrap();
    assert_eq!(
        rustix::process::test_kill_process(child),
        Err(rustix::io::Errno::SRCH)
    );
}

#[test]
fn stop_reports_problems_while_waiting() {
    let home = tempfile::tempdir().unwrap();
    let paths = Paths::new(home.path());
    let mut child = Command::new("/bin/true").spawn().unwrap();
    let gone = child.id();
    child.wait().unwrap();
    let _lock = fake_lock(&paths, &format!("{gone}\n"));
    let pid_file = paths.pid_file();
    let hider = std::thread::spawn(move || {
        sleep(SHORT);
        chmod(&pid_file, 0o000);
    });
    assert_starts(stop(&paths, LONG).unwrap_err(), "cannot open ");
    hider.join().unwrap();
}

#[test]
fn follow_reports_problems_while_following() {
    let home = tempfile::tempdir().unwrap();
    let paths = Paths::new(home.path());
    let _lock = fake_lock(&paths, "1\n");
    std::fs::create_dir_all(paths.logs_dir()).unwrap();
    std::fs::write(paths.log_file(), "").unwrap();
    let pid_file = paths.pid_file();
    let hider = std::thread::spawn(move || {
        sleep(SHORT);
        chmod(&pid_file, 0o000);
    });
    assert_starts(
        follow(&paths, 0, &mut Vec::new(), POLL).unwrap_err(),
        "cannot open ",
    );
    hider.join().unwrap();
}

#[test]
fn limits_match_the_docs() {
    // DEPLOYMENT.md: rotated at 10 MiB, keeping hyscribe.log.1 to .5.
    assert_eq!(MAX_LOG_BYTES, 10_485_760);
    assert_eq!(KEEP_LOGS, 5);
    assert_eq!(LOCK_WAIT, Duration::from_millis(250));
}

#[test]
fn only_a_held_lock_counts_as_held() {
    assert!(is_held(&Err(io::ErrorKind::WouldBlock.into())));
    assert!(!is_held(&Ok(())));
    // Other locking errors give no pid, so nothing is signalled.
    assert!(!is_held(&Err(io::Error::other("no locks"))));
}

#[test]
fn hold_waits_out_a_brief_reader() {
    let home = tempfile::tempdir().unwrap();
    let paths = Paths::new(home.path());
    let reader = fake_lock(&paths, "");
    let release = std::thread::spawn(move || {
        sleep(Duration::from_millis(50));
        drop(reader);
    });
    drop(hold(&paths).unwrap());
    release.join().unwrap();
}

/// Free space is whole blocks of the file system, and an absent path has
/// none to report.
#[test]
fn free_disk_space_is_counted_in_blocks() {
    let home = tempfile::tempdir().unwrap();
    let free = disk_free(home.path()).unwrap();
    let block = rustix::fs::statvfs(home.path()).unwrap().f_frsize;
    assert!(block > 0 && free.is_multiple_of(block));
    assert_eq!(disk_free(&home.path().join("absent")), None);
}

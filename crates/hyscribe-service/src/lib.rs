// SPDX-License-Identifier: AGPL-3.0-or-later

//! The self-managed service behind `hyscribe start` and
//! `hyscribe service start|status|log|restart|stop` (ADR 0005).
//!
//! The running server holds an exclusive lock on `run/hyscribe.pid` for its
//! whole life and writes its pid into it, then a `listening` line once it
//! accepts connections. Holding the lock is what "running" means: a pid file
//! left behind by a crash is never mistaken for a live server, and signals
//! only go to the pid written by the lock holder, so a recycled pid is never
//! signalled.

use std::ffi::OsString;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

/// The pid file name inside `run/`.
pub const PID_FILE: &str = "hyscribe.pid";
/// The log file name inside `logs/`.
pub const LOG_FILE: &str = "hyscribe.log";
/// The log is rotated at `service start` once it reaches this size.
pub const MAX_LOG_BYTES: u64 = 10 * 1024 * 1024;
/// Rotated logs kept: `hyscribe.log.1` to `hyscribe.log.5`.
pub const KEEP_LOGS: u32 = 5;
/// How often waits check for a state change.
pub const POLL: Duration = Duration::from_millis(50);
/// How long [`hold`] retries, because `status` takes a shared lock for a
/// moment and must not make a starting server give up.
pub const LOCK_WAIT: Duration = Duration::from_millis(250);

/// Where the service keeps its files inside the `hyscribe/` folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    home: PathBuf,
}

impl Paths {
    /// Paths below `home`, the folder holding the binary.
    pub fn new(home: impl Into<PathBuf>) -> Paths {
        Paths { home: home.into() }
    }

    /// The `hyscribe/` folder itself.
    pub fn home(&self) -> &Path {
        &self.home
    }

    /// `run/`, holding the pid file.
    pub fn run_dir(&self) -> PathBuf {
        self.home.join("run")
    }

    /// `logs/`.
    pub fn logs_dir(&self) -> PathBuf {
        self.home.join("logs")
    }

    /// `run/hyscribe.pid`.
    pub fn pid_file(&self) -> PathBuf {
        self.run_dir().join(PID_FILE)
    }

    /// `logs/hyscribe.log`.
    pub fn log_file(&self) -> PathBuf {
        self.logs_dir().join(LOG_FILE)
    }

    /// `logs/setup-secret`: setup mode's one-time secret while it runs
    /// without a terminal.
    pub fn setup_secret_file(&self) -> PathBuf {
        self.logs_dir().join("setup-secret")
    }
}

/// Why a service command failed.
#[derive(Debug)]
pub enum Error {
    /// Another server holds the pid file.
    AlreadyRunning(Option<u32>),
    /// The server is starting and has not written its pid yet.
    Starting,
    /// The spawned server exited before it was up.
    ExitedEarly {
        /// Its exit code, if it was not killed by a signal.
        code: Option<i32>,
        /// The last lines of the log.
        tail: Vec<String>,
    },
    /// The spawned server was not listening in time and was killed.
    Timeout {
        /// The last lines of the log.
        tail: Vec<String>,
    },
    /// The server is still running after SIGKILL.
    StillRunning(u32),
    /// A file or process operation failed.
    Io {
        /// What was being done.
        action: String,
        /// The underlying error.
        source: io::Error,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Error::AlreadyRunning(Some(pid)) => format!("already running (pid {pid})"),
            Error::AlreadyRunning(None) | Error::Starting => {
                "another server is starting; try again in a moment".to_string()
            }
            Error::ExitedEarly { code, tail } => {
                let code = code.map_or("a signal".to_string(), |code| format!("code {code}"));
                with_tail(
                    format!("the server exited with {code} while starting"),
                    tail,
                )
            }
            Error::Timeout { tail } => with_tail(
                "the server was not listening in time and was stopped".to_string(),
                tail,
            ),
            Error::StillRunning(pid) => format!("pid {pid} is still running after SIGKILL"),
            Error::Io { action, source } => format!("cannot {action}: {source}"),
        };
        f.write_str(&text)
    }
}

impl std::error::Error for Error {}

/// `text`, then the log lines that say why.
fn with_tail(mut text: String, tail: &[String]) -> String {
    if !tail.is_empty() {
        text.push_str("; last log lines:");
        for line in tail {
            text.push_str("\n  ");
            text.push_str(line);
        }
    }
    text
}

fn io_error(action: impl fmt::Display) -> impl FnOnce(io::Error) -> Error {
    let action = action.to_string();
    move |source| Error::Io { action, source }
}

/// Whether a server is running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// A server holds the pid file and listens.
    Running(u32),
    /// A server holds the pid file and does not listen yet; the pid is
    /// `None` until it is written.
    Starting(Option<u32>),
    /// No server holds the pid file.
    Stopped,
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Status::Running(pid) => write!(f, "running (pid {pid})"),
            Status::Starting(Some(pid)) => write!(f, "starting (pid {pid})"),
            Status::Starting(None) => f.write_str("starting"),
            Status::Stopped => f.write_str("not running"),
        }
    }
}

/// The pid file's second line once the server listens.
const LISTENING: &str = "listening";

/// Reads the service state from the pid file lock.
pub fn status(paths: &Paths) -> Result<Status, Error> {
    let path = paths.pid_file();
    let mut file = match File::open(&path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Status::Stopped),
        Err(err) => return Err(io_error(format_args!("open {}", path.display()))(err)),
    };
    let lock = file.try_lock_shared().map_err(io::Error::from);
    if lock.is_ok() {
        // Nobody holds it; the shared lock is released when `file` drops.
        return Ok(Status::Stopped);
    }
    // Any other locking error also counts as running, but without a pid, so
    // nothing is ever signalled on the strength of an unlocked file.
    let mut text = String::new();
    file.read_to_string(&mut text)
        .map_err(io_error(format_args!("read {}", path.display())))?;
    let mut lines = text.lines();
    let pid = lines
        .next()
        .and_then(|line| line.parse().ok())
        .filter(|_| is_held(&lock));
    Ok(match (pid, lines.next()) {
        (Some(pid), Some(LISTENING)) => Status::Running(pid),
        (pid, _) => Status::Starting(pid),
    })
}

/// Whether a lock attempt failed because someone else holds the lock.
fn is_held(lock: &io::Result<()>) -> bool {
    matches!(lock, Err(err) if err.kind() == io::ErrorKind::WouldBlock)
}

/// The pid file, locked and holding this process's pid. The lock is
/// released when the process exits, however it exits.
#[derive(Debug)]
pub struct PidLock {
    file: File,
}

impl PidLock {
    /// Marks the server as listening, which `service start` waits for. A
    /// failed write leaves it starting, which that wait reports.
    pub fn listening(&self) {
        use std::os::unix::fs::FileExt;
        let text = format!("{}\n{LISTENING}\n", std::process::id());
        // The pid line is rewritten as it was, so no reader sees it missing.
        let _ = self.file.write_all_at(text.as_bytes(), 0);
    }
}

impl Drop for PidLock {
    fn drop(&mut self) {
        // Cosmetic: an empty file reads as "stopped" to humans too. The lock
        // itself is what other commands check.
        let _ = self.file.set_len(0);
    }
}

/// Takes the pid file for this process. Fails if another server has it.
pub fn hold(paths: &Paths) -> Result<PidLock, Error> {
    let path = paths.pid_file();
    create_private_dir(&paths.run_dir())?;
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(&path)
        .map_err(io_error(format_args!("open {}", path.display())))?;
    let deadline = Instant::now() + LOCK_WAIT;
    while file.try_lock().is_err() {
        if Instant::now() >= deadline {
            // Held by another server; any other locking error also refuses
            // to start, which is the safe side. The pid is only reported,
            // never signalled.
            let mut text = String::new();
            let _ = file.read_to_string(&mut text);
            let pid = text.lines().next().and_then(|line| line.parse().ok());
            return Err(Error::AlreadyRunning(pid));
        }
        sleep(Duration::from_millis(10));
    }
    let written = file
        .set_len(0)
        .and_then(|()| writeln!(file, "{}", std::process::id()))
        .and_then(|()| file.sync_data());
    written
        .map(|()| PidLock { file })
        .map_err(io_error(format_args!("write {}", path.display())))
}

/// Creates `dir` readable by its owner only.
fn create_private_dir(dir: &Path) -> Result<(), Error> {
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(io_error(format_args!("create {}", dir.display())))
}

/// Catches SIGTERM and SIGINT so the server can shut down cleanly.
pub struct Shutdown {
    signals: signal_hook::iterator::Signals,
}

impl fmt::Debug for Shutdown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Shutdown")
    }
}

/// Starts catching shutdown signals. Call before [`hold`], so a stop sent
/// as soon as the pid is visible is caught rather than killing the process.
pub fn listen() -> Result<Shutdown, Error> {
    use signal_hook::consts::{SIGINT, SIGTERM};
    signal_hook::iterator::Signals::new([SIGTERM, SIGINT])
        .map(|signals| Shutdown { signals })
        .map_err(io_error("catch shutdown signals"))
}

/// [`listen`] then [`hold`]: what a server does first.
pub fn claim(paths: &Paths) -> Result<(Shutdown, PidLock), Error> {
    listen().and_then(|shutdown| hold(paths).map(|lock| (shutdown, lock)))
}

impl Shutdown {
    /// Blocks until the next shutdown signal and returns its number.
    pub fn wait(&mut self) -> i32 {
        // `forever` only ends when the handle is closed, which nothing does.
        self.signals.forever().next().unwrap_or(0)
    }
}

/// The command `service start` runs in the background.
#[derive(Clone, Debug)]
pub struct Spawn {
    /// The program, normally this binary.
    pub program: PathBuf,
    /// Its arguments, normally `start`.
    pub args: Vec<OsString>,
    /// Its whole environment.
    pub env: Vec<(String, String)>,
}

/// What `service start` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Started {
    /// A new server listens.
    Now(u32),
    /// A server was already running or starting.
    Already(Status),
}

impl fmt::Display for Started {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Started::Now(pid) => write!(f, "started (pid {pid})"),
            Started::Already(status) => write!(f, "already {status}"),
        }
    }
}

/// Starts `spawn` in the background with output to the log, and waits until
/// it listens, exits, or `timeout` passes.
pub fn start(paths: &Paths, spawn: &Spawn, timeout: Duration) -> Result<Started, Error> {
    let status = status(paths)?;
    if status != Status::Stopped {
        return Ok(Started::Already(status));
    }
    let log_path = paths.log_file();
    create_private_dir(&paths.logs_dir())?;
    rotate(&log_path, MAX_LOG_BYTES, KEEP_LOGS)?;
    let (log, log_err) = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(&log_path)
        .and_then(|log| log.try_clone().map(|err| (log, err)))
        .map_err(io_error(format_args!("open {}", log_path.display())))?;
    let mut child = Command::new(&spawn.program)
        .args(&spawn.args)
        .env_clear()
        .envs(spawn.env.iter().map(|(k, v)| (k, v)))
        .current_dir(paths.home())
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(log_err)
        // Its own process group, so Ctrl-C in this terminal does not reach it.
        .process_group(0)
        .spawn()
        .map_err(io_error(format_args!("run {}", spawn.program.display())))?;
    let outcome = wait_for_server(paths, &mut child, &log_path, timeout);
    if outcome.is_err() {
        // Never leave a half-started server behind.
        let _ = child.kill();
        let _ = child.wait();
    }
    outcome
}

fn wait_for_server(
    paths: &Paths,
    child: &mut std::process::Child,
    log_path: &Path,
    timeout: Duration,
) -> Result<Started, Error> {
    let deadline = Instant::now() + timeout;
    loop {
        // A failed check counts as "still running"; the deadline still holds.
        if let Ok(Some(exit)) = child.try_wait() {
            return Err(Error::ExitedEarly {
                code: exit.code(),
                tail: tail(log_path, 20)?,
            });
        }
        if status(paths)? == Status::Running(child.id()) {
            return Ok(Started::Now(child.id()));
        }
        if Instant::now() >= deadline {
            return Err(Error::Timeout {
                tail: tail(log_path, 20)?,
            });
        }
        sleep(POLL);
    }
}

/// What `service stop` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stopped {
    /// Nothing was running.
    NotRunning,
    /// The server shut down after SIGTERM.
    Stopped(u32),
    /// The server ignored SIGTERM for the grace period and was killed.
    Killed(u32),
}

impl fmt::Display for Stopped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Stopped::NotRunning => f.write_str("not running"),
            Stopped::Stopped(pid) => write!(f, "stopped (pid {pid})"),
            Stopped::Killed(pid) => write!(f, "killed (pid {pid}) after it ignored SIGTERM"),
        }
    }
}

/// Sends SIGTERM, waits up to `grace`, then sends SIGKILL.
pub fn stop(paths: &Paths, grace: Duration) -> Result<Stopped, Error> {
    let pid = match status(paths)? {
        Status::Stopped => return Ok(Stopped::NotRunning),
        Status::Starting(None) => return Err(Error::Starting),
        Status::Starting(Some(pid)) | Status::Running(pid) => pid,
    };
    let target = i32::try_from(pid)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
        .ok_or_else(|| Error::Io {
            action: format!("signal pid {pid}"),
            source: io::Error::from(io::ErrorKind::InvalidData),
        })?;
    let signal = |sig| {
        match rustix::process::kill_process(target, sig) {
            // Already gone.
            Err(rustix::io::Errno::SRCH) => Ok(()),
            other => other.map_err(|err| Error::Io {
                action: format!("signal pid {pid}"),
                source: err.into(),
            }),
        }
    };
    let steps = [
        (rustix::process::Signal::TERM, Stopped::Stopped(pid)),
        (rustix::process::Signal::KILL, Stopped::Killed(pid)),
    ];
    for (sig, outcome) in steps {
        signal(sig)?;
        if wait_until_stopped(paths, grace)? {
            return Ok(outcome);
        }
    }
    Err(Error::StillRunning(pid))
}

fn wait_until_stopped(paths: &Paths, timeout: Duration) -> Result<bool, Error> {
    let deadline = Instant::now() + timeout;
    loop {
        if status(paths)? == Status::Stopped {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        sleep(POLL);
    }
}

/// The last `lines` lines of a log file; empty if it does not exist.
pub fn tail(path: &Path, lines: usize) -> Result<Vec<String>, Error> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(io_error(format_args!("read {}", path.display()))(err)),
    };
    Ok(last_lines(&bytes, lines))
}

fn last_lines(bytes: &[u8], lines: usize) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    let all: Vec<&str> = text.lines().collect();
    let skip = all.len().saturating_sub(lines);
    all.into_iter().skip(skip).map(String::from).collect()
}

/// Prints the last `lines` lines of the log, then everything appended to it
/// until the server stops. Both come from one open file, so no line written
/// in between is lost.
pub fn follow(
    paths: &Paths,
    lines: usize,
    out: &mut dyn Write,
    poll: Duration,
) -> Result<(), Error> {
    let path = paths.log_file();
    let mut file = File::open(&path).map_err(io_error(format_args!("open {}", path.display())))?;
    // The log is bounded by rotation, so reading it whole is fine.
    let mut buf = Vec::new();
    let _ = file.read_to_end(&mut buf);
    let mut start = String::new();
    for line in last_lines(&buf, lines) {
        start.push_str(&line);
        start.push('\n');
    }
    out.write_all(start.as_bytes())
        .map_err(io_error("write the log"))?;
    loop {
        // Checked before reading, so lines written just before the stop are
        // still copied.
        let stopped = status(paths)? == Status::Stopped;
        buf.clear();
        // A failed read shows as no new output; the next poll tries again.
        let _ = file.read_to_end(&mut buf);
        out.write_all(&buf)
            .and_then(|()| out.flush())
            .map_err(io_error("write the log"))?;
        if stopped {
            return Ok(());
        }
        sleep(poll);
    }
}

/// Renames `log` to `log.1` (and `log.1` to `log.2` and so on, dropping the
/// oldest) once it reaches `max_bytes`.
pub fn rotate(log: &Path, max_bytes: u64, keep: u32) -> Result<(), Error> {
    let size = match std::fs::metadata(log) {
        Ok(metadata) => metadata.len(),
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(io_error(format_args!("check {}", log.display()))(err)),
    };
    if size < max_bytes {
        return Ok(());
    }
    let numbered = |n: u32| {
        let mut name = log.as_os_str().to_owned();
        name.push(format!(".{n}"));
        PathBuf::from(name)
    };
    for n in (1..keep).rev() {
        match std::fs::rename(numbered(n), numbered(n + 1)) {
            Err(err) if err.kind() != io::ErrorKind::NotFound => {
                return Err(io_error(format_args!("rotate {}", log.display()))(err));
            }
            _ => {}
        }
    }
    std::fs::rename(log, numbered(1)).map_err(io_error(format_args!("rotate {}", log.display())))
}

/// The effective user id of this process.
pub fn effective_uid() -> u32 {
    rustix::process::geteuid().as_raw()
}

#[cfg(test)]
mod tests;

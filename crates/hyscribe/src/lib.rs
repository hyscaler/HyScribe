// SPDX-License-Identifier: AGPL-3.0-or-later

//! The `hyscribe` command line. `main.rs` only gathers the process
//! arguments, environment and folder and calls [`run`], so every command is
//! testable in process.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Parser, Subcommand};
use hyscribe_config::{Config, Mode};
use hyscribe_service::{Paths, Spawn, Status};

/// Source code location, offered to every user as AGPL section 13 requires.
pub const SOURCE_URL: &str = "https://github.com/hyscaler/HyScribe";

/// How long `service start` waits for the server to come up.
pub const START_TIMEOUT: Duration = Duration::from_secs(30);
/// How long `service stop` waits after SIGTERM before SIGKILL.
pub const STOP_GRACE: Duration = Duration::from_secs(15);
/// Exit code of `service status` when nothing runs, as in LSB init scripts.
pub const EXIT_NOT_RUNNING: u8 = 3;

const NO_CONFIG: &str = "no configuration: the setup wizard is not available yet, \
                         so copy .env.example to .env, fill it in and chmod 600 it";

#[derive(Debug, Parser)]
#[command(name = "hyscribe", about = "Rust drop-in replacement for WordPress")]
struct Cli {
    /// With no command, `start`.
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print the version, license and source location.
    Version,
    /// Run the server in the foreground.
    Start,
    /// Run the server as a background service.
    Service {
        #[command(subcommand)]
        command: ServiceCommand,
    },
    /// Inspect configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    /// Load .env and the environment, and report problems without starting.
    Check {
        /// Folder holding .env. Defaults to the binary's folder.
        #[arg(long)]
        dir: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
enum ServiceCommand {
    /// Start the server in the background.
    Start,
    /// Stop the server: SIGTERM, then SIGKILL after a grace period.
    Stop,
    /// Stop, then start.
    Restart,
    /// Show whether the server runs and its last log line. Exits with 3
    /// when it does not run.
    Status,
    /// Print the last log lines.
    Log {
        /// How many lines.
        #[arg(short = 'n', long, default_value_t = 20)]
        lines: usize,
        /// Keep printing new lines until the server stops.
        #[arg(short, long)]
        follow: bool,
    },
}

/// What the process needs from the outside world.
pub struct Context<'a> {
    /// Command line arguments, including the program name.
    pub args: Vec<OsString>,
    /// Environment variables that are valid UTF-8.
    pub env: HashMap<String, String>,
    /// The folder holding the binary.
    pub home: PathBuf,
    /// The effective user id.
    pub euid: u32,
    /// The program `service start` runs in the background, and its
    /// arguments: this binary and `start`.
    pub server: (PathBuf, Vec<OsString>),
    /// Standard output.
    pub out: &'a mut dyn Write,
    /// Standard error.
    pub err: &'a mut dyn Write,
}

/// Runs one command and returns the process exit code.
pub fn run(ctx: Context<'_>) -> u8 {
    let Context {
        args,
        env,
        home,
        euid,
        server,
        out,
        err: err_out,
    } = ctx;
    let cli = match Cli::try_parse_from(&args) {
        Ok(cli) => cli,
        Err(err) => {
            let stream = if err.use_stderr() { err_out } else { out };
            // Nothing useful can be done if the terminal is gone.
            let _ = write!(stream, "{}", err.render());
            return u8::try_from(err.exit_code()).unwrap_or(2);
        }
    };
    let site = Site {
        paths: Paths::new(&home),
        home: &home,
        env: &env,
        euid,
        server: &server,
    };
    let result = match cli.command.unwrap_or(Command::Start) {
        Command::Version => Ok(Reply::text(version())),
        Command::Config {
            command: ConfigCommand::Check { dir },
        } => config_check(dir.as_ref().unwrap_or(&home), &env).map(Reply::text),
        Command::Start => serve(&site, out),
        Command::Service { command } => service(command, &site, out),
    };
    let written = result.and_then(|reply| {
        out.write_all(reply.text.as_bytes())
            .map(|()| reply.code)
            .map_err(|err| err.to_string())
    });
    match written {
        Ok(code) => code,
        Err(message) => {
            let _ = writeln!(err_out, "error: {message}");
            1
        }
    }
}

/// The parts of the context the server commands use.
struct Site<'a> {
    paths: Paths,
    home: &'a Path,
    env: &'a HashMap<String, String>,
    euid: u32,
    server: &'a (PathBuf, Vec<OsString>),
}

/// Text for standard output and the exit code.
struct Reply {
    text: String,
    code: u8,
}

impl Reply {
    fn text(text: String) -> Reply {
        Reply { text, code: 0 }
    }
}

fn version() -> String {
    format!(
        "hyscribe {}\nLicense: AGPL-3.0-or-later\nSource: {SOURCE_URL}\n",
        env!("CARGO_PKG_VERSION")
    )
}

fn config_check(dir: &Path, env: &HashMap<String, String>) -> Result<String, String> {
    let config = hyscribe_config::load(dir, env).map_err(|err| err.to_string())?;
    Ok(match config {
        None => format!(
            "No configuration in {}: HyScribe will start in setup mode.\n",
            dir.display()
        ),
        Some(config) => describe(&config),
    })
}

/// The server refuses root: a compromise would own the whole machine.
fn refuse_root(euid: u32) -> Result<(), String> {
    if euid == 0 {
        Err("refusing to run as root; run HyScribe as an unprivileged user".to_string())
    } else {
        Ok(())
    }
}

/// Loads the configuration a server needs before it can run.
fn server_config(home: &Path, env: &HashMap<String, String>, euid: u32) -> Result<Config, String> {
    refuse_root(euid)?;
    hyscribe_config::load(home, env)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| NO_CONFIG.to_string())
}

/// `hyscribe start`: serves the site until SIGTERM or SIGINT.
fn serve(site: &Site<'_>, out: &mut dyn Write) -> Result<Reply, String> {
    let config = server_config(site.home, site.env, site.euid)?;
    let (mut shutdown, lock) =
        hyscribe_service::claim(&site.paths).map_err(|err| err.to_string())?;
    // The blocking signal wait runs on its own thread and wakes the server.
    let (stop, stopped) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let _ = stop.send(shutdown.wait());
    });
    let signal = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(0));
    let seen = signal.clone();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(failed("cannot start the runtime".into()));
    runtime.and_then(|runtime| {
        runtime.block_on(async {
            let db = hyscribe_db::Db::connect(&config.database)
                .await
                .map_err(|err| format!("cannot connect to the database: {err}"))?;
            let site = hyscribe_http::Site::load(db, config.home_url.clone())
                .await
                .map_err(|err| format!("cannot read the site options: {err}"))?;
            let address = std::net::SocketAddr::new(config.server.bind_host, config.server.port);
            let listener = tokio::net::TcpListener::bind(address)
                .await
                .map_err(failed(format!("cannot listen on {address}")))?;
            // Log lines are best effort: a lost log must not stop the server.
            let _ = writeln!(
                out,
                "hyscribe {} running (pid {}) in {} mode, listening on http://{address}",
                env!("CARGO_PKG_VERSION"),
                std::process::id(),
                mode_name(config.mode),
            );
            let _ = out.flush();
            hyscribe_http::serve(listener, std::sync::Arc::new(site), async move {
                // The sender only goes away after sending.
                let number = stopped.await.unwrap_or(0);
                seen.store(number, std::sync::atomic::Ordering::SeqCst);
            })
            .await
            .map_err(failed("the server failed".into()))
        })
    })?;
    // The last line is written before the lock is released, so whoever sees
    // the server stopped also sees the whole log.
    let _ = writeln!(
        out,
        "stopped on signal {}",
        signal.load(std::sync::atomic::Ordering::SeqCst)
    );
    let _ = out.flush();
    drop(lock);
    Ok(Reply::text(String::new()))
}

/// Formats an I/O failure with what was being done.
fn failed(context: String) -> impl FnOnce(std::io::Error) -> String {
    move |err| format!("{context}: {err}")
}

fn service(command: ServiceCommand, site: &Site<'_>, out: &mut dyn Write) -> Result<Reply, String> {
    let paths = &site.paths;
    let error = |err: hyscribe_service::Error| err.to_string();
    let start = || -> Result<String, String> {
        server_config(site.home, site.env, site.euid)?;
        let spawn = Spawn {
            program: site.server.0.clone(),
            args: site.server.1.clone(),
            env: site
                .env
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        };
        let started = hyscribe_service::start(paths, &spawn, START_TIMEOUT).map_err(error)?;
        Ok(format!("{started}\nlogs: {}\n", paths.log_file().display()))
    };
    let stop = || {
        hyscribe_service::stop(paths, STOP_GRACE)
            .map(|stopped| format!("{stopped}\n"))
            .map_err(error)
    };
    match command {
        ServiceCommand::Start => start().map(Reply::text),
        ServiceCommand::Stop => stop().map(Reply::text),
        ServiceCommand::Restart => {
            refuse_root(site.euid)?;
            let stopped = stop()?;
            Ok(Reply::text(stopped + &start()?))
        }
        ServiceCommand::Status => {
            let status = hyscribe_service::status(paths).map_err(error)?;
            let last = hyscribe_service::tail(&paths.log_file(), 1).map_err(error)?;
            let mut text = format!("{status}\n");
            for line in last {
                text.push_str(&format!("last log line: {line}\n"));
            }
            let code = if status == Status::Stopped {
                EXIT_NOT_RUNNING
            } else {
                0
            };
            Ok(Reply { text, code })
        }
        ServiceCommand::Log {
            lines,
            follow: true,
        } => {
            hyscribe_service::follow(paths, lines, out, hyscribe_service::POLL).map_err(error)?;
            Ok(Reply::text(String::new()))
        }
        ServiceCommand::Log { lines, .. } => {
            let mut text = String::new();
            for line in hyscribe_service::tail(&paths.log_file(), lines).map_err(error)? {
                text.push_str(&line);
                text.push('\n');
            }
            Ok(Reply::text(text))
        }
    }
}

fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Shadow => "shadow (read only)",
        Mode::Primary => "primary",
    }
}

/// A summary that is safe to show: no passwords, keys or salts.
fn describe(config: &Config) -> String {
    let db = &config.database;
    let mode = mode_name(config.mode);
    let password = if db.password.expose().is_empty() {
        "not set"
    } else {
        "set"
    };
    format!(
        "Configuration OK\n\
         Database: {user}@{host}:{port}/{name} (password {password}, prefix {prefix}, charset {charset})\n\
         Listen: {bind}:{http_port}{proxy}\n\
         Mode: {mode}\n\
         Uploads: {uploads}\n\
         Keys and salts: {salts} of 8 set\n",
        user = db.user,
        host = db.host,
        port = db.port,
        name = db.name,
        prefix = db.prefix,
        charset = db.charset,
        bind = config.server.bind_host,
        http_port = config.server.port,
        proxy = if config.server.behind_proxy {
            " behind a proxy"
        } else {
            ""
        },
        uploads = config
            .uploads_dir
            .as_ref()
            .map_or("hyscribe/uploads".into(), |p| p.display().to_string()),
        salts = config.salts.len(),
    )
}

#[cfg(test)]
mod tests;

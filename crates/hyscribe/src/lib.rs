// SPDX-License-Identifier: AGPL-3.0-or-later

//! The `hyscribe` command line. `main.rs` only gathers the process
//! arguments, environment and folder and calls [`run`], so every command is
//! testable in process.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{BufRead, Write};
use std::os::fd::BorrowedFd;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Parser, Subcommand};
use hyscribe_auth::sessions::Sessions;
use hyscribe_config::{Config, ConfigError, Mode};
use hyscribe_service::{Paths, Spawn, Status};

/// Source code location, offered to every user as AGPL section 13 requires.
pub const SOURCE_URL: &str = "https://github.com/hyscaler/HyScribe";

/// How long `service start` waits for the server to listen.
pub const START_TIMEOUT: Duration = Duration::from_secs(30);
/// How long `service stop` waits after SIGTERM before SIGKILL.
pub const STOP_GRACE: Duration = Duration::from_secs(15);
/// Exit code of `service status` when nothing runs, as in LSB init scripts.
pub const EXIT_NOT_RUNNING: u8 = 3;

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
    /// Write .env in the terminal: read an existing site's wp-config.php,
    /// check the database and choose the mode and the network.
    Setup,
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
    /// WP-Cron, for DISABLE_WP_CRON sites whose system cron runs it.
    Cron {
        #[command(subcommand)]
        command: CronCommand,
    },
}

#[derive(Debug, Subcommand)]
enum CronCommand {
    /// Run the due events of the hooks HyScribe runs once, as
    /// wp-cron.php does, then exit.
    Run,
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
    /// Standard input, which answers the setup wizard.
    pub input: &'a mut dyn BufRead,
    /// Standard input's terminal, if it is one: the wizard turns its echo
    /// off for passwords.
    pub terminal: Option<BorrowedFd<'a>>,
    /// Whether standard output is a terminal, which may be shown the setup
    /// secret.
    pub screen: bool,
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
        input,
        terminal,
        screen,
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
        env: &env,
        euid,
        server: &server,
        screen,
    };
    let mut term = setup::Terminal {
        input,
        out: &mut *out,
        tty: terminal,
    };
    let result = match cli.command.unwrap_or(Command::Start) {
        Command::Version => Ok(Reply::text(version())),
        Command::Config {
            command: ConfigCommand::Check { dir },
        } => config_check(dir.as_ref().unwrap_or(&home), &env).map(Reply::text),
        Command::Start => serve(&site, &mut term),
        // The `.env` it writes would be root's, which no server may run as.
        Command::Setup => {
            refuse_root(euid).and_then(|()| setup::command(&home, &mut term).map(Reply::text))
        }
        Command::Service { command } => service(command, &site, &mut term),
        Command::Cron {
            command: CronCommand::Run,
        } => refuse_root(euid)
            .and_then(|()| hyscribe_config::load(&home, &env).map_err(|err| err.to_string()))
            .and_then(|config| {
                config.ok_or_else(|| "no configuration: run ./hyscribe setup".to_string())
            })
            .and_then(|config| cron::run(&config, &home, &mut *term.out))
            .map(Reply::text),
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
    env: &'a HashMap<String, String>,
    euid: u32,
    server: &'a (PathBuf, Vec<OsString>),
    screen: bool,
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

/// Loads the configuration a server needs before it can run, `None` for
/// none yet. Asked at a terminal, the terminal wizard may write it first;
/// otherwise the server starts in setup mode.
fn server_config(
    site: &Site<'_>,
    term: &mut setup::Terminal<'_>,
) -> Result<Option<Config>, String> {
    refuse_root(site.euid)?;
    let home = site.paths.home();
    let config = hyscribe_config::load(home, site.env).map_err(|err| err.to_string())?;
    if config.is_some() || !term.interactive() || !term.in_terminal()? {
        return Ok(config);
    }
    // No `.env` to keep, so the wizard writes one.
    let written = setup::run(home, term)?;
    let written = written.map(|path| format!("Wrote {} (mode 600)\n", path.display()));
    term.say(&written.unwrap_or_default())
        .and_then(|()| hyscribe_config::load(home, site.env).map_err(|err| err.to_string()))
}

/// The installation the admin describes: `binary`, and what `.env` sets.
fn install(config: &Config, binary: &Path) -> hyscribe_http::Install {
    hyscribe_http::Install {
        binary: binary.display().to_string(),
        upload_max_size: config.upload_max_size,
        database_user: config.database.user.clone(),
        database_name: config.database.name.clone(),
        charset: config.database.charset.clone(),
    }
}

/// `hyscribe start`: serves the site until SIGTERM or SIGINT, after setup
/// mode when there is no configuration yet.
fn serve(site: &Site<'_>, term: &mut setup::Terminal<'_>) -> Result<Reply, String> {
    let config = server_config(site, term)?;
    let out = &mut *term.out;
    let (mut shutdown, lock) =
        hyscribe_service::claim(&site.paths).map_err(|err| err.to_string())?;
    // A secret a killed server left behind; setup mode makes a new one.
    let secret_file = site.paths.setup_secret_file();
    let _ = std::fs::remove_file(&secret_file);
    let setup_start = setup::web::Start {
        secret_file: (!site.screen).then_some(secret_file.as_path()),
        listening: &|| lock.listening(),
    };
    // The blocking signal wait runs on its own thread and wakes the server.
    let (stop, stopped) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let _ = stop.send(shutdown.wait());
    });
    let signal = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(0));
    let seen = signal.clone();
    let sessions_dir = site.paths.home().join("sessions");
    let site_paths = &site.paths;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(failed("cannot start the runtime".into()));
    let home = site.paths.home();
    runtime.and_then(|runtime| {
        runtime.block_on(async {
            let mut stopped = stopped;
            let config = match config {
                Some(config) => config,
                None => match setup_mode(
                    home,
                    site.env,
                    out,
                    &mut stopped,
                    SETUP_TIMEOUT,
                    &setup_start,
                )
                .await?
                {
                    // An `.env` gone again since is no configuration.
                    Ended::Written => hyscribe_config::load(home, site.env)
                        .and_then(|config| config.ok_or(ConfigError::Missing("DB_NAME")))
                        .map_err(|err| err.to_string())?,
                    Ended::Stopped(number) => {
                        seen.store(number, std::sync::atomic::Ordering::SeqCst);
                        return Ok(());
                    }
                },
            };
            let db = hyscribe_db::Db::connect(&config.database)
                .await
                .map_err(|err| format!("cannot connect to the database: {err}"))?;
            let writer = open_writer(&config.database, config.mode, out).await?;
            let site =
                hyscribe_http::Site::load(db, config.home_url.clone(), config.site_url.clone())
                    .await
                    .map_err(|err| format!("cannot read the site options: {err}"))?
                    .with_salts(config.salts.clone())
                    .with_public_users(config.rest_public_users)
                    .with_constants(config.constants)
                    .with_proxy(config.server.behind_proxy)
                    .with_login_limits(config.login_limits)
                    .with_install(install(&config, &site.server.0))
                    .with_writer(writer);
            let fetch = config.http_outbound.then(outbound);
            let fetching = outbound_line(fetch.as_ref());
            let site = site.with_fetch(fetch);
            let hooks = std::sync::Arc::new(hyscribe_mail::NoHooks);
            let (site, mailing) = site.with_mail(&config.mail, hooks);
            // Logging in needs primary mode.
            let site = match config.mode {
                Mode::Primary => site.with_sessions(Sessions::new(sessions_dir)),
                Mode::Shadow => site,
            };
            let site = std::sync::Arc::new(site);
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
            let _ = writeln!(out, "{fetching}");
            let _ = writeln!(out, "{mailing}");
            let (lines, cron) = cron::service(&config, site_paths.home());
            for line in lines {
                let _ = writeln!(out, "{line}");
            }
            let _ = out.flush();
            // After the log lines, so `service start` returns with them logged.
            lock.listening();
            let stop = std::sync::Arc::new(tokio::sync::Notify::new());
            let stopping = stop.clone();
            let server = hyscribe_http::serve(listener, site.clone(), async move {
                // The sender only goes away after sending.
                let number = stopped.await.unwrap_or(0);
                seen.store(number, std::sync::atomic::Ordering::SeqCst);
                stopping.notify_one();
            });
            match (cron, site.writer()) {
                (Some(cron), Some(writer)) => {
                    let task = cron::task(&writer, &cron, (cron::EVERY, &stop), out);
                    tokio::join!(server, task);
                }
                _ => server.await,
            }
            Ok(())
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

/// The writer of `mode`, none in shadow mode (ADR 0010), with a log line
/// for each write it refuses: tables that cannot roll back, a `db_version` it does not write to.
async fn open_writer<W: Write + ?Sized>(
    database: &hyscribe_config::Database,
    mode: Mode,
    out: &mut W,
) -> Result<Option<hyscribe_db::Writer>, String> {
    let writer = hyscribe_db::Writer::open(database, mode)
        .await
        .map_err(|err| format!("cannot open the writer: {err}"))?;
    if let Some(writer) = &writer {
        for refusal in writer.refusals().await {
            let _ = writeln!(out, "writes refused: {refusal}");
        }
    }
    Ok(writer)
}

/// Formats an I/O failure with what was being done.
/// The outbound client (ADR 0019) over the host's certificate store.
fn outbound() -> hyscribe_fetch::Client {
    let net = std::sync::Arc::new(hyscribe_fetch::System);
    hyscribe_fetch::Client::new(hyscribe_fetch::host_roots(), net, std::sync::Arc::new(()))
}

/// What the start log says of outbound fetches: a host without a
/// certificate store fetches nothing over HTTPS.
fn outbound_line(client: Option<&hyscribe_fetch::Client>) -> &'static str {
    match client {
        None => "outbound fetches off",
        Some(client) if client.has_roots() => "outbound fetches on",
        Some(_) => "outbound fetches on over HTTP only: no certificate store was found",
    }
}

fn failed(context: String) -> impl FnOnce(std::io::Error) -> String {
    move |err| format!("{context}: {err}")
}

fn service(
    command: ServiceCommand,
    site: &Site<'_>,
    term: &mut setup::Terminal<'_>,
) -> Result<Reply, String> {
    let paths = &site.paths;
    let error = |err: hyscribe_service::Error| err.to_string();
    let start = |term: &mut setup::Terminal<'_>| -> Result<String, String> {
        let setup = server_config(site, term)?.is_none();
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
        let mut text = format!("{started}\nlogs: {}\n", paths.log_file().display());
        // Without a configuration the server is in setup mode, its secret
        // in a file that only a terminal is shown.
        if setup {
            let file = paths.setup_secret_file();
            match std::fs::read_to_string(&file) {
                Ok(open) if site.screen => text.push_str(&format!("Setup mode: {open}")),
                _ => text.push_str(&format!(
                    "Setup mode: the address to open and the one-time setup secret are in {} \
                     until setup mode ends\n",
                    file.display()
                )),
            }
        }
        Ok(text)
    };
    let stop = || {
        hyscribe_service::stop(paths, STOP_GRACE)
            .map(|stopped| format!("{stopped}\n"))
            .map_err(error)
    };
    match command {
        ServiceCommand::Start => start(term).map(Reply::text),
        ServiceCommand::Stop => stop().map(Reply::text),
        ServiceCommand::Restart => {
            refuse_root(site.euid)?;
            let stopped = stop()?;
            Ok(Reply::text(stopped + &start(term)?))
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
            hyscribe_service::follow(paths, lines, term.out, hyscribe_service::POLL)
                .map_err(error)?;
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
         Database: {user}@{address}/{name} (password {password}, prefix {prefix}, charset {charset})\n\
         Listen: {bind}:{http_port}{proxy}\n\
         Mode: {mode}\n\
         Uploads: {uploads}\n\
         Keys and salts: {salts} of 8 set\n",
        user = db.user,
        address = db.address(),
        name = db.name,
        prefix = db.prefix.as_str(),
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

mod cron;
mod setup;

use setup::web::{Ended, SETUP_TIMEOUT, setup_mode};

/// Parsers of request input that only the fuzz targets in `fuzz/` call
/// from outside.
#[doc(hidden)]
pub mod fuzzing {
    pub use crate::setup::web::read_form;
}

#[cfg(test)]
mod tests;

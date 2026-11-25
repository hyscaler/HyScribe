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
use hyscribe_ext::installed::Vouch;
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
    /// Plugins: HyScribe extensions packaged as `.hyx` files.
    Plugin {
        #[command(subcommand)]
        command: PluginCommand,
    },
    /// WP-Cron, for DISABLE_WP_CRON sites whose system cron runs it.
    Cron {
        #[command(subcommand)]
        command: CronCommand,
    },
}

#[derive(Debug, Subcommand)]
enum PluginCommand {
    /// Install a `.hyx` package into plugins/<slug>/, replacing an older
    /// one; activate it from the Plugins screen. A local file's signature,
    /// `<file>.minisig`, must verify against the registry's key list kept
    /// in registry/ or a key in EXTENSION_TRUSTED_KEYS; a name is installed
    /// from the hosted registry at EXTENSION_REGISTRY.
    Install {
        /// A package's path (ending in .hyx or holding a /), or a plugin's
        /// name in the hosted registry, <name>[@<version>].
        target: String,
        /// A local file: install it without a signature this site trusts.
        #[arg(long)]
        unsigned: bool,
        #[command(flatten)]
        allow: AllowArgs,
        /// From the registry: replace a plugin installed from a local file.
        #[arg(long)]
        replace_local: bool,
    },
    /// Update plugins installed from the hosted registry, or list the
    /// updates it offers.
    #[command(group(clap::ArgGroup::new("which").required(true).args(["ids", "check", "all"])))]
    Update {
        /// The plugins' ids.
        ids: Vec<String>,
        /// List the updates and revocations, install nothing.
        #[arg(long)]
        check: bool,
        /// Every plugin installed from the registry.
        #[arg(long)]
        all: bool,
        #[command(flatten)]
        allow: AllowArgs,
    },
    /// Delete an installed plugin's folder and kept packages; an active
    /// one is refused (deactivate it from the Plugins screen first).
    Delete {
        /// The plugin's id, its folder's name in plugins/.
        id: String,
    },
}

/// What installing from the registry may do beyond the default.
#[derive(Debug, clap::Args)]
struct AllowArgs {
    /// From the registry: install a version older than the installed one.
    #[arg(long)]
    downgrade: bool,
    /// From the registry: install a version asking for other grants or
    /// limits than those approved; it runs again once activated.
    #[arg(long)]
    allow_new_grants: bool,
}

impl AllowArgs {
    fn allow(&self, replace_local: bool) -> hyscribe_registry::install::Allow {
        hyscribe_registry::install::Allow {
            downgrade: self.downgrade,
            new_grants: self.allow_new_grants,
            replace_local,
        }
    }
}

/// Which plugins `update` works on; clap allows one of the three.
fn which(check: bool, all: bool, ids: &[String]) -> hyscribe_registry::install::Update<'_> {
    use hyscribe_registry::install::Update;
    if check {
        Update::Check
    } else if all {
        Update::All
    } else {
        Update::Ids(ids)
    }
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
        Command::Plugin {
            command:
                PluginCommand::Install {
                    target,
                    unsigned,
                    allow,
                    replace_local,
                },
        } => refuse_root(euid).and_then(|()| {
            if registry::is_file(&target) {
                install_plugin(&home, &env, Path::new(&target), unsigned).map(Reply::text)
            } else {
                let asked = registry::Asked::Install(&target);
                registry::command(&home, &env, &outbound(), asked, allow.allow(replace_local))
            }
        }),
        Command::Plugin {
            command:
                PluginCommand::Update {
                    ids,
                    check,
                    all,
                    allow,
                },
        } => refuse_root(euid).and_then(|()| {
            let asked = registry::Asked::Update(which(check, all, &ids));
            registry::command(&home, &env, &outbound(), asked, allow.allow(false))
        }),
        Command::Plugin {
            command: PluginCommand::Delete { id },
        } => refuse_root(euid)
            .and_then(|()| delete_plugin(&home, &env, &id))
            .map(Reply::text),
        Command::Cron {
            command: CronCommand::Run,
        } => refuse_root(euid)
            .and_then(|()| configured(&home, &env))
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

/// `hyscribe plugin install <file>`: the package read (no larger than a
/// package may be), vouched for by its signature beside it or the
/// operator's `--unsigned` (ADR 0024), checked, its component loaded once,
/// then unpacked into `plugins/` and kept in `packages/` (MVP 11.3).
fn install_plugin(
    home: &Path,
    env: &HashMap<String, String>,
    file: &Path,
    unsigned: bool,
) -> Result<String, String> {
    let read = hyscribe_ext::installed::read_at_most(file, hyscribe_ext::hyx::MAX_TOTAL);
    let bytes = read.map_err(|err| format!("cannot read {}: {err}", file.display()))?;
    let mut path = file.as_os_str().to_owned();
    path.push(".minisig");
    let vouched = signed::vouched(home, env, &bytes, Path::new(&path), signed::now());
    let (vouch, how) = match &vouched {
        Ok((signer, key, signature)) => (
            Vouch::Signed(*signer, key, signature),
            signed::told(*signer, key),
        ),
        Err(why) if unsigned => (Vouch::Unsigned, format!("unsigned ({why})")),
        Err(why) => {
            return Err(format!(
                "{why}; a package without a signature this site trusts installs only with --unsigned"
            ));
        }
    };
    let folders = (home.join("plugins"), home.join("packages"));
    let manifest = hyscribe_ext::installed::install((&folders.0, &folders.1), &bytes, vouch)?;
    Ok(format!(
        "Installed {} {} into plugins/{}/, standing in for {}, {how}. Activate it from the Plugins screen.\n",
        manifest.name,
        manifest.version,
        manifest.id,
        manifest.replaces.unwrap_or_default()
    ))
}

/// `hyscribe plugin delete <id>`: `plugins/<id>/` and the plugin's kept
/// packages removed unless `active_plugins` names one of its files. PHP's
/// `delete_plugins()` writes only for the PHP plugin's own files (its
/// `uninstall_plugins` entry and update transient), which stay as the
/// extension is not them, so nothing is written to the database: it is
/// read, and only for a plugin that is installed.
fn delete_plugin(home: &Path, env: &HashMap<String, String>, id: &str) -> Result<String, String> {
    let folders = (home.join("plugins"), home.join("packages"));
    let active = || active_plugins(&configured(home, env)?);
    let deleted = hyscribe_ext::installed::delete((&folders.0, &folders.1), id, &active)?;
    Ok(format!(
        "Deleted plugins/{id}/ and {} kept package(s).\n",
        deleted.len()
    ))
}

/// The files `active_plugins` names, read from the database.
fn active_plugins(config: &hyscribe_config::Config) -> Result<Vec<String>, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(failed("cannot start the runtime".into()));
    let read = |runtime: tokio::runtime::Runtime| {
        let stored = runtime.block_on(async {
            let db = hyscribe_db::Db::connect(&config.database).await?;
            db.option("active_plugins").await
        });
        stored.map_err(|err| format!("cannot read active_plugins: {err}"))
    };
    let stored = runtime.and_then(read)?;
    let Some(stored) = stored else {
        return Ok(Vec::new());
    };
    match hyscribe_phpser::maybe_unserialize(&stored) {
        hyscribe_phpser::Maybe::Value(hyscribe_phpser::Value::Array(listed)) => Ok(listed
            .iter()
            .filter_map(|(_, entry)| match entry {
                hyscribe_phpser::Value::String(file) => Some(String::from_utf8_lossy(file).into()),
                _ => None,
            })
            .collect()),
        _ => Err("active_plugins is not a list, so no plugin can be told inactive".into()),
    }
}

/// The configuration of a set up site.
fn configured(
    home: &Path,
    env: &HashMap<String, String>,
) -> Result<hyscribe_config::Config, String> {
    hyscribe_config::load(home, env)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "no configuration: run ./hyscribe setup".to_string())
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
/// The peers `X-Forwarded-For` is believed from, none when not behind a
/// proxy.
fn trusted_proxies(config: &Config) -> Option<Vec<hyscribe_config::proxies::Cidr>> {
    if config.server.behind_proxy {
        Some(config.trusted_proxies.clone())
    } else {
        None
    }
}

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
                    .with_author_archives(config.author_archives)
                    .with_constants(config.constants)
                    .with_proxy(trusted_proxies(&config))
                    .with_time_limit(std::time::Duration::from_secs(u64::from(
                        config.request_time_limit,
                    )))
                    .with_login_limits(config.login_limits)
                    .with_uploads(hyscribe_http::Uploads::new(
                        uploads_dir(&config, home),
                        config.upload_max_size,
                        config.image_limits,
                    ))
                    .with_install(install(&config, &site.server.0))
                    .with_extensions(&home.join("plugins"), &home.join("themes"))
                    .with_registry(&home.join("registry"), signed::roots())
                    .with_writer(writer);
            let fetch = config.http_outbound.then(outbound);
            let fetching = outbound_line(fetch.as_ref());
            let site = site
                .with_fetch(fetch)
                .with_feed_hosts(&config.rss_allowed_hosts)
                .with_embed_hosts(config.embed_allowed.providers, &config.embed_allowed.hosts);
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
            let (mut lines, cron) = cron::service(&config, site_paths.home());
            let (line, check) = updates::service(&config, site_paths.home(), outbound);
            lines.extend(line);
            for line in lines {
                let _ = writeln!(out, "{line}");
            }
            let _ = out.flush();
            // After the log lines, so `service start` returns with them logged.
            lock.listening();
            let stops = [(); 2].map(|()| std::sync::Arc::new(tokio::sync::Notify::new()));
            let stopping = stops.clone();
            let server = hyscribe_http::serve(listener, site.clone(), async move {
                // The sender only goes away after sending.
                let number = stopped.await.unwrap_or(0);
                seen.store(number, std::sync::atomic::Ordering::SeqCst);
                stopping.iter().for_each(|stop| stop.notify_one());
            });
            let log = std::sync::Mutex::new(&mut *out);
            let cron = async {
                if let (Some(cron), Some(writer)) = (cron, site.writer()) {
                    let every = (cron::EVERY, &*stops[0]);
                    cron::task(&writer, &cron, every, &mut Shared(&log)).await;
                }
            };
            let every = (updates::EVERY, &*stops[1]);
            let mut checks = Shared(&log);
            let check = updates::task(check.as_ref(), every, &mut checks);
            tokio::join!(server, cron, check);
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

/// One log written by the service's tasks, a call at a time, so their
/// lines never mix.
struct Shared<'a, W: Write>(&'a std::sync::Mutex<W>);

impl<W: Write> Write for Shared<'_, W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut out = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        out.write(buf)
    }

    fn write_fmt(&mut self, args: std::fmt::Arguments<'_>) -> std::io::Result<()> {
        let mut out = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        out.write_fmt(args)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let mut out = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        out.flush()
    }
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

/// The uploads folder: `UPLOADS_DIR`, an existing site's, else `uploads/`
/// in the installation's folder.
fn uploads_dir(config: &Config, home: &Path) -> std::path::PathBuf {
    config
        .uploads_dir
        .clone()
        .unwrap_or_else(|| home.join("uploads"))
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
mod registry;
mod setup;
mod signed;
mod updates;

use setup::web::{Ended, SETUP_TIMEOUT, setup_mode};

/// Parsers of request input that only the fuzz targets in `fuzz/` call
/// from outside.
#[doc(hidden)]
pub mod fuzzing {
    pub use crate::setup::web::read_form;
}

#[cfg(test)]
mod tests;

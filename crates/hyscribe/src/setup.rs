// SPDX-License-Identifier: AGPL-3.0-or-later

//! The setup wizard (docs/SETUP_WIZARD.md) without the compatibility scan:
//! it reads an existing site's `wp-config.php`, asks for what it cannot
//! read, checks the database and writes `.env`. `hyscribe setup` asks in
//! the terminal, setup mode ([`web`]) in the browser, through the same
//! steps. It never writes to the database, and never shows a password, key
//! or salt.

use std::collections::HashMap;
use std::io::{BufRead, IsTerminal as _, Read as _, Write};
use std::path::{Path, PathBuf};

use hyscribe_config::setup::{self, Setting};
use hyscribe_config::wpconfig::{self, Value};
use hyscribe_config::{Config, Database, ENV_FILE, Mode, SALT_KEYS};
use hyscribe_db::{DB_VERSIONS, Db, DbError};
use hyscribe_write::install::{Answers, Installed, Plan};

use crate::{Tty, failed};

pub(crate) mod web;

/// The terminal the wizard talks to.
pub(crate) struct Terminal<'a> {
    /// The answers.
    pub input: &'a mut dyn BufRead,
    /// The questions and what was found.
    pub out: &'a mut dyn Write,
    /// The input's terminal, if it is one, whose echo is off for passwords.
    pub tty: Option<Tty<'a>>,
}

impl Terminal<'_> {
    pub(crate) fn say(&mut self, text: &str) -> Result<(), String> {
        self.out
            .write_all(text.as_bytes())
            .and_then(|()| self.out.flush())
            .map_err(|err| err.to_string())
    }

    fn line(&mut self) -> Result<String, String> {
        let mut line = String::new();
        match self.input.read_line(&mut line) {
            Ok(0) => {
                Err("setup stopped: the input ended before every question was answered".into())
            }
            Ok(_) => Ok(line.trim_end_matches(['\n', '\r']).to_string()),
            Err(err) => Err(format!("setup stopped: {err}")),
        }
    }

    /// Asks `question`; an empty answer takes `default`.
    fn ask(&mut self, question: &str, default: &str) -> Result<String, String> {
        if default.is_empty() {
            self.say(&format!("? {question}: "))?;
        } else {
            self.say(&format!("? {question} [{default}]: "))?;
        }
        let answer = self.line()?;
        Ok(if answer.is_empty() {
            default.to_string()
        } else {
            answer
        })
    }

    /// Asks for a password with the terminal's echo off.
    fn secret(&mut self, question: &str) -> Result<String, String> {
        self.say(&format!("? {question}: "))?;
        // The terminal's settings, when it is one and took echo off.
        let saved = self.tty.and_then(echo_off);
        let answer = self.line();
        if let Some(saved) = saved {
            restore(saved);
            // The Enter that ended the answer was not echoed either.
            self.say("\n")?;
        }
        answer
    }

    /// Asks a yes or no question, three times at most.
    fn yes(&mut self, question: &str, default: bool) -> Result<bool, String> {
        let hint = if default { "Y/n" } else { "y/N" };
        for _ in 0..3 {
            let answer = self.ask(&format!("{question}? [{hint}]"), "")?;
            match answer.to_ascii_lowercase().as_str() {
                "" => return Ok(default),
                "y" | "yes" => return Ok(true),
                "n" | "no" => return Ok(false),
                _ => self.say("  Answer yes or no.\n")?,
            }
        }
        Err(format!(
            "setup stopped: \"{question}?\" was not answered yes or no"
        ))
    }

    /// Whether someone answers at a terminal.
    pub(crate) fn interactive(&self) -> bool {
        self.tty.is_some_and(|tty| tty.is_terminal())
    }

    /// Asks where to set up, three times at most: true for the terminal.
    pub(crate) fn in_terminal(&mut self) -> Result<bool, String> {
        let question =
            "No .env found. Set up in the browser or in the terminal? [browser/terminal]";
        for _ in 0..3 {
            match self.ask(question, "")?.to_ascii_lowercase().as_str() {
                "" | "b" | "browser" => return Ok(false),
                "t" | "terminal" => return Ok(true),
                _ => self.say("  Answer browser or terminal.\n")?,
            }
        }
        Err("setup stopped: neither browser nor terminal was chosen".into())
    }
}

/// The terminal's settings, after turning its echo off; none when it is no
/// terminal or kept its echo.
#[cfg(unix)]
fn echo_off(fd: Tty<'_>) -> Option<(Tty<'_>, rustix::termios::Termios)> {
    use rustix::termios::{LocalModes, OptionalActions, tcgetattr, tcsetattr};
    let termios = tcgetattr(fd).ok().filter(|termios| {
        let mut quiet = termios.clone();
        quiet.local_modes.remove(LocalModes::ECHO);
        tcsetattr(fd, OptionalActions::Now, &quiet).is_ok()
    });
    termios.map(|termios| (fd, termios))
}

/// The terminal's settings back as [`echo_off`] found them.
#[cfg(unix)]
fn restore((fd, termios): (Tty<'_>, rustix::termios::Termios)) {
    use rustix::termios::{OptionalActions, tcsetattr};
    let _ = tcsetattr(fd, OptionalActions::Now, &termios);
}

/// The console's mode, after turning its echo off; none when it is no
/// console or kept its echo.
#[cfg(windows)]
fn echo_off(handle: Tty<'_>) -> Option<(std::fs::File, u32)> {
    use winapi_util::console::{mode, set_mode};
    const ENABLE_ECHO_INPUT: u32 = 0x0004;
    let console = std::fs::File::from(handle.try_clone_to_owned().ok()?);
    let found = mode(&console).ok()?;
    set_mode(&console, found & !ENABLE_ECHO_INPUT).ok()?;
    Some((console, found))
}

/// The console's mode back as [`echo_off`] found it.
#[cfg(windows)]
fn restore((console, found): (std::fs::File, u32)) {
    let _ = winapi_util::console::set_mode(&console, found);
}

/// What the wizard read about an existing site.
#[derive(Clone)]
pub(crate) struct Found {
    /// The keys copied.
    pub values: Vec<(&'static str, String)>,
    /// The keys to ask for, and the constants not copied.
    pub unread: Vec<&'static str>,
    /// What was read, to say so.
    pub source: String,
    /// `wp-content`, where `object-cache.php` and the uploads would be.
    pub content: Content,
    /// The uploads folder a literal `UPLOADS` gives, `ABSPATH . UPLOADS`.
    pub uploads: Option<String>,
}

/// Where `wp-content` is.
#[derive(Clone)]
pub(crate) enum Content {
    /// No WordPress folder was read.
    Unknown,
    /// Found.
    Known(PathBuf),
    /// Computed by PHP: asked for, with this answer by default.
    Asked(PathBuf),
}

/// The details asked for when there is no `wp-config.php`.
pub(crate) fn manual() -> Found {
    Found {
        values: Vec::new(),
        unread: vec!["DB_NAME", "DB_USER", "DB_PASSWORD", "DB_HOST", "DB_PREFIX"],
        source: String::new(),
        content: Content::Unknown,
        uploads: None,
    }
}

/// The site whose WordPress folder or `wp-config.php` is `answer`.
pub(crate) fn from_file(answer: &str) -> Result<Found, String> {
    let (file, abspath) = locate(Path::new(answer))?;
    let config = read_wp_config(&file)?;
    let default = abspath.join("wp-content");
    let content = match setup::setting(&config, "WP_CONTENT_DIR") {
        Setting::Literal(Value::String(dir)) => Content::Known(PathBuf::from(dir)),
        Setting::Unset => Content::Known(default),
        _ => Content::Asked(default),
    };
    let source = format!("Read {}", file.display());
    Ok(found(&config, source, content, Some(&abspath)))
}

/// The site of the `wp-config.php` text `source`, given without its folder.
pub(crate) fn from_text(source: &[u8]) -> Result<Found, String> {
    let config = wpconfig::parse(source).map_err(|err| err.to_string())?;
    let content = match setup::setting(&config, "WP_CONTENT_DIR") {
        Setting::Literal(Value::String(dir)) => Content::Known(PathBuf::from(dir)),
        _ => Content::Unknown,
    };
    let source = "Read the wp-config.php given".into();
    Ok(found(&config, source, content, None))
}

/// What `config` gives, at `abspath` when the WordPress folder is known.
fn found(
    config: &wpconfig::WpConfig,
    source: String,
    content: Content,
    abspath: Option<&Path>,
) -> Found {
    let mut import = setup::import(config);
    // `wp_upload_dir()` takes `UPLOADS` relative to the WordPress folder.
    let uploads = match (setup::setting(config, "UPLOADS"), abspath) {
        (Setting::Unset, _) => None,
        (Setting::Literal(Value::String(dir)), Some(abspath)) => {
            Some(format!("{}/{dir}", abspath.display()))
        }
        _ => {
            import.unread.push("UPLOADS");
            None
        }
    };
    Found {
        values: import.values,
        unread: import.unread,
        source,
        content,
        uploads,
    }
}

impl Found {
    /// The site's uploads folder: `UPLOADS`'s, else the one in `content`,
    /// its `wp-content`.
    pub(crate) fn uploads(&self, content: Option<&Path>) -> String {
        match (&self.uploads, content) {
            (Some(uploads), _) => uploads.clone(),
            (None, Some(dir)) => dir.join("uploads").display().to_string(),
            (None, None) => String::new(),
        }
    }

    /// What was read and what was not, as lines to show.
    pub(crate) fn summary(&self) -> String {
        let mut text = String::new();
        if !self.source.is_empty() {
            text.push_str(&format!("  {}\n", self.source));
        }
        if !self.unread.is_empty() && !self.source.is_empty() {
            text.push_str(&format!(
                "  Not a literal there, or missing: {}\n",
                self.unread.join(", ")
            ));
        }
        text
    }
}

/// A question for a `.env` key.
pub(crate) struct Question {
    /// The key the answer goes to.
    pub key: &'static str,
    /// The question.
    pub text: &'static str,
    /// The answer an empty one takes.
    pub default: &'static str,
    /// Whether the answer is a password, never shown.
    pub secret: bool,
}

/// The questions for `key`, which `wp-config.php` did not give; none for a
/// constant, which is not asked for but noted.
pub(crate) fn questions(key: &'static str) -> Vec<Question> {
    let (text, default) = match key {
        "DB_NAME" => ("Database name", ""),
        "DB_USER" => ("Database user", ""),
        "DB_PASSWORD" => ("Database password", ""),
        "DB_HOST" => ("Database host", "localhost"),
        "DB_CHARSET" => ("Database charset", "utf8mb4"),
        "DB_PREFIX" => ("Table prefix", "wp_"),
        "WP_HOME" => ("WP_HOME, the site address (empty for the stored one)", ""),
        "WP_SITEURL" => (
            "WP_SITEURL, the HyScribe address (empty for the stored one)",
            "",
        ),
        _ => return Vec::new(),
    };
    let mut asked = vec![Question {
        key,
        text,
        default,
        secret: key == "DB_PASSWORD",
    }];
    // `DB_HOST` comes with its port.
    if key == "DB_HOST" {
        asked.push(Question {
            key: "DB_PORT",
            text: "Database port",
            default: "3306",
            secret: false,
        });
    }
    asked
}

/// Why a constant of `wp-config.php` was not copied.
pub(crate) fn not_copied(key: &str) -> String {
    match key {
        "EMPTY_TRASH_DAYS" => format!(
            "{key} is not a literal number of days, so it is not copied: the default of 30 holds"
        ),
        "UPLOADS" => format!(
            "{key} is not a literal in a WordPress folder given, so it is not copied: \
             give the site's uploads folder"
        ),
        _ => format!(
            "{key} is not a literal true or false, so it is not copied: the safer value holds"
        ),
    }
}

/// What both wizards ask after the database details.
pub(crate) struct Choices {
    /// Shadow mode, or primary.
    pub shadow: bool,
    /// `DB_READ_USER` and its password, when given.
    pub read_user: Option<(String, String)>,
    /// `BEHIND_PROXY`.
    pub behind_proxy: bool,
    /// `BIND_HOST`.
    pub bind_host: String,
    /// `PORT`.
    pub port: String,
    /// `UPLOADS_DIR`, empty for none.
    pub uploads: String,
}

impl Choices {
    fn push(self, values: &mut Vec<(&'static str, String)>) {
        values.push((
            "MODE",
            if self.shadow { "shadow" } else { "primary" }.into(),
        ));
        if let Some((user, password)) = self.read_user {
            values.push(("DB_READ_USER", user));
            values.push(("DB_READ_PASSWORD", password));
        }
        values.push(("BIND_HOST", self.bind_host));
        values.push(("PORT", self.port));
        values.push(("BEHIND_PROXY", self.behind_proxy.to_string()));
        if !self.uploads.is_empty() {
            values.push(("UPLOADS_DIR", self.uploads));
        }
    }
}

/// `hyscribe setup`: runs the wizard and says what it did.
pub(crate) fn command(home: &Path, term: &mut Terminal<'_>) -> Result<String, String> {
    Ok(match run(home, term)? {
        Some(path) => format!(
            "Wrote {} (mode 600)\nStart the site with: ./hyscribe service start\n",
            path.display()
        ),
        None => format!("Kept {}; nothing written.\n", home.join(ENV_FILE).display()),
    })
}

/// Runs the wizard for the installation in `home`: where `.env` went, or
/// `None` when an existing one is kept.
pub(crate) fn run(home: &Path, term: &mut Terminal<'_>) -> Result<Option<PathBuf>, String> {
    let env = home.join(ENV_FILE);
    let replace = env.symlink_metadata().is_ok();
    if replace && !term.yes(&format!("{} exists. Replace it", env.display()), false)? {
        return Ok(None);
    }
    let answer = term.ask(
        "Path to the WordPress folder or its wp-config.php (none if there is none, new for a new site)",
        &find_wordpress(home),
    )?;
    let new_site = answer == "new";
    let found = if answer.is_empty() || answer == "none" || new_site {
        manual()
    } else {
        from_file(&answer)?
    };
    term.say(&found.summary())?;
    let content = match &found.content {
        Content::Unknown => None,
        Content::Known(dir) => Some(dir.clone()),
        Content::Asked(default) => Some(PathBuf::from(term.ask(
            "Path to wp-content (WP_CONTENT_DIR is computed by PHP)",
            &default.display().to_string(),
        )?)),
    };
    let site_uploads = found.uploads(content.as_deref());
    let mut values = found.values;
    for key in found.unread {
        for question in questions(key) {
            let answer = if question.secret {
                term.secret(question.text)?
            } else {
                term.ask(question.text, question.default)?
            };
            if !answer.is_empty() {
                values.push((question.key, answer));
            }
        }
        if questions(key).is_empty() {
            term.say(&format!("  {}\n", not_copied(key)))?;
        }
    }
    // A new site is written to, so it starts in primary mode.
    let shadow = !new_site && term.yes("Start in shadow mode (read only) first", true)?;
    let read_user = term.ask(
        "Database user granted only SELECT, for every read (empty for none)",
        "",
    )?;
    let read_user = if read_user.is_empty() {
        None
    } else {
        Some((read_user, term.secret("Its password")?))
    };
    let behind_proxy = term.yes("Running HyScribe behind a proxy", false)?;
    let bind_host = term.ask("Address to listen on", "127.0.0.1")?;
    let port = term.ask("Port to listen on", "8080")?;
    let plan = if new_site {
        Some(ask_new_site(term, &default_home(&bind_host, &port))?)
    } else {
        None
    };
    let uploads = term.ask(
        "The site's uploads folder (empty for hyscribe/uploads)",
        &site_uploads,
    )?;
    let choices = Choices {
        shadow,
        read_user,
        behind_proxy,
        bind_host,
        port,
        uploads,
    };
    choices.push(&mut values);
    let (_, path) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(failed("cannot start the runtime".into()))
        .and_then(|runtime| {
            let content = content.as_deref();
            let mut say = |report: &str| term.say(report);
            let settled = settle(home, values, content, plan.as_ref(), replace, &mut say);
            runtime.block_on(settled)
        })?;
    Ok(Some(path))
}

/// The site address offered for a site listening on `bind_host` and `port`:
/// where a browser on this machine opens it.
pub(crate) fn default_home(bind_host: &str, port: &str) -> String {
    let host = match bind_host {
        "0.0.0.0" | "::" => "127.0.0.1".to_string(),
        host if host.contains(':') => format!("[{host}]"),
        host => host.to_string(),
    };
    match port {
        "80" => format!("http://{host}"),
        port => format!("http://{host}:{port}"),
    }
}

/// PHP's `time()`.
pub(crate) fn now() -> i64 {
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    since.map_or(0, |since| since.as_secs() as i64)
}

/// What PHP's install screen asks, three times at most: the answers that
/// pass its checks, as the plan of the install.
fn ask_new_site(term: &mut Terminal<'_>, home: &str) -> Result<Plan, String> {
    for _ in 0..3 {
        let title = term.ask("Site title", "")?;
        let login = term.ask("Administrator user name", "")?;
        let password = term.secret("Administrator password (empty for a generated one)")?;
        let repeat = term.secret("Password again")?;
        let email = term.ask("Administrator email address", "")?;
        let hidden = term.yes("Discourage search engines from indexing this site", false)?;
        let home = term.ask("Site address, where visitors will open it", home)?;
        let answers = Answers {
            title: &title,
            login: &login,
            password: &password,
            repeat: &repeat,
            email: &email,
            public: !hidden,
            home: &home,
        };
        match Plan::new(&answers, now()) {
            Ok(plan) => return Ok(plan),
            Err(refusal) => term.say(&format!("  {refusal}\n"))?,
        }
    }
    Err("setup stopped: the new site's answers were refused three times".into())
}

/// A new site's tables, which a later failure drops again.
type Made = (Database, Installed);

/// Drops the tables of `made`, if any, after the failure `err`: the failure,
/// and what became of them.
async fn unmade(made: Option<Made>, err: String) -> String {
    let Some((database, installed)) = made else {
        return err;
    };
    match Plan::remove(&database, &installed).await {
        Ok(()) => format!("{err}; the new site's tables were dropped"),
        Err(more) => format!("{err}; dropping the new site's tables failed too: {more}"),
    }
}

/// Checks the configuration, installs the new site if there is one, tells
/// the report to `announce` and writes `.env`: what the checks found, and
/// where `.env` went. A failure after the install drops the new site's
/// tables again.
pub(crate) async fn settle(
    home: &Path,
    values: Vec<(&'static str, String)>,
    content: Option<&Path>,
    new: Option<&Plan>,
    replace: bool,
    announce: &mut impl FnMut(&str) -> Result<(), String>,
) -> Result<(String, PathBuf), String> {
    let (report, text, made) = checked(values, content, new).await?;
    // The report is told before `.env` is written, as the terminal shows it.
    match announce(&report).and_then(|()| write_env(home, &text, replace)) {
        Ok(path) => Ok((report, path)),
        Err(err) => Err(unmade(made, err).await),
    }
}

/// Generates the keys and salts not copied and checks the configuration:
/// what the checks found, and the text of `.env`.
async fn checked(
    mut values: Vec<(&'static str, String)>,
    content: Option<&Path>,
    new: Option<&Plan>,
) -> Result<(String, String, Option<Made>), String> {
    let copied = SALT_KEYS
        .iter()
        .filter(|key| values.iter().any(|(k, _)| k == *key))
        .count();
    let salts: std::io::Result<Vec<_>> = SALT_KEYS
        .into_iter()
        .filter(|key| !values.iter().any(|(k, _)| k == key))
        .map(|key| hyscribe_auth::generate_password(64, true).map(|salt| (key, salt)))
        .collect();
    let config = salts
        .map_err(failed("cannot generate keys and salts".into()))
        .and_then(|salts| {
            values.extend(salts);
            let map: HashMap<String, String> = values
                .iter()
                .map(|(key, value)| (key.to_string(), value.clone()))
                .collect();
            hyscribe_config::from_map(&map).map_err(|err| err.to_string())
        })?;
    let mut report = format!(
        "  Keys and salts: {copied} copied, {} generated\n",
        SALT_KEYS.len() - copied
    );
    let text = setup::render(&values)
        .ok_or("an answer holds a carriage return, which .env cannot hold")?;
    let mut made = None;
    if let Some(plan) = new {
        let installed = plan.install(&config.database).await?;
        report.push_str(&format!(
            "  Installed the site's {} tables and first content in {}\n",
            installed.tables.len(),
            config.database.name
        ));
        if let Some(password) = &installed.generated {
            report.push_str(&format!(
                "  Administrator password, shown once: {password}\n"
            ));
        }
        made = Some((config.database.clone(), installed));
    }
    match check(&config).await {
        Ok(found) => report.push_str(&found),
        Err(err) => return Err(unmade(made, err).await),
    }
    // A site that never had an upload has no uploads folder yet.
    if let Some(dir) = config.uploads_dir.as_ref().filter(|dir| !dir.is_dir()) {
        report.push_str(&format!(
            "  Warning: UPLOADS_DIR {} is not a folder yet\n",
            dir.display()
        ));
    }
    if let Some(cache) = content
        .map(|dir| dir.join("object-cache.php"))
        .filter(|cache| cache.exists())
    {
        report.push_str(&format!(
            "  Warning: {} is a persistent object cache. HyScribe's writes do not clear it, \
             so PHP WordPress may show stale data until it expires.\n",
            cache.display()
        ));
    }
    Ok((report, text, made))
}

/// Writes `text` to `home`'s `.env`, replacing one only when `replace`.
pub(crate) fn write_env(home: &Path, text: &str, replace: bool) -> Result<PathBuf, String> {
    setup::write(home, text, replace).map_err(failed(format!(
        "cannot write {}",
        home.join(ENV_FILE).display()
    )))
}

/// The first `wp-config.php` beside the installation: in its parent folder,
/// or in a folder next to it.
pub(crate) fn find_wordpress(home: &Path) -> String {
    let parent = home.parent().unwrap_or(home);
    let mut folders: Vec<PathBuf> = std::fs::read_dir(parent)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .collect();
    folders.sort();
    std::iter::once(parent.to_path_buf())
        .chain(folders)
        .find(|folder| folder.join("wp-config.php").is_file())
        .map(|folder| folder.display().to_string())
        .unwrap_or_default()
}

/// The `wp-config.php` for `path` and the WordPress folder, as
/// `wp-load.php` looks: in the folder, or in the one above it unless that
/// is another WordPress folder.
fn locate(path: &Path) -> Result<(PathBuf, PathBuf), String> {
    if path.is_file() {
        let folder = path.parent().unwrap_or(path).to_path_buf();
        return Ok((path.to_path_buf(), folder));
    }
    let here = path.join("wp-config.php");
    let above = path.join("../wp-config.php");
    if here.is_file() {
        Ok((here, path.to_path_buf()))
    } else if above.is_file() && !path.join("../wp-settings.php").exists() {
        Ok((above, path.to_path_buf()))
    } else {
        Err(format!(
            "no wp-config.php in {} or the folder above it",
            path.display()
        ))
    }
}

fn read_wp_config(file: &Path) -> Result<wpconfig::WpConfig, String> {
    let mut source = Vec::new();
    std::fs::File::open(file)
        .and_then(|f| {
            f.take(wpconfig::MAX_SIZE as u64 + 1)
                .read_to_end(&mut source)
        })
        .map_err(failed(format!("cannot read {}", file.display())))?;
    wpconfig::parse(&source).map_err(|err| err.to_string())
}

/// Formats a database failure with what was being done.
fn db_failed(context: String) -> impl FnOnce(DbError) -> String {
    move |err| format!("{context}: {err}")
}

/// The first run checks (ADRs 0010 and 0012): both users connect,
/// `db_version` is supported, the read user may only read, and in primary
/// mode every table written can roll back.
async fn check(config: &Config) -> Result<String, String> {
    let database = &config.database;
    let reader = database.read_user.as_ref().unwrap_or(&database.user);
    let db = Db::connect(database).await.map_err(db_failed(format!(
        "cannot connect to {} on {} as {reader}",
        database.name,
        database.address()
    )))?;
    let version = db
        .option("db_version")
        .await
        .map_err(db_failed("cannot read db_version".into()))?
        .map(|version| String::from_utf8_lossy(&version).into_owned());
    match version.as_deref().map(str::parse::<u64>) {
        // docs/COMPATIBILITY.md: older sites are upgraded by PHP first.
        Some(Ok(number)) if number < *DB_VERSIONS.start() => {
            return Err(format!(
                "the database's db_version ({number}) is older than WordPress 7.0's ({}): \
                 upgrade the site with PHP WordPress first",
                DB_VERSIONS.start()
            ));
        }
        Some(Ok(number)) if DB_VERSIONS.contains(&number) => {}
        _ => {
            return Err(format!(
                "the database's db_version ({}) is not one this release supports ({} to {}, \
                 WordPress 7.0 and 7.1)",
                version.as_deref().unwrap_or("missing"),
                DB_VERSIONS.start(),
                DB_VERSIONS.end()
            ));
        }
    }
    let mut report = format!(
        "  Connected to {} on {}, db_version {}\n",
        database.name,
        database.address(),
        version.unwrap_or_default()
    );
    if let Some(user) = &database.read_user {
        let privileges = db.privileges().await;
        privileges
            .map_err(db_failed(format!("cannot read the privileges of {user}")))
            .and_then(|privileges| {
                // Without SELECT, reading db_version has failed already.
                if privileges.iter().all(|p| p == "SELECT" || p == "USAGE") {
                    Ok(())
                } else {
                    Err(format!(
                        "DB_READ_USER {user} may only be granted SELECT; it has {}",
                        privileges.join(", ")
                    ))
                }
            })?;
        report.push_str(&format!("  DB_READ_USER {user} may only read\n"));
    }
    let mut refusals = Vec::new();
    crate::open_writer(database, Mode::Primary, &mut refusals).await?;
    let refusals = String::from_utf8_lossy(&refusals);
    match config.mode {
        Mode::Primary if !refusals.is_empty() => Err(format!(
            "primary mode cannot write as it should:\n{refusals}"
        )),
        _ => {
            for line in refusals.lines() {
                report.push_str(&format!("  Warning, in primary mode: {line}\n"));
            }
            Ok(report)
        }
    }
}

#[cfg(test)]
#[cfg(unix)]
pub(crate) mod tests;

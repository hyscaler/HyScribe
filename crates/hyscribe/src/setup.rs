// SPDX-License-Identifier: AGPL-3.0-or-later

//! The setup wizard (docs/SETUP_WIZARD.md) without the compatibility scan:
//! it reads an existing site's `wp-config.php`, asks for what it cannot
//! read, checks the database and writes `.env`. `hyscribe setup` asks in
//! the terminal, setup mode ([`web`]) in the browser, through the same
//! steps. It never writes to the database, and never shows a password, key
//! or salt.

use std::collections::HashMap;
use std::io::{BufRead, Read as _, Write};
use std::os::fd::BorrowedFd;
use std::path::{Path, PathBuf};

use hyscribe_config::setup::{self, Setting};
use hyscribe_config::wpconfig::{self, Value};
use hyscribe_config::{Config, ENV_FILE, Mode, SALT_KEYS};
use hyscribe_db::{DB_VERSIONS, Db, DbError};
use rustix::termios::{LocalModes, OptionalActions, isatty, tcgetattr, tcsetattr};

use crate::failed;

pub(crate) mod web;

/// The terminal the wizard talks to.
pub(crate) struct Terminal<'a> {
    /// The answers.
    pub input: &'a mut dyn BufRead,
    /// The questions and what was found.
    pub out: &'a mut dyn Write,
    /// The input's terminal, if it is one, whose echo is off for passwords.
    pub tty: Option<BorrowedFd<'a>>,
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
        let saved = self
            .tty
            .and_then(|fd| tcgetattr(fd).ok().map(|termios| (fd, termios)))
            .filter(|(fd, termios)| {
                let mut quiet = termios.clone();
                quiet.local_modes.remove(LocalModes::ECHO);
                tcsetattr(fd, OptionalActions::Now, &quiet).is_ok()
            });
        let answer = self.line();
        if let Some((fd, termios)) = saved {
            let _ = tcsetattr(fd, OptionalActions::Now, &termios);
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
        self.tty.is_some_and(isatty)
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
    Ok(found(&config, format!("Read {}", file.display()), content))
}

/// The site of the `wp-config.php` text `source`, given without its folder.
pub(crate) fn from_text(source: &[u8]) -> Result<Found, String> {
    let config = wpconfig::parse(source).map_err(|err| err.to_string())?;
    let content = match setup::setting(&config, "WP_CONTENT_DIR") {
        Setting::Literal(Value::String(dir)) => Content::Known(PathBuf::from(dir)),
        _ => Content::Unknown,
    };
    Ok(found(
        &config,
        "Read the wp-config.php given".into(),
        content,
    ))
}

fn found(config: &wpconfig::WpConfig, source: String, content: Content) -> Found {
    let import = setup::import(config);
    Found {
        values: import.values,
        unread: import.unread,
        source,
        content,
    }
}

impl Found {
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
            "WP_SITEURL, the WordPress address (empty for the stored one)",
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
            "{key} is not a literal number of days, so it is not copied: WordPress's 30 holds"
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

/// The uploads folder of a site whose `wp-content` is `content`.
pub(crate) fn uploads(content: Option<&Path>) -> String {
    content.map_or_else(String::new, |dir| dir.join("uploads").display().to_string())
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
        "Path to the WordPress folder or its wp-config.php (none if there is none)",
        &find_wordpress(home),
    )?;
    let found = if answer.is_empty() || answer == "none" {
        manual()
    } else {
        from_file(&answer)?
    };
    term.say(&found.summary())?;
    let content = match found.content {
        Content::Unknown => None,
        Content::Known(dir) => Some(dir),
        Content::Asked(default) => Some(PathBuf::from(term.ask(
            "Path to wp-content (WP_CONTENT_DIR is computed by PHP)",
            &default.display().to_string(),
        )?)),
    };
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
    let shadow = term.yes("Start in shadow mode (read only) first", true)?;
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
    let uploads = term.ask(
        "The site's uploads folder (empty for hyscribe/uploads)",
        &uploads(content.as_deref()),
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
    let (report, text) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(failed("cannot start the runtime".into()))
        .and_then(|runtime| runtime.block_on(checked(values, content.as_deref())))?;
    term.say(&report)?;
    write_env(home, &text, replace).map(Some)
}

/// Generates the keys and salts not copied and checks the configuration:
/// what the checks found, and the text of `.env`.
pub(crate) async fn checked(
    mut values: Vec<(&'static str, String)>,
    content: Option<&Path>,
) -> Result<(String, String), String> {
    let copied = SALT_KEYS
        .iter()
        .filter(|key| values.iter().any(|(k, _)| k == *key))
        .count();
    let salts: Vec<_> = SALT_KEYS
        .into_iter()
        .filter(|key| !values.iter().any(|(k, _)| k == key))
        .map(|key| hyscribe_auth::generate_password(64, true).map(|salt| (key, salt)))
        .collect::<Result<_, _>>()
        .map_err(failed("cannot generate keys and salts".into()))?;
    values.extend(salts);
    let map: HashMap<String, String> = values
        .iter()
        .map(|(key, value)| (key.to_string(), value.clone()))
        .collect();
    let config = hyscribe_config::from_map(&map).map_err(|err| err.to_string())?;
    let mut report = format!(
        "  Keys and salts: {copied} copied, {} generated\n",
        SALT_KEYS.len() - copied
    );
    report.push_str(&check(&config).await?);
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
    let text = setup::render(&values)
        .ok_or("an answer holds a carriage return, which .env cannot hold")?;
    Ok((report, text))
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
        "cannot connect to {} on {}:{} as {reader}",
        database.name, database.host, database.port
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
        "  Connected to {} on {}:{}, db_version {}\n",
        database.name,
        database.host,
        database.port,
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
pub(crate) mod tests;

// SPDX-License-Identifier: AGPL-3.0-or-later

//! `hyscribe setup`, the terminal setup wizard (docs/SETUP_WIZARD.md)
//! without the compatibility scan: it reads an existing site's
//! `wp-config.php`, asks for what it cannot read, checks the database and
//! writes `.env`. It never writes to the database, and never prints a
//! password, key or salt.

use std::collections::HashMap;
use std::io::{BufRead, Read as _, Write};
use std::os::fd::BorrowedFd;
use std::path::{Path, PathBuf};

use hyscribe_config::setup::{self, Setting};
use hyscribe_config::wpconfig::{self, Value};
use hyscribe_config::{Config, ENV_FILE, Mode, SALT_KEYS};
use hyscribe_db::{DB_VERSIONS, Db, DbError};
use rustix::termios::{LocalModes, OptionalActions, tcgetattr, tcsetattr};

use crate::failed;

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
    fn say(&mut self, text: &str) -> Result<(), String> {
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
        let saved = self
            .tty
            .and_then(|fd| tcgetattr(fd).ok().map(|termios| (fd, termios)));
        if let Some((fd, termios)) = &saved {
            let mut quiet = termios.clone();
            quiet.local_modes.remove(LocalModes::ECHO);
            tcsetattr(fd, OptionalActions::Now, &quiet).map_err(failed_tty)?;
        }
        let answer = self.line();
        if let Some((fd, termios)) = saved {
            tcsetattr(fd, OptionalActions::Now, &termios).map_err(failed_tty)?;
            // The Enter that ended the answer was not echoed either.
            self.say("\n")?;
        }
        answer
    }

    /// Asks a yes or no question until it is answered.
    fn yes(&mut self, question: &str, default: bool) -> Result<bool, String> {
        let hint = if default { "Y/n" } else { "y/N" };
        loop {
            let answer = self.ask(&format!("{question}? [{hint}]"), "")?;
            match answer.to_ascii_lowercase().as_str() {
                "" => return Ok(default),
                "y" | "yes" => return Ok(true),
                "n" | "no" => return Ok(false),
                _ => self.say("  Answer yes or no.\n")?,
            }
        }
    }
}

fn failed_tty(err: rustix::io::Errno) -> String {
    format!("cannot turn the terminal's echo off: {err}")
}

/// Runs the wizard for the installation in `home` and says where `.env`
/// went.
pub(crate) fn run(home: &Path, term: &mut Terminal<'_>) -> Result<String, String> {
    let env = home.join(ENV_FILE);
    let replace = env.symlink_metadata().is_ok();
    if replace && !term.yes(&format!("{} exists. Replace it", env.display()), false)? {
        return Ok(format!("Kept {}; nothing written.\n", env.display()));
    }
    let found = find_wordpress(home);
    let answer = term.ask(
        "Path to the WordPress folder or its wp-config.php (none if there is none)",
        &found,
    )?;
    let (mut values, unread, content) = if answer.is_empty() || answer == "none" {
        let keys = vec!["DB_NAME", "DB_USER", "DB_PASSWORD", "DB_HOST", "DB_PREFIX"];
        (Vec::new(), keys, None)
    } else {
        let (file, abspath) = locate(Path::new(&answer))?;
        let config = read_wp_config(&file)?;
        let import = setup::import(&config);
        term.say(&format!("  Read {}\n", file.display()))?;
        if !import.unread.is_empty() {
            term.say(&format!(
                "  Not a literal there, or missing: {}\n",
                import.unread.join(", ")
            ))?;
        }
        let content = content_dir(&config, &abspath, term)?;
        (import.values, import.unread, Some(content))
    };
    for key in unread {
        ask_for(key, &mut values, term)?;
    }
    let copied = SALT_KEYS
        .iter()
        .filter(|key| values.iter().any(|(k, _)| k == *key))
        .count();
    for key in SALT_KEYS {
        if !values.iter().any(|(k, _)| *k == key) {
            let salt = hyscribe_auth::generate_password(64, true)
                .map_err(failed("cannot generate keys and salts".into()))?;
            values.push((key, salt));
        }
    }
    term.say(&format!(
        "  Keys and salts: {copied} copied, {} generated\n",
        SALT_KEYS.len() - copied
    ))?;
    let shadow = term.yes("Start in shadow mode (read only) first", true)?;
    values.push(("MODE", if shadow { "shadow" } else { "primary" }.into()));
    let read_user = term.ask(
        "Database user granted only SELECT, for every read (empty for none)",
        "",
    )?;
    if !read_user.is_empty() {
        let password = term.secret("Its password")?;
        values.push(("DB_READ_USER", read_user));
        values.push(("DB_READ_PASSWORD", password));
    }
    let map: HashMap<String, String> = values
        .iter()
        .map(|(key, value)| (key.to_string(), value.clone()))
        .collect();
    let config = hyscribe_config::from_map(&map).map_err(|err| err.to_string())?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(failed("cannot start the runtime".into()))?;
    let report = runtime.block_on(check(&config))?;
    term.say(&report)?;
    if let Some(content) = content
        && content.join("object-cache.php").exists()
    {
        term.say(&format!(
            "  Warning: {} is a persistent object cache. HyScribe's writes do not clear it, \
             so PHP WordPress may show stale data until it expires.\n",
            content.join("object-cache.php").display()
        ))?;
    }
    let text = setup::render(&values)
        .ok_or("an answer holds a carriage return, which .env cannot hold")?;
    let path = setup::write(home, &text, replace)
        .map_err(failed(format!("cannot write {}", env.display())))?;
    Ok(format!(
        "Wrote {} (mode 600)\nStart the site with: ./hyscribe service start\n",
        path.display()
    ))
}

/// Asks for a key `wp-config.php` did not give, or notes why a capability
/// constant was not copied.
fn ask_for(
    key: &'static str,
    values: &mut Vec<(&'static str, String)>,
    term: &mut Terminal<'_>,
) -> Result<(), String> {
    let mut answer = |question: &str, default: &str| -> Result<(), String> {
        let value = term.ask(question, default)?;
        if !value.is_empty() {
            values.push((key, value));
        }
        Ok(())
    };
    match key {
        "DB_NAME" => answer("Database name", ""),
        "DB_USER" => answer("Database user", ""),
        "DB_CHARSET" => answer("Database charset", "utf8mb4"),
        "DB_PREFIX" => answer("Table prefix", "wp_"),
        "WP_HOME" => answer("WP_HOME, the site address (empty for the stored one)", ""),
        "WP_SITEURL" => answer(
            "WP_SITEURL, the WordPress address (empty for the stored one)",
            "",
        ),
        "DB_HOST" => {
            answer("Database host", "localhost")?;
            let port = term.ask("Database port", "3306")?;
            values.push(("DB_PORT", port));
            Ok(())
        }
        "DB_PASSWORD" => {
            let password = term.secret("Database password")?;
            values.push((key, password));
            Ok(())
        }
        _ => term.say(&format!(
            "  {key} is not a literal true or false, so it is not copied: the safer value holds\n"
        )),
    }
}

/// The first `wp-config.php` beside the installation: in its parent folder,
/// or in a folder next to it.
fn find_wordpress(home: &Path) -> String {
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

/// `wp-content`, where `object-cache.php` would be.
fn content_dir(
    config: &wpconfig::WpConfig,
    abspath: &Path,
    term: &mut Terminal<'_>,
) -> Result<PathBuf, String> {
    let default = abspath.join("wp-content");
    match setup::setting(config, "WP_CONTENT_DIR") {
        Setting::Literal(Value::String(dir)) => Ok(PathBuf::from(dir)),
        Setting::Unset => Ok(default),
        _ => term
            .ask(
                "Path to wp-content (WP_CONTENT_DIR is computed by PHP)",
                &default.display().to_string(),
            )
            .map(PathBuf::from),
    }
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
        Some(Ok(number)) if DB_VERSIONS.contains(&number) => {}
        // docs/COMPATIBILITY.md: older sites are upgraded by PHP first.
        Some(Ok(number)) if number < *DB_VERSIONS.start() => {
            return Err(format!(
                "the database's db_version ({number}) is older than WordPress 7.0's ({}): \
                 upgrade the site with PHP WordPress first",
                DB_VERSIONS.start()
            ));
        }
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
        let privileges = db
            .privileges()
            .await
            .map_err(db_failed(format!("cannot read the privileges of {user}")))?;
        let reads = privileges.iter().any(|p| p == "SELECT");
        if !reads || privileges.iter().any(|p| p != "SELECT" && p != "USAGE") {
            return Err(format!(
                "DB_READ_USER {user} must be granted SELECT and nothing more; it has {}",
                privileges.join(", ")
            ));
        }
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
mod tests;

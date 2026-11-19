// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_install()` (ADR 0021): the tables and rows PHP WordPress's installer
//! leaves in an empty database, for the setup wizard's new site path.
//! `install/schema.sql` and `install/rows.tsv` are what PHP made, written by
//! `reference/install/golden.sh` with the values that depend on the
//! answers or the time masked as `%name%`; [`Plan::install`] fills those in.
//! The questions and their refusals are `wp-admin/install.php`'s and
//! `wp_insert_user()`'s. The site is installed as PHP leaves one whose pretty
//! permalink probe succeeds: `permalink_structure` and
//! `rewrite_rules` are set, the rules differing for a site address with a
//! path. Not done: the notification mail (no mail is set up).

use hyscribe_config::Database;
use hyscribe_db::{DbError, Installer, Rows};
use hyscribe_kses::esc_html;
use hyscribe_render::date::{Zone, format_unix};
use hyscribe_render::formatting::{is_email, sanitize_email, sanitize_title, sanitize_user_strict};

/// Each table's `CREATE TABLE`, the prefix written `{prefix}`.
const SCHEMA: &str = include_str!("install/schema.sql");
/// The rows, each table as `@table<TAB>column...` and a line per row.
const ROWS: &str = include_str!("install/rows.tsv");
/// The `rewrite_rules` value of a site address with a path, which has no
/// rules for `robots.txt`, `favicon.ico` and the sitemap (`rows.tsv` has the
/// value of one without).
const SUBFOLDER_RULES: &str = include_str!("install/rewrite_rules_subfolder.txt");
/// The length of the password made for an empty one, as WordPress makes it.
const GENERATED_LENGTH: usize = 12;

/// What the install screen asks.
pub struct Answers<'a> {
    /// The site title, which may be empty.
    pub title: &'a str,
    /// The first administrator's user name.
    pub login: &'a str,
    /// Its password, or empty for one made at random.
    pub password: &'a str,
    /// The password again.
    pub repeat: &'a str,
    /// Its email address, which is also the site's.
    pub email: &'a str,
    /// Whether search engines may index the site (`blog_public`).
    pub public: bool,
    /// The site's address, `http://host[:port][/path]`, which PHP takes from
    /// the request.
    pub home: &'a str,
}

/// A checked install: what [`Plan::install`] writes.
pub struct Plan {
    title: String,
    login: String,
    nicename: String,
    email: String,
    password: String,
    public: bool,
    home: String,
    now: i64,
}

/// Everything but the password, which no log or panic may show.
impl std::fmt::Debug for Plan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Plan")
            .field("title", &self.title)
            .field("login", &self.login)
            .field("email", &self.email)
            .field("public", &self.public)
            .field("home", &self.home)
            .finish_non_exhaustive()
    }
}

/// What an install made.
pub struct Installed {
    /// The password made for an empty one, shown once as PHP shows it.
    pub generated: Option<String>,
    /// The tables created, by name without the prefix.
    pub tables: Vec<String>,
}

/// Where an install draws the password it makes and the hash it stores.
struct Secrets {
    generate: fn(usize, bool) -> std::io::Result<String>,
    hash: fn(&[u8]) -> std::io::Result<String>,
}

/// The made password is not printed, whatever prints it.
impl std::fmt::Debug for Installed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Installed")
            .field("generated", &self.generated.as_ref().map(|_| "<password>"))
            .field("tables", &self.tables)
            .finish()
    }
}

/// Why an install stopped, in words for the person at the wizard.
#[derive(Debug)]
pub struct Failure(String);

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<DbError> for Failure {
    fn from(err: DbError) -> Failure {
        Failure(err.to_string())
    }
}

impl From<std::io::Error> for Failure {
    fn from(err: std::io::Error) -> Failure {
        Failure(format!("the kernel's random source failed: {err}"))
    }
}

impl From<Failure> for String {
    fn from(failure: Failure) -> String {
        failure.0
    }
}

/// What a stopped install says: why, and whether its tables were dropped.
fn stopped(err: &DbError, undone: Result<(), DbError>) -> Failure {
    Failure(match undone {
        Ok(()) => format!("the install stopped and its tables were dropped: {err}"),
        Err(more) => format!("the install stopped: {err}; its tables were not all dropped: {more}"),
    })
}

/// `text` trimmed of the whitespace PHP's `trim()` takes.
fn trim(text: &str) -> &str {
    text.trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\0' | '\x0B'))
}

/// `home` as a site address: `http` or `https`, a host (a name, an IPv4
/// address or an IPv6 one in brackets), a port from 1 to 65535 and a path of
/// letters, digits and `._~/-`, without the trailing slashes. It reaches
/// option and post values unescaped, so it admits nothing that needs it.
pub fn site_address(home: &str) -> Option<String> {
    let home = trim(home).trim_end_matches('/');
    let rest = home
        .strip_prefix("http://")
        .or_else(|| home.strip_prefix("https://"))?;
    let (authority, path) = rest.find('/').map_or((rest, ""), |at| rest.split_at(at));
    let (host, port) = if authority.starts_with('[') {
        let (host, after) = authority.split_at(authority.find(']')? + 1);
        match after {
            "" => (host, None),
            after => (host, Some(after.strip_prefix(':')?)),
        }
    } else {
        match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        }
    };
    let host_ok = match host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
    {
        Some(address) => {
            !address.is_empty()
                && address
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() || b == b':' || b == b'.')
        }
        None => {
            !host.is_empty()
                && host.len() <= 253
                && host
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
                && !host.starts_with(['.', '-'])
        }
    };
    let port_ok = port.is_none_or(|port| {
        !port.is_empty()
            && port.len() <= 5
            && port.bytes().all(|b| b.is_ascii_digit())
            && port.parse::<u32>().is_ok_and(|n| (1..=65535).contains(&n))
    });
    let path_ok = path
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"._~/-".contains(&b));
    (host_ok && port_ok && path_ok && !path.contains("//")).then(|| home.to_string())
}

impl Plan {
    /// Checks `answers` as `wp-admin/install.php` does, in its order, then
    /// as `wp_insert_user()` does; the message is PHP's. `now` is `time()`.
    pub fn new(answers: &Answers<'_>, now: i64) -> Result<Plan, String> {
        let login = trim(answers.login);
        let email = trim(answers.email);
        if login.is_empty() {
            return Err("Please provide a valid username.".into());
        }
        if sanitize_user_strict(login.as_bytes()) != login.as_bytes() {
            return Err("The username you provided has invalid characters.".into());
        }
        if answers.password != answers.repeat {
            return Err("Your passwords do not match. Please try again.".into());
        }
        if email.is_empty() {
            return Err("You must provide an email address.".into());
        }
        if !is_email(email.as_bytes()) {
            return Err(
                "Sorry, that is not a valid email address. Email addresses look like \
                        username@example.com."
                    .into(),
            );
        }
        if login.chars().count() > 60 {
            return Err("Username may not be longer than 60 characters.".into());
        }
        let short: String = login.chars().take(50).collect();
        let nicename = String::from_utf8_lossy(&sanitize_title(short.as_bytes())).into_owned();
        if nicename.is_empty() {
            return Err("Cannot create a user with an empty nicename.".into());
        }
        let email = String::from_utf8_lossy(&sanitize_email(email.as_bytes())).into_owned();
        let home = site_address(answers.home).ok_or(
            "The site address must be a URL such as https://example.com: http or https, a host, \
             an optional port and an optional path of letters, digits and . _ ~ / -",
        )?;
        Ok(Plan {
            title: esc_html(trim(answers.title).as_bytes()),
            login: login.into(),
            nicename,
            email,
            password: trim(answers.password).into(),
            public: answers.public,
            home,
            now,
        })
    }

    /// Creates the tables and rows of a new site in `config`'s database,
    /// which must have no table of its prefix. A failure drops the tables
    /// made, so the database is as it was.
    pub async fn install(&self, config: &Database) -> Result<Installed, Failure> {
        let mut db = Installer::open(config).await?;
        let secrets = Secrets {
            generate: hyscribe_auth::generate_password,
            hash: hyscribe_auth::hash_password,
        };
        self.install_on(&mut db, config, &secrets).await
    }

    /// [`Plan::install`] over an open installer and the `secrets` it draws
    /// its password and hash from, which tests make fail.
    async fn install_on(
        &self,
        db: &mut Installer,
        config: &Database,
        secrets: &Secrets,
    ) -> Result<Installed, Failure> {
        // The password and its hash come first: nothing is made without them.
        let generated = if self.password.is_empty() {
            Some((secrets.generate)(GENERATED_LENGTH, false)?)
        } else {
            None
        };
        let password = generated.as_deref().unwrap_or(&self.password);
        let hash = (secrets.hash)(password.as_bytes())?;
        let prefix = config.prefix.as_str();
        let tables = tables(
            &|token| self.fill(token, &hash, prefix),
            generated.is_some(),
            has_path(&self.home),
        );
        let statements: Vec<&str> = SCHEMA.split(";\n").filter(|s| !s.is_empty()).collect();
        let existing = db.existing().await?;
        if !existing.is_empty() {
            return Err(Failure(format!(
                "the database {} already has tables with the prefix {}: {}. A new site needs a \
                 database without them: choose another database or prefix, or set up the \
                 existing site instead",
                config.name,
                config.prefix.as_str(),
                existing.join(", ")
            )));
        }
        let made = Self::make(db, &statements, &tables).await;
        match made {
            Ok(names) => Ok(Installed {
                generated,
                tables: names,
            }),
            Err(err) => {
                let undone = db.undo().await;
                Err(stopped(&err, undone))
            }
        }
    }

    /// Drops what [`Plan::install`] made, when a later step of the wizard
    /// failed, so the database is as it was.
    pub async fn remove(config: &Database, installed: &Installed) -> Result<(), Failure> {
        let mut db = Installer::open(config).await?;
        Self::remove_on(&mut db, installed).await
    }

    /// [`Plan::remove`] over an open installer.
    async fn remove_on(db: &mut Installer, installed: &Installed) -> Result<(), Failure> {
        db.drop_tables(&installed.tables)
            .await
            .map_err(Failure::from)
    }

    async fn make(
        db: &mut Installer,
        statements: &[&str],
        tables: &[Rows],
    ) -> Result<Vec<String>, DbError> {
        let mut names = Vec::new();
        for statement in statements {
            names.push(db.create(statement).await?);
        }
        db.insert(tables).await?;
        Ok(names)
    }

    /// What the mask `token` (the name between the percent signs) stands
    /// for, `None` for text that is not one of them.
    fn fill(&self, token: &str, hash: &str, prefix: &str) -> Option<String> {
        Some(match token {
            "now" => format_unix("Y-m-d H:i:s", self.now, Zone::Utc),
            "unix" => self.now.to_string(),
            "password" => hash.into(),
            "prefix" => prefix.into(),
            "public" => u8::from(self.public).to_string(),
            "home" => self.home.clone(),
            "title" => self.title.clone(),
            "email" => self.email.clone(),
            "login" => self.login.clone(),
            "nicename" => self.nicename.clone(),
            _ => {
                // Digits only: `parse()` would take a sign.
                let digits = token
                    .strip_prefix("unix+")
                    .filter(|digits| digits.bytes().all(|b| b.is_ascii_digit()))?;
                (self.now + i64::from(digits.parse::<u32>().ok()?)).to_string()
            }
        })
    }
}

/// Whether the site address `home` (as [`site_address`] leaves it) has a path.
fn has_path(home: &str) -> bool {
    home.split_once("://")
        .is_some_and(|(_, rest)| rest.contains('/'))
}

/// `text` with each `%token%` `fill` knows replaced, in one pass, so no
/// answer is read as a token.
fn expand(text: &str, fill: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some((before, after)) = rest.split_once('%') {
        out.push_str(before);
        let known = after
            .split_once('%')
            .and_then(|(token, tail)| fill(token).map(|value| (value, tail)));
        match known {
            Some((value, tail)) => {
                out.push_str(&value);
                rest = tail;
            }
            None => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A value of the rows file: `\N` is `NULL`, and the escapes of
/// `reference/install/install.php` are undone.
fn unescape(value: &str, fill: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    if value == "\\N" {
        return None;
    }
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    Some(expand(&out, fill))
}

/// The tables of `rows.tsv`, with the masks filled in by `fill`,
/// `default_password_nag` among the user's meta when `nag`, and the rewrite
/// rules of a site address with a path when `subfolder`.
fn tables(fill: &dyn Fn(&str) -> Option<String>, nag: bool, subfolder: bool) -> Vec<Rows> {
    // A value never holds a raw newline, so `\n@` starts each table; what
    // comes before the first is the file's comment.
    let sections = ROWS.split("\n@").skip(1);
    let mut tables: Vec<Rows> = sections
        .map(|section| {
            let mut lines = section.lines();
            let mut names = lines.next().unwrap_or_default().split('\t');
            Rows {
                table: names.next().unwrap_or_default().into(),
                columns: names.map(String::from).collect(),
                rows: lines
                    .map(|line| {
                        line.split('\t')
                            .map(|value| unescape(value, fill))
                            .collect()
                    })
                    .collect(),
            }
        })
        .filter(|table| !table.rows.is_empty())
        .collect();
    if nag {
        password_nag(&mut tables);
    }
    if subfolder {
        subfolder_rules(&mut tables, fill);
    }
    tables
}

/// The value of the `rewrite_rules` option replaced by `SUBFOLDER_RULES`.
fn subfolder_rules(tables: &mut [Rows], fill: &dyn Fn(&str) -> Option<String>) {
    let rules = unescape(SUBFOLDER_RULES.trim_end_matches('\n'), fill);
    for table in tables.iter_mut().filter(|table| table.table == "options") {
        // The columns are option_id, option_name, option_value and autoload.
        for row in &mut table.rows {
            if let [_, Some(name), value, _] = row.as_mut_slice()
                && name == "rewrite_rules"
            {
                value.clone_from(&rules);
            }
        }
    }
}

/// `update_user_meta( $user_id, 'default_password_nag', true )`, which
/// `wp_install()` calls for a password it made, right before
/// `show_welcome_panel`, which takes the next ID.
fn password_nag(tables: &mut [Rows]) {
    for table in tables.iter_mut().filter(|table| table.table == "usermeta") {
        // The columns are umeta_id, user_id, meta_key and meta_value.
        for mut row in std::mem::take(&mut table.rows) {
            if let [Some(id), user, Some(key), _] = row.as_mut_slice()
                && key == "show_welcome_panel"
            {
                let nag_id = id.clone();
                *id = nag_id
                    .parse::<u64>()
                    .map_or(nag_id.clone(), |n| (n + 1).to_string());
                let nag = [
                    Some(nag_id),
                    user.clone(),
                    Some("default_password_nag".into()),
                    Some("1".into()),
                ];
                table.rows.push(nag.into());
            }
            table.rows.push(row);
        }
    }
}

#[cfg(test)]
mod tests;

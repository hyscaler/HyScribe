// SPDX-License-Identifier: AGPL-3.0-or-later

//! HyScribe configuration (ADR 0005).
//!
//! Settings come from `.env` in the binary's folder, and real environment
//! variables always win over it. No `.env` and no `DB_NAME` in the
//! environment means the site is not set up yet, and the binary starts in
//! setup mode.

use std::collections::HashMap;
use std::fmt;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

pub mod wpconfig;

/// The file name looked up next to the binary.
pub const ENV_FILE: &str = ".env";

/// The WordPress secret keys and salts, in `wp-config.php` order.
pub const SALT_KEYS: [&str; 8] = [
    "AUTH_KEY",
    "SECURE_AUTH_KEY",
    "LOGGED_IN_KEY",
    "NONCE_KEY",
    "AUTH_SALT",
    "SECURE_AUTH_SALT",
    "LOGGED_IN_SALT",
    "NONCE_SALT",
];

/// A validated `$table_prefix`: letters, digits and underscores only, the
/// rule `wp-settings.php` applies. Nothing else can be built, so the prefix
/// is safe to put into SQL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prefix(String);

impl Prefix {
    /// Checks `prefix`; `None` if it has any other character.
    pub fn new(prefix: &str) -> Option<Prefix> {
        prefix
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            .then(|| Prefix(prefix.to_string()))
    }

    /// The prefix itself.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for Prefix {
    /// `wp_`, WordPress's default.
    fn default() -> Prefix {
        Prefix("wp_".into())
    }
}

/// A value that must never be printed or logged.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Wraps a secret value.
    pub fn new(value: impl Into<String>) -> Secret {
        Secret(value.into())
    }

    /// The secret itself, for the code that needs it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret([redacted])")
    }
}

/// Whether HyScribe may write to the database.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Read only, never writes. The default, because it is always safe.
    Shadow,
    /// Serves the site and writes to the database.
    Primary,
}

/// Database connection settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Database {
    /// `DB_HOST`, default `localhost`.
    pub host: String,
    /// `DB_PORT`, default 3306.
    pub port: u16,
    /// `DB_NAME`, required.
    pub name: String,
    /// `DB_USER`, required.
    pub user: String,
    /// `DB_PASSWORD`, default empty.
    pub password: Secret,
    /// `DB_READ_USER`: a user that may only read, for every read and all of
    /// shadow mode (ADR 0010). Without it reads use `user`, read only.
    pub read_user: Option<String>,
    /// `DB_READ_PASSWORD`, default empty.
    pub read_password: Secret,
    /// `DB_PREFIX`, WordPress's `$table_prefix`, default `wp_`.
    pub prefix: Prefix,
    /// `DB_CHARSET`, default `utf8mb4`.
    pub charset: String,
}

/// HTTP listener settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Server {
    /// `BIND_HOST`, default `127.0.0.1` so nothing is exposed by accident.
    pub bind_host: IpAddr,
    /// `PORT`, default 8080.
    pub port: u16,
    /// `BEHIND_PROXY`: trust forwarded headers. Default false.
    pub behind_proxy: bool,
}

/// The complete configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// Database connection.
    pub database: Database,
    /// HTTP listener.
    pub server: Server,
    /// `MODE`: `shadow` (default) or `primary`.
    pub mode: Mode,
    /// `UPLOADS_DIR`: an existing site's `wp-content/uploads`.
    pub uploads_dir: Option<PathBuf>,
    /// `WP_HOME`: overrides the `home` option, as the constant does.
    pub home_url: Option<String>,
    /// `WP_SITEURL`: overrides the `siteurl` option, as the constant does.
    pub site_url: Option<String>,
    /// Keys and salts by name, as far as they are set.
    pub salts: Vec<(&'static str, Secret)>,
}

/// Why configuration could not be loaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigError {
    /// A required key is not set.
    Missing(&'static str),
    /// A key has a value that cannot be used. Secret values are never
    /// included.
    Invalid {
        /// The key.
        key: &'static str,
        /// What is wrong.
        reason: String,
    },
    /// `.env` can be read by other users.
    InsecurePermissions {
        /// The file.
        path: PathBuf,
        /// Its permission bits.
        mode: u32,
    },
    /// `.env` could not be read or parsed.
    Read {
        /// The file.
        path: PathBuf,
        /// The underlying error.
        message: String,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Missing(key) => write!(f, "{key} is not set"),
            ConfigError::Invalid { key, reason } => write!(f, "{key} is invalid: {reason}"),
            ConfigError::InsecurePermissions { path, mode } => write!(
                f,
                "{} has mode {mode:o}; it holds secrets and must be readable by its owner only (chmod 600)",
                path.display()
            ),
            ConfigError::Read { path, message } => {
                write!(f, "cannot read {}: {message}", path.display())
            }
        }
    }
}

impl std::error::Error for ConfigError {}

/// Loads `.env` from `dir` with `env` layered on top.
///
/// Returns `Ok(None)` when the site has not been set up yet.
pub fn load(dir: &Path, env: &HashMap<String, String>) -> Result<Option<Config>, ConfigError> {
    let path = dir.join(ENV_FILE);
    let mut values = HashMap::new();
    if path.exists() {
        check_permissions(&path)?;
        let read_error = |err: dotenvy::Error| ConfigError::Read {
            path: path.clone(),
            message: err.to_string(),
        };
        for item in dotenvy::from_path_iter(&path).map_err(read_error)? {
            let (key, value) = item.map_err(read_error)?;
            values.insert(key, value);
        }
    } else if !env.contains_key("DB_NAME") {
        return Ok(None);
    }
    values.extend(env.iter().map(|(k, v)| (k.clone(), v.clone())));
    from_map(&values).map(Some)
}

/// Builds a configuration from key and value pairs.
pub fn from_map(values: &HashMap<String, String>) -> Result<Config, ConfigError> {
    let get = |key: &str| {
        values
            .get(key)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    };
    let required = |key: &'static str| get(key).map(String::from).ok_or(ConfigError::Missing(key));

    let prefix = Prefix::new(get("DB_PREFIX").unwrap_or("wp_"))
        .ok_or_else(|| invalid("DB_PREFIX", "only letters, digits and underscores"))?;

    if get("DB_READ_PASSWORD").is_some() && get("DB_READ_USER").is_none() {
        return Err(invalid("DB_READ_PASSWORD", "is set without DB_READ_USER"));
    }
    let database = Database {
        host: get("DB_HOST").unwrap_or("localhost").to_string(),
        port: port(get("DB_PORT"), "DB_PORT", 3306)?,
        name: required("DB_NAME")?,
        user: required("DB_USER")?,
        password: Secret::new(get("DB_PASSWORD").unwrap_or_default()),
        read_user: get("DB_READ_USER").map(String::from),
        read_password: Secret::new(get("DB_READ_PASSWORD").unwrap_or_default()),
        prefix,
        charset: get("DB_CHARSET").unwrap_or("utf8mb4").to_string(),
    };

    let bind_host = match get("BIND_HOST") {
        Some(value) => value
            .parse()
            .map_err(|_| invalid("BIND_HOST", "not an IP address"))?,
        None => IpAddr::from([127, 0, 0, 1]),
    };
    let server = Server {
        bind_host,
        port: port(get("PORT"), "PORT", 8080)?,
        behind_proxy: boolean(get("BEHIND_PROXY"), "BEHIND_PROXY")?,
    };

    let mode = match get("MODE") {
        None | Some("shadow") => Mode::Shadow,
        Some("primary") => Mode::Primary,
        Some(_) => return Err(invalid("MODE", "must be shadow or primary")),
    };

    let salts = SALT_KEYS
        .iter()
        .filter_map(|&key| get(key).map(|value| (key, Secret::new(value))))
        .collect();

    Ok(Config {
        database,
        server,
        mode,
        uploads_dir: get("UPLOADS_DIR").map(PathBuf::from),
        home_url: get("WP_HOME").map(String::from),
        site_url: get("WP_SITEURL").map(String::from),
        salts,
    })
}

fn invalid(key: &'static str, reason: &str) -> ConfigError {
    ConfigError::Invalid {
        key,
        reason: reason.to_string(),
    }
}

fn port(value: Option<&str>, key: &'static str, default: u16) -> Result<u16, ConfigError> {
    match value {
        None => Ok(default),
        Some(value) => value
            .parse()
            .ok()
            .filter(|&port| port != 0)
            .ok_or_else(|| invalid(key, "must be a port number from 1 to 65535")),
    }
}

fn boolean(value: Option<&str>, key: &'static str) -> Result<bool, ConfigError> {
    match value.map(str::to_ascii_lowercase).as_deref() {
        None | Some("false" | "0" | "no") => Ok(false),
        Some("true" | "1" | "yes") => Ok(true),
        Some(_) => Err(invalid(key, "must be true or false")),
    }
}

/// `.env` holds the database password and salts: refuse it if group or
/// others can access it.
fn check_permissions(path: &Path) -> Result<(), ConfigError> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = std::fs::metadata(path).map_err(|err| ConfigError::Read {
        path: path.to_path_buf(),
        message: err.to_string(),
    })?;
    let mode = metadata.permissions().mode() & 0o777;
    if mode & 0o077 == 0 {
        Ok(())
    } else {
        Err(ConfigError::InsecurePermissions {
            path: path.to_path_buf(),
            mode,
        })
    }
}

#[cfg(test)]
mod tests;

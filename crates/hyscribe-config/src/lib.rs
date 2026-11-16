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

pub mod setup;
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

/// The `wp-config.php` constants HyScribe reads, as `.env` keys of the same
/// names; a missing key takes the safer value, or WordPress's default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Constants {
    /// `DISALLOW_UNFILTERED_HTML`, default true.
    pub disallow_unfiltered_html: bool,
    /// `ALLOW_UNFILTERED_UPLOADS`, default false.
    pub allow_unfiltered_uploads: bool,
    /// `EMPTY_TRASH_DAYS`, default 30: posts and comments go to the trash
    /// before they are deleted unless it is 0.
    pub empty_trash_days: u32,
}

impl Default for Constants {
    fn default() -> Constants {
        Constants {
            disallow_unfiltered_html: true,
            allow_unfiltered_uploads: false,
            empty_trash_days: 30,
        }
    }
}

impl Constants {
    /// Whether `wp-config.php` would define `name` as true. `MULTISITE`
    /// never is: HyScribe refuses to start with it.
    pub fn defined(&self, name: &str) -> bool {
        match name {
            "DISALLOW_UNFILTERED_HTML" => self.disallow_unfiltered_html,
            "ALLOW_UNFILTERED_UPLOADS" => self.allow_unfiltered_uploads,
            _ => false,
        }
    }
}

/// Login rate limits (docs/SECURITY_MODEL.md): the failed logins one client
/// address, or one account, may make in a window before guesses are refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoginLimits {
    /// `LOGIN_LIMIT_ADDRESS`, default 5.
    pub address: u32,
    /// `LOGIN_LIMIT_ACCOUNT`, default 10.
    pub account: u32,
    /// `LOGIN_LIMIT_WINDOW`, in seconds, default 900.
    pub window: u32,
}

impl Default for LoginLimits {
    fn default() -> LoginLimits {
        LoginLimits {
            address: 5,
            account: 10,
            window: 900,
        }
    }
}

/// `UPLOAD_MAX_SIZE`'s default, 64 MB.
pub const DEFAULT_UPLOAD_MAX_SIZE: u64 = 64 << 20;

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
    /// `REST_PUBLIC_USERS`: `on` (default) answers visitors' REST users
    /// requests as WordPress does, `off` as if no user had published posts.
    pub rest_public_users: bool,
    /// The capability constants.
    pub constants: Constants,
    /// The login rate limits.
    pub login_limits: LoginLimits,
    /// `UPLOAD_MAX_SIZE`: the largest upload, in bytes, which the admin
    /// prints where PHP prints `wp_max_upload_size()`.
    pub upload_max_size: u64,
    /// The limits uploaded images are decoded within.
    pub image_limits: ImageLimits,
}

/// The limits an uploaded image is decoded within (ADR 0015); the defaults
/// are the oracle's ImageMagick policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageLimits {
    /// `IMAGE_MAX_BYTES`: the largest file decoded, default 64 MB.
    pub bytes: u64,
    /// `IMAGE_MAX_WIDTH`, default 32,000 pixels.
    pub width: u32,
    /// `IMAGE_MAX_HEIGHT`, default 32,000 pixels.
    pub height: u32,
    /// `IMAGE_MAX_AREA`: width times height, default 256,000,000 pixels.
    pub area: u64,
    /// `IMAGE_MAX_MEMORY`: one image's buffers in bytes, default 1 GB.
    pub memory: u64,
}

impl Default for ImageLimits {
    fn default() -> ImageLimits {
        ImageLimits {
            bytes: 64 << 20,
            width: 32_000,
            height: 32_000,
            area: 256_000_000,
            memory: 1 << 30,
        }
    }
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

    let server = server(values)?;

    let mode = match get("MODE") {
        None | Some("shadow") => Mode::Shadow,
        Some("primary") => Mode::Primary,
        Some(_) => return Err(invalid("MODE", "must be shadow or primary")),
    };

    let rest_public_users = match get("REST_PUBLIC_USERS") {
        None | Some("on") => true,
        Some("off") => false,
        Some(_) => return Err(invalid("REST_PUBLIC_USERS", "must be on or off")),
    };

    if constant(get("MULTISITE"), "MULTISITE", false)? {
        return Err(invalid("MULTISITE", "multisite networks are not supported"));
    }
    let safer = Constants::default();
    let constants = Constants {
        disallow_unfiltered_html: constant(
            get("DISALLOW_UNFILTERED_HTML"),
            "DISALLOW_UNFILTERED_HTML",
            safer.disallow_unfiltered_html,
        )?,
        allow_unfiltered_uploads: constant(
            get("ALLOW_UNFILTERED_UPLOADS"),
            "ALLOW_UNFILTERED_UPLOADS",
            safer.allow_unfiltered_uploads,
        )?,
        empty_trash_days: match get("EMPTY_TRASH_DAYS") {
            None => safer.empty_trash_days,
            Some(days) => days
                .parse()
                .map_err(|_| invalid("EMPTY_TRASH_DAYS", "must be a number of days, 0 or more"))?,
        },
    };

    let limits = LoginLimits::default();
    let limit = |key: &'static str, default, max| number(get(key), key, default, max);
    let login_limits = LoginLimits {
        address: limit("LOGIN_LIMIT_ADDRESS", limits.address, 1000)?,
        account: limit("LOGIN_LIMIT_ACCOUNT", limits.account, 1000)?,
        window: limit("LOGIN_LIMIT_WINDOW", limits.window, 86_400)?,
    };

    let upload_max_size = match get("UPLOAD_MAX_SIZE") {
        None => DEFAULT_UPLOAD_MAX_SIZE,
        Some(value) => size(value).ok_or_else(|| {
            invalid(
                "UPLOAD_MAX_SIZE",
                "must be a size in bytes, or with a K, M or G suffix, such as 64M",
            )
        })?,
    };

    let image = ImageLimits::default();
    let bytes = |key: &'static str, default| match get(key) {
        None => Ok(default),
        Some(value) => size(value).ok_or_else(|| {
            invalid(
                key,
                "must be a size in bytes, or with a K, M or G suffix, such as 64M",
            )
        }),
    };
    let image_limits = ImageLimits {
        bytes: bytes("IMAGE_MAX_BYTES", image.bytes)?,
        width: limit("IMAGE_MAX_WIDTH", image.width, u32::MAX)?,
        height: limit("IMAGE_MAX_HEIGHT", image.height, u32::MAX)?,
        area: bytes("IMAGE_MAX_AREA", image.area)?,
        memory: bytes("IMAGE_MAX_MEMORY", image.memory)?,
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
        rest_public_users,
        constants,
        login_limits,
        upload_max_size,
        image_limits,
    })
}

/// The HTTP listener settings of `values`, which setup mode listens on too.
pub fn server(values: &HashMap<String, String>) -> Result<Server, ConfigError> {
    let get = |key: &str| {
        values
            .get(key)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    };
    let bind_host = match get("BIND_HOST") {
        Some(value) => value
            .parse()
            .map_err(|_| invalid("BIND_HOST", "not an IP address"))?,
        None => IpAddr::from([127, 0, 0, 1]),
    };
    Ok(Server {
        bind_host,
        port: port(get("PORT"), "PORT", 8080)?,
        behind_proxy: boolean(get("BEHIND_PROXY"), "BEHIND_PROXY")?,
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

/// A whole number from 1 to `max`.
fn number(
    value: Option<&str>,
    key: &'static str,
    default: u32,
    max: u32,
) -> Result<u32, ConfigError> {
    match value {
        None => Ok(default),
        Some(value) => value
            .parse()
            .ok()
            .filter(|number| (1..=max).contains(number))
            .ok_or_else(|| invalid(key, &format!("must be a whole number from 1 to {max}"))),
    }
}

/// A size as PHP's shorthand writes it: digits, then `K`, `M` or `G` in
/// either case; at least a byte, and within `i64`.
fn size(value: &str) -> Option<u64> {
    let (digits, shift) = match value.as_bytes().split_last() {
        Some((b'K' | b'k', head)) => (head, 10),
        Some((b'M' | b'm', head)) => (head, 20),
        Some((b'G' | b'g', head)) => (head, 30),
        _ => (value.as_bytes(), 0),
    };
    if digits.is_empty() {
        return None;
    }
    let number = digits.iter().try_fold(0u64, |number, &digit| {
        digit.is_ascii_digit().then_some(())?;
        number.checked_mul(10)?.checked_add(u64::from(digit - b'0'))
    })?;
    let bytes = number.checked_mul(1 << shift)?;
    (1..=i64::MAX.unsigned_abs())
        .contains(&bytes)
        .then_some(bytes)
}

fn boolean(value: Option<&str>, key: &'static str) -> Result<bool, ConfigError> {
    match value.map(str::to_ascii_lowercase).as_deref() {
        None | Some("false" | "0" | "no") => Ok(false),
        Some("true" | "1" | "yes") => Ok(true),
        Some(_) => Err(invalid(key, "must be true or false")),
    }
}

/// A constant the setup wizard copies from a literal `true` or `false`
/// `define()`; 1 and 0 are read too.
fn constant(value: Option<&str>, key: &'static str, default: bool) -> Result<bool, ConfigError> {
    match value.map(str::to_ascii_lowercase).as_deref() {
        None => Ok(default),
        Some("true" | "1") => Ok(true),
        Some("false" | "0") => Ok(false),
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

// SPDX-License-Identifier: AGPL-3.0-or-later

//! What the setup wizard writes (docs/SETUP_WIZARD.md): the `.env` keys an
//! existing site's `wp-config.php` gives, and `.env` itself.

use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use crate::wpconfig::{Value, WpConfig, parse_db_host};
use crate::{ENV_FILE, SALT_KEYS};

/// The placeholder `wp-config-sample.php` gives every key and salt, which
/// `wp_salt()` treats as unset.
const UNSET_SALT: &str = "put your unique phrase here";

/// The constants the wizard copies when they are literal `true` or `false`:
/// the capability constants and the cron ones (ADR 0017).
pub const CONSTANTS: [&str; 5] = [
    "DISALLOW_UNFILTERED_HTML",
    "ALLOW_UNFILTERED_UPLOADS",
    "MULTISITE",
    "DISABLE_WP_CRON",
    "ALTERNATE_WP_CRON",
];

/// How `wp-config.php` defines a constant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting<'a> {
    /// Not defined.
    Unset,
    /// Defined unconditionally as a literal.
    Literal(&'a Value),
    /// Defined by a PHP expression, or only under a condition, so only PHP
    /// can tell its value.
    Unreadable,
}

/// How `config` defines `name`.
pub fn setting<'a>(config: &'a WpConfig, name: &str) -> Setting<'a> {
    match config.get(name) {
        None => Setting::Unset,
        Some(c) if c.conditional || matches!(c.value, Value::Computed(_)) => Setting::Unreadable,
        Some(c) => Setting::Literal(&c.value),
    }
}

/// The `.env` keys read from a `wp-config.php`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Import {
    /// The keys copied, in `.env` order.
    pub values: Vec<(&'static str, String)>,
    /// The keys `wp-config.php` sets in a way the wizard cannot copy, and
    /// the database keys it does not set, which the wizard asks for.
    pub unread: Vec<&'static str>,
}

/// The `.env` keys `config` gives: the database details, the table prefix,
/// the keys and salts, `WP_HOME`, `WP_SITEURL`, the capability constants and
/// `EMPTY_TRASH_DAYS` when it is a whole number of days.
pub fn import(config: &WpConfig) -> Import {
    let mut import = Import::default();
    let string = |name: &str| match setting(config, name) {
        Setting::Literal(Value::String(text)) => Ok(Some(text.clone())),
        Setting::Unset => Ok(None),
        _ => Err(()),
    };
    for key in ["DB_NAME", "DB_USER", "DB_PASSWORD"] {
        match string(key) {
            Ok(Some(value)) => import.values.push((key, value)),
            _ => import.unread.push(key),
        }
    }
    // A port outside 1 to 65535 is not something `.env` holds. `mysqli`
    // takes the socket for `localhost` only, else the host and port.
    let host = string("DB_HOST").ok().flatten().and_then(|host| {
        let host = parse_db_host(&host)?;
        let name = if host.host.is_empty() {
            "localhost".to_string()
        } else {
            host.host
        };
        if let Some(socket) = host.socket.filter(|_| name == "localhost") {
            return Some((format!("{name}:{socket}"), None));
        }
        let port = match host.port {
            Some(port) => Some(u16::try_from(port).ok().filter(|&port| port != 0)?),
            None => None,
        };
        Some((name, port))
    });
    match host {
        Some((name, port)) => {
            import.values.push(("DB_HOST", name));
            if let Some(port) = port {
                import.values.push(("DB_PORT", port.to_string()));
            }
        }
        None => import.unread.push("DB_HOST"),
    }
    match string("DB_CHARSET") {
        // `wpdb::determine_charset()` takes utf8 as utf8mb4 on every server
        // HyScribe supports.
        Ok(Some(charset)) if charset == "utf8" => {
            import.values.push(("DB_CHARSET", "utf8mb4".into()))
        }
        Ok(Some(charset)) if !charset.is_empty() => import.values.push(("DB_CHARSET", charset)),
        Ok(_) => {}
        Err(()) => import.unread.push("DB_CHARSET"),
    }
    match &config.table_prefix {
        Some(Value::String(prefix)) => import.values.push(("DB_PREFIX", prefix.clone())),
        _ => import.unread.push("DB_PREFIX"),
    }
    for key in SALT_KEYS {
        match string(key) {
            Ok(Some(salt)) if salt != UNSET_SALT && !salt.is_empty() => {
                import.values.push((key, salt));
            }
            _ => {}
        }
    }
    for key in ["WP_HOME", "WP_SITEURL"] {
        match string(key) {
            Ok(Some(url)) => import.values.push((key, url)),
            Ok(None) => {}
            Err(()) => import.unread.push(key),
        }
    }
    for key in CONSTANTS {
        match setting(config, key) {
            Setting::Literal(Value::Bool(value)) => import.values.push((key, value.to_string())),
            Setting::Unset => {}
            _ => import.unread.push(key),
        }
    }
    let days = match setting(config, "EMPTY_TRASH_DAYS") {
        Setting::Literal(Value::Int(days)) => Some(days.to_string()),
        Setting::Literal(Value::String(days)) => Some(days.clone()),
        Setting::Unset => None,
        _ => Some(String::new()),
    };
    match days {
        Some(days) if days.parse::<u32>().is_ok() => import.values.push(("EMPTY_TRASH_DAYS", days)),
        Some(_) => import.unread.push("EMPTY_TRASH_DAYS"),
        None => {}
    }
    import
}

/// `.env` text for `values`, each value double quoted so any text but a
/// carriage return reads back as written; `None` if a value has one.
pub fn render(values: &[(&str, String)]) -> Option<String> {
    let mut text = String::from("# Written by hyscribe setup. .env.example documents every key.\n");
    for (key, value) in values {
        if value.contains('\r') {
            return None;
        }
        text.push_str(key);
        text.push_str("=\"");
        for c in value.chars() {
            match c {
                '\\' | '"' | '$' => {
                    text.push('\\');
                    text.push(c);
                }
                '\n' => text.push_str("\\n"),
                _ => text.push(c),
            }
        }
        text.push_str("\"\n");
    }
    Some(text)
}

/// Writes `text` to `dir/.env` with mode 0600, through a new file renamed
/// into place so no reader sees half of it. An existing `.env` is replaced
/// only when `replace` is true.
pub fn write(dir: &Path, text: &str, replace: bool) -> std::io::Result<PathBuf> {
    let path = dir.join(ENV_FILE);
    let new = dir.join(format!("{ENV_FILE}.new"));
    // Left by an interrupted run; if it cannot go, creating it fails.
    let _ = std::fs::remove_file(&new);
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&new)
        .and_then(|mut file| {
            file.write_all(text.as_bytes())
                .and_then(|()| file.sync_all())
        });
    // A hard link fails if `.env` exists, so nothing is replaced unasked.
    let placed = written.and_then(|()| {
        if replace {
            std::fs::rename(&new, &path)
        } else {
            std::fs::hard_link(&new, &path)
        }
    });
    let _ = std::fs::remove_file(&new);
    placed.map(|()| path)
}

#[cfg(test)]
mod tests;

// SPDX-License-Identifier: AGPL-3.0-or-later

//! Options and meta: `add_option()`, `update_option()`, `delete_option()`,
//! the transients' database side without an object cache, and the meta
//! functions, and `sanitize_option()` for the Settings screens' options.
//! `update_option()` takes sanitized values; the hooks on single options
//! (as `update_option_home`) are their callers'.

use hyscribe_db::{DbError, Write};
use hyscribe_phpser::{Maybe, Value, encode, is_empty_string, is_serialized, maybe_unserialize};

use crate::{Error, now};

mod meta;
mod sanitize;

pub use hyscribe_db::Meta;
pub use meta::{add_metadata, delete_metadata, update_metadata};
pub(crate) use meta::{delete_object_meta, get_metadata};
pub use sanitize::{INVALID_ZONE, Sanitized, is_zone, sanitize_option};

/// An `$autoload` argument other than `null`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Autoload {
    /// `true`, `'yes'` or `'on'`.
    On,
    /// `false`, `'no'` or `'off'`.
    Off,
    /// Any other value: WordPress decides by the value's size.
    Auto,
}

/// The autoload values `update_option()` evaluates again on each update.
const AUTOMATIC: [&str; 3] = ["auto-on", "auto-off", "auto"];

/// `maybe_serialize()`: arrays, objects and strings that look serialized
/// are serialized.
pub(crate) fn maybe_serialize(value: &Value) -> Value {
    match value {
        Value::Null | Value::Bool(_) | Value::Int(_) | Value::Float(_) => value.clone(),
        Value::String(text) if !is_serialized(text, false) => value.clone(),
        _ => Value::String(encode(value)),
    }
}

/// A value as `wpdb` stores it after `maybe_serialize()`.
pub(crate) fn stored_bytes(value: &Value) -> Vec<u8> {
    maybe_serialize(value).to_php_string()
}

/// A stored value as `maybe_unserialize()` gives it: `false` for one that
/// looks serialized but does not decode.
pub(crate) fn stored(raw: &[u8]) -> Value {
    match maybe_unserialize(raw) {
        Maybe::Plain(text) => Value::String(text.to_vec()),
        Maybe::Value(value) => value,
        Maybe::Invalid(_) => Value::Bool(false),
    }
}

/// The trimmed option name, `None` for an empty one; the names of the
/// options cache are refused, as `wp_protect_special_option()` dies on them.
fn option_name(name: &str) -> Result<Option<&str>, Error> {
    let name = name.trim_matches([' ', '\t', '\n', '\r', '\0', '\x0B']);
    if ["alloptions", "notoptions"].contains(&name) {
        return Err(Error::Refused("protected option"));
    }
    Ok((!is_empty_string(name.as_bytes())).then_some(name))
}

/// `get_option( $name )`: `false` for a missing option. An empty `home` is
/// the site URL, the URL options lose their trailing slashes, and `home`
/// and `siteurl` are the configuration's when it sets them (`WP_HOME`).
pub async fn get_option(write: &mut Write<'_>, name: &str) -> Result<Value, DbError> {
    if let Some(url) = write.configured(name) {
        return Ok(Value::String(url.as_bytes().to_vec()));
    }
    let raw = match write.option_value(name).await? {
        Some(raw) if name == "home" && raw.is_empty() => write.option_value("siteurl").await?,
        raw => raw,
    };
    let Some(mut raw) = raw else {
        return Ok(Value::Bool(false));
    };
    if ["siteurl", "home", "category_base", "tag_base"].contains(&name) {
        let end = raw
            .iter()
            .rposition(|b| !b"/\\".contains(b))
            .map_or(0, |i| i + 1);
        raw.truncate(end);
    }
    Ok(stored(&raw))
}

/// `wp_determine_option_autoload_value()` with the default filter, which
/// leaves values over 150000 bytes out of autoloading.
pub(crate) fn autoload_value(serialized: &[u8], autoload: Option<Autoload>) -> &'static str {
    match autoload {
        Some(Autoload::On) => "on",
        Some(Autoload::Off) => "off",
        _ if serialized.len() > 150_000 => "auto-off",
        _ => "auto",
    }
}

/// `add_option( $name, $value, '', $autoload )`: false when the option
/// exists, as `get_option()` sees it.
pub async fn add_option(
    write: &mut Write<'_>,
    name: &str,
    value: &Value,
    autoload: Option<Autoload>,
) -> Result<bool, Error> {
    let Some(name) = option_name(name)? else {
        return Ok(false);
    };
    if get_option(write, name).await? != Value::Bool(false) {
        return Ok(false);
    }
    let serialized = stored_bytes(value);
    let autoload = autoload_value(&serialized, autoload);
    write.add_option(name, &serialized, autoload).await?;
    Ok(true)
}

/// `update_option( $name, $value, $autoload )`: nothing for a value equal
/// to the stored one, and `add_option()` for a missing option. Without
/// `autoload`, an automatic autoload is evaluated again. True when the row
/// changed, as PHP's count of changed rows says.
pub async fn update_option(
    write: &mut Write<'_>,
    name: &str,
    value: &Value,
    autoload: Option<Autoload>,
) -> Result<bool, Error> {
    let Some(name) = option_name(name)? else {
        return Ok(false);
    };
    let old = get_option(write, name).await?;
    // PHP compares the values too, which match exactly when these do.
    if maybe_serialize(value) == maybe_serialize(&old) {
        return Ok(false);
    }
    if old == Value::Bool(false) {
        return add_option(write, name, value, autoload).await;
    }
    let serialized = stored_bytes(value);
    let raw = write.option_value(name).await?;
    let current = write.option_autoload(name).await?.unwrap_or_default();
    let new = match autoload {
        None if !AUTOMATIC.contains(&current.as_str()) => current.as_str(),
        _ => autoload_value(&serialized, autoload),
    };
    let changed = raw.as_deref() != Some(&serialized[..]) || new != current;
    if new == current.as_str() {
        write.update_option(name, &serialized).await?;
    } else {
        write.update_option_autoload(name, &serialized, new).await?;
    }
    Ok(changed)
}

/// `delete_option( $name )`: false for a missing option.
pub async fn delete_option(write: &mut Write<'_>, name: &str) -> Result<bool, Error> {
    let Some(name) = option_name(name)? else {
        return Ok(false);
    };
    if write.option_autoload(name).await?.is_none() {
        return Ok(false);
    }
    write.delete_option(name).await?;
    Ok(true)
}

/// `set_transient( $name, $value, $expiration )` without an object cache:
/// the `_transient_` option, and a `_transient_timeout_` one when it
/// expires, neither autoloaded then.
pub async fn set_transient(
    write: &mut Write<'_>,
    name: &str,
    value: &Value,
    expiration: i64,
) -> Result<bool, Error> {
    let timeout = format!("_transient_timeout_{name}");
    let option = format!("_transient_{name}");
    let expires = Value::Int(now() + expiration);
    if get_option(write, &option).await? == Value::Bool(false) {
        let mut autoload = Autoload::On;
        if expiration != 0 {
            autoload = Autoload::Off;
            add_option(write, &timeout, &expires, Some(Autoload::Off)).await?;
        }
        return add_option(write, &option, value, Some(autoload)).await;
    }
    if expiration != 0 {
        if get_option(write, &timeout).await? == Value::Bool(false) {
            delete_option(write, &option).await?;
            add_option(write, &timeout, &expires, Some(Autoload::Off)).await?;
            return add_option(write, &option, value, Some(Autoload::Off)).await;
        }
        update_option(write, &timeout, &expires, None).await?;
    }
    update_option(write, &option, value, None).await
}

/// What `get_settings_errors()` writes on the screen a save sends back
/// (`settings-updated`): an expired `settings_errors` transient is
/// deleted by `get_transient()`, one it shows by `delete_transient()`.
pub async fn settings_errors_shown(write: &mut Write<'_>) -> Result<(), Error> {
    let timeout = get_option(write, "_transient_timeout_settings_errors").await?;
    if timeout != Value::Bool(false) && timeout.to_int() < now() {
        delete_option(write, "_transient_settings_errors").await?;
        delete_option(write, "_transient_timeout_settings_errors").await?;
    } else if !get_option(write, "_transient_settings_errors")
        .await?
        .is_empty()
    {
        delete_transient(write, "settings_errors").await?;
    }
    Ok(())
}

/// `delete_transient( $name )` without an object cache.
pub async fn delete_transient(write: &mut Write<'_>, name: &str) -> Result<bool, Error> {
    delete_pair(write, "_transient_", name).await
}

/// `set_site_transient( $name, $value, $expiration )` on a single site:
/// `_site_transient_` options, never autoloaded.
pub async fn set_site_transient(
    write: &mut Write<'_>,
    name: &str,
    value: &Value,
    expiration: i64,
) -> Result<bool, Error> {
    let timeout = format!("_site_transient_timeout_{name}");
    let option = format!("_site_transient_{name}");
    let expires = Value::Int(now() + expiration);
    // `add_site_option()` and `update_site_option()` on a single site.
    let off = Some(Autoload::Off);
    if get_option(write, &option).await? == Value::Bool(false) {
        if expiration != 0 {
            add_option(write, &timeout, &expires, off).await?;
        }
        return add_option(write, &option, value, off).await;
    }
    if expiration != 0 {
        update_option(write, &timeout, &expires, off).await?;
    }
    update_option(write, &option, value, off).await
}

/// `delete_site_transient( $name )` on a single site.
pub async fn delete_site_transient(write: &mut Write<'_>, name: &str) -> Result<bool, Error> {
    delete_pair(write, "_site_transient_", name).await
}

/// Deletes a transient's option, and its timeout when there was one.
async fn delete_pair(write: &mut Write<'_>, prefix: &str, name: &str) -> Result<bool, Error> {
    let deleted = delete_option(write, &format!("{prefix}{name}")).await?;
    if deleted {
        let timeout = format!("{prefix}timeout_{name}");
        delete_option(write, &timeout).await?;
    }
    Ok(deleted)
}

#[cfg(test)]
mod tests;

// SPDX-License-Identifier: AGPL-3.0-or-later

//! The meta functions of every meta type: `add_metadata()`,
//! `update_metadata()` and `delete_metadata()`. As in PHP, keys and values
//! arrive slashed and are unslashed here, but a previous value is not.
//! `sanitize_meta()` and the `*_metadata` filters are not ported.

use hyscribe_db::{DbError, Meta, Write};
use hyscribe_phpser::{Key, Value, is_empty_string, stripslashes};

use super::{maybe_serialize, stored, stored_bytes};

/// `wp_unslash()`: `stripslashes()` on every string, in arrays and in an
/// object's public properties too.
pub(super) fn unslash(value: &Value) -> Value {
    let entries = |entries: &[(Key, Value)]| {
        let entries = entries.iter().map(|(key, value)| {
            let private = matches!(key, Key::String(name) if name.first() == Some(&0));
            (
                key.clone(),
                if private {
                    value.clone()
                } else {
                    unslash(value)
                },
            )
        });
        entries.collect()
    };
    match value {
        Value::String(text) => Value::String(stripslashes(text)),
        Value::Array(items) => Value::Array(entries(items)),
        Value::Object { class, properties } => Value::Object {
            class: class.clone(),
            properties: entries(properties),
        },
        _ => value.clone(),
    }
}

fn unslash_key(key: &str) -> String {
    String::from_utf8_lossy(&stripslashes(key.as_bytes())).into_owned()
}

/// A meta value as `wpdb` stores it after `maybe_serialize()`: `None`, a
/// NULL, for PHP's `null`.
fn stored_value(value: &Value) -> Option<Vec<u8>> {
    match maybe_serialize(value) {
        Value::Null => None,
        value => Some(value.to_php_string()),
    }
}

/// `get_metadata_raw( $type, $object, $key )`: every value of `key`, in ID
/// order, unserialized. Keys match exactly, as in PHP's meta cache.
pub(crate) async fn get_metadata(
    write: &mut Write<'_>,
    meta: Meta,
    object: u64,
    key: &str,
) -> Result<Vec<Value>, DbError> {
    let rows = write.object_meta(meta, object).await?;
    let values = rows.into_iter().filter(|(name, _)| name == key.as_bytes());
    Ok(values
        .map(|(_, value)| value.map_or(Value::Null, |raw| stored(&raw)))
        .collect())
}

/// `add_metadata( $type, $object, $key, $value, $unique )`: the new row's
/// ID, or `None` where PHP returns false (no key or object, or a unique
/// key the object has).
pub async fn add_metadata(
    write: &mut Write<'_>,
    meta: Meta,
    object: u64,
    key: &str,
    value: &Value,
    unique: bool,
) -> Result<Option<u64>, DbError> {
    if is_empty_string(key.as_bytes()) || object == 0 {
        return Ok(None);
    }
    let key = unslash_key(key);
    if unique && !write.meta_ids(meta, object, &key).await?.is_empty() {
        return Ok(None);
    }
    let value = stored_value(&unslash(value));
    write
        .add_meta(meta, object, &key, value.as_deref())
        .await
        .map(Some)
}

/// `update_metadata( $type, $object, $key, $value, $previous )`: the rows
/// of `key` (those holding `previous` unless it is empty) take `value`, or
/// it is added when the object has no such row. Nothing is written when
/// the only row holds `value` already. True when a row was added or
/// changed, as PHP's count of changed rows says.
pub async fn update_metadata(
    write: &mut Write<'_>,
    meta: Meta,
    object: u64,
    key: &str,
    value: &Value,
    previous: &Value,
) -> Result<bool, DbError> {
    if is_empty_string(key.as_bytes()) || object == 0 {
        return Ok(false);
    }
    let name = unslash_key(key);
    let new = unslash(value);
    let old = get_metadata(write, meta, object, &name).await?;
    if previous.is_empty() && old.as_slice() == [new.clone()] {
        return Ok(false);
    }
    if write.meta_ids(meta, object, &name).await?.is_empty() {
        let added = add_metadata(write, meta, object, key, value, false).await?;
        return Ok(added.is_some());
    }
    let new = stored_value(&new);
    if previous.is_empty() {
        write
            .update_meta(meta, object, &name, new.as_deref())
            .await?;
        return Ok(old.iter().any(|old| stored_value(old) != new));
    }
    let previous = stored_bytes(previous);
    let matched = write
        .update_meta_from(meta, (object, &name), &previous, new.as_deref())
        .await?;
    Ok(matched > 0 && Some(previous) != new)
}

/// `delete_metadata( $type, $object, $key, $value, $delete_all )`: the rows
/// of `key` of `object`, or of every object with `delete_all`, those
/// holding `value` unless it is `''`, `null` or `false`.
pub async fn delete_metadata(
    write: &mut Write<'_>,
    meta: Meta,
    object: u64,
    key: &str,
    value: &Value,
    delete_all: bool,
) -> Result<bool, DbError> {
    if is_empty_string(key.as_bytes()) || object == 0 && !delete_all {
        return Ok(false);
    }
    let key = unslash_key(key);
    let value = maybe_serialize(&unslash(value));
    let any =
        matches!(value, Value::Null | Value::Bool(false)) || value == Value::String(Vec::new());
    let value = value.to_php_string();
    let ids = match (delete_all, any) {
        (true, true) => write.meta_ids_of_key(meta, &key).await?,
        (true, false) => write.meta_ids_of_key_valued(meta, &key, &value).await?,
        (false, true) => write.meta_ids(meta, object, &key).await?,
        (false, false) => write.meta_ids_valued(meta, (object, &key), &value).await?,
    };
    if ids.is_empty() {
        return Ok(false);
    }
    write.delete_meta(meta, &ids).await?;
    Ok(true)
}

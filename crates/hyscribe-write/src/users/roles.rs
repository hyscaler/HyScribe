// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_User::set_role()`: the user's `{prefix}capabilities` meta, and the
//! `{prefix}user_level` its roles give.

use hyscribe_db::{DbError, Meta, Write};
use hyscribe_phpser::{Key, Value, is_empty_string};

use crate::options::{get_metadata, get_option, update_metadata};

/// A user's capabilities as `WP_User` loads them for the site.
struct User {
    id: u64,
    /// `{prefix}capabilities`: roles and capabilities, each with a flag.
    caps: Vec<(Key, Value)>,
    /// `{prefix}user_roles`: the site's roles.
    registry: Value,
}

impl User {
    async fn load(write: &mut Write<'_>, id: u64) -> Result<User, DbError> {
        let key = format!("{}capabilities", write.prefix());
        let caps = match get_metadata(write, Meta::User, id, &key).await?.first() {
            Some(Value::Array(caps)) => caps.clone(),
            _ => Vec::new(),
        };
        let roles = format!("{}user_roles", write.prefix());
        let registry = get_option(write, &roles).await?;
        Ok(User { id, caps, registry })
    }

    /// `$this->roles`: the keys of `caps` that are roles of the site.
    fn roles(&self) -> Vec<Key> {
        let caps = self.caps.iter().map(|(key, _)| key);
        caps.filter(|key| self.registry.get(key).is_some())
            .cloned()
            .collect()
    }

    /// `$this->caps[ $role ] = true`.
    fn grant(&mut self, role: Key) {
        match self.caps.iter_mut().find(|(key, _)| *key == role) {
            Some(entry) => entry.1 = Value::Bool(true),
            None => self.caps.push((role, Value::Bool(true))),
        }
    }

    /// `update_user_meta()` of the capabilities, then
    /// `update_user_level_from_caps()`: the highest `level_N` key of the
    /// roles' capabilities and the user's own.
    async fn save(self, write: &mut Write<'_>) -> Result<(), DbError> {
        let roles = self.roles();
        let granted = roles.iter().filter_map(|role| {
            let role = self.registry.get(role);
            role.and_then(|role| role.get_str("capabilities"))
                .and_then(Value::as_array)
        });
        let keys = granted.flatten().chain(&self.caps).map(|(key, _)| key);
        let level = keys.filter_map(level).max().unwrap_or(0);
        let prefix = write.prefix().to_string();
        let caps = Value::Array(self.caps);
        let none = Value::Null;
        let key = format!("{prefix}capabilities");
        update_metadata(write, Meta::User, self.id, &key, &caps, &none).await?;
        let key = format!("{prefix}user_level");
        update_metadata(write, Meta::User, self.id, &key, &Value::Int(level), &none).await?;
        Ok(())
    }
}

/// `level_reduction()`: the level of a `/^level_(10|[0-9])$/i` key.
pub(super) fn level(key: &Key) -> Option<i64> {
    let Key::String(name) = key else {
        return None;
    };
    let name = name.strip_suffix(b"\n").unwrap_or(name);
    let digits = name
        .get(..6)
        .filter(|head| head.eq_ignore_ascii_case(b"level_"))
        .and(name.get(6..))?;
    match digits {
        [digit] if digit.is_ascii_digit() => Some(i64::from(digit - b'0')),
        b"10" => Some(10),
        _ => None,
    }
}

/// `WP_User::add_role( $role )`: `role` beside the user's roles, unless it
/// is empty or one of them.
pub async fn add_role(write: &mut Write<'_>, id: u64, role: &[u8]) -> Result<(), DbError> {
    let mut user = User::load(write, id).await?;
    let role = Key::from_php_string(role.to_vec());
    if is_empty_string(&role.to_php_string()) || user.roles().contains(&role) {
        return Ok(());
    }
    user.grant(role);
    user.save(write).await
}

/// `WP_User::set_role( $role )`: the user's roles give way to `role`, or to
/// none when it is empty.
pub async fn set_role(write: &mut Write<'_>, id: u64, role: &[u8]) -> Result<(), DbError> {
    let mut user = User::load(write, id).await?;
    let role = Key::from_php_string(role.to_vec());
    let roles = user.roles();
    if roles == [role.clone()] {
        return Ok(());
    }
    user.caps.retain(|(key, _)| !roles.contains(key));
    if !is_empty_string(&role.to_php_string()) {
        user.grant(role);
    }
    user.save(write).await
}

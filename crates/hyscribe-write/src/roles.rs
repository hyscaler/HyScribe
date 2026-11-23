// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_Roles`: the site's roles in `{prefix}user_roles`, as `get_role()`
//! reads them and `add_role()`, `remove_role()`, `WP_Role::add_cap()` and
//! `WP_Role::remove_cap()` write them.

use hyscribe_db::Write;
use hyscribe_phpser::{Key, Value, is_empty_string, isset};

use crate::Error;
use crate::options::{get_option, update_option};

/// A role's capabilities, each granted or denied, in their stored order.
pub type Caps = Vec<(String, bool)>;

/// The roles as stored, and the option holding them.
async fn load(write: &mut Write<'_>) -> Result<(String, Vec<(Key, Value)>), Error> {
    let option = format!("{}user_roles", write.prefix());
    let roles = match get_option(write, &option).await? {
        Value::Array(roles) => roles,
        _ => Vec::new(),
    };
    Ok((option, roles))
}

fn key(name: &str) -> Key {
    Key::from_php_string(name.as_bytes().to_vec())
}

/// `isset( $roles[ $role ] )`'s entry.
fn find<'a>(roles: &'a mut [(Key, Value)], role: &str) -> Option<&'a mut Value> {
    let role = key(role);
    let found = roles
        .iter_mut()
        .find(|(name, value)| *name == role && isset(Some(value)));
    found.map(|(_, value)| value)
}

/// `$entries[ $name ] = $value`: in place, or added at the end.
fn set(entries: &mut Vec<(Key, Value)>, name: Key, value: Value) {
    match entries.iter_mut().find(|(held, _)| *held == name) {
        Some((_, held)) => *held = value,
        None => entries.push((name, value)),
    }
}

/// A role's `capabilities`, made an empty array when it is not one, as
/// PHP's assignment makes a missing one; none for a role not an array.
fn capabilities(role: &mut Value) -> Option<&mut Vec<(Key, Value)>> {
    let Value::Array(fields) = role else {
        return None;
    };
    let caps = key("capabilities");
    if !matches!(
        fields.iter().find(|(name, _)| *name == caps),
        Some((_, Value::Array(_)))
    ) {
        set(fields, caps.clone(), Value::Array(Vec::new()));
    }
    fields.iter_mut().find_map(|(name, value)| match value {
        Value::Array(held) if *name == caps => Some(held),
        _ => None,
    })
}

/// `get_role( $role )->capabilities`: none for no such role.
pub async fn get_role(write: &mut Write<'_>, role: &str) -> Result<Option<Caps>, Error> {
    let (_, mut roles) = load(write).await?;
    let caps = find(&mut roles, role).map(|found| {
        let caps = found.get_str("capabilities").and_then(Value::as_array);
        let caps = caps.unwrap_or_default().iter();
        caps.map(|(cap, grant)| {
            let cap = String::from_utf8_lossy(&cap.to_php_string()).into_owned();
            (cap, !grant.is_empty())
        })
        .collect()
    });
    Ok(caps)
}

/// `add_role( $role, $display_name, $capabilities )`: false, writing
/// nothing, for an empty name or a role that exists.
pub async fn add_role(
    write: &mut Write<'_>,
    role: &str,
    display_name: &str,
    caps: &[(String, bool)],
) -> Result<bool, Error> {
    let (option, mut roles) = load(write).await?;
    if is_empty_string(role.as_bytes()) || find(&mut roles, role).is_some() {
        return Ok(false);
    }
    let mut granted = Vec::new();
    for (cap, grant) in caps {
        set(&mut granted, key(cap), Value::Bool(*grant));
    }
    let name = Value::String(display_name.as_bytes().to_vec());
    let entry = vec![
        (key("name"), name),
        (key("capabilities"), Value::Array(granted)),
    ];
    set(&mut roles, key(role), Value::Array(entry));
    update_option(write, &option, &Value::Array(roles), None).await?;
    Ok(true)
}

/// `remove_role( $role )`: the role gone, and `default_role` back to
/// `subscriber` when it named it; false for no such role.
pub async fn remove_role(write: &mut Write<'_>, role: &str) -> Result<bool, Error> {
    let (option, mut roles) = load(write).await?;
    let name = key(role);
    let before = roles.len();
    roles.retain(|(held, _)| *held != name);
    if roles.len() == before {
        return Ok(false);
    }
    update_option(write, &option, &Value::Array(roles), None).await?;
    if get_option(write, "default_role").await?.as_bytes() == Some(role.as_bytes()) {
        let subscriber = Value::String(b"subscriber".to_vec());
        update_option(write, "default_role", &subscriber, None).await?;
    }
    Ok(true)
}

/// `WP_Role::add_cap( $cap, $grant )`, or `remove_cap( $cap )` without a
/// grant: false, writing nothing, for no such role.
pub async fn set_cap(
    write: &mut Write<'_>,
    role: &str,
    cap: &str,
    grant: Option<bool>,
) -> Result<bool, Error> {
    let (option, mut roles) = load(write).await?;
    let Some(caps) = find(&mut roles, role).and_then(capabilities) else {
        return Ok(false);
    };
    match grant {
        Some(grant) => set(caps, key(cap), Value::Bool(grant)),
        None => caps.retain(|(held, _)| *held != key(cap)),
    }
    update_option(write, &option, &Value::Array(roles), None).await?;
    Ok(true)
}

#[cfg(test)]
mod tests;

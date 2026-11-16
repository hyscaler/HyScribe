// SPDX-License-Identifier: AGPL-3.0-or-later

//! `admin-ajax.php`'s `heartbeat`: `wp_ajax_heartbeat()` with core's
//! filters for the screens hs-admin serves, the edit lock it renews
//! written as PHP writes it, and `wp_ajax_nopriv_heartbeat()`.

use hyscribe_admin::actions::Asked;
use hyscribe_db::{Meta, UserFields, Write};
use hyscribe_phpser::{Key, Value, absint, json_encode, lookup};
use hyscribe_render::avatar_url;
use hyscribe_write::Error;
use hyscribe_write::options::update_metadata;

use super::posts::locked;
use super::{Acting, Ajax, Saving, read};

/// A response under construction: its keys in the order PHP sets them.
type Response = Vec<(Key, Value)>;

fn key(name: &str) -> Key {
    Key::String(name.as_bytes().to_vec())
}

fn object(entries: Vec<(&str, Value)>) -> Value {
    Value::Array(entries.into_iter().map(|(k, v)| (key(k), v)).collect())
}

fn string(text: impl Into<Vec<u8>>) -> Value {
    Value::String(text.into())
}

/// `$response[ $name ] = $value`.
fn set(response: &mut Response, name: &str, value: Value) {
    match response.iter_mut().find(|(k, _)| *k == key(name)) {
        Some((_, old)) => *old = value,
        None => response.push((key(name), value)),
    }
}

/// `wp_send_json( $response )`, whose text is all UTF-8.
fn sent(response: Response) -> Ajax {
    let json = json_encode(&Value::Array(response)).unwrap_or_default();
    Ajax::Json(String::from_utf8_lossy(&json).into_owned())
}

/// `$received['post_id']` of a part of the data, sent as an array or a
/// string: `None` where PHP fails, reading an offset of a string.
fn post_id(received: &Value) -> Option<Option<&Value>> {
    received
        .as_array()
        .map(|entries| lookup(entries, "post_id"))
}

/// The holder's name, text and avatars, as the lock filters send them.
fn holder(site: &dyn Acting, user: &UserFields, text: &str, sizes: [i64; 2]) -> Value {
    let name = String::from_utf8_lossy(&user.display_name).into_owned();
    let mut entries = vec![
        ("name", string(name.clone())),
        ("text", string(text.replace("%s", &name))),
    ];
    let option = |name: &str| read(|| site.option(name));
    if option("show_avatars").is_some_and(|shown| !Value::String(shown).is_empty()) {
        let url = |size| string(avatar_url(&option, &user.email, &user.display_name, size));
        entries.push(("avatar_src", url(sizes[0])));
        entries.push(("avatar_src_2x", url(sizes[1])));
    }
    object(entries)
}

/// `current_user_can( 'edit_post', $id )`.
fn can_edit(site: &dyn Acting, id: i64) -> bool {
    read(|| site.current_user_can("edit_post", &[Value::Int(id)]))
}

/// `wp_refresh_nonces`: `wp_refresh_post_nonces()`,
/// `wp_refresh_metabox_loader_nonces()` and `wp_refresh_heartbeat_nonces()`.
fn refresh_nonces(site: &dyn Acting, data: &[(Key, Value)], response: &mut Response) -> Option<()> {
    let nonce = |action: &str| string(read(|| site.create_nonce(action)));
    if let Some(received) = lookup(data, "wp-refresh-post-nonces") {
        set(
            response,
            "wp-refresh-post-nonces",
            object(vec![("check", Value::Int(1))]),
        );
        let id = absint(post_id(received)?.unwrap_or(&Value::Null));
        if id != 0 && can_edit(site, id.cast_signed()) {
            let replace = object(vec![
                ("getpermalinknonce", nonce("getpermalink")),
                ("samplepermalinknonce", nonce("samplepermalink")),
                ("closedpostboxesnonce", nonce("closedpostboxes")),
                ("_ajax_linking_nonce", nonce("internal-linking")),
                ("_wpnonce", nonce(&format!("update-post_{id}"))),
            ]);
            set(
                response,
                "wp-refresh-post-nonces",
                object(vec![("replace", replace)]),
            );
        }
    }
    if let Some(received) =
        lookup(data, "wp-refresh-metabox-loader-nonces").filter(|v| !v.is_empty())
    {
        let id = post_id(received)?.map_or(0, Value::to_int);
        if id != 0 && can_edit(site, id) {
            let replace = object(vec![
                ("metabox_loader_nonce", nonce("meta-box-loader")),
                ("_wpnonce", nonce(&format!("update-post_{id}"))),
            ]);
            set(
                response,
                "wp-refresh-metabox-loader-nonces",
                object(vec![("replace", replace)]),
            );
        }
    }
    set(response, "rest_nonce", nonce("wp_rest"));
    set(response, "heartbeat_nonce", nonce("heartbeat-nonce"));
    Some(())
}

/// `wp_check_locked_posts()`: the posts of the list another user edits.
async fn check_locked_posts(
    write: &mut Write<'_>,
    (asked, site): (&Asked<'_>, &dyn Acting),
    data: &[(Key, Value)],
    response: &mut Response,
) -> Result<(), Error> {
    let Some(Value::Array(keys)) = lookup(data, "wp-check-locked-posts") else {
        return Ok(());
    };
    let mut checked = Vec::new();
    for (_, name) in keys {
        let Value::String(name) = name else {
            return Err(Error::Unported("a post key that is not text"));
        };
        let id = absint(&string(name.get(5..).unwrap_or_default()));
        if id == 0 {
            continue;
        }
        let Some(user) = locked(write, id, asked.user, asked.now).await? else {
            continue;
        };
        if can_edit(site, id.cast_signed()) {
            let send = holder(site, &user, "%s is currently editing", [18, 36]);
            checked.push((Key::String(name.clone()), send));
        }
    }
    if !checked.is_empty() {
        set(response, "wp-check-locked-posts", Value::Array(checked));
    }
    Ok(())
}

/// `wp_refresh_post_lock()`: the lock renewed, written now, unless another
/// user has taken over.
async fn refresh_post_lock(
    write: &mut Write<'_>,
    (asked, site): (&Asked<'_>, &dyn Acting),
    data: &[(Key, Value)],
    response: &mut Response,
) -> Result<(), Error> {
    let Some(received) = lookup(data, "wp-refresh-post-lock") else {
        return Ok(());
    };
    let id = post_id(received).ok_or(Error::Unported("a post lock that is not an array"))?;
    let id = absint(id.unwrap_or(&Value::Null));
    if id == 0 || !can_edit(site, id.cast_signed()) {
        return Ok(());
    }
    let send = match locked(write, id, asked.user, asked.now).await? {
        Some(user) => {
            let text = "%s has taken over and is currently editing.";
            object(vec![("lock_error", holder(site, &user, text, [64, 128]))])
        }
        None => {
            // `wp_set_post_lock()`.
            let lock = format!("{}:{}", asked.now, asked.user);
            let value = string(lock.clone());
            update_metadata(write, Meta::Post, id, "_edit_lock", &value, &Value::Null).await?;
            object(vec![("new_lock", string(lock))])
        }
    };
    set(response, "wp-refresh-post-lock", send);
    Ok(())
}

/// `wp_ajax_heartbeat()`, past the check of its nonce's presence.
pub(super) fn heartbeat<'a>(
    write: &'a mut Write<'_>,
    asked: &'a Asked<'a>,
    site: &'a dyn Acting,
) -> Saving<'a, Ajax> {
    Box::pin(async move {
        let nonce = lookup(asked.post, "_nonce").map_or_else(Vec::new, Value::to_php_string);
        let state = read(|| site.nonce_state(&nonce, "heartbeat-nonce"));
        let data = match lookup(asked.post, "data").filter(|data| !data.is_empty()) {
            Some(Value::Array(entries)) => entries.clone(),
            Some(value) => vec![(Key::Int(0), value.clone())],
            None => Vec::new(),
        };
        let mut response = Response::new();
        if state != 1 {
            refresh_nonces(site, &data, &mut response)
                .ok_or(Error::Unported("a post ID read from a string"))?;
            if state == 0 {
                set(&mut response, "nonces_expired", Value::Bool(true));
                return Ok(sent(response));
            }
        }
        if !data.is_empty() {
            check_locked_posts(write, (asked, site), &data, &mut response).await?;
            refresh_post_lock(write, (asked, site), &data, &mut response).await?;
            // `heartbeat_autosave()`: the classic editor's autosave.
            if lookup(&data, "wp_autosave").is_some_and(|saved| !saved.is_empty()) {
                return Err(Error::Unported("the classic editor's autosave").into());
            }
        }
        let logged_in = read(|| site.logged_in());
        set(&mut response, "wp-auth-check", Value::Bool(logged_in));
        set(&mut response, "server_time", Value::Int(asked.now));
        Ok(sent(response))
    })
}

/// `wp_ajax_nopriv_heartbeat()`: no one is logged in.
pub(in crate::admin) fn nopriv(now: i64) -> String {
    format!("{{\"wp-auth-check\":false,\"server_time\":{now}}}")
}

#[cfg(test)]
mod tests;

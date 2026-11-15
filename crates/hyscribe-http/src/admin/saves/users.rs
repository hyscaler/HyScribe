// SPDX-License-Identifier: AGPL-3.0-or-later

//! The users list's writes (`users.php`).

use hyscribe_admin::actions::{Asked, user_ids};
use hyscribe_admin::posts::Answer;
use hyscribe_db::{DbError, Write};
use hyscribe_phpser::{
    Key, Maybe, Value, add_query_arg, is_empty_string, lookup, maybe_unserialize, php_trim,
    remove_query_arg, stripslashes,
};
use hyscribe_render::formatting::sanitize_user;
use hyscribe_write::Error;
use hyscribe_write::posts::Context;
use hyscribe_write::users::{get_password_reset_key, set_role, wp_delete_user};

use super::{Saving, Stop};

/// `$redirect`: the list's own `wp_http_referer` field, else the list.
fn redirect(asked: &Asked) -> Result<Vec<u8>, Stop> {
    match asked.request("wp_http_referer") {
        Some(Value::String(referer)) => Ok(remove_query_arg(
            &[b"wp_http_referer", b"updated", b"delete_count"],
            referer,
        )),
        // `remove_query_arg()` of an array is a fatal error.
        Some(_) => Err(Error::Unported("an array as wp_http_referer").into()),
        None => Ok(b"users".to_vec()),
    }
}

/// `promote`, the bulk role change: each user but the current one gets
/// `new_role` (no role for `none`), then the list is sent back with `update`.
pub(super) fn promote<'a>(write: &'a mut Write<'_>, asked: &'a Asked<'a>) -> Saving<'a> {
    Box::pin(async move {
        let redirect = redirect(asked)?;
        if asked.request("users").is_none_or(Value::is_empty) {
            return Ok(Answer::Redirect(text(&redirect)));
        }
        let mut role = asked
            .request("new_role")
            .map(Value::to_php_string)
            .unwrap_or_default();
        if role == b"none" {
            role.clear();
        }
        let me = i64::try_from(asked.user).unwrap_or_default();
        let mut update: &[u8] = b"promote";
        for id in user_ids(asked) {
            if id == me {
                if role.is_empty() {
                    let message = "Sorry, you cannot remove your own role.";
                    return Err(Stop::Died(Answer::Denied(message)));
                }
                if !can_promote(write, &role).await? {
                    update = b"err_admin_role";
                }
                continue;
            }
            // `get_userdata()` of no user is false, and its `set_role()` a fatal error.
            let id = u64::try_from(id).unwrap_or_default();
            if write.user(id).await?.is_none() {
                return Err(Error::Unported("a role change of a missing user").into());
            }
            set_role(write, id, &role).await?;
        }
        let to = add_query_arg(&[(b"update", Some(update))], &redirect);
        Ok(Answer::Redirect(text(&to)))
    })
}

/// `dodelete`, the deletion the confirmation page asks for: each user but
/// the current one deleted, their content with them or passed to the user
/// `reassign_user` names; then the list with how many were.
pub(super) fn dodelete<'a>(write: &'a mut Write<'_>, asked: &'a Asked<'a>) -> Saving<'a> {
    Box::pin(async move {
        let redirect = redirect(asked)?;
        if asked.request("users").is_none_or(Value::is_empty) {
            return Ok(Answer::Redirect(text(&redirect)));
        }
        // PHP sends a request with no option back to the confirmation page,
        // whose link carries a nonce.
        let Some(Value::Array(options)) = asked.request("delete_option").filter(|o| !o.is_empty())
        else {
            return Err(Error::Unported("a deletion with no option chosen").into());
        };
        let heirs = asked.request("reassign_user");
        // The uploads are under the home URL, whose host has its port.
        let host = asked.home.split_once("://").map_or("", |(_, rest)| rest);
        let request = Context {
            user: asked.user,
            can_publish: false,
            home: asked.home.as_bytes().to_vec(),
            upload_host: host.split('/').next().unwrap_or_default().into(),
            now: crate::now(),
        };
        let me = i64::try_from(asked.user).unwrap_or_default();
        let (mut update, mut count): (&[u8], u64) = (b"del", 0);
        for id in user_ids(asked) {
            if id == me {
                update = b"err_admin_del";
                continue;
            }
            let option = lookup(options, &id.to_string()).and_then(Value::as_bytes);
            let heir = heirs.and_then(|heirs| heirs.get_str(&id.to_string()));
            let heir = heir.filter(|heir| !heir.is_empty()).map(Value::to_int);
            let id = u64::try_from(id).unwrap_or_default();
            match (option, heir) {
                (Some(b"reassign"), None) => {
                    update = b"err_missing_reassign";
                    continue;
                }
                (Some(b"delete"), _) => {
                    wp_delete_user(write, id, None, &request).await?;
                }
                (Some(b"reassign"), Some(heir)) => {
                    // Content passed to no user is not ported.
                    let heir = u64::try_from(heir).ok().filter(|heir| *heir > 0);
                    let heir = heir.ok_or(Error::Unported("content passed to no user"))?;
                    wp_delete_user(write, id, Some(heir), &request).await?;
                }
                _ => {}
            }
            count += 1;
        }
        let count = count.to_string();
        let args: [(&[u8], Option<&[u8]>); 2] = [
            (b"delete_count", Some(count.as_bytes())),
            (b"update", Some(update)),
        ];
        Ok(Answer::Redirect(text(&add_query_arg(&args, &redirect))))
    })
}

/// `resetpassword`: a password reset link for each user but the current
/// one, the mails taken as sent; then the list with how many were.
pub(super) fn resetpassword<'a>(write: &'a mut Write<'_>, asked: &'a Asked<'a>) -> Saving<'a> {
    Box::pin(async move {
        let redirect = redirect(asked)?;
        if asked.request("users").is_none_or(Value::is_empty) {
            return Ok(Answer::Redirect(text(&redirect)));
        }
        let me = i64::try_from(asked.user).unwrap_or_default();
        let mut count = 0;
        for id in user_ids(asked).into_iter().filter(|id| *id != me) {
            let user = write.user(u64::try_from(id).unwrap_or_default()).await?;
            let login = user.map(|(login, _)| login).unwrap_or_default();
            if retrieve_password(write, &login).await?.is_none() {
                count += 1;
            }
        }
        let count = count.to_string();
        let args: [(&[u8], Option<&[u8]>); 2] = [
            (b"reset_count", Some(count.as_bytes())),
            (b"update", Some(b"resetpassword")),
        ];
        Ok(Answer::Redirect(text(&add_query_arg(&args, &redirect))))
    })
}

/// `retrieve_password( $user_login )`: the user's password reset key kept,
/// the mail taken as sent (MVP phase 8 sends it); else the error PHP gives
/// for no such user.
pub(super) async fn retrieve_password(
    write: &mut Write<'_>,
    login: &[u8],
) -> Result<Option<&'static str>, Stop> {
    let login = stripslashes(login);
    let login = php_trim(&login);
    if is_empty_string(login) {
        return Ok(Some(
            "<strong>Error:</strong> Please enter a username or email address.",
        ));
    }
    let mut found = None;
    // `strpos( $user_login, '@' )`, true past the first character.
    if login.get(1..).is_some_and(|rest| rest.contains(&b'@')) {
        found = write.user_id_by_email(login).await?;
    }
    if found.is_none() {
        found = write.user_id_by_login(&sanitize_user(login)).await?;
    }
    let Some(id) = found else {
        return Ok(Some(
            "<strong>Error:</strong> There is no account with that username or email address.",
        ));
    };
    get_password_reset_key(write, id).await?;
    Ok(None)
}

/// `$wp_roles->role_objects[ $role ]->has_cap( 'promote_users' )`.
pub(super) async fn can_promote(write: &mut Write<'_>, role: &[u8]) -> Result<bool, DbError> {
    let name = format!("{}user_roles", write.prefix());
    let roles = write.option_value(&name).await?.unwrap_or_default();
    let role = Key::from_php_string(role.to_vec());
    let granted = |roles: &Value| {
        let caps = roles
            .get(&role)
            .and_then(|role| role.get_str("capabilities"));
        caps.and_then(|caps| caps.get_str("promote_users"))
            .is_some_and(|granted| !granted.is_empty())
    };
    Ok(matches!(maybe_unserialize(&roles), Maybe::Value(roles) if granted(&roles)))
}

pub(super) fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(test)]
mod tests;

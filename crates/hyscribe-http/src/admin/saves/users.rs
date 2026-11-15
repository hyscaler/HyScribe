// SPDX-License-Identifier: AGPL-3.0-or-later

//! The users list's writes (`users.php`).

use hyscribe_admin::actions::{Asked, user_ids};
use hyscribe_admin::posts::Answer;
use hyscribe_db::{DbError, Write};
use hyscribe_phpser::{Key, Maybe, Value, add_query_arg, maybe_unserialize, remove_query_arg};
use hyscribe_write::Error;
use hyscribe_write::users::set_role;

use super::{Saving, Stop, text};

/// `promote`, the bulk role change: each user but the current one gets
/// `new_role` (no role for `none`), then the list is sent back with `update`.
pub(super) fn promote<'a>(write: &'a mut Write<'_>, asked: &'a Asked<'a>) -> Saving<'a> {
    Box::pin(async move {
        // `$redirect`: the list's own `wp_http_referer` field, else the list.
        let redirect = match asked.request("wp_http_referer") {
            Some(Value::String(referer)) => {
                remove_query_arg(&[b"wp_http_referer", b"updated", b"delete_count"], referer)
            }
            // `remove_query_arg()` of an array is a fatal error.
            Some(_) => return Err(Error::Unported("an array as wp_http_referer").into()),
            None => b"users".to_vec(),
        };
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

/// `$wp_roles->role_objects[ $role ]->has_cap( 'promote_users' )`.
async fn can_promote(write: &mut Write<'_>, role: &[u8]) -> Result<bool, DbError> {
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

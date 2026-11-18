// SPDX-License-Identifier: AGPL-3.0-or-later

//! Personal data requests (`wp-includes/user.php`): a request's
//! confirmation, which `wp-login.php?action=confirmaction` fires.

use hyscribe_db::{Meta, Write};
use hyscribe_model::hooks::do_action;
use hyscribe_phpser::Value;

use crate::Error;
use crate::options::{get_metadata, update_metadata};
use crate::posts::{Context, wp_update_post};

/// A confirmed request the site's admin is to be told of.
#[derive(Debug, PartialEq, Eq)]
pub struct Confirmed {
    /// The request's `email` (its `post_title`).
    pub email: Vec<u8>,
    /// The request's `action_name` (its `post_name`).
    pub action_name: Vec<u8>,
}

/// `do_action( 'user_request_action_confirmed', $request_id )` for the
/// `(int)` of the link's `request_id`: `_wp_privacy_account_request_confirmed()`
/// marks a pending or failed request confirmed, at `ctx.now`; then
/// `_wp_privacy_send_request_confirmation_notification()` finds the
/// request to tell the admin of, when it is confirmed and they have not been
/// told (see [`admin_notified`], which the caller runs once the mail is sent).
/// A negative ID finds its request through `absint()`, but `wp_update_post()`
/// finds no post for it: the meta is written, the status is not changed.
pub async fn user_request_action_confirmed(
    write: &mut Write<'_>,
    ctx: &Context,
    request_id: i64,
) -> Result<Option<Confirmed>, Error> {
    do_action("user_request_action_confirmed", &[&request_id]);
    let id = request_id.unsigned_abs();
    let Some(request) = write.post(id).await? else {
        return Ok(None);
    };
    if request.post_type != "user_request" {
        return Ok(None);
    }
    let mut status = request.status;
    if status == "request-pending" || status == "request-failed" {
        let at = Value::Int(ctx.now);
        let none = Value::Null;
        let key = "_wp_user_request_confirmed_timestamp";
        update_metadata(write, Meta::Post, id, key, &at, &none).await?;
        if request_id > 0 {
            let confirm =
                |args: &mut crate::posts::PostArgs| args.status = "request-confirmed".into();
            wp_update_post(write, ctx, id, &confirm).await?;
            status = "request-confirmed".into();
        }
    }
    if status != "request-confirmed" {
        return Ok(None);
    }
    let notified = get_metadata(write, Meta::Post, id, "_wp_admin_notified").await?;
    if notified.first().is_some_and(|value| !value.is_empty()) {
        return Ok(None);
    }
    Ok(Some(Confirmed {
        email: request.title,
        action_name: request.name,
    }))
}

/// `update_post_meta( $request_id, '_wp_admin_notified', true )`, once the
/// admin's mail is sent.
pub async fn admin_notified(write: &mut Write<'_>, id: u64) -> Result<(), Error> {
    let (yes, none) = (Value::Bool(true), Value::Null);
    update_metadata(write, Meta::Post, id, "_wp_admin_notified", &yes, &none).await?;
    Ok(())
}

#[cfg(test)]
mod tests;

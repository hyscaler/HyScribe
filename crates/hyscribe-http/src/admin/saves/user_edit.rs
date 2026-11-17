// SPDX-License-Identifier: AGPL-3.0-or-later

//! The saves of Add User, Edit User and the profile (`user-new.php`,
//! `user-edit.php` and `profile.php`, through `edit_user()`), and the
//! user's Ajax writes. Mail is phase 8: PHP's mails are not sent, and the
//! answers are those PHP gives once they are.

use hyscribe_admin::actions::Asked;
use hyscribe_admin::posts::Answer;
use hyscribe_auth::sessions::Sessions;
use hyscribe_config::Constants;
use hyscribe_db::{Meta, Write};
use hyscribe_kses::{ALLOWED_PROTOCOLS, esc_url_raw, sanitize_key};
use hyscribe_phpser::{
    Key, Maybe, Value, absint, add_query_arg, addslashes, is_empty_string, json_encode, lookup,
    maybe_unserialize, php_trim, remove_query_arg, stripslashes, urlencode,
};
use hyscribe_render::formatting::{
    is_email, sanitize_text_field, sanitize_user, sanitize_user_strict,
};
use hyscribe_write::Error;
use hyscribe_write::caps::Caps;
use hyscribe_write::options::{add_metadata, get_metadata};
use hyscribe_write::users::{
    UserData, get_password_reset_key, request_email_change, retrieve_password, wp_insert_user,
    wp_update_user,
};

use super::screen::update_user_meta;
use super::users::can_promote;
use super::{Acting, Ajax, Saving, Stop, id_of, text};

/// What PHP cannot go on with, or shows again: not ported.
fn unported(what: &'static str) -> Stop {
    Stop::Failed(Error::Unported(what))
}

/// A write function's refusal, which `edit_user()` returns for its form
/// to show again; not ported.
fn refused(err: Error) -> Stop {
    match err {
        Error::Refused(_) => unported("the user form shown again with its errors"),
        err => Stop::Failed(err),
    }
}

/// The fields of the user forms that PHP's string functions read.
const FIELDS: [&str; 13] = [
    "user_login",
    "pass1",
    "pass2",
    "role",
    "email",
    "url",
    "first_name",
    "last_name",
    "nickname",
    "display_name",
    "description",
    "locale",
    "admin_color",
];

/// An array as a field, which PHP's string functions cannot take: not ported.
fn arrays_refused(asked: &Asked) -> Result<(), Stop> {
    let array = |name| matches!(lookup(asked.post, name), Some(Value::Array(_)));
    match FIELDS.into_iter().any(array) {
        true => Err(unported("an array as a field of the user form")),
        false => Ok(()),
    }
}

/// `$_POST[ $name ]`, slashed as PHP holds it.
fn posted(asked: &Asked, name: &str) -> Option<Vec<u8>> {
    lookup(asked.post, name).map(|value| addslashes(&value.to_php_string()))
}

/// `isset( $_POST[ $name ] ) && $text === $_POST[ $name ]`.
fn posted_is(asked: &Asked, name: &str, text: &str) -> bool {
    lookup(asked.post, name).and_then(Value::as_bytes) == Some(text.as_bytes())
}

/// `edit_user()`'s URL: one with a protocol WordPress allows, else `http://`.
fn user_url(url: &[u8]) -> Vec<u8> {
    if is_empty_string(url) || url == b"http://" {
        return Vec::new();
    }
    let url = esc_url_raw(url);
    let lower = url.to_ascii_lowercase();
    let allowed = ALLOWED_PROTOCOLS
        .iter()
        .any(|protocol| lower.starts_with(format!("{protocol}:").as_bytes()));
    if allowed {
        url
    } else {
        [&b"http://"[..], &url].concat()
    }
}

/// `edit_user( $user_id )` by `current`: the user `id`, 0 for a new one,
/// from `$_POST`, its email the one `email` (unslashed) leaves. The user's
/// ID; a form with errors, which PHP shows again, is not ported.
async fn edit_user(
    write: &mut Write<'_>,
    asked: &Asked<'_>,
    (id, current): (u64, &Caps),
    email: Option<Vec<u8>>,
) -> Result<u64, Stop> {
    let update = id != 0;
    let mut user = UserData {
        id,
        ..UserData::default()
    };
    // An empty login is refused below as invalid, one not posted by
    // `wp_insert_user()`.
    let mut wrong = false;
    if !update {
        let login = lookup(asked.post, "user_login");
        user.user_login = login.map(|login| sanitize_user_strict(&login.to_php_string()));
    }
    let pass = |name| php_trim(&posted(asked, name).unwrap_or_default()).to_vec();
    let (pass1, pass2) = (pass("pass1"), pass("pass2"));
    if let Some(role) = posted(asked, "role").filter(|_| current.can("promote_users", &[])) {
        let role = sanitize_text_field(&role);
        let roles = write
            .option_value(&format!("{}user_roles", write.prefix()))
            .await?;
        let editable = matches!(
            maybe_unserialize(&roles.unwrap_or_default()),
            Maybe::Value(roles) if roles.get(&Key::from_php_string(role.clone())).is_some()
        );
        if !is_empty_string(&role) && !editable {
            let message = "Sorry, you are not allowed to give users that role.";
            return Err(Stop::Died(Answer::Denied(message)));
        }
        if asked.user != id || can_promote(write, &role).await? {
            user.role = Some(role);
        }
    }
    if let Some(email) = &email {
        wrong |= !is_email(email);
        user.user_email = Some(email.clone());
    }
    user.user_url = posted(asked, "url").map(|url| user_url(&url));
    let field = |name| posted(asked, name).map(|text| sanitize_text_field(&text));
    user.first_name = field("first_name");
    user.last_name = field("last_name");
    user.nickname = field("nickname");
    user.display_name = field("display_name");
    user.description = posted(asked, "description").map(|text| php_trim(&text).to_vec());
    // No language is installed, and none is downloaded.
    user.locale = field("locale").map(|locale| match &locale[..] {
        b"" => b"en_US".to_vec(),
        _ => Vec::new(),
    });
    if update {
        let flag = |name| {
            if posted_is(asked, name, "false") {
                "false"
            } else {
                "true"
            }
        };
        user.rich_editing = Some(flag("rich_editing").into());
        user.syntax_highlighting = Some(flag("syntax_highlighting").into());
        user.infinite_scrolling = Some(flag("infinite_scrolling").into());
        let color = field("admin_color");
        user.admin_color = Some(color.unwrap_or_else(|| b"modern".to_vec()));
        let bar = lookup(asked.post, "admin_bar_front").is_some();
        user.show_admin_bar_front = Some(if bar { "true" } else { "false" }.into());
        wrong |= user.nickname.as_deref().is_none_or(is_empty_string);
    }
    let shortcuts = posted_is(asked, "comment_shortcuts", "true");
    user.comment_shortcuts = Some(if shortcuts { "true" } else { "" }.into());
    let ssl = lookup(asked.post, "use_ssl").is_some_and(|ssl| !ssl.is_empty());
    user.use_ssl = Some(if ssl { "1" } else { "0" }.into());
    wrong |= !update && is_empty_string(&pass1);
    wrong |= stripslashes(&pass1).contains(&b'\\');
    wrong |= (update || !is_empty_string(&pass1)) && pass1 != pass2;
    if !is_empty_string(&pass1) {
        user.user_pass = Some(pass1);
    }
    if let (false, Some(posted)) = (update, posted(asked, "user_login")) {
        // `validate_username()` of the login as posted.
        let valid = sanitize_user_strict(&posted);
        wrong |= valid != posted || is_empty_string(&valid);
        // `username_exists()`.
        let login = php_trim(user.user_login.as_deref().unwrap_or_default());
        let taken = write.user_id_by_login(&sanitize_user(login)).await?;
        wrong |= taken.is_some();
    }
    let email = user.user_email.as_deref().unwrap_or_default();
    wrong |= is_empty_string(email);
    if is_email(email) {
        let owner = write.user_id_by_email(php_trim(email)).await?;
        // A new user's ID is 0, which no owner has.
        wrong |= owner.is_some_and(|owner| owner != id);
    }
    if wrong {
        return Err(unported("the user form shown again with its errors"));
    }
    if update {
        wp_update_user(write, &user).await.map_err(refused)
    } else {
        wp_insert_user(write, &user).await.map_err(refused)
    }
}

/// `user-new.php`'s `createuser`: the new user, past `user_register`'s
/// admin hook, their password reset key kept when they are to be told of
/// the account; then the users list, or the form for one who may not list
/// users.
pub(super) fn create<'a>(
    write: &'a mut Write<'_>,
    asked: &'a Asked<'a>,
    site: &'a dyn Acting,
) -> Saving<'a> {
    Box::pin(async move {
        let current = Caps::load(write, asked.user, None, site.constants()).await?;
        arrays_refused(asked)?;
        let email = posted(asked, "email").map(|email| stripslashes(&email));
        let id = edit_user(write, asked, (0, &current), email).await?;
        let none = Value::String(Vec::new());
        add_metadata(write, Meta::User, id, "dismissed_wp_pointers", &none, false).await?;
        if lookup(asked.post, "send_user_notification").is_some() {
            get_password_reset_key(write, id).await?;
        }
        Ok(Answer::Redirect(if current.can("list_users", &[]) {
            format!("users?update=add&id={id}")
        } else {
            "user-new?update=add".into()
        }))
    })
}

/// `update` of `profile.php` and `user-edit.php`: the user saved by
/// `edit_user()`, after an email change of one's own waits for its
/// confirmation; then their edit screen with `updated`.
pub(super) fn update<'a>(
    write: &'a mut Write<'_>,
    asked: &'a Asked<'a>,
    site: &'a dyn Acting,
) -> Saving<'a> {
    Box::pin(updated(write, asked, site.constants()))
}

async fn updated(
    write: &mut Write<'_>,
    asked: &Asked<'_>,
    constants: Constants,
) -> Result<Answer, Stop> {
    let given = asked
        .request("user_id")
        .filter(|id| !id.is_empty())
        .map_or(0, absint);
    let id = if given == 0 { asked.user } else { given };
    let referer = match asked.request("wp_http_referer").filter(|r| !r.is_empty()) {
        Some(Value::Array(_)) => return Err(unported("an array as wp_http_referer")),
        referer => referer.map(|r| esc_url_raw(&addslashes(&r.to_php_string()))),
    };
    let referer = referer.map(|r| remove_query_arg(&[b"update", b"delete_count", b"user_id"], &r));
    arrays_refused(asked)?;
    let mut email = posted(asked, "email");
    // `send_confirmation_on_profile_email()` of one's own save, the only
    // one it acts on (and always on the profile page).
    if id == asked.user {
        let own = write.user(id).await?.map(|(_, user)| user.email);
        let own = own.unwrap_or_default();
        if email.as_deref() != Some(&own[..]) {
            if let Some(new) = email.as_deref().filter(|email| is_email(email)) {
                request_email_change(write, id, new).await?;
            }
            email = Some(own);
        }
    }
    let current = Caps::load(write, asked.user, None, constants).await?;
    let email = email.map(|email| stripslashes(&email));
    edit_user(write, asked, (id, &current), email).await?;
    let link = if id == asked.user {
        format!("{}/hs-admin/profile", asked.home)
    } else {
        format!("{}/hs-admin/user-edit?user_id={id}", asked.home)
    };
    let mut redirect = add_query_arg(&[(b"updated", Some(b"1"))], link.as_bytes());
    if let Some(referer) = referer.filter(|referer| !is_empty_string(referer)) {
        let referer = urlencode(&referer);
        redirect = add_query_arg(&[(b"wp_http_referer", Some(&referer))], &redirect);
    }
    Ok(Answer::Redirect(text(&redirect)))
}

/// `wp_send_json_success( $data )` or `wp_send_json_error( $data )`, an
/// Ajax action's answer.
fn json(success: bool, data: Value) -> Ajax {
    Ajax::Json(text(&json_body(success, data)))
}

/// The JSON of `json()`.
fn json_body(success: bool, data: Value) -> Vec<u8> {
    let response = Value::Array(vec![
        (Key::String(b"success".to_vec()), Value::Bool(success)),
        (Key::String(b"data".to_vec()), data),
    ]);
    json_encode(&response).unwrap_or_default()
}

/// `admin-ajax.php`'s `save-user-color-scheme`: the current user's scheme,
/// registered as the checks found.
pub(super) fn color_scheme<'a>(
    write: &'a mut Write<'_>,
    asked: &'a Asked<'a>,
    _: &'a dyn Acting,
) -> Saving<'a, Ajax> {
    Box::pin(async move {
        let scheme = lookup(asked.post, "color_scheme").map(Value::to_php_string);
        let scheme = sanitize_key(&scheme.unwrap_or_default());
        let previous = get_metadata(write, Meta::User, asked.user, "admin_color").await?;
        let previous = previous.first().map(Value::to_php_string);
        let previous = previous.unwrap_or_default();
        let value = Value::String(scheme.clone().into_bytes());
        update_user_meta(write, asked.user, "admin_color", &value).await?;
        let class = |scheme: &[u8]| Value::String([&b"admin-color-"[..], scheme].concat());
        Ok(json(
            true,
            Value::Array(vec![
                (Key::String(b"previousScheme".to_vec()), class(&previous)),
                (
                    Key::String(b"currentScheme".to_vec()),
                    class(scheme.as_bytes()),
                ),
            ]),
        ))
    })
}

/// `admin-ajax.php`'s `destroy-sessions` past its checks, which write no
/// row: HyScribe's own sessions of the user (an open question, ADR 0011) end, all but
/// the one of `token` for the current user. `name` gives a display name.
pub(in crate::admin) fn destroy_sessions(
    sessions: Option<&Sessions>,
    asked: &Asked,
    token: &[u8],
    name: impl FnOnce(u64) -> Vec<u8>,
) -> Vec<u8> {
    let id = lookup(asked.post, "user_id").map_or(0, absint);
    let own = id == asked.user;
    // A session that cannot be removed expires all the same.
    let _ = sessions.map(|sessions| match own {
        true => sessions.destroy_others(id, token, crate::now()),
        false => sessions.destroy_all(id),
    });
    let message = match own {
        true => b"You are now logged out everywhere else.".to_vec(),
        false => [&name(id)[..], b" has been logged out."].concat(),
    };
    let message = vec![(Key::String(b"message".to_vec()), Value::String(message))];
    json_body(true, Value::Array(message))
}

/// `admin-ajax.php`'s `send-password-reset` for a user the current one may
/// edit, as the checks found: `retrieve_password()`'s key kept, the mail
/// taken as sent.
pub(super) fn password_reset<'a>(
    write: &'a mut Write<'_>,
    asked: &'a Asked<'a>,
    _: &'a dyn Acting,
) -> Saving<'a, Ajax> {
    Box::pin(async move {
        let user = write.user(id_of(lookup(asked.post, "user_id"))).await?;
        let (login, name) =
            user.map_or_else(Default::default, |(login, user)| (login, user.display_name));
        Ok(match retrieve_password(write, &login).await? {
            Ok(_) => {
                let message = [&b"A password reset link was emailed to "[..], &name, b"."];
                json(true, Value::String(message.concat()))
            }
            Err(refusal) => json(false, Value::String(refusal.message.into())),
        })
    })
}

#[cfg(test)]
mod tests;

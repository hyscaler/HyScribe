// SPDX-License-Identifier: AGPL-3.0-or-later

//! The saves of Add User, Edit User and the profile (`user-new.php`,
//! `user-edit.php` and `profile.php`, through `edit_user()`), and the
//! user's Ajax writes. Mail is phase 8: PHP's mails are not sent, and the
//! answers are those PHP gives once they are.

use hyscribe_admin::actions::Asked;
use hyscribe_admin::posts::Answer;
use hyscribe_auth::caps::{CapPost, CapabilityData, User};
use hyscribe_auth::sessions::Sessions;
use hyscribe_db::{DbError, Meta, Write};
use hyscribe_kses::{ALLOWED_PROTOCOLS, esc_url_raw, sanitize_key};
use hyscribe_phpser::{
    Key, Maybe, Value, absint, add_query_arg, addslashes, is_empty_string, json_encode, lookup,
    maybe_unserialize, php_trim, remove_query_arg, stripslashes, urlencode,
};
use hyscribe_render::formatting::{
    is_email, sanitize_text_field, sanitize_user, sanitize_user_strict,
};
use hyscribe_write::Error;
use hyscribe_write::options::{add_metadata, get_metadata, update_metadata};
use hyscribe_write::users::{
    UserData, get_password_reset_key, request_email_change, wp_insert_user, wp_update_user,
};

use super::users::{can_promote, retrieve_password, text};
use super::{Saving, Stop};

/// The current user's capabilities, read through the action's `Write`.
struct Current {
    id: u64,
    prefix: String,
    /// `{prefix}user_roles`.
    roles: Option<Vec<u8>>,
    /// The user's `{prefix}capabilities`.
    caps: Option<Vec<u8>>,
}

impl Current {
    async fn load(write: &mut Write<'_>, id: u64) -> Result<Current, DbError> {
        let prefix = write.prefix().to_string();
        let roles = write.option_value(&format!("{prefix}user_roles")).await?;
        let key = format!("{prefix}capabilities");
        let meta = write.object_meta(Meta::User, id).await?;
        let caps = meta.into_iter().find(|(name, _)| *name == key.as_bytes());
        Ok(Current {
            id,
            prefix,
            roles,
            caps: caps.and_then(|(_, caps)| caps),
        })
    }

    /// `current_user_can( $cap )`.
    fn can(&self, cap: &str) -> bool {
        User::load(self, &self.prefix, self.id).has_cap(self, cap, &[])
    }
}

/// Only the roles and the current user's capabilities are read.
impl CapabilityData for Current {
    fn option(&self, _: &str) -> Option<Vec<u8>> {
        self.roles.clone()
    }
    fn user_meta(&self, _: u64, _: &str) -> Option<Vec<u8>> {
        self.caps.clone()
    }
    fn user_exists(&self, id: u64) -> bool {
        id == self.id
    }
    fn post(&self, _: u64) -> Option<CapPost> {
        None
    }
    fn post_meta(&self, _: u64, _: &str) -> Option<Vec<u8>> {
        None
    }
    fn comment_post(&self, _: u64) -> Option<u64> {
        None
    }
    fn term_taxonomy(&self, _: u64) -> Option<String> {
        None
    }
    fn constant(&self, _: &str) -> bool {
        false
    }
}

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

/// `$_POST[ $name ]`, slashed as PHP holds it; an array, which PHP's string
/// functions cannot take, is not ported.
fn posted(asked: &Asked, name: &str) -> Result<Option<Vec<u8>>, Stop> {
    match lookup(asked.post, name) {
        Some(Value::Array(_)) => Err(unported("an array as a field of the user form")),
        value => Ok(value.map(|value| addslashes(&value.to_php_string()))),
    }
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
    (id, current): (u64, &Current),
    email: Option<Vec<u8>>,
) -> Result<u64, Stop> {
    let update = id != 0;
    let mut user = UserData {
        id,
        ..UserData::default()
    };
    let mut wrong = false;
    if !update {
        user.user_login = match lookup(asked.post, "user_login") {
            Some(Value::Array(_)) => return Err(unported("an array as the new user's login")),
            login => login.map(|login| sanitize_user_strict(&login.to_php_string())),
        };
        // `'' === $user->user_login`; a login not posted, a WP_Error later.
        wrong |= user.user_login.as_deref().is_none_or(<[u8]>::is_empty);
    }
    let pass = |name| posted(asked, name).map(|pass| php_trim(&pass.unwrap_or_default()).to_vec());
    let (pass1, pass2) = (pass("pass1")?, pass("pass2")?);
    if let Some(role) = posted(asked, "role")?.filter(|_| current.can("promote_users")) {
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
    user.user_url = posted(asked, "url")?.map(|url| user_url(&url));
    let field = |name| posted(asked, name).map(|text| text.map(|text| sanitize_text_field(&text)));
    user.first_name = field("first_name")?;
    user.last_name = field("last_name")?;
    user.nickname = field("nickname")?;
    user.display_name = field("display_name")?;
    user.description = posted(asked, "description")?.map(|text| php_trim(&text).to_vec());
    // No language is installed, and none is downloaded.
    user.locale = field("locale")?.map(|locale| match &locale[..] {
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
        let color = field("admin_color")?;
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
    if let (false, Some(posted)) = (update, posted(asked, "user_login")?) {
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
        wrong |= owner.is_some_and(|owner| !update || owner != id);
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
pub(super) fn create<'a>(write: &'a mut Write<'_>, asked: &'a Asked<'a>) -> Saving<'a> {
    Box::pin(async move {
        let current = Current::load(write, asked.user).await?;
        let email = posted(asked, "email")?.map(|email| stripslashes(&email));
        let id = edit_user(write, asked, (0, &current), email).await?;
        let none = Value::String(Vec::new());
        add_metadata(write, Meta::User, id, "dismissed_wp_pointers", &none, false).await?;
        if lookup(asked.post, "send_user_notification").is_some() {
            get_password_reset_key(write, id).await?;
        }
        Ok(Answer::Redirect(if current.can("list_users") {
            format!("users?update=add&id={id}")
        } else {
            "user-new?update=add".into()
        }))
    })
}

/// `update` of `profile.php`.
pub(super) fn profile<'a>(write: &'a mut Write<'_>, asked: &'a Asked<'a>) -> Saving<'a> {
    Box::pin(update(write, asked, true))
}

/// `update` of `user-edit.php`.
pub(super) fn user_edit<'a>(write: &'a mut Write<'_>, asked: &'a Asked<'a>) -> Saving<'a> {
    Box::pin(update(write, asked, false))
}

/// The user saved by `edit_user()`, after an email change of one's own
/// waits for its confirmation; then their edit screen with `updated`.
async fn update(write: &mut Write<'_>, asked: &Asked<'_>, profile: bool) -> Result<Answer, Stop> {
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
    let mut email = posted(asked, "email")?;
    // `send_confirmation_on_profile_email()`, on the profile page.
    if (profile || given == asked.user) && id == asked.user {
        let own = write.user(id).await?.map(|(_, user)| user.email);
        let own = own.unwrap_or_default();
        if email.as_deref() != Some(&own[..]) {
            if let Some(new) = email.as_deref().filter(|email| is_email(email)) {
                request_email_change(write, id, new).await?;
            }
            email = Some(own);
        }
    }
    let current = Current::load(write, asked.user).await?;
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
/// Ajax action's answer: its JSON as the page.
fn json(success: bool, data: Value) -> Answer {
    Answer::Page(text(&json_body(success, data)))
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
pub(super) fn color_scheme<'a>(write: &'a mut Write<'_>, asked: &'a Asked<'a>) -> Saving<'a> {
    Box::pin(async move {
        let scheme = lookup(asked.post, "color_scheme").map(Value::to_php_string);
        let scheme = sanitize_key(&scheme.unwrap_or_default());
        let previous = get_metadata(write, Meta::User, asked.user, "admin_color").await?;
        let previous = previous.first().map(Value::to_php_string);
        let previous = previous.unwrap_or_default();
        let value = Value::String(scheme.clone().into_bytes());
        update_metadata(
            write,
            Meta::User,
            asked.user,
            "admin_color",
            &value,
            &Value::Null,
        )
        .await?;
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
    if let Some(sessions) = sessions {
        // A session that cannot be removed expires all the same.
        let _ = match own {
            true => sessions.destroy_others(id, token, crate::now()),
            false => sessions.destroy_all(id),
        };
    }
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
pub(super) fn password_reset<'a>(write: &'a mut Write<'_>, asked: &'a Asked<'a>) -> Saving<'a> {
    Box::pin(async move {
        let id = lookup(asked.post, "user_id").map_or(0, Value::to_int);
        let user = write.user(u64::try_from(id).unwrap_or_default()).await?;
        let (login, name) =
            user.map_or_else(Default::default, |(login, user)| (login, user.display_name));
        Ok(match retrieve_password(write, &login).await? {
            None => {
                let message = [&b"A password reset link was emailed to "[..], &name, b"."];
                json(true, Value::String(message.concat()))
            }
            Some(error) => json(false, Value::String(error.into())),
        })
    })
}

#[cfg(test)]
mod tests;

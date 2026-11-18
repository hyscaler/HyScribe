// SPDX-License-Identifier: AGPL-3.0-or-later

//! `register_new_user()`: the checks of the registration form, the new
//! user, and the reset key their welcome mail carries.

use hyscribe_auth::generate_password;
use hyscribe_db::{Meta, Write};
use hyscribe_kses::esc_attr;
use hyscribe_model::hooks::{apply_filters, do_action};
use hyscribe_phpser::{Value, addslashes, is_empty_string, php_trim};
use hyscribe_render::formatting::{is_email, sanitize_user, sanitize_user_strict};

use super::{ResetKey, UserData, get_password_reset_key, wp_insert_user};
use crate::Error;
use crate::options::{get_option, update_metadata};
use crate::steps::{Steps, step};

/// An error of `register_new_user()`'s `WP_Error`: its code and message,
/// as markup.
#[derive(Debug, PartialEq, Eq)]
pub struct Rejected {
    /// The error code.
    pub code: &'static str,
    /// The message.
    pub message: String,
}

fn rejected(code: &'static str, message: &str) -> Rejected {
    Rejected {
        code,
        message: message.into(),
    }
}

/// `validate_username()` of a login as given: it keeps every character
/// `sanitize_user( $login, true )` keeps, and is not `empty()`.
fn validate_username(login: &[u8]) -> bool {
    let strict = sanitize_user_strict(login);
    strict == login && !is_empty_string(&strict)
}

/// The checks of `register_new_user( $user_login, $user_email )` on the
/// login and address as given, unslashed; `login_url` is `wp_login_url()`.
/// The errors in the order they are added, none when the registration may
/// go on, with the login as sanitized and the address.
async fn check(
    steps: &mut Steps<'_, '_, Error>,
    (login, email): (&[u8], &[u8]),
    login_url: &str,
) -> (Vec<Rejected>, Vec<u8>, Vec<u8>) {
    let mut errors = Vec::new();
    let mut sanitized = sanitize_user(login);
    let email = apply_filters("user_registration_email", email.to_vec(), &[]);
    if sanitized.is_empty() {
        errors.push(rejected(
            "empty_username",
            "<strong>Error:</strong> Please enter a username.",
        ));
    } else if !validate_username(login) {
        errors.push(rejected(
            "invalid_username",
            "<strong>Error:</strong> This username is invalid because it uses illegal characters. Please enter a valid username.",
        ));
        sanitized.clear();
    } else if step!(steps, user_id_by_login(&sanitized)).is_some() {
        errors.push(rejected(
            "username_exists",
            "<strong>Error:</strong> This username is already registered. Please choose another one.",
        ));
    }
    let mut email = email;
    if email.is_empty() {
        errors.push(rejected(
            "empty_email",
            "<strong>Error:</strong> Please type your email address.",
        ));
    } else if !is_email(&email) {
        errors.push(rejected(
            "invalid_email",
            "<strong>Error:</strong> The email address is not correct.",
        ));
        email.clear();
    } else if step!(steps, user_id_by_email(php_trim(&email))).is_some() {
        let url = hyscribe_kses::esc_url(login_url.as_bytes());
        errors.push(Rejected {
            code: "email_exists",
            message: format!(
                "<strong>Error:</strong> This email address is already registered. <a href=\"{}\">Log in</a> with this address or choose another one.",
                String::from_utf8_lossy(&url)
            ),
        });
    }
    do_action("register_post", &[&sanitized, &email, &errors.len()]);
    let errors = apply_filters("registration_errors", errors, &[&sanitized, &email]);
    (errors, sanitized, email)
}

/// `register_new_user( $user_login, $user_email )` on the login and
/// address as given, unslashed: the new user with a reset key for their
/// mail, or the errors; `login_url` is `wp_login_url()`.
pub async fn register_new_user(
    write: &mut Write<'_>,
    (login, email): (&[u8], &[u8]),
    login_url: &str,
) -> Result<Result<ResetKey, Vec<Rejected>>, Error> {
    let mut steps = Steps::new(write);
    let (errors, login, email) = check(&mut steps, (login, email), login_url).await;
    if !errors.is_empty() {
        return steps.finish(Ok(Err(errors)));
    }
    let pass = steps.keep(generate_password(12, false).map_err(Error::Random));
    let data = UserData {
        user_login: Some(addslashes(&login)),
        user_email: Some(addslashes(&email)),
        user_pass: Some(pass.into_bytes()),
        ..UserData::default()
    };
    // A refusal of `wp_insert_user()` comes before it writes anything.
    let created = steps
        .run(async |w| match wp_insert_user(w, &data).await {
            Err(Error::Refused(_)) => Ok(0),
            other => other,
        })
        .await;
    if created == 0 {
        let admin = steps.run(async |w| get_option(w, "admin_email").await);
        let admin = esc_attr(&admin.await.to_php_string());
        let message = format!(
            "<strong>Error:</strong> Could not register you&hellip; please contact the <a href=\"mailto:{}\">site admin</a>!",
            admin
        );
        let registerfail = Rejected {
            code: "registerfail",
            message,
        };
        return steps.finish(Ok(Err(vec![registerfail])));
    }
    let nag = (Value::Bool(true), Value::Null);
    let meta = async |w: &mut Write<'_>| {
        update_metadata(
            w,
            Meta::User,
            created,
            "default_password_nag",
            &nag.0,
            &nag.1,
        )
        .await
    };
    steps.run(meta).await;
    do_action("register_new_user", &[&created]);
    steps.finish(Ok(()))?;
    get_password_reset_key(write, created).await.map(Ok)
}

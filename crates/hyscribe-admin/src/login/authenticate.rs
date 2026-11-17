// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_authenticate()`: the `authenticate` filter with core's callbacks,
//! which a site adds to or replaces, then `wp_login_failed`.

use hyscribe_kses::esc_html;
use hyscribe_phpser::{is_empty_string, php_trim};
use hyscribe_render::formatting::{is_email, sanitize_user};

use super::{Notice, Session, Site, User, error, lost_password};

/// A `WP_User` as the login hooks pass it on: its ID and `user_login`.
#[derive(Clone)]
pub struct Account {
    /// `ID`.
    pub id: u64,
    /// `user_login`.
    pub login: Vec<u8>,
}

/// What an `authenticate` callback is passed and returns.
pub enum Authenticated {
    /// `null`: no callback has decided.
    Undecided,
    /// A `WP_User`: the credentials are good.
    User(Account),
    /// A `WP_Error`.
    Error(Vec<Notice>),
}

/// An `authenticate` callback: `( $user, $username, $password )`.
pub type Callback<'a> = Box<dyn Fn(Authenticated, &[u8], &[u8]) -> Authenticated + 'a>;

struct Filter<'a> {
    name: &'static str,
    priority: i32,
    callback: Callback<'a>,
}

/// The `authenticate` filter's callbacks, in the order they run.
pub struct Authenticate<'a>(Vec<Filter<'a>>);

impl<'a> Authenticate<'a> {
    /// `add_filter( 'authenticate', $callback, $priority )`: after the
    /// callbacks of a lower or the same priority.
    pub fn add(&mut self, name: &'static str, priority: i32, callback: Callback<'a>) {
        let at = self.0.partition_point(|filter| filter.priority <= priority);
        self.0.insert(
            at,
            Filter {
                name,
                priority,
                callback,
            },
        );
    }

    /// The callback `name` replaced at its priority; false for none.
    pub fn replace(&mut self, name: &str, callback: Callback<'a>) -> bool {
        let found = self.0.iter_mut().find(|filter| filter.name == name);
        found.map(|filter| filter.callback = callback).is_some()
    }

    /// `apply_filters( 'authenticate', null, $username, $password )`.
    fn apply(&self, username: &[u8], password: &[u8]) -> Authenticated {
        let filters = self.0.iter();
        filters.fold(Authenticated::Undecided, |user, filter| {
            (filter.callback)(user, username, password)
        })
    }
}

/// The result of `wp_check_password()` for a `user` found by its `kind` of
/// name.
fn checked(
    site: &dyn Site,
    (user, password): (User, &[u8]),
    (kind, name): (&str, &[u8]),
    secure: bool,
) -> Authenticated {
    if site.check_password(&user, password) {
        return Authenticated::User(Account {
            id: user.id,
            login: user.login,
        });
    }
    Authenticated::Error(vec![error(
        "incorrect_password",
        format!(
            "<strong>Error:</strong> The password you entered for the {kind} <strong>{}</strong> is incorrect.{}",
            esc_html(name),
            lost_password(site, secure)
        ),
    )])
}

/// The errors of a blank field: `name`'s, the password's, or both.
fn blank(user: Authenticated, name: (&[u8], &str), password: &[u8]) -> Authenticated {
    if let Authenticated::Error(_) = user {
        return user;
    }
    let mut errors = Vec::new();
    if is_empty_string(name.0) {
        let text = format!("<strong>Error:</strong> The {} field is empty.", name.1);
        errors.push(error("empty_username", text));
    }
    if is_empty_string(password) {
        let text = "<strong>Error:</strong> The password field is empty.";
        errors.push(error("empty_password", text.into()));
    }
    Authenticated::Error(errors)
}

/// Core's callbacks, `wp_authenticate_cookie()` as `wp_signon()` adds it,
/// with `session` as the auth cookie and `secure` as `force_ssl_admin()`.
pub(super) fn core<'a>(site: &'a dyn Site, session: &'a Session, secure: bool) -> Authenticate<'a> {
    let mut filters = Authenticate(Vec::new());
    filters.add(
        "wp_authenticate_username_password",
        20,
        Box::new(move |user, username, password| {
            if let Authenticated::User(_) = user {
                return user;
            }
            if is_empty_string(username) || is_empty_string(password) {
                return blank(user, (username, "username"), password);
            }
            match site.user_by_login(username) {
                Some(found) => checked(site, (found, password), ("username", username), secure),
                None => Authenticated::Error(vec![error(
                    "invalid_username",
                    format!(
                        "<strong>Error:</strong> The username <strong>{}</strong> is not registered on this site. If you are unsure of your username, try your email address instead.",
                        esc_html(username)
                    ),
                )]),
            }
        }),
    );
    filters.add(
        "wp_authenticate_email_password",
        20,
        Box::new(move |user, email, password| {
            if let Authenticated::User(_) = user {
                return user;
            }
            if is_empty_string(email) || is_empty_string(password) {
                return blank(user, (email, "email"), password);
            }
            if !is_email(email) {
                return user;
            }
            match site.user_by_email(email) {
                Some(found) => checked(site, (found, password), ("email address", email), secure),
                None => Authenticated::Error(vec![error(
                    "invalid_email",
                    "Unknown email address. Check again or try your username.".into(),
                )]),
            }
        }),
    );
    filters.add(
        "wp_authenticate_cookie",
        30,
        Box::new(move |user, username, password| {
            let empty = is_empty_string(username) && is_empty_string(password);
            match session {
                _ if matches!(user, Authenticated::User(_)) || !empty => user,
                Session::User(account) => Authenticated::User(account.clone()),
                Session::Expired => Authenticated::Error(vec![error(
                    "expired_session",
                    "Please log in again.".into(),
                )]),
                Session::None => user,
            }
        }),
    );
    // A single site marks no user as a spammer.
    filters.add(
        "wp_authenticate_spam_check",
        99,
        Box::new(|user, _, _| user),
    );
    filters
}

/// `wp_authenticate()` over `filters`, firing `wp_login_failed` for a
/// failure other than a blank field.
pub(super) fn wp_authenticate(
    site: &dyn Site,
    filters: &Authenticate,
    username: &[u8],
    password: &[u8],
) -> Result<Account, Vec<Notice>> {
    let username = sanitize_user(username);
    let errors = match filters.apply(&username, php_trim(password)) {
        Authenticated::User(account) => return Ok(account),
        Authenticated::Undecided => vec![error(
            "authentication_failed",
            "<strong>Error:</strong> Invalid username, email address or incorrect password.".into(),
        )],
        Authenticated::Error(errors) => errors,
    };
    let code = errors.first().map(|e| e.code);
    if !matches!(code, Some("empty_username" | "empty_password")) {
        site.wp_login_failed(&username, &errors);
    }
    Err(errors)
}

#[cfg(test)]
mod tests;

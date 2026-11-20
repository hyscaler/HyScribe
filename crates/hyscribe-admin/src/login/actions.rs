// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp-login.php`'s other actions: the lost password and registration
//! forms, the reset password link and form, the check your email screen and
//! a personal data request's confirmation. The caller writes what they
//! write.

use hyscribe_auth::{generate_password, wp_verify_fast_hash};
use hyscribe_kses::esc_attr;
use hyscribe_model::hooks::{apply_filters, do_action};
use hyscribe_phpser::{
    Value, addslashes, hash_equals, is_empty_string, php_trim, remove_query_arg, string_to_number,
};
use hyscribe_render::formatting::{sanitize_text_field, sanitize_user};

use super::{Frame, Notice, Request, Site, User, UserRequest, as_text, error, message, url};

/// What one of these actions answers.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    /// The screen.
    Page(Vec<u8>),
    /// `wp_die()`'s page, with a 500.
    Died(Vec<u8>),
    /// A 302 to this location, with the reset password cookie as given.
    Redirect(Vec<u8>, Reset),
    /// `reset_password()` of this user with this password, slashed, then
    /// the page [`password_reset`] shows.
    ResetPassword(u64, Vec<u8>),
    /// A good confirmation link: confirm this request, under the `(int)` of
    /// the link's `request_id`, then show [`request_confirmed`].
    ConfirmRequest(UserRequest, i64),
    /// What this port does not answer: the action writes, or PHP fails
    /// with a `TypeError`.
    NotPorted,
}

/// What a redirect does to the reset password cookie.
#[derive(Debug, PartialEq, Eq)]
pub enum Reset {
    /// Leaves it.
    Keep,
    /// Sets it to this value, for the session.
    Set(Vec<u8>),
    /// Expires it.
    Expire,
}

/// `$_REQUEST['redirect_to']` as printed when not `empty()`; a query
/// string's array is never empty.
fn redirect_to(request: &Request) -> Vec<u8> {
    match request.request("redirect_to") {
        Some(Value::String(to)) if !is_empty_string(to) => addslashes(to),
        Some(Value::String(_)) | None => Vec::new(),
        Some(_) => b"Array".to_vec(),
    }
}

/// `<p id="nav">` with the log in link and `extra` after the separator.
fn nav(site: &dyn Site, extra: Option<String>) -> String {
    let extra = extra.map_or(String::new(), |link| format!(" | {link}"));
    format!(
        "\n\t\t<p id=\"nav\">\n\t\t\t<a class=\"wp-login-log-in\" href=\"{}\">Log in</a>\n\t\t\t{extra}\t\t</p>\n\t\t",
        url(&site.login_url(""))
    )
}

/// `wp_get_admin_notice()`'s info notice with `classes`.
fn info(classes: &str, text: &str) -> String {
    format!("<div class=\"notice notice-info message{classes}\"><p>{text}</p></div>")
}

fn frame(
    site: &dyn Site,
    request: &Request,
    (title, action): (&str, &str),
    message: &str,
    errors: &[Notice],
    body: &str,
    input_id: &str,
) -> Vec<u8> {
    Frame {
        title,
        action,
        interim: request.request("interim-login").is_some(),
        customize: false,
        loggedout: false,
        message,
        errors,
        head_scripts: &[],
        scripts: &[],
        body,
        input_id,
    }
    .render(site)
}

/// The registration link of the forms' `<p id="nav">`, on a site anyone
/// may register on.
fn register_link(site: &dyn Site) -> Option<String> {
    site.can_register().then(|| {
        format!(
            "<a class=\"wp-login-register\" href=\"{}\">Register</a>",
            url(&site.login_url("?action=register"))
        )
    })
}

/// The lost password form, with the error `retrieve_password()` gave a
/// posted form (`$errors`'s code and message) first.
pub fn lost_password(
    site: &dyn Site,
    request: &Request,
    action: &str,
    refused: Option<(&'static str, &str)>,
) -> Vec<u8> {
    let mut errors: Vec<Notice> = refused
        .map(|(code, text)| error(code, text.into()))
        .into_iter()
        .collect();
    match as_text(request.query.get_str("error")) {
        Some(b"invalidkey") => errors.push(error(
            "invalidkey",
            "<strong>Error:</strong> Your password reset link appears to be invalid. Please request a new link below.".into(),
        )),
        Some(b"expiredkey") => errors.push(error(
            "expiredkey",
            "<strong>Error:</strong> Your password reset link has expired. Please request a new link below.".into(),
        )),
        _ => {}
    }
    let redirect = apply_filters("lostpassword_redirect", redirect_to(request), &[]);
    do_action("lost_password", &[&errors.len()]);
    let user_login = as_text(request.posted("user_login")).unwrap_or_default();
    let body = format!(
        "\n\t\t<form name=\"lostpasswordform\" id=\"lostpasswordform\" action=\"{}\" method=\"post\">\n\t\t\t<p>\n\t\t\t\t<label for=\"user_login\">Username or Email Address</label>\n\t\t\t\t<input type=\"text\" name=\"user_login\" id=\"user_login\" class=\"input ltr\" value=\"{}\" size=\"20\" autocapitalize=\"off\" autocomplete=\"username\" required=\"required\" />\n\t\t\t</p>\n\t\t\t\t\t\t<input type=\"hidden\" name=\"redirect_to\" value=\"{}\" />\n\t\t\t<p class=\"submit\">\n\t\t\t\t<input type=\"submit\" name=\"wp-submit\" id=\"wp-submit\" class=\"button button-primary button-large\" value=\"Get New Password\" />\n\t\t\t</p>\n\t\t</form>\n{}",
        url(&site.login_url("?action=lostpassword")),
        esc_attr(user_login),
        esc_attr(&redirect),
        nav(site, register_link(site))
    );
    let message = info(
        "",
        "Please enter your username or email address. You will receive an email message with instructions on how to reset your password.",
    );
    frame(
        site,
        request,
        ("Lost Password", action),
        &message,
        &errors,
        &body,
        "user_login",
    )
}

/// The registration form, or the redirect of a site nobody may register on.
fn register(site: &dyn Site, request: &Request) -> Action {
    if !site.can_register() {
        let to = site.login_url("?registration=disabled");
        return Action::Redirect(to.into_bytes(), Reset::Keep);
    }
    Action::Page(registration(site, request, &[], (b"", b"")))
}

/// The registration form of a site anyone may register on, with the errors
/// `register_new_user()` gave a posted one (each a code and message) and the
/// login and address as posted.
pub fn registration(
    site: &dyn Site,
    request: &Request,
    refused: &[(&'static str, &str)],
    (login, email): (&[u8], &[u8]),
) -> Vec<u8> {
    let errors: Vec<Notice> = refused
        .iter()
        .map(|&(code, text)| error(code, text.into()))
        .collect();
    let redirect = apply_filters("registration_redirect", redirect_to(request), &[]);
    let lost = format!(
        "<a class=\"wp-login-lost-password\" href=\"{}\">Lost your password?</a>",
        url(&site.login_url("?action=lostpassword"))
    );
    let body = format!(
        "\t\t<form name=\"registerform\" id=\"registerform\" action=\"{}\" method=\"post\" novalidate=\"novalidate\">\n\t\t\t<p>\n\t\t\t\t<label for=\"user_login\">Username</label>\n\t\t\t\t<input type=\"text\" name=\"user_login\" id=\"user_login\" class=\"input ltr\" value=\"{}\" size=\"20\" autocapitalize=\"off\" autocomplete=\"username\" required=\"required\" />\n\t\t\t</p>\n\t\t\t<p>\n\t\t\t\t<label for=\"user_email\">Email</label>\n\t\t\t\t<input type=\"email\" name=\"user_email\" id=\"user_email\" class=\"input\" value=\"{}\" size=\"25\" autocomplete=\"email\" required=\"required\" />\n\t\t\t</p>\n\t\t\t\t\t\t<p id=\"reg_passmail\">\n\t\t\t\tRegistration confirmation will be emailed to you.\t\t\t</p>\n\t\t\t<input type=\"hidden\" name=\"redirect_to\" value=\"{}\" />\n\t\t\t<p class=\"submit\">\n\t\t\t\t<input type=\"submit\" name=\"wp-submit\" id=\"wp-submit\" class=\"button button-primary button-large\" value=\"Register\" />\n\t\t\t</p>\n\t\t</form>\n{}",
        url(&site.login_url("?action=register")),
        esc_attr(login),
        esc_attr(email),
        esc_attr(&redirect),
        nav(site, Some(lost))
    );
    frame(
        site,
        request,
        ("Registration Form", "register"),
        &info(" register", "Register For This Site"),
        &errors,
        &body,
        "user_login",
    )
}

/// `explode( ':', $text, 2 )` of a text with a colon.
fn split(text: &[u8]) -> Option<(&[u8], &[u8])> {
    let at = text.iter().position(|&b| b == b':')?;
    let (first, rest) = text.split_at(at);
    rest.split_first().map(|(_, rest)| (first, rest))
}

/// What `check_password_reset_key()` finds.
enum Checked {
    /// The key is the user's and has not expired.
    User(User),
    /// `expired_key`.
    Expired,
    /// `invalid_key`.
    Invalid,
    /// PHP's `TypeError`: the stored request time holds no number.
    Fatal,
}

/// `check_password_reset_key( $key, $login )` at `now`.
fn check_password_reset_key(site: &dyn Site, key: &[u8], login: &[u8], now: i64) -> Checked {
    let key: Vec<u8> = key
        .iter()
        .copied()
        .filter(u8::is_ascii_alphanumeric)
        .collect();
    // `get_user_by()` trims the login, and finds no one for an empty one.
    let login = php_trim(login);
    if is_empty_string(&key) || is_empty_string(login) {
        return Checked::Invalid;
    }
    let Some(user) = site.user_by_login(&sanitize_user(login)) else {
        return Checked::Invalid;
    };
    let duration = apply_filters("password_reset_expiration", 86_400.0, &[]);
    let stored = user.activation_key.clone();
    let (pass_key, expiration) = match split(&stored) {
        Some((time, pass_key)) => match string_to_number(time) {
            Some(time) => (pass_key, time + duration),
            None => return Checked::Fatal,
        },
        None => (&stored[..], 0.0),
    };
    if is_empty_string(pass_key) {
        return Checked::Invalid;
    }
    let correct = wp_verify_fast_hash(&key, pass_key);
    // A key without a time has expired, its expiration 0 never ahead.
    if correct && (now as f64) < expiration {
        return Checked::User(user);
    }
    // A plain key, as WordPress before 4.3 stored it, has expired too.
    if correct || hash_equals(&stored, &key) {
        return Checked::Expired;
    }
    Checked::Invalid
}

/// Back to the lost password form with `error`, the cookie expired.
fn lost_again(site: &dyn Site, error: &str) -> Action {
    let to = site.login_url(&format!("?action=lostpassword&error={error}"));
    Action::Redirect(to.into_bytes(), Reset::Expire)
}

/// `$_POST[ $key ]` as PHP 8 trims it, slashed: `None` for an array,
/// which `trim()` refuses, and `''` for nothing.
fn trimmed(request: &Request, key: &str) -> Option<Vec<u8>> {
    match request.posted(key) {
        Some(Value::String(text)) => Some(php_trim(&addslashes(text)).to_vec()),
        Some(_) => None,
        None => Some(Vec::new()),
    }
}

/// The reset password link, to the cookie, and form: a bad or expired key
/// goes back to the lost password form, a good form to `reset_password()`.
fn reset_password(
    site: &dyn Site,
    request: &Request,
    (action, request_uri): (&str, &[u8]),
    now: i64,
) -> Action {
    match (request.query.get_str("key"), request.query.get_str("login")) {
        (Some(Value::String(key)), Some(Value::String(login))) => {
            let value = [&login[..], b":", key].concat();
            let to = remove_query_arg(&[b"key", b"login"], request_uri);
            return Action::Redirect(to, Reset::Set(value));
        }
        // `sprintf()` of an array.
        (Some(_), Some(_)) => return Action::NotPorted,
        _ => {}
    }
    let pair = request.reset_cookie.and_then(split);
    let Some((login, key)) = pair.filter(|(login, _)| !login.is_empty()) else {
        return lost_again(site, "invalidkey");
    };
    let mut checked = check_password_reset_key(site, key, login, now);
    if request.posted("pass1").is_some() {
        match request.posted("rp_key") {
            Some(Value::String(posted)) if hash_equals(key, &addslashes(posted)) => {}
            Some(Value::String(_)) => checked = Checked::Invalid,
            // `hash_equals()` of an array, or of nothing.
            _ => return Action::NotPorted,
        }
    }
    let user = match checked {
        Checked::User(user) => user,
        Checked::Expired => return lost_again(site, "expiredkey"),
        Checked::Invalid => return lost_again(site, "invalidkey"),
        Checked::Fatal => return Action::NotPorted,
    };
    // `$_POST['pass1']`, trimmed unless `empty()`, which trimming keeps; a
    // form's array is never empty.
    let Some(pass1) = trimmed(request, "pass1") else {
        return Action::NotPorted;
    };
    let given = as_text(request.posted("pass1")).is_some_and(|pass| !is_empty_string(pass));
    let mut errors = Vec::new();
    if given && is_empty_string(&pass1) {
        errors.push(error(
            "password_reset_empty_space",
            "The password cannot be a space or all spaces.".into(),
        ));
    }
    if !is_empty_string(&pass1) {
        let Some(pass2) = trimmed(request, "pass2") else {
            return Action::NotPorted;
        };
        if pass2 != pass1 {
            errors.push(error(
                "password_reset_mismatch",
                "<strong>Error:</strong> The passwords do not match.".into(),
            ));
        }
    }
    do_action("validate_password_reset", &[&errors.len(), &user.id]);
    if errors.is_empty() && !is_empty_string(&pass1) {
        return Action::ResetPassword(user.id, pass1);
    }
    let body = format!(
        "\t\t<form name=\"resetpassform\" id=\"resetpassform\" action=\"{}\" method=\"post\" autocomplete=\"off\">\n\t\t\t<input type=\"hidden\" id=\"user_login\" value=\"{}\" autocomplete=\"off\" />\n\n\t\t\t<div class=\"user-pass1-wrap\">\n\t\t\t\t<p>\n\t\t\t\t\t<label for=\"pass1\">New password</label>\n\t\t\t\t</p>\n\n\t\t\t\t<div class=\"wp-pwd\">\n\t\t\t\t\t<input type=\"password\" name=\"pass1\" id=\"pass1\" class=\"input password-input ltr\" size=\"24\" value=\"\" autocomplete=\"new-password\" spellcheck=\"false\" data-reveal=\"1\" data-pw=\"{}\" aria-describedby=\"pass-strength-result\" />\n\n\t\t\t\t\t<button type=\"button\" class=\"button button-secondary wp-hide-pw hide-if-no-js\" data-toggle=\"0\" aria-label=\"Hide password\">\n\t\t\t\t\t\t<span class=\"dashicons dashicons-hidden\" aria-hidden=\"true\"></span>\n\t\t\t\t\t</button>\n\t\t\t\t\t<div id=\"pass-strength-result\" class=\"hide-if-no-js\" aria-live=\"polite\">Strength indicator</div>\n\t\t\t\t</div>\n\t\t\t\t<div class=\"pw-weak\">\n\t\t\t\t\t<input type=\"checkbox\" name=\"pw_weak\" id=\"pw-weak\" class=\"pw-checkbox\" />\n\t\t\t\t\t<label for=\"pw-weak\">Confirm use of weak password</label>\n\t\t\t\t</div>\n\t\t\t</div>\n\n\t\t\t<p class=\"user-pass2-wrap\">\n\t\t\t\t<label for=\"pass2\">Confirm new password</label>\n\t\t\t\t<input type=\"password\" name=\"pass2\" id=\"pass2\" class=\"input\" size=\"20\" value=\"\" autocomplete=\"new-password\" spellcheck=\"false\" />\n\t\t\t</p>\n\n\t\t\t<p class=\"description indicator-hint\">Hint: The password should be at least twelve characters long. To make it stronger, use upper and lower case letters, numbers, and symbols like ! \" ? $ % ^ &amp; ).</p>\n\n\t\t\t\t\t\t<input type=\"hidden\" name=\"rp_key\" value=\"{}\" />\n\t\t\t<p class=\"submit reset-pass-submit\">\n\t\t\t\t<button type=\"button\" class=\"button wp-generate-pw hide-if-no-js skip-aria-expanded\">Generate Password</button>\n\t\t\t\t<input type=\"submit\" name=\"wp-submit\" id=\"wp-submit\" class=\"button button-primary button-large\" value=\"Save Password\" />\n\t\t\t</p>\n\t\t</form>\n{}",
        url(&site.login_url("?action=resetpass")),
        esc_attr(login),
        esc_attr(generate_password(16, true).unwrap_or_default().as_bytes()),
        esc_attr(key),
        nav(site, register_link(site))
    );
    let page = Frame {
        title: "Reset Password",
        action,
        interim: request.request("interim-login").is_some(),
        customize: false,
        loggedout: false,
        message: &info(
            " reset-pass",
            "Enter your new password below or generate one.",
        ),
        errors: &errors,
        head_scripts: &["utils", "user-profile"],
        scripts: &[],
        body: &body,
        input_id: "pass1",
    };
    Action::Page(page.render(site))
}

/// The page once `reset_password()` has reset the password.
pub fn password_reset(site: &dyn Site, request: &Request, action: &str) -> Vec<u8> {
    let text = format!(
        "Your password has been reset. <a href=\"{}\">Log in</a>",
        url(&site.login_url(""))
    );
    frame(
        site,
        request,
        ("Password Reset", action),
        &info(" reset-pass", &text),
        &[],
        "",
        "",
    )
}

/// The check your email screen.
fn check_email(site: &dyn Site, request: &Request) -> Action {
    let login = url(&site.login_url(""));
    let notices = match as_text(request.query.get_str("checkemail")) {
        Some(b"confirm") => vec![message(
            "confirm",
            format!(
                "Check your email for the confirmation link, then visit the <a href=\"{login}\">login page</a>."
            ),
        )],
        Some(b"registered") => vec![message(
            "registered",
            format!(
                "Registration complete. Please check your email, then visit the <a href=\"{login}\">login page</a>."
            ),
        )],
        _ => Vec::new(),
    };
    Action::Page(frame(
        site,
        request,
        ("Check your email", "checkemail"),
        "",
        &notices,
        "",
        "",
    ))
}

/// `wp_die()`'s page for a refused link, with a 500.
fn refused(site: &dyn Site, text: &str) -> Action {
    Action::Died(crate::die::page(
        site.public(),
        "HyScribe &rsaquo; Error",
        text,
    ))
}

/// A personal data request's confirmation link: refused unless the request
/// is pending or failed, and the key is the one mailed and under a day old
/// (`wp_validate_user_request_key()`); else the request, whose confirmation
/// writes.
fn confirm_action(site: &dyn Site, request: &Request, now: i64) -> Action {
    let (id, key) = match (
        request.query.get_str("request_id"),
        request.query.get_str("confirm_key"),
    ) {
        (None, _) => return refused(site, "Missing request ID."),
        (_, None) => return refused(site, "Missing confirm key."),
        (Some(id), Some(key)) => (id, key),
    };
    // `sanitize_text_field()` of an array is an empty text.
    let key = match key {
        Value::String(key) => sanitize_text_field(key),
        _ => Vec::new(),
    };
    let found = site.user_request(id.to_int().unsigned_abs());
    let Some(found) = found.filter(|r| !is_empty_string(&r.confirm_key) && r.modified != 0) else {
        return refused(site, "Invalid personal data request.");
    };
    if found.status != "request-pending" && found.status != "request-failed" {
        return refused(site, "This personal data request has expired.");
    }
    if is_empty_string(&key) {
        return refused(
            site,
            "The confirmation key is missing from this personal data request.",
        );
    }
    let duration = apply_filters("user_request_key_expiration", 86_400, &[]);
    if !wp_verify_fast_hash(&key, &found.confirm_key) {
        return refused(
            site,
            "The confirmation key is invalid for this personal data request.",
        );
    }
    if now > found.modified + duration {
        return refused(
            site,
            "The confirmation key has expired for this personal data request.",
        );
    }
    // The mails of any other action are not ported.
    match &found.action_name[..] {
        b"export_personal_data" | b"remove_personal_data" => {
            Action::ConfirmRequest(found, id.to_int())
        }
        _ => Action::NotPorted,
    }
}

/// The page of a confirmed request, as `_wp_privacy_account_request_confirmed_message()`
/// words it for the request's action.
pub fn request_confirmed(site: &dyn Site, request: &Request, action_name: &[u8]) -> Vec<u8> {
    let (thanks, then) = match action_name {
        b"export_personal_data" => (
            "Thanks for confirming your export request.",
            "The site administrator has been notified. You will receive a link to download your export via email when they fulfill your request.",
        ),
        b"remove_personal_data" => (
            "Thanks for confirming your erasure request.",
            "The site administrator has been notified. You will receive an email confirmation when they erase your data.",
        ),
        _ => (
            "Action has been confirmed.",
            "The site administrator has been notified and will fulfill your request as soon as possible.",
        ),
    };
    let message = format!("<p class=\"success\">{thanks}</p><p>{then}</p>");
    let message = apply_filters("user_request_action_confirmed_message", message, &[]);
    frame(
        site,
        request,
        ("User action confirmed.", "confirmaction"),
        &message,
        &[],
        "",
        "",
    )
}

/// `action` for a GET of `request_uri`, slashed, at `now`, or for a POST
/// of the reset password form.
pub fn action(
    site: &dyn Site,
    request: &Request,
    action: &str,
    (request_uri, now): (&[u8], i64),
) -> Action {
    match action {
        "lostpassword" | "retrievepassword" => {
            Action::Page(lost_password(site, request, action, None))
        }
        "register" => register(site, request),
        "resetpass" | "rp" => reset_password(site, request, (action, request_uri), now),
        "checkemail" => check_email(site, request),
        "confirmaction" => confirm_action(site, request, now),
        _ => Action::NotPorted,
    }
}

#[cfg(test)]
mod tests;

// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp-login.php`'s other actions as a GET shows them: the lost password and
//! registration forms, the reset password link, the check your email
//! screen and a personal data request's confirmation.

use hyscribe_kses::esc_attr;
use hyscribe_phpser::{Value, addslashes, is_empty_string, remove_query_arg};
use hyscribe_render::formatting::sanitize_user;

use super::{Frame, Notice, Request, Site, as_text, error, message, url};

/// What one of these actions answers.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    /// The screen.
    Page(Vec<u8>),
    /// `wp_die()`'s page, with a 500.
    Died(Vec<u8>),
    /// A 302 to this location, with the reset password cookie as given.
    Redirect(Vec<u8>, Reset),
    /// What this port does not answer: the action writes, or needs
    /// `wp_verify_fast_hash()`.
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
        scripts: &[],
        body,
        input_id,
    }
    .render(site)
}

/// The lost password form.
fn lost_password(site: &dyn Site, request: &Request, action: &str) -> Action {
    let errors: Vec<Notice> = match as_text(request.query.get_str("error")) {
        Some(b"invalidkey") => vec![error(
            "invalidkey",
            "<strong>Error:</strong> Your password reset link appears to be invalid. Please request a new link below.".into(),
        )],
        Some(b"expiredkey") => vec![error(
            "expiredkey",
            "<strong>Error:</strong> Your password reset link has expired. Please request a new link below.".into(),
        )],
        _ => Vec::new(),
    };
    let register = site.can_register().then(|| {
        format!(
            "<a class=\"wp-login-register\" href=\"{}\">Register</a>",
            url(&site.login_url("?action=register"))
        )
    });
    let body = format!(
        "\n\t\t<form name=\"lostpasswordform\" id=\"lostpasswordform\" action=\"{}\" method=\"post\">\n\t\t\t<p>\n\t\t\t\t<label for=\"user_login\">Username or Email Address</label>\n\t\t\t\t<input type=\"text\" name=\"user_login\" id=\"user_login\" class=\"input ltr\" value=\"\" size=\"20\" autocapitalize=\"off\" autocomplete=\"username\" required=\"required\" />\n\t\t\t</p>\n\t\t\t\t\t\t<input type=\"hidden\" name=\"redirect_to\" value=\"{}\" />\n\t\t\t<p class=\"submit\">\n\t\t\t\t<input type=\"submit\" name=\"wp-submit\" id=\"wp-submit\" class=\"button button-primary button-large\" value=\"Get New Password\" />\n\t\t\t</p>\n\t\t</form>\n{}",
        url(&site.login_url("?action=lostpassword")),
        esc_attr(&redirect_to(request)),
        nav(site, register)
    );
    let message = info(
        "",
        "Please enter your username or email address. You will receive an email message with instructions on how to reset your password.",
    );
    Action::Page(frame(
        site,
        request,
        ("Lost Password", action),
        &message,
        &errors,
        &body,
        "user_login",
    ))
}

/// The registration form, or the redirect of a site nobody may register on.
fn register(site: &dyn Site, request: &Request) -> Action {
    if !site.can_register() {
        let to = site.login_url("?registration=disabled");
        return Action::Redirect(to.into_bytes(), Reset::Keep);
    }
    let lost = format!(
        "<a class=\"wp-login-lost-password\" href=\"{}\">Lost your password?</a>",
        url(&site.login_url("?action=lostpassword"))
    );
    let body = format!(
        "\t\t<form name=\"registerform\" id=\"registerform\" action=\"{}\" method=\"post\" novalidate=\"novalidate\">\n\t\t\t<p>\n\t\t\t\t<label for=\"user_login\">Username</label>\n\t\t\t\t<input type=\"text\" name=\"user_login\" id=\"user_login\" class=\"input ltr\" value=\"\" size=\"20\" autocapitalize=\"off\" autocomplete=\"username\" required=\"required\" />\n\t\t\t</p>\n\t\t\t<p>\n\t\t\t\t<label for=\"user_email\">Email</label>\n\t\t\t\t<input type=\"email\" name=\"user_email\" id=\"user_email\" class=\"input\" value=\"\" size=\"25\" autocomplete=\"email\" required=\"required\" />\n\t\t\t</p>\n\t\t\t\t\t\t<p id=\"reg_passmail\">\n\t\t\t\tRegistration confirmation will be emailed to you.\t\t\t</p>\n\t\t\t<input type=\"hidden\" name=\"redirect_to\" value=\"{}\" />\n\t\t\t<p class=\"submit\">\n\t\t\t\t<input type=\"submit\" name=\"wp-submit\" id=\"wp-submit\" class=\"button button-primary button-large\" value=\"Register\" />\n\t\t\t</p>\n\t\t</form>\n{}",
        url(&site.login_url("?action=register")),
        esc_attr(&redirect_to(request)),
        nav(site, Some(lost))
    );
    Action::Page(frame(
        site,
        request,
        ("Registration Form", "register"),
        &info(" register", "Register For This Site"),
        &[],
        &body,
        "user_login",
    ))
}

/// The reset password link: its key and login go to the cookie, and a
/// cookie without a valid key back to the lost password form.
fn reset_password(site: &dyn Site, request: &Request, request_uri: &[u8]) -> Action {
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
    let split = |text: &[u8]| {
        let at = text.iter().position(|&b| b == b':')?;
        let (first, rest) = text.split_at(at);
        Some((
            first.to_vec(),
            rest.iter().skip(1).copied().collect::<Vec<u8>>(),
        ))
    };
    let pair = request
        .reset_cookie
        .and_then(split)
        .filter(|(login, _)| !login.is_empty());
    // `check_password_reset_key()` up to the hash it would verify.
    if let Some((login, key)) = pair
        && key.iter().any(u8::is_ascii_alphanumeric)
        && login != b"0"
        && let Some(user) = site.user_by_login(&sanitize_user(&login))
    {
        let stored = &user.activation_key;
        let hash = split(stored).map_or(stored.clone(), |(_, hash)| hash);
        if !is_empty_string(&hash) {
            return Action::NotPorted;
        }
    }
    let to = site.login_url("?action=lostpassword&error=invalidkey");
    Action::Redirect(to.into_bytes(), Reset::Expire)
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

/// A personal data request's confirmation: refused unless the request
/// exists, whose confirmation writes.
fn confirm_action(site: &dyn Site, request: &Request) -> Action {
    let text = match (
        request.query.get_str("request_id"),
        request.query.get_str("confirm_key"),
    ) {
        (None, _) => "Missing request ID.",
        (_, None) => "Missing confirm key.",
        (Some(id), _) if site.is_user_request(id.to_int().unsigned_abs()) => {
            return Action::NotPorted;
        }
        _ => "Invalid personal data request.",
    };
    let page = crate::die::page(site.public(), "WordPress &rsaquo; Error", text);
    Action::Died(page)
}

/// `action` for a GET of `request_uri`, slashed.
pub fn action(site: &dyn Site, request: &Request, action: &str, request_uri: &[u8]) -> Action {
    match action {
        "lostpassword" | "retrievepassword" => lost_password(site, request, action),
        "register" => register(site, request),
        "resetpass" | "rp" => reset_password(site, request, request_uri),
        "checkemail" => check_email(site, request),
        "confirmaction" => confirm_action(site, request),
        _ => Action::NotPorted,
    }
}

#[cfg(test)]
mod tests;

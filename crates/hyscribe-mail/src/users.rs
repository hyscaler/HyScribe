// SPDX-License-Identifier: AGPL-3.0-or-later

//! The account messages: password resets, new users, email and password
//! changes (`wp-includes/user.php`, `pluggable.php`, `functions.php` and
//! `wp-admin/includes/misc.php`).

use crate::{Mail, Site, fill, mail, subject, wp_specialchars_decode};
use hyscribe_kses::{esc_url, url_host};
use hyscribe_phpser::{addslashes, is_empty_string, rawurlencode};

/// The user a message is about, as stored.
#[derive(Debug, Clone, Copy)]
pub struct User<'a> {
    /// `user_login`.
    pub login: &'a [u8],
    /// `user_email`.
    pub email: &'a [u8],
}

/// The password reset link of `retrieve_password()` and
/// `wp_new_user_notification()`, before what follows the action.
fn reset_link(site: &Site<'_>, login: &[u8], key: &[u8]) -> Vec<u8> {
    let login = rawurlencode(login);
    [
        site.site,
        b"/wp-login.php?login=",
        &login,
        b"&key=",
        key,
        b"&action=rp",
    ]
    .concat()
}

/// `retrieve_password()`'s message to `user` with the reset `key`, in the
/// user's `locale`. `requester` is the visitor's address, `None` for a
/// logged in request.
pub fn retrieve_password(
    site: &Site<'_>,
    user: &User<'_>,
    key: &[u8],
    locale: &[u8],
    requester: Option<&[u8]>,
) -> Mail {
    let title = site.title();
    let mut message = [
        &b"Someone has requested a password reset for the following account:\r\n\r\nSite Name: "[..],
        &title,
        b"\r\n\r\nUsername: ",
        user.login,
        b"\r\n\r\nIf this was a mistake, ignore this email and nothing will happen.\r\n\r\n",
        b"To reset your password, visit the following address:\r\n\r\n",
        &reset_link(site, user.login, key),
        b"&wp_lang=",
        locale,
        b"\r\n\r\n",
    ]
    .concat();
    if let Some(address) = requester.filter(|address| !is_empty_string(address)) {
        message.extend_from_slice(
            &[
                &b"This password reset request originated from the IP address "[..],
                address,
                b".\r\n",
            ]
            .concat(),
        );
    }
    let subject = wp_specialchars_decode(&subject(&title, b"Password Reset"), false);
    mail(user.email, subject, message)
}

/// `wp_new_user_notification()`'s message to the site's admin.
pub fn new_user_to_admin(site: &Site<'_>, user: &User<'_>) -> Mail {
    let title = site.title();
    let message = [
        &b"New user registration on your site "[..],
        &title,
        b":\r\n\r\nUsername: ",
        user.login,
        b"\r\n\r\nEmail: ",
        user.email,
        b"\r\n",
    ]
    .concat();
    let subject = wp_specialchars_decode(&subject(&title, b"New User Registration"), false);
    mail(site.admin_email, subject, message)
}

/// `wp_new_user_notification()`'s message to the new user, with the
/// `key` that sets a password.
pub fn new_user_to_user(site: &Site<'_>, user: &User<'_>, key: &[u8]) -> Mail {
    let message = [
        &b"To set your password, visit the following address:\r\n\r\n"[..],
        &reset_link(site, user.login, key),
        b"\r\n",
    ]
    .concat();
    let subject = wp_specialchars_decode(&subject(&site.title(), b"Login Details"), false);
    mail(user.email, subject, message)
}

/// `send_confirmation_on_profile_email()`: the link with the `hash` that
/// confirms `new_email` for the current user `login`, sent to that address.
pub fn email_change_request(site: &Site<'_>, login: &[u8], new_email: &[u8], hash: &[u8]) -> Mail {
    let title = site.title();
    let link = esc_url(&[site.admin, b"profile.php?newuseremail=", hash].concat());
    let message = fill(
        b"Howdy ###USERNAME###,\n\n\
You recently requested to have the email address on your account changed.\n\n\
If this is correct, please click on the following link to change it:\n\
###ADMIN_URL###\n\n\
You can safely ignore and delete this email if you do not want to\n\
take this action.\n\n\
This email has been sent to ###EMAIL###\n\n\
Regards,\n\
All at ###SITENAME###\n\
###SITEURL###",
        &[
            (b"USERNAME", login),
            (b"ADMIN_URL", &link),
            (b"EMAIL", new_email),
            (b"SITENAME", &title),
            (b"SITEURL", site.home),
        ],
    );
    mail(new_email, subject(&title, b"Email Change Request"), message)
}

/// `update_option_new_admin_email()`: the link with the `hash` that
/// confirms `new_email` as the admin email, asked by the current user
/// `login`, sent to that address.
pub fn new_admin_email(site: &Site<'_>, login: &[u8], new_email: &[u8], hash: &[u8]) -> Mail {
    let title = site.title();
    let link = esc_url(&[site.admin, b"options.php?adminhash=", hash].concat());
    let message = fill(
        b"Howdy,\n\n\
A site administrator (###USERNAME###) recently requested to have the\n\
administration email address changed on this site:\n\
###SITEURL###\n\n\
To confirm this change, please click on the following link:\n\
###ADMIN_URL###\n\n\
You can safely ignore and delete this email if you do not want to\n\
take this action.\n\n\
This email has been sent to ###EMAIL###\n\n\
Regards,\n\
All at ###SITENAME###\n\
###SITEURL###",
        &[
            (b"USERNAME", login),
            (b"ADMIN_URL", &link),
            (b"EMAIL", new_email),
            (b"SITENAME", &title),
            (b"SITEURL", site.home),
        ],
    );
    let name = if site.name.is_empty() {
        url_host(site.home).unwrap_or_default()
    } else {
        title
    };
    mail(
        new_email,
        subject(&name, b"New Admin Email Address"),
        message,
    )
}

/// `wp_site_admin_email_change_notification()`: the notice to the old
/// admin email once `new_email` replaced it; none from an empty address or
/// the installer's `you@example.com`.
pub fn admin_email_changed(site: &Site<'_>, old_email: &[u8], new_email: &[u8]) -> Option<Mail> {
    if is_empty_string(old_email) || old_email == b"you@example.com" {
        return None;
    }
    let title = site.title();
    let message = fill(
        b"Hi,\n\n\
This notice confirms that the admin email address was changed on ###SITENAME###.\n\n\
The new admin email address is ###NEW_EMAIL###.\n\n\
This email has been sent to ###OLD_EMAIL###\n\n\
Regards,\n\
All at ###SITENAME###\n\
###SITEURL###",
        &[
            (b"OLD_EMAIL", old_email),
            (b"NEW_EMAIL", new_email),
            (b"SITENAME", &title),
            (b"SITEURL", site.home),
        ],
    );
    Some(mail(
        old_email,
        subject(&title, b"Admin Email Changed"),
        message,
    ))
}

/// `wp_update_user()`'s notice that `user`'s password changed. The user's
/// fields are `add_magic_quotes()`'d there, so a quote in them gains a
/// backslash.
pub fn password_changed(site: &Site<'_>, user: &User<'_>) -> Mail {
    let (login, email) = (addslashes(user.login), addslashes(user.email));
    let title = site.title();
    let message = fill(
        b"Hi ###USERNAME###,\n\n\
This notice confirms that your password was changed on ###SITENAME###.\n\n\
If you did not change your password, please contact the Site Administrator at\n\
###ADMIN_EMAIL###\n\n\
This email has been sent to ###EMAIL###\n\n\
Regards,\n\
All at ###SITENAME###\n\
###SITEURL###",
        &[
            (b"USERNAME", &login),
            (b"ADMIN_EMAIL", site.admin_email),
            (b"EMAIL", &email),
            (b"SITENAME", &title),
            (b"SITEURL", site.home),
        ],
    );
    mail(&email, subject(&title, b"Password Changed"), message)
}

/// `wp_update_user()`'s notice to `user`'s old address that it changed to
/// `new_email`, with the user's fields slashed as in [`password_changed`].
pub fn email_changed(site: &Site<'_>, user: &User<'_>, new_email: &[u8]) -> Mail {
    let (login, email) = (addslashes(user.login), addslashes(user.email));
    let title = site.title();
    let message = fill(
        b"Hi ###USERNAME###,\n\n\
This notice confirms that your email address on ###SITENAME### was changed to ###NEW_EMAIL###.\n\n\
If you did not change your email, please contact the Site Administrator at\n\
###ADMIN_EMAIL###\n\n\
This email has been sent to ###EMAIL###\n\n\
Regards,\n\
All at ###SITENAME###\n\
###SITEURL###",
        &[
            (b"USERNAME", &login),
            (b"ADMIN_EMAIL", site.admin_email),
            (b"NEW_EMAIL", new_email),
            (b"EMAIL", &email),
            (b"SITENAME", &title),
            (b"SITEURL", site.home),
        ],
    );
    mail(&email, subject(&title, b"Email Changed"), message)
}

/// `wp_password_change_notification()`: the admin's notice that `user`
/// reset their password; none when the user has the admin email.
pub fn password_reset_to_admin(site: &Site<'_>, user: &User<'_>) -> Option<Mail> {
    if user.email.eq_ignore_ascii_case(site.admin_email) {
        return None;
    }
    let message = [&b"Password changed for user: "[..], user.login, b"\r\n"].concat();
    let subject = wp_specialchars_decode(&subject(&site.title(), b"Password Changed"), false);
    Some(mail(site.admin_email, subject, message))
}

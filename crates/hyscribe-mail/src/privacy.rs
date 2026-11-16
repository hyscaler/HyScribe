// SPDX-License-Identifier: AGPL-3.0-or-later

//! The personal data requests' messages (`wp-includes/user.php` and
//! `wp-admin/includes/privacy-tools.php`).

use crate::{Mail, Site, fill, mail, subject};
use hyscribe_kses::esc_url_raw;
use hyscribe_phpser::add_query_arg;

/// A personal data request's `action_name`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// `export_personal_data`.
    Export,
    /// `remove_personal_data`.
    Erase,
}

impl Action {
    /// `wp_user_request_action_description()`.
    fn description(self) -> &'static [u8] {
        match self {
            Self::Export => b"Export Personal Data",
            Self::Erase => b"Erase Personal Data",
        }
    }
}

/// `wp_send_user_request()`: the link with the request's ID and
/// `confirm_key` that confirms it, sent to the request's `email`.
pub fn user_request(
    site: &Site<'_>,
    email: &[u8],
    action: Action,
    request_id: u64,
    confirm_key: &[u8],
) -> Mail {
    let title = site.title();
    let id = request_id.to_string();
    let args: [(&[u8], Option<&[u8]>); 3] = [
        (b"action", Some(b"confirmaction")),
        (b"request_id", Some(id.as_bytes())),
        (b"confirm_key", Some(confirm_key)),
    ];
    let link = add_query_arg(&args, &[site.site, b"/wp-login.php"].concat());
    let message = fill(
        b"Howdy,\n\n\
A request has been made to perform the following action on your account:\n\n     \
###DESCRIPTION###\n\n\
To confirm this, please click on the following link:\n\
###CONFIRM_URL###\n\n\
You can safely ignore and delete this email if you do not want to\n\
take this action.\n\n\
Regards,\n\
All at ###SITENAME###\n\
###SITEURL###",
        &[
            (b"DESCRIPTION", action.description()),
            (b"CONFIRM_URL", &esc_url_raw(&link)),
            (b"EMAIL", email),
            (b"SITENAME", &title),
            (b"SITEURL", &esc_url_raw(site.home)),
        ],
    );
    let what = [&b"Confirm Action: "[..], action.description()].concat();
    mail(email, subject(&title, &what), message)
}

/// `_wp_privacy_send_request_confirmation_notification()`: the admin's
/// notice that the request of `email` was confirmed.
pub fn request_confirmed(site: &Site<'_>, email: &[u8], action: Action) -> Mail {
    let title = site.title();
    let screen: &[u8] = match action {
        Action::Export => b"export-personal-data.php",
        Action::Erase => b"erase-personal-data.php",
    };
    let message = fill(
        b"Howdy,\n\n\
A user data privacy request has been confirmed on ###SITENAME###:\n\n\
User: ###USER_EMAIL###\n\
Request: ###DESCRIPTION###\n\n\
You can view and manage these data privacy requests here:\n\n\
###MANAGE_URL###\n\n\
Regards,\n\
All at ###SITENAME###\n\
###SITEURL###",
        &[
            (b"SITENAME", &title),
            (b"USER_EMAIL", email),
            (b"DESCRIPTION", action.description()),
            (b"MANAGE_URL", &esc_url_raw(&[site.admin, screen].concat())),
            (b"SITEURL", &esc_url_raw(site.home)),
        ],
    );
    let what = [&b"Action Confirmed: "[..], action.description()].concat();
    mail(site.admin_email, subject(&title, &what), message)
}

/// `_wp_privacy_send_erasure_fulfillment_notification()`: the notice to
/// `email` that its data was erased, naming the privacy policy page's URL
/// when there is one.
pub fn erasure_fulfilled(site: &Site<'_>, email: &[u8], privacy_policy_url: &[u8]) -> Mail {
    let title = site.title();
    let text: &[u8] = if privacy_policy_url.is_empty() {
        b"Howdy,\n\n\
Your request to erase your personal data on ###SITENAME### has been completed.\n\n\
If you have any follow-up questions or concerns, please contact the site administrator.\n\n\
Regards,\n\
All at ###SITENAME###\n\
###SITEURL###"
    } else {
        b"Howdy,\n\n\
Your request to erase your personal data on ###SITENAME### has been completed.\n\n\
If you have any follow-up questions or concerns, please contact the site administrator.\n\n\
For more information, you can also read our privacy policy: ###PRIVACY_POLICY_URL###\n\n\
Regards,\n\
All at ###SITENAME###\n\
###SITEURL###"
    };
    let message = fill(
        text,
        &[
            (b"SITENAME", &title),
            (b"PRIVACY_POLICY_URL", privacy_policy_url),
            (b"SITEURL", &esc_url_raw(site.home)),
        ],
    );
    mail(
        email,
        subject(&title, b"Erasure Request Fulfilled"),
        message,
    )
}

/// `wp_privacy_send_personal_data_export_email()`: the link to the export
/// file sent to `email`, deleted after `expiration_date` (the date format's
/// date three days on).
pub fn export_ready(
    site: &Site<'_>,
    email: &[u8],
    expiration_date: &[u8],
    export_file_url: &[u8],
) -> Mail {
    let title = site.title();
    let message = fill(
        b"Howdy,\n\n\
Your request for an export of personal data has been completed. You may\n\
download your personal data by clicking on the link below. For privacy\n\
and security, we will automatically delete the file on ###EXPIRATION###,\n\
so please download it before then.\n\n\
###LINK###\n\n\
Regards,\n\
All at ###SITENAME###\n\
###SITEURL###",
        &[
            (b"EXPIRATION", expiration_date),
            (b"LINK", &esc_url_raw(export_file_url)),
            (b"EMAIL", email),
            (b"SITENAME", &title),
            (b"SITEURL", &esc_url_raw(site.home)),
        ],
    );
    mail(email, subject(&title, b"Personal Data Export"), message)
}

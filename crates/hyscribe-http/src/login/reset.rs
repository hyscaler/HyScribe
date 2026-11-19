// SPDX-License-Identifier: AGPL-3.0-or-later

//! The lost and reset password forms' writes (MVP 8.3), each one writer
//! action, and their mails, sent once it commits (ADR 0016).

use axum::http::{StatusCode, header};
use hyscribe_admin::assets::Urls;
use hyscribe_admin::login::actions::{
    lost_password as lost_password_page, password_reset, registration, request_confirmed,
};
use hyscribe_admin::login::{READ_ONLY, Request, Site as _, UserRequest};
use hyscribe_mail::{Action as PrivacyAction, Mail, User};
use hyscribe_phpser::{Value, addslashes, is_empty_string};
use hyscribe_write::privacy::{admin_notified, user_request_action_confirmed};
use hyscribe_write::users::{
    ResetKey, register_new_user, registration_failed, reset_password, retrieve_password,
};

use super::{LoginReads, PATH, redirected};
use crate::mail::EMAIL_FAILURE;
use crate::redirect::wp_safe_redirect;

/// A form's status and page, and the mails it hands `wp_mail()`.
pub(super) type Written = ((StatusCode, Vec<u8>), Vec<Mail>);

/// Shadow mode's answer to a form that writes, as hs-admin's.
fn read_only(reads: &LoginReads) -> Written {
    let page =
        hyscribe_admin::die::page(reads.site.public(), "WordPress &rsaquo; Error", READ_ONLY);
    ((StatusCode::FORBIDDEN, page), Vec::new())
}

/// The database's error page, for a write that failed.
fn failed(reads: &LoginReads, err: &hyscribe_write::Error) -> Written {
    let page = crate::database_error(reads.site, &format!("{err:?}"));
    ((page.status, page.document.into_bytes()), Vec::new())
}

/// The site's texts the messages read: its name as stored, home, site and
/// admin URLs, and the admin's email.
fn mail_texts(reads: &LoginReads) -> [String; 5] {
    let site = reads.site;
    [
        site.option("blogname"),
        site.home.clone(),
        site.option("siteurl"),
        reads.admin_url(""),
        site.option("admin_email"),
    ]
}

fn mail_site(texts: &[String; 5]) -> hyscribe_mail::Site<'_> {
    let [name, home, site, admin, admin_email] = texts;
    hyscribe_mail::Site {
        name: name.as_bytes(),
        home: home.as_bytes(),
        site: site.as_bytes(),
        admin: admin.as_bytes(),
        admin_email: admin_email.as_bytes(),
    }
}

/// `retrieve_password()`'s message with the user's new key, in their
/// locale; a visitor's names the address they asked from.
fn reset_mail(reads: &LoginReads, key: &ResetKey) -> Mail {
    let texts = mail_texts(reads);
    let user = User {
        login: &key.login,
        email: &key.email,
    };
    let visitor = reads.current.0 == 0;
    let requester = reads.address().filter(|_| visitor).map(|ip| ip.to_string());
    hyscribe_mail::retrieve_password(
        &mail_site(&texts),
        &user,
        key.key.as_bytes(),
        &reads.locale(key.id),
        requester.as_deref().map(str::as_bytes),
    )
}

/// A posted lost password form: `retrieve_password()` of
/// `$_POST['user_login']`, then the check your email screen, or
/// `redirect_to` kept on the site; else the form with PHP's error.
pub(super) fn lost_password(
    reads: &LoginReads,
    request: &Request,
    action: &str,
    headers: &mut Vec<(header::HeaderName, String)>,
) -> Written {
    let Some(writer) = &reads.site.writer else {
        return read_only(reads);
    };
    // `$_POST['user_login']` when a string, slashed.
    let login = match request.posted("user_login") {
        Some(Value::String(login)) => addslashes(login),
        _ => Vec::new(),
    };
    let redirect_to = request.request("redirect_to");
    // `wp_safe_redirect()` of an array (never empty from a form) is PHP's fatal
    // error, after the write.
    let fatal = matches!(redirect_to, Some(Value::Array(_)));
    let retrieved = reads.reads.block_on(async {
        let mut write = writer.begin().await?;
        let retrieved = retrieve_password(&mut write, &login).await?;
        // A refusal wrote nothing, so committing it changes nothing.
        let committed = if fatal { Ok(()) } else { write.commit().await };
        committed.map(|()| retrieved).map_err(Into::into)
    });
    let key = match retrieved {
        Ok(Ok(_)) if fatal => return ((StatusCode::NOT_IMPLEMENTED, Vec::new()), Vec::new()),
        Ok(Ok(key)) => key,
        Ok(Err(refusal)) => {
            let refused = Some((refusal.code, refusal.message));
            let page = lost_password_page(reads, request, action, refused);
            return ((StatusCode::OK, page), Vec::new());
        }
        Err(err) => return failed(reads, &err),
    };
    let to = match redirect_to.and_then(Value::as_bytes) {
        Some(to) if !is_empty_string(to) => addslashes(to),
        // Relative, as PHP's `wp-login.php?checkemail=confirm`.
        _ => format!("{}?checkemail=confirm", PATH.trim_start_matches('/')).into_bytes(),
    };
    let mail = reset_mail(reads, &key);
    let mailer = &reads.site.mailer;
    let sent = reads.reads.block_on(mailer.retrieve_password(
        mail.clone(),
        key.id,
        reads.client.as_ref().ok().copied().flatten(),
    ));
    if !sent {
        // The key is kept, as PHP keeps it when its mail fails.
        let refused = Some(("retrieve_password_email_failure", EMAIL_FAILURE));
        let page = lost_password_page(reads, request, action, refused);
        return ((StatusCode::OK, page), vec![mail]);
    }
    let location = wp_safe_redirect(reads.site, &to, &reads.admin_url(""));
    (redirected(headers, location), vec![mail])
}

/// A posted registration form: `register_new_user()` of `$_POST`'s login
/// and address, then PHP's check your email redirect, or `redirect_to`
/// kept on the site; else the form with PHP's errors. The new user's
/// bcrypt hash makes this a password check for the login limits: it takes
/// its address's one turn and a place under the site wide cap, or is
/// refused as a failure to register, with nothing written.
pub(super) fn register(
    reads: &LoginReads,
    request: &Request,
    headers: &mut Vec<(header::HeaderName, String)>,
) -> Written {
    let Some(writer) = &reads.site.writer else {
        return read_only(reads);
    };
    let text = |key| match request.posted(key) {
        Some(Value::String(text)) => text.clone(),
        _ => Vec::new(),
    };
    let (login, email) = (text("user_login"), text("user_email"));
    let texts = mail_texts(reads);
    let limiter = &reads.site.limiter;
    let turn = reads
        .client
        .as_ref()
        .ok()
        .and_then(|client| limiter.turn(*client));
    let place = turn.as_ref().and_then(|_| limiter.checks.start());
    let client = reads.client.as_ref().ok().copied().flatten();
    // Past the cap, PHP's own registration error, with nothing written.
    if place.is_none() || limiter.registrations_full(client, reads.now) {
        let refused = [registration_failed(texts[4].as_bytes())];
        let refused = [(refused[0].code, refused[0].message.as_str())];
        return refusal(reads, request, &refused, (&login, &email));
    }
    let redirect_to = request.posted("redirect_to");
    // `wp_safe_redirect()` of an array (never empty from a form) is PHP's fatal
    // error, after the write.
    let fatal = matches!(redirect_to, Some(Value::Array(_)));
    let url = reads.login_url("");
    let registered = reads.reads.block_on(async {
        let mut write = writer.begin().await?;
        let registered = register_new_user(&mut write, (&login, &email), &url).await?;
        // A refusal wrote nothing, so committing it changes nothing.
        let committed = if fatal { Ok(()) } else { write.commit().await };
        committed.map(|()| registered).map_err(Into::into)
    });
    drop(place);
    if !fatal && matches!(registered, Ok(Ok(_))) {
        limiter.registered(client, reads.now);
    }
    let key = match registered {
        Ok(Ok(_)) if fatal => return ((StatusCode::NOT_IMPLEMENTED, Vec::new()), Vec::new()),
        Ok(Ok(key)) => key,
        Ok(Err(refused)) => {
            let refused: Vec<_> = refused
                .iter()
                .map(|e| (e.code, e.message.as_str()))
                .collect();
            return refusal(reads, request, &refused, (&login, &email));
        }
        Err(err) => return failed(reads, &err),
    };
    let to = match redirect_to.and_then(Value::as_bytes) {
        Some(to) if !is_empty_string(to) => addslashes(to),
        // Relative, as PHP's `wp-login.php?checkemail=registered`.
        _ => format!("{}?checkemail=registered", PATH.trim_start_matches('/')).into_bytes(),
    };
    let user = User {
        login: &key.login,
        email: &key.email,
    };
    let to_admin = hyscribe_mail::new_user_to_admin(&mail_site(&texts), &user);
    let mailer = &reads.site.mailer;
    let kind = "wp_new_user_notification";
    reads
        .reads
        .block_on(mailer.wp_mail(to_admin.clone(), kind, false));
    let to_user = hyscribe_mail::new_user_to_user(&mail_site(&texts), &user, key.key.as_bytes());
    // The key it carries is a password reset's: sent within their limits.
    let sent = mailer.retrieve_password(to_user.clone(), key.id, client);
    reads.reads.block_on(sent);
    let location = wp_safe_redirect(reads.site, &to, &reads.admin_url(""));
    (redirected(headers, location), vec![to_admin, to_user])
}

/// The registration form shown again with `refused`.
fn refusal(
    reads: &LoginReads,
    request: &Request,
    refused: &[(&'static str, &str)],
    values: (&[u8], &[u8]),
) -> Written {
    let page = registration(reads, request, refused, values);
    ((StatusCode::OK, page), Vec::new())
}

/// A good confirmation link: `user_request_action_confirmed` for the
/// request, then PHP's page; the admin is mailed when the request is
/// confirmed and they were not told, and told so once the mail is sent.
pub(super) fn confirm(
    reads: &LoginReads,
    request: &Request,
    (found, request_id): (&UserRequest, i64),
) -> Written {
    let Some(writer) = &reads.site.writer else {
        return read_only(reads);
    };
    let ctx = hyscribe_write::posts::Context {
        user: 0,
        can_publish: false,
        home: reads.site.home.as_bytes().to_vec(),
        upload_host: hyscribe_render::link::host(reads.site.raw_option("siteurl")),
        unfiltered_html: false,
        constants: reads.site.constants,
        now: reads.now,
    };
    let confirmed = reads.reads.block_on(async {
        let mut write = writer.begin().await?;
        let notice = user_request_action_confirmed(&mut write, &ctx, request_id).await?;
        write.commit().await.map(|()| notice).map_err(Into::into)
    });
    let notice = match confirmed {
        Ok(notice) => notice,
        Err(err) => return failed(reads, &err),
    };
    let mut mails = Vec::new();
    if let Some(confirmed) = notice {
        // Only requests of the two actions reach this, as `confirm_action()` sees.
        let action = match &confirmed.action_name[..] {
            b"export_personal_data" => PrivacyAction::Export,
            _ => PrivacyAction::Erase,
        };
        let texts = mail_texts(reads);
        let mail = hyscribe_mail::request_confirmed(&mail_site(&texts), &confirmed.email, action);
        let kind = "wp_privacy_request_confirmed";
        let sent = reads
            .reads
            .block_on(reads.site.mailer.wp_mail(mail.clone(), kind, false));
        mails.push(mail);
        if sent {
            let told = reads.reads.block_on(async {
                let mut write = writer.begin().await?;
                admin_notified(&mut write, found.id).await?;
                write.commit().await.map_err(hyscribe_write::Error::from)
            });
            if let Err(err) = told {
                return failed(reads, &err);
            }
        }
    }
    let page = request_confirmed(reads, request, &found.action_name);
    ((StatusCode::OK, page), mails)
}

/// A reset password form that passed: `reset_password()`, the user's
/// sessions ended, PHP's page, and the notice to the site's admin.
pub(super) fn reset(
    reads: &LoginReads,
    request: &Request,
    action: &str,
    (id, password): (u64, &[u8]),
) -> Written {
    let Some(writer) = &reads.site.writer else {
        return read_only(reads);
    };
    let reset = reads.reads.block_on(async {
        let mut write = writer.begin().await?;
        let user = reset_password(&mut write, id, password).await?;
        write.commit().await.map(|()| user).map_err(Into::into)
    });
    let (login, email) = match reset {
        Ok(user) => user,
        Err(err) => return failed(reads, &err),
    };
    // The new password's fragment ends every cookie, as in PHP; the
    // sessions they were for end with them, or fail all the same.
    let _ = reads.site.sessions.as_ref().map(|s| s.destroy_all(id));
    let texts = mail_texts(reads);
    let user = User {
        login: &login,
        email: &email,
    };
    let mails = hyscribe_mail::password_reset_to_admin(&mail_site(&texts), &user);
    if let Some(mail) = mails.clone() {
        let kind = "wp_password_change_notification";
        reads
            .reads
            .block_on(reads.site.mailer.wp_mail(mail, kind, false));
    }
    let page = password_reset(reads, request, action);
    ((StatusCode::OK, page), mails.into_iter().collect())
}

#[cfg(test)]
mod tests;

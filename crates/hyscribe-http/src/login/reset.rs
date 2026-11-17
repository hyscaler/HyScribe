// SPDX-License-Identifier: AGPL-3.0-or-later

//! The lost and reset password forms' writes (MVP 8.3), each one writer
//! action, with their mails taken as sent until the transport (ADR 0016).

use axum::http::{StatusCode, header};
use hyscribe_admin::assets::Urls;
use hyscribe_admin::login::actions::{lost_password as lost_password_page, password_reset};
use hyscribe_admin::login::{READ_ONLY, Request};
use hyscribe_mail::{Mail, User};
use hyscribe_phpser::{Value, addslashes, is_empty_string};
use hyscribe_write::users::{ResetKey, reset_password, retrieve_password};

use super::{LoginReads, PATH, redirected};
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
    let requester = reads.client.filter(|_| visitor).map(|ip| ip.to_string());
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
        if !fatal {
            write.commit().await?;
        }
        Ok::<_, hyscribe_write::Error>(retrieved)
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
    let location = wp_safe_redirect(reads.site, &to, &reads.admin_url(""));
    (redirected(headers, location), vec![reset_mail(reads, &key)])
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
        write.commit().await?;
        Ok::<_, hyscribe_write::Error>(user)
    });
    let (login, email) = match reset {
        Ok(user) => user,
        Err(err) => return failed(reads, &err),
    };
    // The new password's fragment ends every cookie, as in PHP; the
    // sessions they were for end with them.
    if let Some(sessions) = &reads.site.sessions {
        // A session that cannot be removed fails all the same.
        let _ = sessions.destroy_all(id);
    }
    let texts = mail_texts(reads);
    let user = User {
        login: &login,
        email: &email,
    };
    let mails = hyscribe_mail::password_reset_to_admin(&mail_site(&texts), &user);
    let page = password_reset(reads, request, action);
    ((StatusCode::OK, page), mails.into_iter().collect())
}

#[cfg(test)]
mod tests;

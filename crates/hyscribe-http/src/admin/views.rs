// SPDX-License-Identifier: AGPL-3.0-or-later

//! A page view's writes in primary mode: the user settings synced with
//! their cookie and a list's `?mode=`, the welcome panel's
//! `?welcome=`, the Quick Draft form's auto-draft, the Settings
//! saves' notices, deleted once a screen shows them, and the Permalinks
//! screen's rewrite rules. They are kept with the page, and
//! `?mode=`'s with the redirect past a list's last page.

use axum::http::StatusCode;
use axum::response::Response;
use hyscribe_admin::dashboard::DashboardSite;
use hyscribe_admin::menu::MenuUser;
use hyscribe_db::{DbError, Write, Writer};
use hyscribe_phpser::Value;
use hyscribe_write::Error;
use hyscribe_write::options::settings_errors_shown;
use hyscribe_write::posts::Context;
use tokio::runtime::Handle;

use super::saves::dashboard::{context, quick_press_draft, welcome};
use super::saves::options::flush_rewrite_rules;
use super::saves::screen::{Found, Synced, save_settings, sync};
use super::{Admin, failed, with_cookies};
use crate::login::{Names, cookie, is_ssl, php_cookie};

/// What a page view writes.
pub(super) struct View {
    synced: Synced,
    /// `?welcome=`'s state of the welcome panel.
    welcome: Option<bool>,
    /// Whether the Quick Draft form needs a new auto-draft.
    draft: bool,
    /// Whether a Settings screen shows what a save left (`settings-updated`).
    settings: bool,
    /// Whether the view is the Permalinks screen's, which refreshes the rules.
    rules: bool,
}

/// The Settings screens, whose `options-head.php` shows a save's notices.
const SETTINGS: [&str; 7] = [
    "/hs-admin/options-general",
    "/hs-admin/options-writing",
    "/hs-admin/options-reading",
    "/hs-admin/options-discussion",
    "/hs-admin/options-media",
    "/hs-admin/options-permalink",
    "/hs-admin/options-privacy",
];

/// What the view of `path` with the query `get` writes, and what the page
/// reads after: the settings, the welcome panel's state.
pub(super) fn plan(
    admin: &mut Admin,
    cookies: &[(Vec<u8>, Vec<u8>)],
    path: &str,
    get: &Value,
    now: i64,
) -> View {
    let id = admin.user.id;
    // Both are saved as text, which `get_user_option()` gives as it is.
    let option = |name| admin.reads.user_option(id, name).map(Value::String);
    let found = Found {
        cookie: cookie(cookies, &format!("wp-settings-{id}")),
        time: cookie(cookies, &format!("wp-settings-time-{id}")),
        stored: option("user-settings"),
        saved: option("user-settings-time"),
    };
    // The lists read the settings before the header syncs them.
    let list = matches!(path, "/hs-admin/edit" | "/hs-admin/edit-comments");
    let mode = get.get_str("mode").filter(|mode| list && !mode.is_empty());
    let synced = sync(&found, mode.map(Value::to_php_string).as_deref(), now);
    let read = synced
        .mode
        .as_ref()
        .or(synced.cookie.as_ref().filter(|_| !list));
    if let Some(read) = read {
        admin.settings = Some(String::from_utf8_lossy(read).into_owned());
    }
    let dashboard = matches!(path, "/hs-admin/" | "/hs-admin/index");
    let welcome = get
        .get_str("welcome")
        .filter(|_| dashboard && admin.can("edit_theme_options"));
    let welcome = welcome.map(|shown| !shown.is_empty());
    admin.welcome = welcome.map(|shown| i64::from(shown).to_string().into_bytes());
    let updated = get
        .get_str("settings-updated")
        .is_some_and(|updated| !updated.is_empty());
    View {
        synced,
        welcome,
        draft: dashboard && needs_draft(admin),
        settings: updated && SETTINGS.contains(&path),
        rules: path == "/hs-admin/options-permalink",
    }
}

/// Whether the Quick Draft form needs a new auto-draft: the user may
/// write posts and has none to reuse.
fn needs_draft(admin: &Admin) -> bool {
    admin.can("edit_posts") && admin.quick_draft().is_none()
}

/// The view of the Quick Draft form alone, as `post.php` prints it after a
/// save: no admin header, so no settings synced.
pub(super) fn quick_press(admin: &Admin) -> View {
    View {
        synced: Synced::default(),
        welcome: None,
        draft: needs_draft(admin),
        settings: false,
        rules: false,
    }
}

/// The view's writes in `write`, once begun, as PHP makes them: the
/// settings, and with the page the rewrite rules, the welcome panel's
/// state, the auto-draft and the notices a save left, once shown.
pub(super) async fn written<'w>(
    write: Result<Write<'w>, DbError>,
    ctx: &Context,
    view: &View,
    page: bool,
) -> Result<(Write<'w>, Option<u64>), Error> {
    let mut write = write?;
    // `options-permalink.php` flushes before its header syncs the settings.
    if view.rules && page {
        flush_rewrite_rules(&mut write).await?;
    }
    save_settings(&mut write, ctx.user, &view.synced, (ctx.now, page)).await?;
    if let Some(shown) = view.welcome.filter(|_| page) {
        welcome(&mut write, ctx.user, shown).await?;
    }
    let draft = if view.draft && page {
        Some(quick_press_draft(&mut write, ctx).await?)
    } else {
        None
    };
    if view.settings && page {
        settings_errors_shown(&mut write, ctx.now).await?;
    }
    Ok((write, draft))
}

/// The page `render` answers with the view's writes: the dashboard's
/// first, as the page shows its auto-draft, the others' after it.
pub(super) fn viewed(
    admin: &mut Admin,
    writer: &Writer,
    view: View,
    render: impl FnOnce(&Admin) -> Response,
) -> Response {
    let handle = Handle::current();
    let ctx = Context {
        unfiltered_html: admin.can("unfiltered_html"),
        ..context(
            admin.user.id,
            &admin.site.home,
            &admin.site.option("siteurl"),
            admin.site.constants,
        )
    };
    let first = view
        .draft
        .then(|| handle.block_on(async { written(writer.begin().await, &ctx, &view, true).await }));
    let drafted = match first.transpose() {
        Ok(drafted) => drafted,
        Err(err) => return failed(admin, err),
    };
    admin.draft = drafted.as_ref().and_then(|(_, id)| *id);
    let response = render(admin);
    let page = response.status() == StatusCode::OK;
    let kept = page || response.status() == StatusCode::FOUND && view.synced.mode.is_some();
    // Only a page, or `?mode=`'s redirect, is kept.
    let after = view.synced.mode.is_some()
        || view.synced.newer.is_some()
        || view.welcome.is_some()
        || view.settings
        || view.rules;
    let saved = match drafted {
        // The Quick Draft form is shown to all who may write posts.
        Some((write, _)) => handle.block_on(write.commit()).map_err(Error::from),
        None if kept && after => handle.block_on(async {
            let (write, _) = written(writer.begin().await, &ctx, &view, page).await?;
            write.commit().await.map_err(Error::from)
        }),
        _ => Ok(()),
    };
    if let Err(err) = saved {
        return failed(admin, err);
    }
    if let Some(settings) = view.synced.cookie.as_ref().filter(|_| page) {
        let names = Names::new(admin.site);
        let year = (ctx.now + 31_536_000, ctx.now);
        // PHP sends an empty value as a deletion.
        let (value, expires): (&[u8], _) = match settings.is_empty() {
            true => (b"deleted", (1, ctx.now)),
            false => (settings, year),
        };
        let id = admin.user.id;
        let secure = is_ssl(admin.site);
        let path = names.site_path();
        let time = ctx.now.to_string();
        let cookies = vec![
            php_cookie(&format!("wp-settings-{id}"), value, expires, path, secure),
            php_cookie(
                &format!("wp-settings-time-{id}"),
                time.as_bytes(),
                year,
                path,
                secure,
            ),
        ];
        return with_cookies(response, cookies);
    }
    response
}

#[cfg(test)]
mod tests;

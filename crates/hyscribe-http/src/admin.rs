// SPDX-License-Identifier: AGPL-3.0-or-later

//! hs-admin's screens at `/hs-admin/` (ADR 0008): `auth_redirect()` before
//! any of them; screens not ported yet answer `501`.

use std::sync::Arc;

use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::response::Response;
use hyscribe_admin::admin_bar::{BarSite, Browser};
use hyscribe_admin::assets::Urls;
use hyscribe_admin::login::Site as LoginSite;
use hyscribe_admin::menu::MenuUser;
use hyscribe_admin::page::PageSite;
use hyscribe_auth::caps::{CapabilityData, User as Capabilities};
use hyscribe_phpser::{Value, remove_query_arg, urlencode};
use hyscribe_render::site::Site as RenderSite;
use tokio::runtime::Handle;

use crate::login::{LoginReads, Names, PATH, cookie, cookies, is_ssl, joined, nocache, response};
use crate::redirect::wp_sanitize_redirect;
use crate::{Site, now};

/// Whether `path` is an hs-admin screen: its files have an extension and
/// are served, or missing, as Apache serves `wp-admin`'s.
pub(crate) fn is_admin(path: &str) -> bool {
    let screen = path
        .rsplit('/')
        .next()
        .is_none_or(|name| !name.contains('.'));
    (path == "/hs-admin" || path.starts_with("/hs-admin/")) && screen
}

/// The answer to a request for an admin screen.
pub(crate) async fn respond(
    site: &Arc<Site>,
    uri: Uri,
    headers: HeaderMap,
    post: bool,
) -> Response {
    let site = Arc::clone(site);
    let (now, handle) = (now(), Handle::current());
    joined(
        tokio::task::spawn_blocking(move || answer(&site, &uri, &headers, post, now, handle)).await,
    )
}

/// A `302` to `location`, as `wp_redirect()` sends it.
fn redirect(mut headers: Vec<(header::HeaderName, String)>, location: &[u8]) -> Response {
    let location = String::from_utf8_lossy(&wp_sanitize_redirect(location)).into_owned();
    headers.push((
        header::HeaderName::from_static("x-redirect-by"),
        "WordPress".into(),
    ));
    headers.push((header::LOCATION, location));
    response(StatusCode::FOUND, headers, Vec::new())
}

/// A logged in user's answer: over HTTPS if their `use_ssl` asks for it.
fn admitted(https: bool, use_ssl: bool, here: &[u8], serve: impl FnOnce() -> Response) -> Response {
    if !https && use_ssl {
        redirect(nocache(), &[&b"https://"[..], here].concat())
    } else {
        serve()
    }
}

/// `wp_removable_query_args()`: what the canonical admin URL leaves out.
const REMOVABLE: [&[u8]; 28] = [
    b"activate",
    b"activated",
    b"admin_email_remind_later",
    b"approved",
    b"core-major-auto-updates-saved",
    b"deactivate",
    b"delete_count",
    b"deleted",
    b"disabled",
    b"doing_wp_cron",
    b"enabled",
    b"error",
    b"hotkeys_highlight_first",
    b"hotkeys_highlight_last",
    b"ids",
    b"locked",
    b"message",
    b"same",
    b"saved",
    b"settings-updated",
    b"skipped",
    b"spammed",
    b"trashed",
    b"unspammed",
    b"untrashed",
    b"update",
    b"updated",
    b"wp-post-new-reload",
];

/// A logged in user in the admin, on a blocking thread.
struct Admin<'a> {
    reads: &'a LoginReads<'a>,
    site: &'a Site,
    caps: Capabilities,
    user: hyscribe_model::User,
}

impl MenuUser for Admin<'_> {
    fn can(&self, cap: &str) -> bool {
        self.caps.has_cap(self.reads, cap, &[])
    }
    fn moderated_comments(&self) -> u64 {
        self.reads.reads.wait(self.site.db.moderated_comments())
    }
    /// `get_transient()` without its timeout: the transients read here
    /// never expire.
    fn transient(&self, name: &str) -> Option<Vec<u8>> {
        self.reads.reads.option(&format!("_transient_{name}"))
    }
}

impl BarSite for Admin<'_> {
    fn home_url(&self) -> String {
        LoginSite::home_url(self.reads)
    }
    fn admin_url(&self, path: &str) -> String {
        Urls::admin_url(self.reads, path)
    }
    fn blogname(&self) -> String {
        self.reads.name()
    }
    fn logout_url(&self) -> String {
        let nonce = LoginSite::nonce(self.reads, "log-out");
        self.reads
            .login_url(&format!("?action=logout&amp;_wpnonce={nonce}"))
    }
    fn names(&self) -> (String, String) {
        let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
        (text(&self.user.display_name), text(&self.user.login))
    }
    fn avatar(&self, size: i64) -> String {
        let avatar =
            hyscribe_render::user_avatar(&|name| self.reads.reads.option(name), &self.user, size);
        String::from_utf8_lossy(&avatar).into_owned()
    }
}

impl Urls for Admin<'_> {
    fn site_url(&self) -> String {
        self.reads.site_url()
    }
    fn admin_url(&self, path: &str) -> String {
        Urls::admin_url(self.reads, path)
    }
    fn fill(&self, text: &str) -> String {
        self.reads.fill(text)
    }
}

impl PageSite for Admin<'_> {
    fn user_id(&self) -> u64 {
        self.user.id
    }
    fn nonce(&self, action: &str) -> String {
        LoginSite::nonce(self.reads, action)
    }
    fn admin_color(&self) -> Option<Vec<u8>> {
        self.reads.user_option(self.user.id, "admin_color")
    }
    fn preferences(&self) -> Option<Vec<u8>> {
        let key = format!("{}persisted_preferences", self.site.db.prefix());
        self.reads.user_meta(self.user.id, &key)
    }
    fn compression_tested(&self) -> bool {
        self.reads.reads.option("can_compress_scripts").is_some()
    }
    fn login_url(&self) -> String {
        LoginSite::login_url(self.reads, "")
    }
    fn template(&self) -> String {
        self.site.option("template")
    }
    fn font_faces(&self) -> Vec<u8> {
        self.site.theme.admin_font_faces(&self.reads.site_url())
    }
}

/// The headers of an admin screen: `nocache_headers()` and
/// `send_frame_options_header()`; every response has `wp_admin_headers()`'s.
fn screen_headers() -> Vec<(header::HeaderName, String)> {
    let mut headers = nocache();
    headers.push((header::CONTENT_TYPE, "text/html; charset=UTF-8".to_string()));
    headers.push((header::X_FRAME_OPTIONS, "SAMEORIGIN".to_string()));
    headers
}

/// The screen at `path` for `admin`, `None` for one not ported yet.
fn screen(
    admin: &Admin,
    path: &str,
    url: &str,
    origin: &str,
    browser: &Browser,
) -> Option<Response> {
    let canonical =
        String::from_utf8_lossy(&remove_query_arg(&REMOVABLE, url.as_bytes())).into_owned();
    let page = match path {
        "/hs-admin/tools" => hyscribe_admin::tools::tools(admin, &canonical, origin, browser),
        _ => return None,
    };
    Some(match page {
        Some(html) => response(StatusCode::OK, screen_headers(), html.into_bytes()),
        // `wp_die()` from `wp-admin/includes/menu.php`.
        None => {
            let body = hyscribe_admin::die::page(
                admin.reads.public(),
                "WordPress &rsaquo; Error",
                "Sorry, you are not allowed to access this page.",
            );
            response(StatusCode::FORBIDDEN, screen_headers(), body)
        }
    })
}

fn answer(
    site: &Site,
    uri: &Uri,
    headers: &HeaderMap,
    post: bool,
    now: i64,
    handle: Handle,
) -> Response {
    let names = Names::new(site);
    let https = is_ssl(site);
    let cookies = cookies(
        headers
            .get_all(header::COOKIE)
            .iter()
            .map(HeaderValue::as_bytes),
    );
    let mut reads = LoginReads::new(site, handle, now);
    let user = reads.auth_user(cookie(&cookies, &names.auth(https)), post, https);
    // `$_SERVER['HTTP_HOST'] . $_SERVER['REQUEST_URI']`.
    let host = headers
        .get(header::HOST)
        .map_or(&b""[..], HeaderValue::as_bytes);
    let requested = uri.path_and_query().map_or("", |p| p.as_str());
    let here = [host, requested.as_bytes()].concat();
    let scheme: &[u8] = if https { b"https://" } else { b"http://" };
    let answer = match user {
        Some((id, token)) => {
            let use_ssl = reads
                .user_option(id, "use_ssl")
                .is_some_and(|v| !Value::String(v).is_empty());
            admitted(https, use_ssl, &here, || {
                // The auth cookie's token, which `/hs-login` gives the logged
                // in cookie too: `wp_get_session_token()`'s.
                reads.act_as(id, token);
                let admin = Admin {
                    reads: &reads,
                    site,
                    caps: Capabilities::load(&reads, site.db.prefix(), id),
                    user: reads.reads.wait(site.db.user(id)).unwrap_or_default(),
                };
                let header = |name: &str| headers.get(name).map(HeaderValue::as_bytes);
                let browser = Browser {
                    user_agent: header("user-agent").unwrap_or_default(),
                    mobile_hint: header("sec-ch-ua-mobile"),
                };
                let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
                let url = text(&[scheme, &here].concat());
                let origin = text(&[scheme, host].concat());
                screen(&admin, uri.path(), &url, &origin, &browser)
                    .unwrap_or_else(|| response(StatusCode::NOT_IMPLEMENTED, nocache(), Vec::new()))
            })
        }
        None => {
            // `wp_login_url( $redirect, true )`.
            let back = urlencode(&[scheme, &here].concat());
            let login = format!(
                "{}{PATH}?redirect_to={}&reauth=1",
                reads.site_url(),
                String::from_utf8_lossy(&back)
            );
            redirect(nocache(), login.as_bytes())
        }
    };
    match reads.reads.finish() {
        Ok(()) => answer,
        Err(err) => crate::html(crate::database_error(site, &err)),
    }
}

#[cfg(test)]
mod tests;

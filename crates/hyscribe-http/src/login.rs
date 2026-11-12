// SPDX-License-Identifier: AGPL-3.0-or-later

//! `/hs-login`: `wp-login.php`'s login action (hyscribe-admin) over the
//! database, with HyScribe's own cookies and sessions (an open question, ADR 0011).

use std::cell::RefCell;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use hyscribe_admin::assets::{Urls, admin_links, cookie_path};
use hyscribe_admin::login::{self, Login, Request, Session, Site as LoginSite, User};
use hyscribe_auth::caps::{CapPost, CapabilityData, User as Capabilities};
use hyscribe_auth::cookie::{generate_auth_cookie, parse_auth_cookie};
use hyscribe_auth::sessions::Origin;
use hyscribe_auth::{create_nonce, nonce_tick};
use hyscribe_model::{Credentials, UserField};
use hyscribe_phpser::{Value, md5, parse_str, rawurlencode, urldecode, urlencode};
use hyscribe_render::site::Site as RenderSite;
use tokio::runtime::Handle;

use crate::reads::Reads;
use crate::redirect::{wp_sanitize_redirect, wp_validate_redirect};
use crate::{Site, now};

/// The login screen's path.
pub(crate) const PATH: &str = "/hs-login";

const DAY: i64 = 86_400;

/// HyScribe's cookie names, which never stand for a PHP WordPress login,
/// and their paths.
pub(crate) struct Names {
    hash: String,
    /// `COOKIEPATH`, from the home URL.
    path: String,
    /// `SITECOOKIEPATH`, from the site URL.
    site_path: String,
}

impl Names {
    pub(crate) fn new(site: &Site) -> Names {
        let siteurl = site.option("siteurl");
        Names {
            hash: md5(siteurl.as_bytes()),
            path: cookie_path(&site.home),
            site_path: cookie_path(&siteurl),
        }
    }

    fn test(&self) -> String {
        "hyscribe_test_cookie".into()
    }

    /// `AUTH_COOKIE`, or `SECURE_AUTH_COOKIE` when `secure`.
    pub(crate) fn auth(&self, secure: bool) -> String {
        format!("hyscribe_{}{}", if secure { "sec_" } else { "" }, self.hash)
    }

    fn logged_in(&self) -> String {
        format!("hyscribe_logged_in_{}", self.hash)
    }

    fn reset(&self) -> String {
        format!("hyscribe-resetpass-{}", self.hash)
    }

    /// The paths a cookie for the whole site goes to: `COOKIEPATH`, and
    /// `SITECOOKIEPATH` when it differs.
    fn site_paths(&self) -> Vec<&str> {
        let mut paths = vec![self.path.as_str()];
        if self.site_path != self.path {
            paths.push(&self.site_path);
        }
        paths
    }
}

/// `setcookie()`, with `SameSite=Lax` added (docs/SECURITY_MODEL.md):
/// `expires` 0 for a session cookie.
fn set_cookie(
    name: &str,
    value: &[u8],
    (expires, now): (i64, i64),
    path: &str,
    secure: bool,
) -> String {
    let mut cookie = format!("{name}={}", String::from_utf8_lossy(&rawurlencode(value)));
    if expires > 0 {
        let at = UNIX_EPOCH + Duration::from_secs(expires.unsigned_abs());
        let max_age = (expires - now).max(0);
        cookie.push_str(&format!(
            "; expires={}; Max-Age={max_age}",
            httpdate::fmt_http_date(at)
        ));
    }
    cookie.push_str(&format!("; path={path}"));
    if secure {
        cookie.push_str("; secure");
    }
    cookie.push_str("; HttpOnly; SameSite=Lax");
    cookie
}

/// `$_COOKIE` from the Cookie headers' values: each name's first value,
/// both decoded. PHP also turns dots
/// and spaces in names into underscores, which HyScribe's names have none of.
pub fn cookies<'a>(values: impl IntoIterator<Item = &'a [u8]>) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut cookies: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    let pairs = values
        .into_iter()
        .flat_map(|value| value.split(|&b| b == b';'));
    for pair in pairs {
        let pair = hyscribe_phpser::php_trim(pair);
        let mut halves = pair.splitn(2, |&b| b == b'=');
        let name = urldecode(halves.next().unwrap_or_default());
        let value = urldecode(halves.next().unwrap_or_default());
        if !name.is_empty() && !cookies.iter().any(|(known, _)| *known == name) {
            cookies.push((name, value));
        }
    }
    cookies
}

pub(crate) fn cookie<'a>(cookies: &'a [(Vec<u8>, Vec<u8>)], name: &str) -> Option<&'a [u8]> {
    cookies
        .iter()
        .find(|(known, _)| known == name.as_bytes())
        .map(|(_, value)| value.as_slice())
}

/// What the login screen reads, on a blocking thread.
pub(crate) struct LoginReads<'a> {
    pub(crate) reads: Reads<'a>,
    site: &'a Site,
    now: i64,
    /// The users found, for the cookies of the one who logs in and for the
    /// current user.
    found: RefCell<Vec<Credentials>>,
    /// `wp_get_current_user()`'s ID and session token, from the logged in
    /// cookie: 0 and none for a visitor.
    current: (u64, Vec<u8>),
    /// `user-profile`'s user: the request's `user_id`, cast to an integer.
    profile_user: i64,
}

impl<'a> LoginReads<'a> {
    pub(crate) fn new(site: &'a Site, handle: Handle, now: i64) -> LoginReads<'a> {
        LoginReads {
            reads: Reads::new(site, handle),
            site,
            now,
            found: RefCell::default(),
            current: (0, Vec::new()),
            profile_user: 0,
        }
    }

    /// `get_user_locale( $id )`: the user's own, else the site's.
    fn locale(&self, id: u64) -> Vec<u8> {
        self.user_meta(id, "locale")
            .filter(|locale| !locale.is_empty())
            .or_else(|| self.reads.option("WPLANG").filter(|lang| !lang.is_empty()))
            .unwrap_or_else(|| b"en_US".to_vec())
    }

    fn credentials(&self, field: UserField, value: &[u8]) -> Option<Credentials> {
        let found = self.reads.wait(self.site.db.credentials(field, value))?;
        self.found.borrow_mut().push(found.clone());
        Some(found)
    }

    /// `wp_salt( $scheme )`.
    fn salt(&self, scheme: &str) -> Option<Vec<u8>> {
        self.site.salt(scheme, |name| self.reads.option(name))
    }

    /// `get_user_option( $option )`: the site's own meta first.
    pub(crate) fn user_option(&self, id: u64, option: &str) -> Option<Vec<u8>> {
        let prefixed = format!("{}{option}", self.site.db.prefix());
        self.reads
            .wait(self.site.db.user_meta(id, &prefixed))
            .or_else(|| self.reads.wait(self.site.db.user_meta(id, option)))
    }

    fn user(&self, found: Option<Credentials>) -> Option<User> {
        found.map(|user| User {
            use_ssl: self
                .user_option(user.id, "use_ssl")
                .is_some_and(|v| !Value::String(v).is_empty()),
            id: user.id,
            pass: user.pass,
        })
    }

    /// `wp_validate_auth_cookie()` of the auth cookie, with the session:
    /// the secure one's salt over HTTPS.
    pub(crate) fn session(&self, cookie: Option<&[u8]>, post: bool, https: bool) -> Session {
        if cookie.is_none_or(<[u8]>::is_empty) {
            return Session::None;
        }
        self.auth_user(cookie, post, https)
            .map_or(Session::Expired, |(id, _)| Session::User(id))
    }

    /// The user of a valid auth cookie, the secure one over HTTPS, and its
    /// session token.
    pub(crate) fn auth_user(
        &self,
        cookie: Option<&[u8]>,
        post: bool,
        https: bool,
    ) -> Option<(u64, Vec<u8>)> {
        self.validate(cookie?, post, if https { "secure_auth" } else { "auth" })
    }

    /// The user of a valid cookie of `scheme` and its session token.
    fn validate(&self, cookie: &[u8], post: bool, scheme: &str) -> Option<(u64, Vec<u8>)> {
        let parsed = parse_auth_cookie(cookie)?;
        let sessions = self.site.sessions.as_ref()?;
        let user = self.credentials(UserField::Login, parsed.username)?;
        let salt = self.salt(scheme)?;
        let good = parsed.is_valid(&user.pass, &salt, self.now, post)
            && sessions.verify(user.id, parsed.token, self.now);
        good.then(|| (user.id, parsed.token.to_vec()))
    }

    /// Acts for a logged in user: their nonces.
    pub(crate) fn act_as(&mut self, user: u64, token: Vec<u8>) {
        self.current = (user, token);
    }

    /// `wp_verify_nonce( $nonce, $action )` for the current user.
    fn verify_nonce(&self, nonce: &[u8], action: &str) -> bool {
        let (user, token) = &self.current;
        self.salt("nonce").is_some_and(|salt| {
            hyscribe_auth::verify_nonce(nonce, &salt, nonce_tick(self.now), action, *user, token)
                > 0
        })
    }
}

impl Urls for LoginReads<'_> {
    fn site_url(&self) -> String {
        self.site.option("siteurl")
    }

    /// `/wp-admin/<screen>.php` is `/hs-admin/<screen>` (ADR 0008).
    fn admin_url(&self, path: &str) -> String {
        format!(
            "{}/hs-admin/{}",
            self.site_url(),
            hyscribe_admin::menu::link(path)
        )
    }

    fn fill(&self, text: &str) -> String {
        let rest_url = if self.site.permalinks().pretty() {
            format!("{}/wp-json/", self.site.home)
        } else {
            format!("{}/index.php?rest_route=/", self.site.home)
        };
        let admin_path = format!("{}hs-admin/", cookie_path(&self.site_url()));
        let text = admin_links(text, "%admin_url%", &self.admin_url(""));
        admin_links(&text, "\"%admin_path%", &format!("\"{admin_path}"))
            .replace("%rest_url%", &rest_url)
            .replace("%site_url%", &self.site_url())
            .replace("%nonce:wp_rest%", &self.nonce("wp_rest"))
            .replace(
                "%nonce:reset-password-for-user%",
                &self.nonce("reset-password-for-0"),
            )
            .replace("%user_id%", &self.profile_user.to_string())
            .replace("%time%", &self.now.to_string())
    }
}

impl LoginSite for LoginReads<'_> {
    fn blogname(&self) -> String {
        self.site.name()
    }

    fn name(&self) -> String {
        self.site.option("blogname")
    }

    fn public(&self) -> bool {
        !Value::String(self.site.raw_option("blog_public").to_vec()).is_empty()
    }

    /// `wp_create_nonce( $action )` for the current user.
    fn nonce(&self, action: &str) -> String {
        let (user, token) = &self.current;
        self.salt("nonce").map_or(String::new(), |salt| {
            create_nonce(&salt, nonce_tick(self.now), action, *user, token)
        })
    }

    fn home_url(&self) -> String {
        format!("{}/", self.site.home)
    }

    fn login_url(&self, query: &str) -> String {
        format!("{}{PATH}{query}", self.site_url())
    }

    fn user_by_login(&self, login: &[u8]) -> Option<User> {
        self.user(self.credentials(UserField::Login, login))
    }

    fn user_by_email(&self, email: &[u8]) -> Option<User> {
        self.user(self.credentials(UserField::Email, email))
    }
}

/// What the login's redirect asks of a user's capabilities. Nothing else is
/// read, and an unknown constant is taken at its safer value.
impl CapabilityData for LoginReads<'_> {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        self.reads.option(name)
    }

    fn user_meta(&self, user_id: u64, key: &str) -> Option<Vec<u8>> {
        self.reads.wait(self.site.db.user_meta(user_id, key))
    }

    fn user_exists(&self, user_id: u64) -> bool {
        self.found.borrow().iter().any(|user| user.id == user_id)
    }

    fn post(&self, _: u64) -> Option<CapPost> {
        None
    }

    fn post_meta(&self, _: u64, _: &str) -> Option<Vec<u8>> {
        None
    }

    fn comment_post(&self, _: u64) -> Option<u64> {
        None
    }

    fn term_taxonomy(&self, _: u64) -> Option<String> {
        None
    }

    fn constant(&self, name: &str) -> bool {
        name == "DISALLOW_UNFILTERED_HTML"
    }
}

/// `is_ssl()`: HyScribe serves HTTP, so a site whose URL is https is reached
/// over a TLS proxy and its cookies stay on HTTPS (ADMIN.md).
pub(crate) fn is_ssl(site: &Site) -> bool {
    site.option("siteurl")
        .to_ascii_lowercase()
        .starts_with("https:")
}

/// The request as the login screen sees it.
pub(crate) struct Asked {
    pub(crate) post: bool,
    pub(crate) query: Option<String>,
    pub(crate) headers: HeaderMap,
    pub(crate) form: Value,
}

/// The login screen's response.
pub(crate) async fn respond(site: &Arc<Site>, asked: Asked) -> Response {
    let site = Arc::clone(site);
    let now = now();
    let handle = Handle::current();
    joined(tokio::task::spawn_blocking(move || answer(&site, &asked, now, handle)).await)
}

/// The answer, or a server error if working it out panicked.
pub(crate) fn joined(answer: Result<Response, tokio::task::JoinError>) -> Response {
    match answer {
        Ok(response) => response,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `nocache_headers()`.
pub(crate) fn nocache() -> Vec<(header::HeaderName, String)> {
    vec![
        (header::EXPIRES, "Wed, 11 Jan 1984 05:00:00 GMT".to_string()),
        (
            header::CACHE_CONTROL,
            "no-cache, must-revalidate, max-age=0, no-store, private".to_string(),
        ),
    ]
}

/// The headers every login screen response carries: `nocache_headers()`,
/// `send_frame_options_header()` and the test cookie.
fn headers(names: &Names, https: bool) -> Vec<(header::HeaderName, String)> {
    let mut headers = nocache();
    headers.push((header::X_FRAME_OPTIONS, "SAMEORIGIN".to_string()));
    headers.push((header::CONTENT_TYPE, "text/html; charset=UTF-8".to_string()));
    for path in names.site_paths() {
        let cookie = set_cookie(&names.test(), b"WP Cookie check", (0, 0), path, https);
        headers.push((header::SET_COOKIE, cookie));
    }
    headers
}

pub(crate) fn response(
    status: StatusCode,
    headers: Vec<(header::HeaderName, String)>,
    body: Vec<u8>,
) -> Response {
    let mut response = (status, body).into_response();
    for (name, value) in headers {
        // Encoded cookies and sanitized URLs; only a control character in the
        // configured site URLs could leave one empty.
        let value = HeaderValue::try_from(value).unwrap_or(HeaderValue::from_static(""));
        if name == header::SET_COOKIE {
            response.headers_mut().append(name, value);
        } else {
            response.headers_mut().insert(name, value);
        }
    }
    response
}

fn answer(site: &Site, asked: &Asked, now: i64, handle: Handle) -> Response {
    let names = Names::new(site);
    let https = is_ssl(site);
    let query = parse_str(asked.query.as_deref().unwrap_or_default().as_bytes());
    let cookies = cookies(
        asked
            .headers
            .get_all(header::COOKIE)
            .iter()
            .map(HeaderValue::as_bytes),
    );
    let mut reads = LoginReads::new(site, handle, now);
    reads.profile_user = query.get_str("user_id").map_or(0, Value::to_int);
    if let Some(current) = cookie(&cookies, &names.logged_in())
        .and_then(|cookie| reads.validate(cookie, asked.post, "logged_in"))
    {
        reads.current = current;
    }
    let request = Request {
        query: &query,
        form: asked.post.then_some(&asked.form),
        test_cookie: cookie(&cookies, &names.test()).is_some_and(|v| !v.is_empty()),
        logged_in_cookie: cookie(&cookies, &names.logged_in()).is_some_and(|v| !v.is_empty()),
        session: reads.session(cookie(&cookies, &names.auth(https)), asked.post, https),
        reset_cookie: cookie(&cookies, &names.reset()),
        read_only: site.sessions.is_none(),
    };
    let mut headers = headers(&names, https);
    let interim =
        query.get_str("interim-login").is_some() || asked.form.get_str("interim-login").is_some();
    let action = request.action();
    let outcome = match action.as_str() {
        "logout" => {
            let (status, body) = logout(&reads, &names, &request, &mut headers);
            return response(status, headers, body);
        }
        "login" | "entered_recovery_mode" => login::login(&reads, &request),
        // The other actions are not ported yet.
        _ => return response(StatusCode::NOT_IMPLEMENTED, headers, Vec::new()),
    };
    let answer = match outcome {
        Login::Page(page) => Some((StatusCode::OK, page)),
        // The admin's session expiry dialog is not ported yet.
        Login::LoggedIn { .. } if interim => Some((StatusCode::NOT_IMPLEMENTED, Vec::new())),
        Login::LoggedIn {
            user,
            remember,
            secure,
            redirect_to,
        } => logged_in(
            &reads,
            &names,
            &asked.headers,
            (user, remember, secure || https),
            &redirect_to,
        )
        .map(|(cookies, location)| {
            headers.extend(cookies.into_iter().map(|c| (header::SET_COOKIE, c)));
            // `wp_redirect()` sends nothing for an empty location, or "0".
            if hyscribe_phpser::is_empty_string(location.as_bytes()) {
                return (StatusCode::OK, Vec::new());
            }
            headers.push((header::LOCATION, location));
            headers.push((
                header::HeaderName::from_static("x-redirect-by"),
                "WordPress".into(),
            ));
            (StatusCode::FOUND, Vec::new())
        }),
    };
    match (reads.reads.finish(), answer) {
        (Err(err), _) => crate::html(crate::database_error(site, &err)),
        (Ok(()), Some((status, body))) => response(status, headers, body),
        (Ok(()), None) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// The logout action: with a good nonce, `wp_logout()` and the redirect;
/// without, the page asking whether to log out.
fn logout(
    reads: &LoginReads,
    names: &Names,
    request: &Request,
    headers: &mut Vec<(header::HeaderName, String)>,
) -> (StatusCode, Vec<u8>) {
    let nonce = request.request("_wpnonce").map(Value::to_php_string);
    if !nonce.is_some_and(|nonce| reads.verify_nonce(&nonce, "log-out")) {
        return (
            StatusCode::FORBIDDEN,
            login::logout_confirmation(reads, request),
        );
    }
    // `wp_destroy_current_session()`, then `wp_clear_auth_cookie()`.
    let (user, token) = &reads.current;
    // A visitor has no session token, so no session to end.
    if let Some(sessions) = reads.site.sessions.as_ref().filter(|_| !token.is_empty()) {
        // A session that cannot be removed expires all the same.
        let _ = sessions.destroy(*user, token, reads.now);
    }
    let expired = (reads.now - 365 * DAY, reads.now);
    let admin_path = format!("{}hs-admin", names.site_path);
    for name in [names.auth(false), names.auth(true)] {
        headers.push((
            header::SET_COOKIE,
            set_cookie(&name, b" ", expired, &admin_path, false),
        ));
    }
    for path in names.site_paths() {
        headers.push((
            header::SET_COOKIE,
            set_cookie(&names.logged_in(), b" ", expired, path, false),
        ));
    }
    let redirect_to = match request
        .request("redirect_to")
        .and_then(Value::as_bytes)
        .filter(|to| !hyscribe_phpser::is_empty_string(to))
    {
        Some(to) => login::slash(to),
        None => {
            let locale = String::from_utf8_lossy(&reads.locale(*user)).into_owned();
            reads
                .login_url(&format!("?loggedout=true&wp_lang={locale}"))
                .into_bytes()
        }
    };
    // `wp_safe_redirect()`.
    let host = hyscribe_kses::url_host(reads.site.home.as_bytes()).unwrap_or_default();
    let location = wp_validate_redirect(&redirect_to, &host, reads.admin_url("").as_bytes());
    let location = String::from_utf8_lossy(&wp_sanitize_redirect(&location)).into_owned();
    headers.push((header::LOCATION, location));
    headers.push((
        header::HeaderName::from_static("x-redirect-by"),
        "WordPress".into(),
    ));
    (StatusCode::FOUND, Vec::new())
}

/// `wp_set_auth_cookie()` and the redirect after a login: the cookies and
/// where to go, or `None` when the session cannot be kept.
fn logged_in(
    reads: &LoginReads,
    names: &Names,
    headers: &HeaderMap,
    (id, remember, secure): (u64, bool, bool),
    redirect_to: &[u8],
) -> Option<(Vec<String>, String)> {
    let site = reads.site;
    let scheme = if secure { "secure_auth" } else { "auth" };
    let found = reads
        .found
        .borrow()
        .iter()
        .find(|user| user.id == id)
        .cloned();
    let (Some(user), Some(sessions), Some(auth_salt), Some(logged_in_salt)) = (
        found,
        site.sessions.as_ref(),
        reads.salt(scheme),
        reads.salt("logged_in"),
    ) else {
        return None;
    };
    let expiration = reads.now + if remember { 14 * DAY } else { 2 * DAY };
    let expires = if remember { expiration + DAY / 2 } else { 0 };
    let agent = headers
        .get(header::USER_AGENT)
        .map_or(&b""[..], HeaderValue::as_bytes);
    let origin = Origin {
        ip: b"",
        ua: agent,
        login: reads.now,
    };
    let token = sessions.create(id, expiration, &origin).ok()?;
    let cookie = |salt: &[u8]| {
        generate_auth_cookie(&user.login, &user.pass, expiration, token.as_bytes(), salt)
    };
    let (auth, logged_in) = (cookie(&auth_salt), cookie(&logged_in_salt));
    let secure_logged_in = secure && site.home.to_ascii_lowercase().starts_with("https:");
    let admin_path = format!("{}hs-admin", names.site_path);
    let mut cookies = vec![set_cookie(
        &names.auth(secure),
        &auth,
        (expires, reads.now),
        &admin_path,
        secure,
    )];
    for path in names.site_paths() {
        cookies.push(set_cookie(
            &names.logged_in(),
            &logged_in,
            (expires, reads.now),
            path,
            secure_logged_in,
        ));
    }
    let caps = Capabilities::load(reads, site.db.prefix(), id);
    let can = |cap: &str| caps.has_cap(reads, cap, &[]);
    Some((cookies, location(reads, &can, (id, secure), redirect_to)))
}

/// Where the login screen sends a user who logged in, who `can` what; with
/// `force_ssl_admin()` when `secure`.
fn location(
    reads: &LoginReads,
    can: &dyn Fn(&str) -> bool,
    (id, secure): (u64, bool),
    redirect_to: &[u8],
) -> String {
    let scheme = |url: String| match url.strip_prefix("http://") {
        Some(rest) if secure => format!("https://{rest}"),
        _ => url,
    };
    let mut redirect_to = redirect_to.to_vec();
    // An administrator whose email has not been confirmed for six months
    // confirms it first.
    let lifespan = Value::String(
        reads
            .reads
            .option("admin_email_lifespan")
            .unwrap_or_default(),
    )
    .to_int();
    if can("manage_options") && reads.now > lifespan {
        let locale = reads.locale(id);
        let back = if hyscribe_phpser::is_empty_string(&redirect_to) {
            String::new()
        } else {
            format!(
                "redirect_to={}&",
                String::from_utf8_lossy(&urlencode(&redirect_to))
            )
        };
        // `add_query_arg()` leaves the locale as it is.
        redirect_to = scheme(reads.login_url(&format!(
            "?{back}action=confirm_admin_email&wp_lang={}",
            String::from_utf8_lossy(&locale)
        )))
        .into_bytes();
    }
    let admin = scheme(reads.admin_url(""));
    let to_admin = hyscribe_phpser::is_empty_string(&redirect_to)
        || redirect_to == b"wp-admin/"
        || redirect_to == admin.as_bytes();
    let location = if !to_admin {
        let host = hyscribe_kses::url_host(reads.site.home.as_bytes()).unwrap_or_default();
        wp_validate_redirect(&redirect_to, &host, admin.as_bytes())
    } else if can("edit_posts") {
        redirect_to
    } else if can("read") {
        scheme(reads.admin_url("profile.php")).into_bytes()
    } else {
        reads.home_url().into_bytes()
    };
    String::from_utf8_lossy(&wp_sanitize_redirect(&location)).into_owned()
}

#[cfg(test)]
mod tests;

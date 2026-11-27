// SPDX-License-Identifier: AGPL-3.0-or-later

//! Setup mode, the browser wizard (docs/SETUP_WIZARD.md): with no `.env`,
//! `start` serves only these pages until `.env` is written. A stranger who
//! reaches a fresh install cannot claim it: the pages need the one-time
//! secret given at start and then the session cookie it gives, every form
//! carries a token of that session, and setup mode stops after
//! [`SETUP_TIMEOUT`]. Answers stay on the server; no password is sent back.
//! The secret goes to the terminal, or to its own file under `logs/` that
//! setup mode deletes when it ends, never to the log.

use std::collections::HashMap;
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use hyscribe_config::{Server, wpconfig};
use hyscribe_phpser::{hash_equals, urldecode};
use tokio::sync::{Mutex, Notify, oneshot};

use super::{
    Choices, Content, Found, default_home, from_file, from_text, manual, questions, settle,
};
use crate::failed;
use hyscribe_write::install::{Answers, Plan};

/// How long setup mode waits for `.env` before it stops.
pub(crate) const SETUP_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// The secret's length: 32 letters and digits, about 190 bits.
const SECRET_LENGTH: usize = 32;
/// The session cookie's and form tokens' length.
const TOKEN_LENGTH: usize = 43;
/// The largest form: a pasted `wp-config.php`, percent encoded, and the
/// other fields.
const MAX_BODY: usize = 3 * wpconfig::MAX_SIZE + 4096;
/// The most fields a form has.
const MAX_FIELDS: usize = 32;
const SESSION_COOKIE: &str = "hyscribe_setup";
/// Before the session, the secret's form is checked against this cookie.
const FORM_COOKIE: &str = "hyscribe_setup_form";
const STEP_1: &str = "/installation/step-1";
const STEP_2: &str = "/installation/step-2";
const STEP_3: &str = "/installation/step-3";

/// Headers on every page of setup mode: never cached, framed, sniffed or
/// sent as a referrer, and no script or outside resource at all.
const HEADERS: [(&str, &str); 4] = [
    ("cache-control", "no-store"),
    ("x-content-type-options", "nosniff"),
    ("referrer-policy", "no-referrer"),
    (
        "content-security-policy",
        "default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; \
         frame-ancestors 'none'; base-uri 'none'",
    ),
];

/// How setup mode ended.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Ended {
    /// `.env` was written: the site can start.
    Written,
    /// Stopped by this signal.
    Stopped(i32),
}

/// Where setup mode gives the secret, and what it calls once it listens.
pub(crate) struct Start<'a> {
    /// The file the secret goes to, `None` for `out`, a terminal.
    pub secret_file: Option<&'a Path>,
    /// Called once the wizard listens.
    pub listening: &'a dyn Fn(),
}

/// Serves the wizard for the installation in `home` on the address `env`
/// sets, until `.env` is written, `stopped` gives a signal or `timeout`
/// passes. The address goes to `out`, and the secret as `start` says.
pub(crate) async fn setup_mode(
    home: &Path,
    env: &HashMap<String, String>,
    out: &mut dyn Write,
    stopped: &mut oneshot::Receiver<i32>,
    timeout: Duration,
    start: &Start<'_>,
) -> Result<Ended, String> {
    // One draw from the kernel, cut into the secret and the tokens.
    let (server, drawn) = hyscribe_config::server(env)
        .map_err(|err| err.to_string())
        .and_then(|server| {
            hyscribe_auth::generate_password(4 * TOKEN_LENGTH, false)
                .map(|drawn| (server, drawn))
                .map_err(failed("cannot generate the setup secret".into()))
        })?;
    let (secret, drawn) = drawn.split_at(TOKEN_LENGTH);
    let (secret, _) = secret.split_at(SECRET_LENGTH);
    let (form, drawn) = drawn.split_at(TOKEN_LENGTH);
    let (cookie, csrf) = drawn.split_at(TOKEN_LENGTH);
    let tokens = Tokens {
        form: form.into(),
        cookie: cookie.into(),
        csrf: csrf.into(),
    };
    let address = SocketAddr::new(server.bind_host, server.port);
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(failed(format!("cannot listen on {address}")))?;
    let minutes = timeout.as_secs() / 60;
    let open = format!("Open http://{address}{STEP_1} and enter the one-time setup secret");
    // The file is removed when setup mode ends, however it ends.
    let (_kept, given) = match start.secret_file {
        Some(path) => (
            Some(SecretFile::write(path, &format!("{open} {secret}\n"))?),
            format!("{open} in {}", path.display()),
        ),
        None => (None, format!("{open} {secret}")),
    };
    // Log lines are best effort, as the site's are.
    let _ = writeln!(
        out,
        "No .env: setup mode. {given}\nSetup mode stops in {minutes} minutes unless setup finishes."
    );
    let _ = out.flush();
    (start.listening)();
    let shared = Arc::new(Shared {
        wizard: Mutex::new(Wizard {
            home: home.to_path_buf(),
            listen: server,
            secret: Some(secret.into()),
            tokens,
            signed_in: false,
            found: None,
            new_site: false,
        }),
        done: Notify::new(),
    });
    let (stop, stopping) = oneshot::channel::<()>();
    let serving = tokio::spawn(hyscribe_http::serve_app(
        listener,
        router(shared.clone()),
        async move {
            let _ = stopping.await;
        },
    ));
    let ended = tokio::select! {
        () = shared.done.notified() => Ok(Ended::Written),
        signal = &mut *stopped => Ok(Ended::Stopped(signal.unwrap_or(0))),
        () = tokio::time::sleep(timeout) => Err(format!(
            "setup mode stopped after {minutes} minutes without .env; start again for a new secret"
        )),
    };
    let _ = stop.send(());
    let _ = serving.await;
    ended
}

/// The secret's own file, readable by its owner only, deleted on drop.
struct SecretFile<'a>(&'a Path);

impl<'a> SecretFile<'a> {
    fn write(path: &'a Path, text: &str) -> Result<SecretFile<'a>, String> {
        let parent = path.parent().unwrap_or(Path::new("."));
        // Mode 0700 and 0600; on Windows the folder's permissions.
        let mut folder = std::fs::DirBuilder::new();
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
            folder.mode(0o700);
            options.mode(0o600);
        }
        folder
            .recursive(true)
            .create(parent)
            .and_then(|()| options.open(path))
            .and_then(|mut file| {
                let kept = SecretFile(path);
                file.write_all(text.as_bytes()).map(|()| kept)
            })
            .map_err(failed(format!("cannot write {}", path.display())))
    }
}

impl Drop for SecretFile<'_> {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.0);
    }
}

struct Shared {
    wizard: Mutex<Wizard>,
    /// Notified once `.env` is written.
    done: Notify,
}

/// The wizard's answers so far, held on the server.
struct Wizard {
    home: PathBuf,
    /// Where setup mode listens, the site's address by default.
    listen: Server,
    /// The one-time secret, until it is used.
    secret: Option<String>,
    tokens: Tokens,
    /// Whether the secret was given: from then on the session cookie
    /// stands for it.
    signed_in: bool,
    /// The site read at step 2.
    found: Option<Found>,
    /// Whether step 2 chose a new site, to install in an empty database.
    new_site: bool,
}

/// Random tokens drawn at start, as the secret is.
struct Tokens {
    /// Step 1's form token, also in its cookie.
    form: String,
    /// The session cookie the secret gives.
    cookie: String,
    /// The token every form of the session carries.
    csrf: String,
}

fn router(shared: Arc<Shared>) -> Router {
    Router::new()
        .fallback(answer)
        .with_state(shared)
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .layer(axum::middleware::map_response(guarded))
}

async fn guarded(mut response: Response) -> Response {
    for (name, value) in HEADERS {
        response.headers_mut().insert(
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        );
    }
    response
}

async fn answer(
    State(shared): State<Arc<Shared>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let mut wizard = shared.wizard.lock().await;
    let post = method == Method::POST;
    let signed_in = wizard.signed_in
        && cookie(&headers, SESSION_COOKIE)
            .is_some_and(|given| hash_equals(wizard.tokens.cookie.as_bytes(), given));
    let step = match (uri.path(), signed_in) {
        (STEP_1, false) => STEP_1,
        (STEP_2, true) => STEP_2,
        (STEP_3, true) => STEP_3,
        (_, signed_in) => return redirect(if signed_in { STEP_2 } else { STEP_1 }),
    };
    let form = if post {
        let Some(form) = form_of(&headers, &body) else {
            return bad_form();
        };
        // Step 1's token must also be in its cookie, as there is no session
        // yet; the others' is the session's.
        let tokens = &wizard.tokens;
        let (token, fresh) = match step {
            STEP_1 => {
                let given = cookie(&headers, FORM_COOKIE);
                let fresh = given.is_some_and(|value| hash_equals(tokens.form.as_bytes(), value));
                (&tokens.form, fresh)
            }
            _ => (&tokens.csrf, true),
        };
        let sent = field(&form, "_csrf").as_bytes();
        if !fresh || !hash_equals(token.as_bytes(), sent) {
            return expired(step);
        }
        Some(form)
    } else {
        None
    };
    let found = wizard.found.clone();
    match (step, form, found) {
        (STEP_1, None, _) => secret_page(&wizard, StatusCode::OK, ""),
        (STEP_1, Some(form), _) => wizard.sign_in(&form),
        (STEP_2, None, _) => locate_page(&wizard, StatusCode::OK, ""),
        (STEP_2, Some(form), _) => wizard.locate(&form),
        (_, _, None) => redirect(STEP_2),
        (_, None, Some(found)) => details_page(&wizard, &found, StatusCode::OK, "", &|_| None),
        (_, Some(form), Some(found)) => wizard.details(&found, &form, &shared.done).await,
    }
}

impl Wizard {
    /// Step 1's form: the secret, given once, starts the session.
    fn sign_in(&mut self, form: &Form) -> Response {
        let given = field(form, "secret").as_bytes();
        if !self
            .secret
            .as_ref()
            .is_some_and(|secret| hash_equals(secret.as_bytes(), given))
        {
            return secret_page(
                self,
                StatusCode::FORBIDDEN,
                "That is not the setup secret, or it was used already. A new one is printed \
                 each time HyScribe starts in setup mode.",
            );
        }
        self.secret = None;
        self.signed_in = true;
        let mut response = redirect(STEP_2);
        set_cookie(&mut response, SESSION_COOKIE, &self.tokens.cookie);
        set_cookie(&mut response, FORM_COOKIE, "");
        response
    }

    /// Step 2's form: the site's `wp-config.php` by its path or its
    /// contents, or neither for manual entry.
    fn locate(&mut self, form: &Form) -> Response {
        let (path, contents) = (field(form, "path"), field(form, "contents"));
        self.new_site = field(form, "new") == "1";
        let found = if self.new_site {
            Ok(manual())
        } else if !contents.is_empty() {
            from_text(contents.as_bytes())
        } else if path.is_empty() || path == "none" {
            Ok(manual())
        } else {
            from_file(path)
        };
        match found {
            Ok(found) => {
                self.found = Some(found);
                redirect(STEP_3)
            }
            Err(err) => locate_page(self, StatusCode::UNPROCESSABLE_ENTITY, &err),
        }
    }

    /// Step 3's form: checks and writes `.env`, then notifies `done`, or
    /// shows why not.
    async fn details(&mut self, found: &Found, form: &Form, done: &Notify) -> Response {
        let value = |name: &str| field(form, name).to_string();
        let content = match &found.content {
            Content::Unknown => None,
            Content::Known(dir) => Some(dir.clone()),
            // An empty answer takes the default, as in the terminal.
            Content::Asked(default) => Some(match value("WP_CONTENT_DIR") {
                dir if dir.is_empty() => default.clone(),
                dir => PathBuf::from(dir),
            }),
        };
        let mut values = found.values.clone();
        for key in &found.unread {
            for question in questions(key) {
                let answer = value(question.key);
                if !answer.is_empty() {
                    values.push((question.key, answer));
                }
            }
        }
        let read_user = value("DB_READ_USER");
        let choices = Choices {
            // A new site is written to, so it is never in shadow mode.
            shadow: !self.new_site && value("MODE") != "primary",
            read_user: (!read_user.is_empty()).then(|| (read_user, value("DB_READ_PASSWORD"))),
            behind_proxy: value("BEHIND_PROXY") == "true",
            bind_host: value("BIND_HOST"),
            port: value("PORT"),
            uploads: value("UPLOADS_DIR"),
        };
        choices.push(&mut values);
        let answered = |name: &str| Some(field(form, name).to_string());
        let refused = StatusCode::UNPROCESSABLE_ENTITY;
        let plan = if self.new_site {
            let answers = Answers {
                title: &value("weblog_title"),
                login: &value("user_name"),
                password: &value("admin_password"),
                repeat: &value("admin_password2"),
                email: &value("admin_email"),
                public: value("blog_public") != "0",
                home: &value("site_address"),
            };
            match Plan::new(&answers, super::now()) {
                Ok(plan) => Some(plan),
                Err(refusal) => return details_page(self, found, refused, &refusal, &answered),
            }
        } else {
            None
        };
        // The page tells the report once `.env` is written.
        let mut quiet = |_: &str| Ok(());
        let content = content.as_deref();
        let written = settle(
            &self.home,
            values,
            content,
            plan.as_ref(),
            false,
            &mut quiet,
        )
        .await;
        match written {
            Ok((report, path)) => {
                self.signed_in = false;
                let body = format!(
                    "<pre>{}</pre>\n<p>Wrote {} (mode 600). HyScribe now serves the site at the \
                     address it sets; this setup page will not answer again.</p>\n",
                    escape(&report),
                    escape(&path.display().to_string())
                );
                done.notify_one();
                page(StatusCode::OK, "HyScribe is set up", &body)
            }
            Err(err) => details_page(self, found, refused, &err, &answered),
        }
    }
}

/// A form's fields, each name once.
type Form = Vec<(String, String)>;

/// The fields of a form `body` sent as `content_type`: `None` unless it is
/// URL encoded, at most [`MAX_BODY`] bytes, and at most [`MAX_FIELDS`]
/// fields of UTF-8 text, each name once.
pub fn read_form(content_type: &[u8], body: &[u8]) -> Option<Vec<(String, String)>> {
    let kind = content_type
        .split(|&b| b == b';')
        .next()
        .unwrap_or_default();
    if !kind
        .trim_ascii()
        .eq_ignore_ascii_case(b"application/x-www-form-urlencoded")
        || body.len() > MAX_BODY
    {
        return None;
    }
    let mut form = Form::new();
    for pair in body.split(|&b| b == b'&').filter(|pair| !pair.is_empty()) {
        let mut halves = pair.splitn(2, |&b| b == b'=');
        let name = String::from_utf8(urldecode(halves.next().unwrap_or_default())).ok()?;
        let value = String::from_utf8(urldecode(halves.next().unwrap_or_default())).ok()?;
        if form.len() == MAX_FIELDS || form.iter().any(|(known, _)| *known == name) {
            return None;
        }
        form.push((name, value));
    }
    Some(form)
}

fn form_of(headers: &HeaderMap, body: &[u8]) -> Option<Form> {
    let kind = headers.get(header::CONTENT_TYPE);
    read_form(kind.map_or(&b""[..], HeaderValue::as_bytes), body)
}

/// The field `name`, empty when missing.
fn field<'a>(form: &'a Form, name: &str) -> &'a str {
    form.iter()
        .find(|(known, _)| known == name)
        .map_or("", |(_, value)| value)
}

/// The first cookie `name`'s value.
fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a [u8]> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .flat_map(|value| value.as_bytes().split(|&b| b == b';'))
        .find_map(|pair| {
            let mut halves = pair.trim_ascii().splitn(2, |&b| b == b'=');
            (halves.next() == Some(name.as_bytes())).then(|| halves.next().unwrap_or_default())
        })
}

/// Sets the cookie `name`, or clears it when `value` is empty. It goes back
/// to setup mode's pages only, never to another site's.
fn set_cookie(response: &mut Response, name: &str, value: &str) {
    let age = if value.is_empty() { "; Max-Age=0" } else { "" };
    let cookie = format!("{name}={value}; Path=/installation; HttpOnly; SameSite=Strict{age}");
    // Names and tokens are letters, digits and underscores, always valid.
    let cookie = HeaderValue::from_str(&cookie).ok();
    let cookie = cookie.map(|cookie| (header::SET_COOKIE, cookie));
    response.headers_mut().extend(cookie);
}

fn redirect(to: &'static str) -> Response {
    (StatusCode::SEE_OTHER, [(header::LOCATION, to)]).into_response()
}

fn bad_form() -> Response {
    let body = "<p>The form could not be read. Go back and send it again.</p>\n";
    page(StatusCode::BAD_REQUEST, "Unreadable form", body)
}

fn expired(back: &str) -> Response {
    let body = format!(
        "<p>This form has expired. <a href=\"{back}\">Open it again</a> and send it from there.</p>\n"
    );
    page(StatusCode::FORBIDDEN, "Form expired", &body)
}

/// Step 1, with its form token in a cookie too.
fn secret_page(wizard: &Wizard, status: StatusCode, error: &str) -> Response {
    let token = &wizard.tokens.form;
    let body = format!(
        "{}<p>Enter the one-time setup secret HyScribe gave when it started, in the terminal \
         or in <code>logs/setup-secret</code>.</p>\n\
         <form method=\"post\" action=\"{STEP_1}\">\n{}{}<p><button>Continue</button></p>\n</form>\n",
        notice(error),
        hidden(token),
        input("secret", "Setup secret", "", true),
    );
    let mut response = page(status, "Step 1 of 3: setup secret", &body);
    set_cookie(&mut response, FORM_COOKIE, token);
    response
}

/// Step 2: where the site's `wp-config.php` is.
fn locate_page(wizard: &Wizard, status: StatusCode, error: &str) -> Response {
    let found = super::find_wordpress(&wizard.home);
    let body = format!(
        "{}<form method=\"post\" action=\"{STEP_2}\">\n{}{}\
         <p><label for=\"contents\">Or paste the contents of wp-config.php</label>\n\
         <textarea id=\"contents\" name=\"contents\" rows=\"12\"></textarea></p>\n\
         <p>Leave both empty to enter the database details by hand.</p>\n\
         <p><label><input type=\"checkbox\" name=\"new\" value=\"1\"> This is a new site: \
         create its tables and first content in an empty database</label></p>\n\
         <p><button>Continue</button></p>\n</form>\n",
        notice(error),
        hidden(&wizard.tokens.csrf),
        input(
            "path",
            "Path to the WordPress folder or its wp-config.php on this server",
            &found,
            false
        ),
    );
    page(status, "Step 2 of 3: the site", &body)
}

/// Step 3: what `wp-config.php` did not give, the mode, the network and the
/// uploads folder. `answered` gives the answers sent, if any.
fn details_page(
    wizard: &Wizard,
    found: &Found,
    status: StatusCode,
    error: &str,
    answered: &dyn Fn(&str) -> Option<String>,
) -> Response {
    let value = |name: &str, default: &str| answered(name).unwrap_or_else(|| default.into());
    let mut fields = String::new();
    let content = match &found.content {
        Content::Unknown => None,
        Content::Known(dir) => Some(dir.clone()),
        Content::Asked(default) => {
            let default = default.display().to_string();
            fields.push_str(&input(
                "WP_CONTENT_DIR",
                "Path to wp-content (WP_CONTENT_DIR is computed by PHP)",
                &value("WP_CONTENT_DIR", &default),
                false,
            ));
            Some(PathBuf::from(default))
        }
    };
    let mut notes = found.summary();
    for key in &found.unread {
        for question in questions(key) {
            let shown = value(question.key, question.default);
            fields.push_str(&input(question.key, question.text, &shown, question.secret));
        }
        if questions(key).is_empty() {
            notes.push_str(&format!("  {}\n", super::not_copied(key)));
        }
    }
    let primary = value("MODE", "shadow") == "primary";
    let (mode, title) = if wizard.new_site {
        (
            new_site_fields(wizard, answered),
            "Step 3 of 3: new site, database and network",
        )
    } else {
        (
            format!(
                "<fieldset><legend>Mode</legend>\n{}{}</fieldset>\n",
                choice("shadow", "Shadow mode (read only), to start with", !primary),
                choice("primary", "Primary mode (writes)", primary)
            ),
            "Step 3 of 3: database, mode and network",
        )
    };
    let proxy = value("BEHIND_PROXY", &wizard.listen.behind_proxy.to_string()) == "true";
    let body = format!(
        "{}<pre>{}</pre>\n<form method=\"post\" action=\"{STEP_3}\">\n{}{fields}\
         {mode}{}{}\
         <p><label><input type=\"checkbox\" name=\"BEHIND_PROXY\" value=\"true\"{}> Running \
         HyScribe behind a proxy</label></p>\n{}{}{}\
         <p><button>Check and write .env</button></p>\n</form>\n",
        notice(error),
        escape(&notes),
        hidden(&wizard.tokens.csrf),
        input(
            "DB_READ_USER",
            "Database user granted only SELECT, for every read (empty for none)",
            &value("DB_READ_USER", ""),
            false
        ),
        input("DB_READ_PASSWORD", "Its password", "", true),
        if proxy { " checked" } else { "" },
        input(
            "BIND_HOST",
            "Address to listen on",
            &value("BIND_HOST", &wizard.listen.bind_host.to_string()),
            false
        ),
        input(
            "PORT",
            "Port to listen on",
            &value("PORT", &wizard.listen.port.to_string()),
            false
        ),
        input(
            "UPLOADS_DIR",
            "The site's uploads folder (empty for hyscribe/uploads)",
            &value("UPLOADS_DIR", &found.uploads(content.as_deref())),
            false
        ),
    );
    page(status, title, &body)
}

/// What PHP's install screen asks, for a new site: the site's title and
/// address, and the first administrator, whose password is never sent back.
fn new_site_fields(wizard: &Wizard, answered: &dyn Fn(&str) -> Option<String>) -> String {
    let value = |name: &str, default: &str| answered(name).unwrap_or_else(|| default.into());
    let home = default_home(
        &wizard.listen.bind_host.to_string(),
        &wizard.listen.port.to_string(),
    );
    let hidden = value("blog_public", "1") == "0";
    format!(
        "<fieldset><legend>The new site</legend>\n{}{}{}{}{}{}\
         <p><label><input type=\"checkbox\" name=\"blog_public\" value=\"0\"{}> Discourage \
         search engines from indexing this site</label></p></fieldset>\n",
        input(
            "weblog_title",
            "Site title",
            &value("weblog_title", ""),
            false
        ),
        input(
            "user_name",
            "Administrator user name",
            &value("user_name", ""),
            false
        ),
        input(
            "admin_password",
            "Administrator password (empty for a generated one)",
            "",
            true
        ),
        input("admin_password2", "Password again", "", true),
        input(
            "admin_email",
            "Administrator email address",
            &value("admin_email", ""),
            false
        ),
        input(
            "site_address",
            "Site address, where visitors will open it",
            &value("site_address", &home),
            false
        ),
        if hidden { " checked" } else { "" },
    )
}

fn notice(error: &str) -> String {
    if error.is_empty() {
        String::new()
    } else {
        format!("<p class=\"error\">{}</p>\n", escape(error))
    }
}

fn hidden(token: &str) -> String {
    format!(
        "<input type=\"hidden\" name=\"_csrf\" value=\"{}\">\n",
        escape(token)
    )
}

/// A text field, or a password field, which never shows a value.
fn input(name: &str, label: &str, value: &str, secret: bool) -> String {
    let (kind, value) = if secret {
        ("password", "")
    } else {
        ("text", value)
    };
    format!(
        "<p><label for=\"{name}\">{}</label>\n<input type=\"{kind}\" id=\"{name}\" name=\"{name}\" \
         value=\"{}\" autocomplete=\"off\"></p>\n",
        escape(label),
        escape(value)
    )
}

fn choice(mode: &str, label: &str, checked: bool) -> String {
    let checked = if checked { " checked" } else { "" };
    format!(
        "<label><input type=\"radio\" name=\"MODE\" value=\"{mode}\"{checked}> {label}</label><br>\n"
    )
}

/// `htmlspecialchars()` with quotes.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#039;"),
            _ => out.push(c),
        }
    }
    out
}

const STYLE: &str = "body{background:#f0f0f1;color:#1d2327;font:15px/1.5 -apple-system,\
BlinkMacSystemFont,\"Segoe UI\",Roboto,sans-serif;margin:0;padding:24px 16px}\
main{background:#fff;border:1px solid #c3c4c7;margin:0 auto;max-width:640px;padding:8px 24px 24px}\
label{display:block;font-weight:600}input[type=text],input[type=password],textarea{\
box-sizing:border-box;font:inherit;padding:4px 8px;width:100%}fieldset label{display:inline;\
font-weight:400}pre{background:#f6f7f7;overflow-x:auto;padding:8px;white-space:pre-wrap}\
.error{border-left:4px solid #d63638;padding-left:12px}button{font:inherit;padding:6px 16px}";

fn page(status: StatusCode, title: &str, body: &str) -> Response {
    let html = format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width\">\n\
         <meta name=\"robots\" content=\"noindex,nofollow\">\n\
         <title>{title} &lsaquo; HyScribe setup</title>\n<style>{STYLE}</style>\n</head>\n\
         <body>\n<main>\n<h1>{title}</h1>\n{body}</main>\n</body>\n</html>\n"
    );
    (
        status,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        html,
    )
        .into_response()
}

#[cfg(test)]
#[cfg(unix)]
mod tests;

// SPDX-License-Identifier: AGPL-3.0-or-later

//! The login screen, `wp-login.php`'s login action, at `/hs-login`.

use hyscribe_auth::check_password;
use hyscribe_kses::{esc_attr, esc_html, esc_url, str_contains, url_query};
use hyscribe_phpser::{Value, addslashes, is_empty_string, parse_str, php_trim, urlencode};
use hyscribe_render::formatting::{is_email, sanitize_user};

use crate::assets::{Assets, Urls, site_icon_tags};

pub mod actions;

/// The auth cookie a request carries, as `wp_validate_auth_cookie()` finds it.
pub enum Session {
    /// No login cookie.
    None,
    /// A cookie that does not validate.
    Expired,
    /// A valid session of this user.
    User(u64),
}

/// A request to the login screen. Its values are as PHP sees them before
/// `wp_magic_quotes()`.
pub struct Request<'a> {
    /// `$_GET`.
    pub query: &'a Value,
    /// `$_POST`, for a POST.
    pub form: Option<&'a Value>,
    /// Whether the test cookie came back.
    pub test_cookie: bool,
    /// Whether a logged in cookie came, valid or not.
    pub logged_in_cookie: bool,
    /// The auth cookie.
    pub session: Session,
    /// The reset password cookie's value, `login:key`.
    pub reset_cookie: Option<&'a [u8]>,
    /// Shadow mode: the screen says so and nobody logs in.
    pub read_only: bool,
}

/// A user's ID and password hash.
pub struct User {
    /// `ID`.
    pub id: u64,
    /// `user_pass`.
    pub pass: Vec<u8>,
    /// The `use_ssl` user option.
    pub use_ssl: bool,
    /// `user_activation_key`.
    pub activation_key: Vec<u8>,
}

/// The site the screen is for.
pub trait Site: Urls {
    /// `get_bloginfo( 'name', 'display' )`.
    fn blogname(&self) -> String;
    /// `get_bloginfo( 'name' )`, as stored.
    fn name(&self) -> String;
    /// `wp_create_nonce( $action )` for the current user.
    fn nonce(&self, action: &str) -> String;
    /// `get_option( 'blog_public' )`: search engines may index the site.
    fn public(&self) -> bool;
    /// `home_url( '/' )`.
    fn home_url(&self) -> String;
    /// The login screen's URL with `query`, `?...` or empty.
    fn login_url(&self, query: &str) -> String;
    /// `get_user_by( 'login', $login )`.
    fn user_by_login(&self, login: &[u8]) -> Option<User>;
    /// `get_user_by( 'email', $email )`.
    fn user_by_email(&self, email: &[u8]) -> Option<User>;
    /// `get_option( 'users_can_register' )`.
    fn can_register(&self) -> bool;
    /// `get_post( $id )->post_type`, none for no post.
    fn post_type(&self, id: u64) -> Option<String>;
    /// `get_site_icon_url( $size )`, `None` for no icon.
    fn site_icon(&self, size: i64) -> Option<Vec<u8>>;
}

/// What the login screen answers.
pub enum Login {
    /// The screen to show.
    Page(Vec<u8>),
    /// The credentials or the session are good: log `user` in and redirect.
    LoggedIn {
        /// The user's ID.
        user: u64,
        /// "Remember Me" was checked.
        remember: bool,
        /// The user's `use_ssl` asks for a secure cookie.
        secure: bool,
        /// Where to go: `$_REQUEST['redirect_to']`, slashed, or `admin_url()`.
        redirect_to: Vec<u8>,
    },
}

/// A `WP_Error` entry: its code, message and whether it is a message
/// rather than an error.
struct Notice {
    code: &'static str,
    text: String,
    info: bool,
}

fn error(code: &'static str, text: String) -> Notice {
    Notice {
        code,
        text,
        info: false,
    }
}

fn message(code: &'static str, text: String) -> Notice {
    Notice {
        code,
        text,
        info: true,
    }
}

/// A string value, or none for an array or a missing key.
fn as_text(value: Option<&Value>) -> Option<&[u8]> {
    value.and_then(Value::as_bytes)
}

/// `set_url_scheme( $url, $scheme )` of a URL the site made.
pub(crate) fn set_url_scheme(url: &str, scheme: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    format!("{scheme}://{rest}")
}

/// The login screen's URL with `query`, under `force_ssl_admin()` when
/// `secure`.
fn login_url(site: &dyn Site, query: &str, secure: bool) -> String {
    let url = site.login_url(query);
    if secure {
        set_url_scheme(&url, "https")
    } else {
        url
    }
}

impl Request<'_> {
    /// `$_REQUEST[ $key ]`: the form's value over the query's.
    pub fn request(&self, key: &str) -> Option<&Value> {
        self.form
            .and_then(|form| form.get_str(key))
            .or_else(|| self.query.get_str(key))
    }

    fn posted(&self, key: &str) -> Option<&Value> {
        self.form.and_then(|form| form.get_str(key))
    }

    /// The action `wp-login.php` takes: `login` for any it does not know.
    pub fn action(&self) -> String {
        let action = as_text(self.request("action")).unwrap_or(b"login");
        // `checkemail` overrides `key`, which overrides the action.
        if self.query.get_str("checkemail").is_some() {
            return "checkemail".into();
        }
        if self.query.get_str("key").is_some() {
            return "resetpass".into();
        }
        let known = [
            "confirm_admin_email",
            "postpass",
            "logout",
            "lostpassword",
            "retrievepassword",
            "resetpass",
            "rp",
            "register",
            "checkemail",
            "confirmaction",
            "login",
            "entered_recovery_mode",
        ];
        let action = String::from_utf8_lossy(action);
        match known.contains(&action.as_ref()) {
            true => action.into_owned(),
            false => "login".into(),
        }
    }
}

/// The lost password link of the password errors.
fn lost_password(site: &dyn Site, secure: bool) -> String {
    format!(
        " <a href=\"{}\">Lost your password?</a>",
        login_url(site, "?action=lostpassword", secure)
    )
}

/// `wp_authenticate()` over core's `authenticate` filters, with the
/// session standing in for `wp_authenticate_cookie()`.
fn authenticate(
    site: &dyn Site,
    login: &[u8],
    password: &[u8],
    session: &Session,
    secure: bool,
) -> Result<u64, Vec<Notice>> {
    let username = sanitize_user(login);
    let password = php_trim(password);
    let check = |user: Option<User>, missing: Notice, kind: &str| match user {
        None => Err(vec![missing]),
        Some(user) if check_password(password, &user.pass) => Ok(user.id),
        Some(_) => Err(vec![error(
            "incorrect_password",
            format!(
                "<strong>Error:</strong> The password you entered for the {kind} <strong>{}</strong> is incorrect.{}",
                esc_html(&username),
                lost_password(site, secure)
            ),
        )]),
    };
    let blank = is_empty_string(&username) || is_empty_string(password);
    let mut result = if blank {
        let mut errors = Vec::new();
        if is_empty_string(&username) {
            errors.push(error(
                "empty_username",
                "<strong>Error:</strong> The username field is empty.".into(),
            ));
        }
        if is_empty_string(password) {
            errors.push(error(
                "empty_password",
                "<strong>Error:</strong> The password field is empty.".into(),
            ));
        }
        Err(errors)
    } else {
        let missing = error(
            "invalid_username",
            format!(
                "<strong>Error:</strong> The username <strong>{}</strong> is not registered on this site. If you are unsure of your username, try your email address instead.",
                esc_html(&username)
            ),
        );
        check(site.user_by_login(&username), missing, "username")
    };
    if result.is_err() && !blank && is_email(&username) {
        let missing = error(
            "invalid_email",
            "Unknown email address. Check again or try your username.".into(),
        );
        result = check(site.user_by_email(&username), missing, "email address");
    }
    if is_empty_string(&username) && is_empty_string(password) {
        match session {
            Session::User(id) => result = Ok(*id),
            Session::Expired => {
                result = Err(vec![error(
                    "expired_session",
                    "Please log in again.".into(),
                )])
            }
            Session::None => {}
        }
    }
    result
}

/// The login action: a login, or the screen with what went wrong.
pub fn login(site: &dyn Site, request: &Request) -> Login {
    let action = request.action();
    let interim = request.request("interim-login").is_some();
    let customize = request.request("customize-login").is_some();
    let posted = |key| as_text(request.posted(key)).filter(|v| !is_empty_string(v));
    // A user with `use_ssl` gets `force_ssl_admin( true )` and a secure cookie.
    let secure = posted("log").is_some_and(|log| {
        let name = sanitize_user(log);
        let by_email = || match name.iter().position(|&b| b == b'@') {
            Some(at) if at > 0 => site.user_by_email(&name),
            _ => None,
        };
        site.user_by_login(&name)
            .or_else(by_email)
            .is_some_and(|user| user.use_ssl)
    });
    let redirect_to = match as_text(request.request("redirect_to")) {
        Some(to) => {
            let to = addslashes(to);
            match to.strip_prefix(b"http://") {
                Some(rest) if secure && str_contains(&to, b"wp-admin") => {
                    [&b"https://"[..], rest].concat()
                }
                _ => to,
            }
        }
        None if secure => set_url_scheme(&site.admin_url(""), "https").into_bytes(),
        None => site.admin_url("").into_bytes(),
    };
    let reauth = request.request("reauth").is_some_and(|v| !v.is_empty());
    let login = posted("log").unwrap_or_default();
    let password = addslashes(posted("pwd").unwrap_or_default());
    let remember = request.posted("rememberme").is_some_and(|v| !v.is_empty());
    let mut user = authenticate(site, login, &password, &request.session, secure);
    if !request.logged_in_cookie && request.posted("testcookie").is_some() && !request.test_cookie {
        user = Err(vec![error(
            "test_cookie",
            "<strong>Error:</strong> Cookies are blocked or not supported by your browser. You must <a href=\"https://developer.wordpress.org/advanced-administration/wordpress/cookies/#enable-cookies-in-your-browser\">enable cookies</a> to use WordPress.".into(),
        )]);
    }
    let mut errors = match user {
        Ok(user) if !reauth && !request.read_only => {
            return Login::LoggedIn {
                user,
                remember,
                secure,
                redirect_to,
            };
        }
        Ok(_) => Vec::new(),
        Err(errors) => errors,
    };
    let loggedout = request.query.get_str("loggedout");
    if loggedout.is_some_and(|v| !v.is_empty()) || reauth {
        errors.clear();
    }
    let unposted = request.form.is_none_or(Value::is_empty);
    let codes: Vec<&str> = errors.iter().map(|e| e.code).collect();
    if unposted && codes == ["empty_username", "empty_password"] {
        errors.clear();
    }
    let redirect_get = as_text(request.query.get_str("redirect_to"));
    if interim {
        if errors.is_empty() {
            errors.push(message(
                "expired",
                "Your session has expired. Please log in to continue where you left off.".into(),
            ));
        }
    } else if loggedout.is_some_and(|v| !v.is_empty()) {
        errors.push(message("loggedout", "You are now logged out.".into()));
    } else if as_text(request.query.get_str("registration")) == Some(b"disabled") {
        errors.push(error(
            "registerdisabled",
            "<strong>Error:</strong> User registration is currently not allowed.".into(),
        ));
    } else if str_contains(&redirect_to, b"about.php?updated") {
        errors.push(message(
            "updated",
            "<strong>You have successfully updated WordPress!</strong> Please log back in to see what&#8217;s new.".into(),
        ));
    } else if action == "entered_recovery_mode" {
        errors.push(message(
            "enter_recovery_mode",
            "Recovery Mode Initialized. Please log in to continue.".into(),
        ));
    } else if let Some(to) =
        redirect_get.filter(|to| str_contains(to, b"wp-admin/authorize-application.php"))
    {
        let query = url_query(&addslashes(to)).map(|query| parse_str(&query));
        let app = query
            .as_ref()
            .and_then(|q| q.get_str("app_name"))
            .filter(|a| !a.is_empty());
        let text = match app {
            Some(app) => format!(
                "Please log in to {} to authorize <strong>{}</strong> to connect to your account.",
                site.blogname(),
                esc_html(&app.to_php_string())
            ),
            None => format!(
                "Please log in to {} to proceed with authorization.",
                site.blogname()
            ),
        };
        errors.push(message("authorize_application", text));
    }
    if request.read_only {
        errors.push(message(
            "read_only",
            "This site is read only: logging in needs HyScribe&#8217;s primary mode.".into(),
        ));
    }
    // The reset password cookie's login, unless one was posted.
    let mut user_login = request.reset_cookie.map_or(Vec::new(), |cookie| {
        let login = cookie.split(|&b| b == b':').find(|part| !part.is_empty());
        sanitize_user(login.unwrap_or_default())
    });
    if request.posted("log").is_some() {
        user_login = match errors.first().map(|e| e.code) {
            Some("incorrect_password" | "empty_password") => {
                as_text(request.posted("log")).unwrap_or_default().to_vec()
            }
            _ => Vec::new(),
        };
    }
    let screen = Screen {
        action: &action,
        interim,
        customize,
        loggedout: errors.iter().any(|e| e.code == "loggedout"),
        errors: &errors,
        user_login: &user_login,
        remember,
        secure,
        redirect_to: &redirect_to,
    };
    Login::Page(screen.render(site))
}

/// `wp_nonce_ays( 'log-out' )`: the page asking whether to log out, which
/// is sent with a 403.
pub fn logout_confirmation(site: &dyn Site, request: &Request) -> Vec<u8> {
    let title = format!("You are attempting to log out of {}", site.name());
    let redirect = as_text(request.request("redirect_to"))
        .map(addslashes)
        .unwrap_or_default();
    let mut query = String::from("?action=logout");
    if !is_empty_string(&redirect) {
        query.push_str(&format!(
            "&redirect_to={}",
            String::from_utf8_lossy(&urlencode(&redirect))
        ));
    }
    query.push_str(&format!("&_wpnonce={}", site.nonce("log-out")));
    // `wp_nonce_url()` escapes the URL as HTML.
    let url = esc_html(site.login_url(&query).as_bytes());
    let message = format!("{title}</p><p>Do you really want to <a href=\"{url}\">log out</a>?");
    crate::die::page(site.public(), &title, &message)
}

/// `esc_url()` of a URL.
fn url(text: &str) -> String {
    String::from_utf8_lossy(&esc_url(text.as_bytes())).into_owned()
}

/// What the login screen shows.
struct Screen<'a> {
    action: &'a str,
    interim: bool,
    customize: bool,
    loggedout: bool,
    errors: &'a [Notice],
    user_login: &'a [u8],
    remember: bool,
    /// Under `force_ssl_admin()`.
    secure: bool,
    redirect_to: &'a [u8],
}

/// `wp_print_inline_script_tag()`.
fn inline_script(js: &str) -> String {
    format!("<script>\n{js}\n</script>\n")
}

/// What `login_header()` and `login_footer()` print around an action's
/// markup.
struct Frame<'a> {
    title: &'a str,
    action: &'a str,
    interim: bool,
    customize: bool,
    loggedout: bool,
    /// `login_header()`'s `$message`, as markup.
    message: &'a str,
    errors: &'a [Notice],
    /// The scripts the action enqueues after `login_header()`.
    scripts: &'a [&'a str],
    /// The action's markup between the notices and `login_footer()`.
    body: &'a str,
    /// `login_footer( $input_id )`.
    input_id: &'a str,
}

impl Frame<'_> {
    fn render(&self, site: &dyn Site) -> Vec<u8> {
        let mut assets = Assets::new(true);
        assets.enqueue_style("login");
        if self.customize {
            assets.enqueue_script("customize-base");
        }
        let blogname = site.blogname();
        let mut out = String::new();
        out.push_str("<!DOCTYPE html>\n\t<html lang=\"en-US\">\n\t<head>\n\t<meta http-equiv=\"Content-Type\" content=\"text/html; charset=UTF-8\" />\n");
        out.push_str(&format!(
            "\t<title>{} &lsaquo; {blogname} &#8212; WordPress</title>\n\t",
            self.title
        ));
        if self.loggedout {
            out.push_str(&inline_script("if(\"sessionStorage\" in window){try{for(var key in sessionStorage){if(key.indexOf(\"wp-autosave-\")!=-1){sessionStorage.removeItem(key)}}}catch(e){}};"));
        }
        let robots = crate::die::robots(site.public(), true);
        out.push_str(&format!("<meta name='robots' content='{robots}' />\n"));
        let mut bytes = out.into_bytes();
        bytes.extend(assets.print_head_scripts(site));
        bytes.extend(assets.print_admin_styles(site));
        for script in self.scripts {
            assets.enqueue_script(script);
        }
        let mut out = format!(
            "\t<meta name='referrer' content='strict-origin-when-cross-origin' />\n\t\t<meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\" />\n\t{}\t",
            site_icon_tags(&|size| site.site_icon(size))
        );
        let mut classes = vec![
            format!("login-action-{}", self.action),
            "wp-core-ui".into(),
            "admin-color-modern".into(),
        ];
        if self.interim {
            out.push_str("\t<style>html{background-color: transparent;}</style>\n\t\t\t");
            classes.push("interim-login".into());
        }
        classes.push("locale-en-us".into());
        out.push_str(&format!(
            "</head>\n\t<body class=\"login no-js {}\">\n\t",
            esc_attr(classes.join(" ").as_bytes())
        ));
        out.push_str(&inline_script(
            "document.body.className = document.body.className.replace('no-js','js');",
        ));
        out.push_str(&format!("\n\t\t\t\t<h1 class=\"screen-reader-text\">{}</h1>\n\t\t\t<div id=\"login\">\n\t\t<h1 role=\"presentation\" class=\"wp-login-logo\"><a href=\"https://wordpress.org/\">Powered by WordPress</a></h1>\n\t", self.title));
        if !self.message.is_empty() {
            out.push_str(&format!("{}\n", self.message));
        }
        out.push_str(&self.notices());
        out.push_str(self.body);
        if self.interim {
            out.push_str("\t</div>\n\t\n\t");
        } else {
            out.push_str(&format!(
                "\t\t<p id=\"backtoblog\">\n\t\t\t<a href=\"{}\">&larr; Go to {}</a>\t\t</p>\n\t\t\t</div>\n\t\t\n\t",
                url(&site.home_url()),
                site.blogname()
            ));
        }
        if !self.input_id.is_empty() {
            out.push_str(&format!("<script>\n\t\ttry{{document.getElementById('{}').focus();}}catch(e){{}}\n\t\tif(typeof wpOnload==='function')wpOnload();\n\t\t\n</script>\n", self.input_id));
        }
        let shake = [
            "empty_password",
            "empty_email",
            "invalid_email",
            "invalidcombo",
            "empty_username",
            "invalid_username",
            "incorrect_password",
            "retrieve_password_email_failure",
        ];
        if self.errors.first().is_some_and(|e| shake.contains(&e.code)) {
            out.push_str(&inline_script(
                "document.querySelector('form').classList.add('shake');",
            ));
        }
        bytes.extend(out.into_bytes());
        bytes.extend(assets.print_footer_scripts(site));
        bytes.extend(b"\t</body>\n\t</html>\n\t");
        bytes
    }

    /// `login_header()`'s errors, then its messages.
    fn notices(&self) -> String {
        let mut out = String::new();
        let errors: Vec<&str> = self
            .errors
            .iter()
            .filter(|e| !e.info)
            .map(|e| e.text.as_str())
            .collect();
        let messages: String = self
            .errors
            .iter()
            .filter(|e| e.info)
            .map(|e| format!("<p>{}</p>", e.text))
            .collect();
        if let [only] = errors.as_slice() {
            out.push_str(&format!(
                "<div id=\"login_error\" class=\"notice notice-error\"><p>{only}</p></div>"
            ));
        } else if !errors.is_empty() {
            let items: String = errors.iter().map(|e| format!("<li>{e}</li>")).collect();
            out.push_str(&format!("<div id=\"login_error\" class=\"notice notice-error\"><ul class=\"login-error-list\">{items}</ul></div>"));
        }
        if !messages.is_empty() {
            out.push_str(&format!(
                "<div id=\"login-message\" class=\"notice notice-info message\">{messages}</div>"
            ));
        }
        out
    }
}

impl Screen<'_> {
    fn render(&self, site: &dyn Site) -> Vec<u8> {
        let body = self.form(site) + &self.footer(site);
        Frame {
            title: "Log In",
            action: self.action,
            interim: self.interim,
            customize: self.customize,
            loggedout: self.loggedout,
            message: "",
            errors: self.errors,
            scripts: &["user-profile", "wp-tooltip"],
            body: &body,
            input_id: "",
        }
        .render(site)
    }

    fn form(&self, site: &dyn Site) -> String {
        let described = match self.errors.first() {
            Some(first) if first.info => " aria-describedby=\"login-message\"",
            Some(_) => " aria-describedby=\"login_error\"",
            None => "",
        };
        let mut out = format!(
            "\n\t\t<form name=\"loginform\" id=\"loginform\" action=\"{}\" method=\"post\">\n\t\t\t<p>\n\t\t\t\t<label for=\"user_login\">Username or Email Address</label>\n\t\t\t\t<input type=\"text\" name=\"log\" id=\"user_login\"{described} class=\"input ltr\" value=\"{}\" size=\"20\" autocapitalize=\"off\" autocomplete=\"username\" required=\"required\" />\n\t\t\t</p>\n\n",
            url(&login_url(site, "", self.secure)),
            esc_attr(self.user_login)
        );
        out.push_str(&format!("\t\t\t<div class=\"user-pass-wrap\">\n\t\t\t\t<label for=\"user_pass\">Password</label>\n\t\t\t\t<div class=\"wp-pwd\">\n\t\t\t\t\t<input type=\"password\" name=\"pwd\" id=\"user_pass\"{described} class=\"input password-input ltr\" value=\"\" size=\"20\" autocomplete=\"current-password\" spellcheck=\"false\" required=\"required\" />\n\t\t\t\t\t<button type=\"button\" class=\"button button-secondary wp-hide-pw hide-if-no-js\" data-toggle=\"0\" aria-label=\"Show password\">\n\t\t\t\t\t\t<span class=\"dashicons dashicons-visibility\" aria-hidden=\"true\"></span>\n\t\t\t\t\t</button>\n\t\t\t\t</div>\n\t\t\t</div>\n"));
        let checked = if self.remember {
            " checked='checked'"
        } else {
            ""
        };
        out.push_str(&format!("\t\t\t\t\t\t\t\t\t<p class=\"forgetmenot\">\n\t\t\t\t<input name=\"rememberme\" type=\"checkbox\" id=\"rememberme\" value=\"forever\" {checked} />\n\t\t\t\t<label for=\"rememberme\">Remember Me</label>\n"));
        out.push_str("\t\t\t\t<span class=\"wp-tooltip wp-is-toggletip\">\n\t\t\t\t<button aria-haspopup=\"dialog\" aria-label=\"Help\" class=\"wp-tooltip__toggle\" popovertarget=\"rememberme-help-toggletip\" type=\"button\"><span aria-hidden=\"true\" class=\"dashicons dashicons-editor-help\"></span></button>\n\t\t\t\t<span popover=\"auto\" id=\"rememberme-help-toggletip\" class=\"wp-tooltip__bubble\" role=\"dialog\" aria-label=\"Help\" tabindex=\"-1\" autofocus><span id=\"rememberme-help-toggletip-text\" class=\"wp-tooltip__text\">Selecting &quot;Remember Me&quot; increases the length of time until you&#8217;re asked to log in again on this device. To keep your account secure, use this option only on your personal devices.</span><button type=\"button\" class=\"wp-tooltip__close\" popovertarget=\"rememberme-help-toggletip\" popovertargetaction=\"hide\" aria-label=\"Close\"><span class=\"dashicons dashicons-no-alt\" aria-hidden=\"true\"></span></button></span></span>\t\t\t</p>\n");
        out.push_str("\t\t\t<p class=\"submit\">\n\t\t\t\t<input type=\"submit\" name=\"wp-submit\" id=\"wp-submit\" class=\"button button-primary button-large\" value=\"Log In\" />\n");
        if self.interim {
            out.push_str(
                "\t\t\t\t\t\t\t\t\t<input type=\"hidden\" name=\"interim-login\" value=\"1\" />\n",
            );
        } else {
            out.push_str(&format!(
                "\t\t\t\t\t\t\t\t\t<input type=\"hidden\" name=\"redirect_to\" value=\"{}\" />\n",
                esc_attr(self.redirect_to)
            ));
        }
        if self.customize {
            out.push_str("\t\t\t\t\t\t\t\t\t\t<input type=\"hidden\" name=\"customize-login\" value=\"1\" />\n");
        }
        out.push_str("\t\t\t\t\t\t\t\t\t<input type=\"hidden\" name=\"testcookie\" value=\"1\" />\n\t\t\t</p>\n\t\t</form>\n\n");
        out
    }

    /// The links and the focus script.
    fn footer(&self, site: &dyn Site) -> String {
        let mut out = String::new();
        if self.interim {
            out.push_str("\t\t");
        } else {
            out.push_str(&format!(
                "\t\t\t\t\t<p id=\"nav\">\n\t\t\t\t<a class=\"wp-login-lost-password\" href=\"{}\">Lost your password?</a>\t\t\t</p>\n\t\t\t",
                url(&login_url(site, "?action=lostpassword", self.secure))
            ));
        }
        let target = if self.user_login.is_empty() || self.user_login == b"0" {
            let clear = self
                .errors
                .first()
                .is_some_and(|e| e.code == "invalid_username");
            format!(
                "d = document.getElementById( \"user_login\" );{}",
                if clear { "d.value = \"\";" } else { "" }
            )
        } else {
            "d = document.getElementById( \"user_pass\" ); d.value = \"\";".into()
        };
        out.push_str(&inline_script(&format!(
            "function wp_attempt_focus() {{setTimeout( function() {{try {{{target}d.focus(); d.select();}} catch( er ) {{}}}}, 200);}}\nwp_attempt_focus();\nif ( typeof wpOnload === 'function' ) {{ wpOnload() }}"
        )));
        if self.interim {
            out.push_str(&inline_script("\t\t\t( function() {\n\t\t\t\ttry {\n\t\t\t\t\tvar i, links = document.getElementsByTagName( 'a' );\n\t\t\t\t\tfor ( i in links ) {\n\t\t\t\t\t\tif ( links[i].href ) {\n\t\t\t\t\t\t\tlinks[i].target = '_blank';\n\t\t\t\t\t\t}\n\t\t\t\t\t}\n\t\t\t\t} catch( er ) {}\n\t\t\t}());\n\t\t\t"));
        }
        out
    }
}

#[cfg(test)]
mod tests;

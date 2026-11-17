// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;

use super::*;
use crate::tests::{Oracle, first_difference};

/// Every case of `reference/login/golden.sh`.
#[test]
fn the_login_screen_is_php_s() {
    let golden = include_str!("../../tests/golden/login.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let query = parse_str(f[1].as_bytes());
        let form = parse_str(f[2].as_bytes());
        let state = f[3];
        let request = Request {
            query: &query,
            form: (f[0] == "POST").then_some(&form),
            test_cookie: matches!(state, "1" | "resetpass" | "use_ssl" | "private"),
            logged_in_cookie: false,
            session: match state {
                "expired" => Session::Expired,
                _ => Session::None,
            },
            reset_cookie: (state == "resetpass").then_some(&b"::<b>Ad  min:key"[..]),
            read_only: false,
        };
        let oracle = Oracle {
            use_ssl: state == "use_ssl",
            private: state == "private",
            ..Oracle::default()
        };
        if request.action() == "logout" {
            assert_eq!(f[4], "403", "{line}");
            let ours = String::from_utf8(logout_confirmation(&oracle, &request)).unwrap();
            let php = String::from_utf8(unhex(f[5])).unwrap();
            if ours != php {
                failures.push(format!(
                    "{} {}{}",
                    f[0],
                    f[1],
                    first_difference(&php, &ours)
                ));
            }
            continue;
        }
        assert_eq!(f[4], "200", "{line}");
        let Login::Page(ours) = login(&oracle, &request) else {
            failures.push(format!("{} {} {}: logged in", f[0], f[1], f[2]));
            continue;
        };
        let ours = String::from_utf8(ours).unwrap();
        let php = String::from_utf8(unhex(f[5])).unwrap();
        if ours != php {
            let difference = first_difference(&php, &ours);
            failures.push(format!("{} {} {} {state}{difference}", f[0], f[1], f[2]));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} differ:\n{}",
        failures.len(),
        golden.lines().count(),
        failures.join("\n")
    );
}

/// A session of user 7.
fn seven() -> Account {
    Account {
        id: 7,
        login: b"seven".to_vec(),
    }
}

fn request<'a>(query: &'a Value, form: Option<&'a Value>, session: Session) -> Request<'a> {
    Request {
        query,
        form,
        test_cookie: true,
        logged_in_cookie: !matches!(session, Session::None),
        session,
        reset_cookie: None,
        read_only: false,
    }
}

/// `wp-login.php`'s choice of action, before its switch.
#[test]
fn the_action_is_wp_login_php_s() {
    for (query, form, action) in [
        ("", "", "login"),
        ("action=lostpassword", "", "lostpassword"),
        ("action=lostpassword", "action=logout", "logout"),
        ("action=nope", "", "login"),
        ("action%5B%5D=logout", "", "login"),
        ("action=logout&key=x", "", "resetpass"),
        ("checkemail=confirm", "", "checkemail"),
        ("key=x&checkemail=confirm", "", "checkemail"),
    ] {
        let (query, form) = (parse_str(query.as_bytes()), parse_str(form.as_bytes()));
        let request = request(&query, Some(&form), Session::None);
        assert_eq!(request.action(), action, "{query:?} {form:?}");
    }
}

/// Good credentials, or a valid session with none, log in; with `reauth`
/// the screen shows as it does after a failed login.
#[test]
fn good_credentials_log_in() {
    let empty = Value::Array(Vec::new());
    let admin = "http://127.0.0.1:8081/wp-admin/";
    // Query, form, session, `use_ssl`, then the user (0 for the screen),
    // remember, secure and where to.
    let cases = [
        (
            "",
            "log=admin&pwd=reference-only-password&testcookie=1",
            Session::None,
            false,
            1,
            false,
            false,
            admin,
        ),
        (
            "",
            "log=admin%40default-install.test&pwd=+reference-only-password+&rememberme=forever",
            Session::None,
            false,
            1,
            true,
            false,
            admin,
        ),
        (
            "",
            "log=admin&pwd=reference-only-password",
            Session::None,
            true,
            1,
            false,
            true,
            "https://127.0.0.1:8081/wp-admin/",
        ),
        (
            "redirect_to=http%3A%2F%2Fx%2Fwp-admin%2F",
            "log=admin&pwd=reference-only-password",
            Session::None,
            true,
            1,
            false,
            true,
            "https://x/wp-admin/",
        ),
        (
            "redirect_to=%2Fx%22",
            "",
            Session::User(seven()),
            false,
            7,
            false,
            false,
            "/x\\\"",
        ),
        (
            "",
            "log=admin&pwd=wrong",
            Session::User(seven()),
            false,
            0,
            false,
            false,
            "",
        ),
        (
            "",
            "log=admin&pwd=",
            Session::User(seven()),
            false,
            0,
            false,
            false,
            "",
        ),
    ];
    for (query, form, session, use_ssl, user, remember, secure, to) in cases {
        let (query, form) = (parse_str(query.as_bytes()), parse_str(form.as_bytes()));
        let form = Some(&form).filter(|form| **form != empty);
        match login(
            &Oracle {
                use_ssl,
                ..Oracle::default()
            },
            &request(&query, form, session),
        ) {
            Login::LoggedIn {
                user: id,
                remember: kept,
                secure: safe,
                redirect_to,
            } => {
                assert_eq!(
                    (id, kept, safe, redirect_to),
                    (user, remember, secure, to.as_bytes().to_vec())
                );
            }
            Login::Page(_) => assert_eq!(user, 0, "{query:?} {form:?}"),
        }
    }
    let golden = include_str!("../../tests/golden/login.tsv");
    let reauth = golden
        .lines()
        .find(|line| line.starts_with("POST\treauth=1\t"))
        .unwrap();
    let (query, form) = (
        parse_str(b"reauth=1"),
        parse_str(b"log=admin&pwd=reference-only-password"),
    );
    let Login::Page(page) = login(
        &Oracle::default(),
        &request(&query, Some(&form), Session::None),
    ) else {
        panic!("reauth logged in");
    };
    assert_eq!(page, unhex(reauth.rsplit('\t').next().unwrap()));
}

/// `wp_login` fires for the user the credentials or the session log in,
/// as `wp_signon()` fires it, also where the screen then asks again.
#[test]
fn a_login_fires_wp_login() {
    let oracle = Oracle::default();
    let empty = Value::Array(Vec::new());
    let form = parse_str(b"log=admin&pwd=reference-only-password");
    let reauth = parse_str(b"reauth=1");
    login(&oracle, &request(&empty, Some(&form), Session::None));
    login(&oracle, &request(&reauth, Some(&form), Session::None));
    login(&oracle, &request(&empty, None, Session::User(seven())));
    let wrong = parse_str(b"log=admin&pwd=wrong");
    login(
        &oracle,
        &request(&empty, Some(&wrong), Session::User(seven())),
    );
    assert_eq!(
        *oracle.fired.borrow(),
        [
            "wp_login admin 1 admin",
            "wp_login admin 1 admin",
            "wp_login seven 7 seven",
            "wp_login_failed admin [\"incorrect_password\"]",
        ]
    );
}

/// A site whose email lookup finds a `use_ssl` user for any address.
struct AnyEmail(Oracle);

impl Urls for AnyEmail {
    fn site_url(&self) -> String {
        self.0.site_url()
    }
    fn admin_url(&self, path: &str) -> String {
        self.0.admin_url(path)
    }
    fn fill(&self, text: &str) -> String {
        self.0.fill(text)
    }
}

impl Site for AnyEmail {
    fn blogname(&self) -> String {
        self.0.blogname()
    }
    fn name(&self) -> String {
        self.0.name()
    }
    fn nonce(&self, action: &str) -> String {
        self.0.nonce(action)
    }
    fn public(&self) -> bool {
        self.0.public()
    }
    fn home_url(&self) -> String {
        self.0.home_url()
    }
    fn login_url(&self, query: &str) -> String {
        self.0.login_url(query)
    }
    fn user_by_login(&self, _: &[u8]) -> Option<User> {
        None
    }
    fn user_by_email(&self, _: &[u8]) -> Option<User> {
        self.0.user_by_email(b"admin@default-install.test")
    }
    fn check_password(&self, user: &User, password: &[u8]) -> bool {
        self.0.check_password(user, password)
    }
    fn can_register(&self) -> bool {
        self.0.can_register()
    }
    fn post_type(&self, id: u64) -> Option<String> {
        self.0.post_type(id)
    }
    fn site_icon(&self, size: i64) -> Option<Vec<u8>> {
        self.0.site_icon(size)
    }
    fn wp_login_failed(&self, username: &[u8], error: &[Notice]) {
        self.0.wp_login_failed(username, error);
    }
    fn wp_login(&self, user_login: &[u8], user: &Account) {
        self.0.wp_login(user_login, user);
    }
}

/// `use_ssl` is looked up by email only for a name with an `@` after its
/// first character, as `strpos()` is truthy.
#[test]
fn use_ssl_by_email_needs_an_at_past_the_start() {
    let site = AnyEmail(Oracle {
        use_ssl: true,
        ..Oracle::default()
    });
    assert!(!site.can_register() && site.post_type(1).is_none());
    for (log, secure) in [("x%40y", true), ("%40x", false)] {
        let form = parse_str(format!("log={log}&pwd=wrong").as_bytes());
        let query = Value::Array(Vec::new());
        let Login::Page(page) = login(&site, &request(&query, Some(&form), Session::None)) else {
            panic!("{log} logged in");
        };
        let page = String::from_utf8(page).unwrap();
        assert_eq!(
            page.contains("https://127.0.0.1:8081/wp-login.php"),
            secure,
            "{log}"
        );
    }
}

/// In shadow mode the screen says the site is read only, and good
/// credentials log nobody in.
#[test]
fn shadow_mode_logs_nobody_in() {
    let query = Value::Array(Vec::new());
    let form = parse_str(b"log=admin&pwd=reference-only-password");
    let mut shadow = request(&query, Some(&form), Session::None);
    shadow.read_only = true;
    let Login::Page(page) = login(&Oracle::default(), &shadow) else {
        panic!("logged in read only");
    };
    let page = String::from_utf8(page).unwrap();
    assert!(page.contains("<div id=\"login-message\" class=\"notice notice-info message\"><p>This site is read only: logging in needs HyScribe&#8217;s primary mode.</p></div>"));
    assert!(page.contains("aria-describedby=\"login-message\""));
}

/// A `redirect_to` that is not a string, where PHP fails, is left out.
#[test]
fn a_logout_link_leaves_out_an_array() {
    let query = parse_str(b"action=logout&redirect_to%5B%5D=x");
    let page = logout_confirmation(&Oracle::default(), &request(&query, None, Session::None));
    let page = String::from_utf8(page).unwrap();
    assert!(page.contains("wp-login.php?action=logout&amp;_wpnonce=%nonce:log-out%"));
}

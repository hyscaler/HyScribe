// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::setup::tests::wp_config;
use axum::body::Body;
use axum::http::Request;
use tower::ServiceExt;

const SECRET: &str = "SECRETsecretSECRETsecretSECRET12";

/// A wizard for `home`, its secret and tokens known.
fn shared(home: &Path) -> Arc<Shared> {
    Arc::new(Shared {
        wizard: Mutex::new(Wizard {
            home: home.to_path_buf(),
            listen: hyscribe_config::server(&HashMap::new()).unwrap(),
            secret: Some(SECRET.into()),
            tokens: Tokens {
                form: "form-token".into(),
                cookie: "session-cookie".into(),
                csrf: "csrf-token".into(),
            },
            signed_in: false,
            found: None,
        }),
        done: Notify::new(),
    })
}

struct Answer {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
}

impl Answer {
    fn location(&self) -> &str {
        self.headers[header::LOCATION].to_str().unwrap()
    }

    fn cookies(&self) -> Vec<&str> {
        let set = self.headers.get_all(header::SET_COOKIE);
        set.iter().map(|value| value.to_str().unwrap()).collect()
    }
}

/// `method` of `path` with `cookies` and, for a POST, the form `fields`.
async fn ask(
    shared: &Arc<Shared>,
    method: &str,
    path: &str,
    cookies: &str,
    fields: &[(&str, &str)],
) -> Answer {
    let body: Vec<String> = fields
        .iter()
        .map(|(name, value)| {
            let encode =
                |text: &str| String::from_utf8(hyscribe_phpser::urlencode(text.as_bytes()));
            format!("{}={}", encode(name).unwrap(), encode(value).unwrap())
        })
        .collect();
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::COOKIE, cookies)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body.join("&")))
        .unwrap();
    answered(shared, request).await
}

async fn answered(shared: &Arc<Shared>, request: Request<Body>) -> Answer {
    let response = router(shared.clone()).oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let answer = Answer {
        status,
        headers,
        body: String::from_utf8(body.to_vec()).unwrap(),
    };
    for (name, value) in HEADERS {
        assert_eq!(answer.headers[name], value, "{name}");
    }
    assert!(!answer.body.contains(SECRET), "{}", answer.body);
    answer
}

const FORM: &str = "hyscribe_setup_form=form-token";
const SIGNED_IN: &str = "hyscribe_setup=session-cookie";

async fn sign_in(shared: &Arc<Shared>) {
    let fields = [("_csrf", "form-token"), ("secret", SECRET)];
    let answer = ask(shared, "POST", STEP_1, FORM, &fields).await;
    assert_eq!(answer.status, StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn every_page_needs_the_secret_first() {
    let home = tempfile::tempdir().unwrap();
    let shared = shared(home.path());
    for path in ["/", "/wp-admin/", STEP_2, STEP_3, "/installation/step-4"] {
        let answer = ask(&shared, "GET", path, "", &[]).await;
        assert_eq!(answer.status, StatusCode::SEE_OTHER, "{path}");
        assert_eq!(answer.location(), STEP_1);
    }
    let answer = ask(&shared, "GET", STEP_1, "", &[]).await;
    assert_eq!(answer.status, StatusCode::OK);
    assert!(
        answer
            .body
            .contains("<input type=\"hidden\" name=\"_csrf\" value=\"form-token\">"),
        "{}",
        answer.body
    );
    assert!(answer.body.contains("type=\"password\" id=\"secret\""));
    assert_eq!(
        answer.cookies(),
        ["hyscribe_setup_form=form-token; Path=/installation; HttpOnly; SameSite=Strict"]
    );
    assert_eq!(
        answer.headers[header::CONTENT_TYPE],
        "text/html; charset=utf-8"
    );
}

#[tokio::test]
async fn the_secret_works_once_and_only_with_its_form() {
    let home = tempfile::tempdir().unwrap();
    let shared = shared(home.path());
    let right = [("_csrf", "form-token"), ("secret", SECRET)];
    // The form's token must be both in its cookie and in the form.
    for (cookies, csrf) in [
        ("", "form-token"),
        ("hyscribe_setup_form=", ""),
        (FORM, "other"),
        ("hyscribe_setup_form=other", "other"),
    ] {
        let fields = [("_csrf", csrf), ("secret", SECRET)];
        let answer = ask(&shared, "POST", STEP_1, cookies, &fields).await;
        assert_eq!(answer.status, StatusCode::FORBIDDEN, "{cookies}");
        assert!(answer.body.contains("This form has expired."));
        assert!(answer.body.contains("<a href=\"/installation/step-1\">"));
    }
    let wrong = [("_csrf", "form-token"), ("secret", "guess")];
    let answer = ask(&shared, "POST", STEP_1, FORM, &wrong).await;
    assert_eq!(answer.status, StatusCode::FORBIDDEN);
    assert!(answer.body.contains("That is not the setup secret"));
    assert!(answer.body.contains("value=\"form-token\""));
    // Not a form.
    let request = Request::builder()
        .method("POST")
        .uri(STEP_1)
        .header(header::COOKIE, FORM)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let answer = answered(&shared, request).await;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST);
    assert!(answer.body.contains("The form could not be read."));

    let answer = ask(&shared, "POST", STEP_1, FORM, &right).await;
    assert_eq!(answer.status, StatusCode::SEE_OTHER);
    assert_eq!(answer.location(), STEP_2);
    assert_eq!(
        answer.cookies(),
        [
            "hyscribe_setup=session-cookie; Path=/installation; HttpOnly; SameSite=Strict",
            "hyscribe_setup_form=; Path=/installation; HttpOnly; SameSite=Strict; Max-Age=0",
        ]
    );
    assert!(shared.wizard.lock().await.secret.is_none());
    // Used once: another browser cannot use it again.
    let answer = ask(&shared, "POST", STEP_1, FORM, &right).await;
    assert_eq!(answer.status, StatusCode::FORBIDDEN);
    assert!(answer.body.contains("or it was used already"));
    // The session goes on; a wrong cookie is no session.
    let answer = ask(&shared, "GET", STEP_1, SIGNED_IN, &[]).await;
    assert_eq!(answer.location(), STEP_2);
    let answer = ask(&shared, "GET", "/", SIGNED_IN, &[]).await;
    assert_eq!(answer.location(), STEP_2);
    for cookies in ["hyscribe_setup=session-cookiE", "x=session-cookie"] {
        let answer = ask(&shared, "GET", STEP_2, cookies, &[]).await;
        assert_eq!(answer.location(), STEP_1, "{cookies}");
    }
}

#[tokio::test]
async fn a_cookie_alone_is_no_session_before_the_secret() {
    let home = tempfile::tempdir().unwrap();
    let shared = shared(home.path());
    let answer = ask(&shared, "GET", STEP_2, SIGNED_IN, &[]).await;
    assert_eq!(answer.location(), STEP_1);
}

#[tokio::test]
async fn every_form_carries_the_session_s_token() {
    let home = tempfile::tempdir().unwrap();
    let shared = shared(home.path());
    sign_in(&shared).await;
    for (step, fields) in [
        (STEP_2, &[("path", "none")][..]),
        (STEP_2, &[("_csrf", "csrf-tokeN"), ("path", "none")]),
        (STEP_3, &[("_csrf", "form-token")]),
    ] {
        let answer = ask(&shared, "POST", step, SIGNED_IN, fields).await;
        assert_eq!(answer.status, StatusCode::FORBIDDEN, "{step}");
        assert!(answer.body.contains(&format!("<a href=\"{step}\">")));
    }
    assert!(shared.wizard.lock().await.found.is_none());
    let request = Request::builder()
        .method("POST")
        .uri(STEP_2)
        .header(header::COOKIE, SIGNED_IN)
        .body(Body::from("path=none"))
        .unwrap();
    assert_eq!(
        answered(&shared, request).await.status,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn the_site_is_found_by_path_by_contents_or_not_at_all() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("hyscribe");
    let site = dir.path().join("site");
    std::fs::create_dir(&home).unwrap();
    std::fs::create_dir(&site).unwrap();
    std::fs::write(site.join("wp-config.php"), wp_config("")).unwrap();
    let shared = shared(&home);
    sign_in(&shared).await;
    // Step 3 waits for step 2.
    let answer = ask(&shared, "GET", STEP_3, SIGNED_IN, &[]).await;
    assert_eq!(answer.location(), STEP_2);
    let answer = ask(&shared, "GET", STEP_2, SIGNED_IN, &[]).await;
    assert_eq!(answer.status, StatusCode::OK);
    let found = format!("value=\"{}\"", site.display());
    assert!(answer.body.contains(&found), "{}", answer.body);
    assert!(answer.body.contains("value=\"csrf-token\""));

    let csrf = ("_csrf", "csrf-token");
    let answer = ask(
        &shared,
        "POST",
        STEP_2,
        SIGNED_IN,
        &[csrf, ("path", "/nowhere")],
    )
    .await;
    assert_eq!(answer.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        answer
            .body
            .contains("<p class=\"error\">no wp-config.php in /nowhere or the folder above it</p>"),
        "{}",
        answer.body
    );
    // Pasted, only a literal WP_CONTENT_DIR tells where wp-content is.
    let source = "<?php define( 'DB_NAME', 'x' . 'y' ); define( 'WP_CONTENT_DIR', '/srv/c' );";
    let answer = ask(
        &shared,
        "POST",
        STEP_2,
        SIGNED_IN,
        &[csrf, ("contents", source)],
    )
    .await;
    assert_eq!(answer.location(), STEP_3);
    let answer = ask(&shared, "GET", STEP_3, SIGNED_IN, &[]).await;
    assert!(
        answer
            .body
            .contains("name=\"UPLOADS_DIR\" value=\"/srv/c/uploads\"")
    );
    let pasted = [csrf, ("contents", "<?php define( 'DB_NAME', 'x' . 'y' );")];
    let answer = ask(&shared, "POST", STEP_2, SIGNED_IN, &pasted).await;
    assert_eq!(answer.location(), STEP_3);
    let answer = ask(&shared, "GET", STEP_3, SIGNED_IN, &[]).await;
    assert!(answer.body.contains("name=\"UPLOADS_DIR\" value=\"\""));
    assert!(
        answer.body.contains(
            "  Read the wp-config.php given\n  Not a literal there, or missing: DB_NAME, "
        ),
        "{}",
        answer.body
    );
    let large = " ".repeat(wpconfig::MAX_SIZE + 1);
    let broken = [csrf, ("contents", large.as_str())];
    let answer = ask(&shared, "POST", STEP_2, SIGNED_IN, &broken).await;
    assert_eq!(answer.status, StatusCode::UNPROCESSABLE_ENTITY);
    for path in ["", "none"] {
        let answer = ask(&shared, "POST", STEP_2, SIGNED_IN, &[csrf, ("path", path)]).await;
        assert_eq!(answer.location(), STEP_3);
        let answer = ask(&shared, "GET", STEP_3, SIGNED_IN, &[]).await;
        assert!(answer.body.contains("id=\"DB_NAME\""), "{}", answer.body);
        assert!(!answer.body.contains("Read "), "{}", answer.body);
        assert!(!answer.body.contains("Not a literal"), "{}", answer.body);
        // No wp-content, so no default uploads folder.
        assert!(answer.body.contains("name=\"UPLOADS_DIR\" value=\"\""));
    }
    let path = site.display().to_string();
    let answer = ask(&shared, "POST", STEP_2, SIGNED_IN, &[csrf, ("path", &path)]).await;
    assert_eq!(answer.location(), STEP_3);
    let answer = ask(&shared, "GET", STEP_3, SIGNED_IN, &[]).await;
    assert!(!answer.body.contains("id=\"DB_NAME\""), "{}", answer.body);
    assert!(!answer.body.contains("reference-only"), "{}", answer.body);
    assert!(!answer.body.contains("$alt"), "{}", answer.body);
    for shown in [
        format!("<pre>  Read {path}/wp-config.php\n</pre>"),
        "<input type=\"radio\" name=\"MODE\" value=\"shadow\" checked>".into(),
        "name=\"BIND_HOST\" value=\"127.0.0.1\"".into(),
        "name=\"PORT\" value=\"8080\"".into(),
        format!("name=\"UPLOADS_DIR\" value=\"{path}/wp-content/uploads\""),
        "name=\"BEHIND_PROXY\" value=\"true\">".into(),
    ] {
        assert!(answer.body.contains(&shown), "{shown}\n{}", answer.body);
    }
}

#[tokio::test]
async fn step_3_checks_then_writes_env() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("hyscribe");
    let site = dir.path().join("site");
    std::fs::create_dir(&home).unwrap();
    std::fs::create_dir_all(site.join("wp-content/uploads")).unwrap();
    let extra = "define( 'WP_CONTENT_DIR', __DIR__ . '/wp-content' );\n\
                 define( 'MULTISITE', getenv( 'MULTISITE' ) );";
    std::fs::write(site.join("wp-config.php"), wp_config(extra)).unwrap();
    std::fs::write(site.join("wp-content/object-cache.php"), "<?php").unwrap();
    let shared = shared(&home);
    sign_in(&shared).await;
    let csrf = ("_csrf", "csrf-token");
    let path = site.display().to_string();
    ask(&shared, "POST", STEP_2, SIGNED_IN, &[csrf, ("path", &path)]).await;
    let answer = ask(&shared, "GET", STEP_3, SIGNED_IN, &[]).await;
    for shown in [
        format!("id=\"WP_CONTENT_DIR\" name=\"WP_CONTENT_DIR\" value=\"{path}/wp-content\""),
        "  MULTISITE is not a literal true or false, so it is not copied: the safer value holds\n"
            .into(),
    ] {
        assert!(answer.body.contains(&shown), "{shown}\n{}", answer.body);
    }
    // A failing check shows the answers again, but never a password.
    let content = format!("{path}/wp-content");
    let refused = [
        csrf,
        ("WP_CONTENT_DIR", &content),
        ("MODE", "primary"),
        ("DB_READ_USER", "wordpress"),
        ("DB_READ_PASSWORD", "reference-only"),
        ("BEHIND_PROXY", "true"),
        ("BIND_HOST", "::1"),
        ("PORT", "8081"),
        ("UPLOADS_DIR", "/srv/<uploads>"),
    ];
    let answer = ask(&shared, "POST", STEP_3, SIGNED_IN, &refused).await;
    assert_eq!(answer.status, StatusCode::UNPROCESSABLE_ENTITY);
    for shown in [
        "<p class=\"error\">DB_READ_USER wordpress may only be granted SELECT; it has ",
        "value=\"primary\" checked>",
        "name=\"DB_READ_USER\" value=\"wordpress\"",
        "name=\"DB_READ_PASSWORD\" value=\"\"",
        "value=\"true\" checked>",
        "name=\"BIND_HOST\" value=\"::1\"",
        "name=\"PORT\" value=\"8081\"",
        "name=\"UPLOADS_DIR\" value=\"/srv/&lt;uploads&gt;\"",
    ] {
        assert!(answer.body.contains(shown), "{shown}\n{}", answer.body);
    }
    assert!(!answer.body.contains("reference-only"), "{}", answer.body);
    assert!(!home.join(".env").exists());

    let uploads = format!("{content}/uploads");
    let accepted = [
        csrf,
        // Cleared, the asked wp-content takes its default.
        ("WP_CONTENT_DIR", ""),
        ("MODE", "shadow"),
        ("UPLOADS_DIR", &uploads),
    ];
    let answer = ask(&shared, "POST", STEP_3, SIGNED_IN, &accepted).await;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.body);
    let env = home.join(".env");
    for shown in [
        "<h1>HyScribe is set up</h1>".to_string(),
        "  Keys and salts: 8 copied, 0 generated\n".into(),
        format!("  Warning: {content}/object-cache.php is a persistent object cache."),
        format!("Wrote {} (mode 600)", env.display()),
    ] {
        assert!(answer.body.contains(&shown), "{shown}\n{}", answer.body);
    }
    assert!(!answer.body.contains("reference-only"), "{}", answer.body);
    // The wizard is done: the server stops, and nothing more is served.
    tokio::time::timeout(Duration::from_secs(1), shared.done.notified())
        .await
        .unwrap();
    let config = hyscribe_config::load(&home, &HashMap::new())
        .unwrap()
        .unwrap();
    assert_eq!(config.database, hyscribe_testkit::fixture_database());
    assert_eq!(
        config.uploads_dir,
        Some(PathBuf::from(format!("{content}/uploads")))
    );
    assert_eq!(config.server.port, 8080);
    assert!(!config.server.behind_proxy);
    assert_eq!(config.mode, hyscribe_config::Mode::Shadow);
    let answer = ask(&shared, "GET", STEP_3, SIGNED_IN, &[]).await;
    assert_eq!(answer.location(), STEP_1);
}

#[tokio::test]
async fn large_bodies_are_refused() {
    let home = tempfile::tempdir().unwrap();
    let shared = shared(home.path());
    let request = Request::builder()
        .method("POST")
        .uri(STEP_1)
        .body(Body::from(vec![b'a'; MAX_BODY + 1]))
        .unwrap();
    let answer = answered(&shared, request).await;
    assert_eq!(answer.status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[test]
fn forms_are_read_strictly() {
    let kind = b"application/x-www-form-urlencoded";
    let pairs = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect()
    };
    assert_eq!(
        read_form(kind, b"a=1&&b=x+y%21&c&d=e=f"),
        Some(pairs(&[("a", "1"), ("b", "x y!"), ("c", ""), ("d", "e=f")]))
    );
    assert_eq!(
        read_form(
            b" Application/X-WWW-Form-URLEncoded ; charset=UTF-8",
            b"a=1"
        ),
        Some(pairs(&[("a", "1")]))
    );
    assert_eq!(read_form(kind, b""), Some(Vec::new()));
    for (kind, body) in [
        (&b"multipart/form-data"[..], &b"a=1"[..]),
        (b"", b"a=1"),
        (kind, b"a=1&a=2"),
        (kind, b"a=%ff"),
        (kind, b"%ff=a"),
    ] {
        assert_eq!(read_form(kind, body), None, "{body:?}");
    }
    let most: Vec<String> = (0..MAX_FIELDS).map(|n| format!("f{n}=")).collect();
    assert_eq!(
        read_form(kind, most.join("&").as_bytes()).map(|f| f.len()),
        Some(MAX_FIELDS)
    );
    let more = format!("{}&last=", most.join("&"));
    assert_eq!(read_form(kind, more.as_bytes()), None);
    let mut largest = b"a=".to_vec();
    largest.resize(MAX_BODY, b'b');
    assert!(read_form(kind, &largest).is_some());
    largest.push(b'b');
    assert_eq!(read_form(kind, &largest), None);
}

#[test]
fn cookies_and_text_are_read_and_escaped() {
    let mut headers = HeaderMap::new();
    for value in ["a=1; hyscribe_setup=x", "hyscribe_setup=y;b", "c=2"] {
        headers.append(header::COOKIE, HeaderValue::from_static(value));
    }
    assert_eq!(cookie(&headers, "hyscribe_setup"), Some(&b"x"[..]));
    assert_eq!(cookie(&headers, "b"), Some(&b""[..]));
    assert_eq!(cookie(&headers, "c"), Some(&b"2"[..]));
    assert_eq!(cookie(&headers, "d"), None);
    assert_eq!(
        escape("<a href=\"x\">'&'</a>"),
        "&lt;a href=&quot;x&quot;&gt;&#039;&amp;&#039;&lt;/a&gt;"
    );
    let form = vec![("a".to_string(), "1".to_string())];
    assert_eq!((field(&form, "a"), field(&form, "b")), ("1", ""));
}

/// A free port on the loopback address.
fn free_port() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port().to_string()
}

#[tokio::test]
async fn setup_mode_stops_on_a_signal_or_its_timeout() {
    let home = tempfile::tempdir().unwrap();
    let port = free_port();
    let env = HashMap::from([("PORT".to_string(), port.clone())]);
    let (signal, mut stopped) = oneshot::channel();
    signal.send(15).unwrap();
    let mut out = Vec::new();
    let ended = setup_mode(home.path(), &env, &mut out, &mut stopped, SETUP_TIMEOUT).await;
    assert_eq!(ended, Ok(Ended::Stopped(15)));
    let out = String::from_utf8(out).unwrap();
    let start = format!(
        "No .env: setup mode. Open http://127.0.0.1:{port}/installation/step-1 and enter the \
         one-time setup secret "
    );
    let secret = out.strip_prefix(&start).unwrap();
    let (secret, rest) = secret.split_once('\n').unwrap();
    assert_eq!(secret.len(), SECRET_LENGTH);
    assert!(secret.bytes().all(|b| b.is_ascii_alphanumeric()));
    assert_eq!(
        rest,
        "Setup mode stops in 30 minutes unless setup finishes.\n"
    );

    let (_signal, mut stopped) = oneshot::channel();
    let mut out = Vec::new();
    let ended = setup_mode(
        home.path(),
        &env,
        &mut out,
        &mut stopped,
        Duration::from_millis(50),
    )
    .await;
    assert_eq!(
        ended,
        Err("setup mode stopped after 0 minutes without .env; start again for a new secret".into())
    );
    // The listener is closed again.
    assert!(std::net::TcpStream::connect(format!("127.0.0.1:{port}")).is_err());

    let bad = HashMap::from([("BIND_HOST".to_string(), "x".to_string())]);
    let ended = setup_mode(home.path(), &bad, &mut out, &mut stopped, SETUP_TIMEOUT).await;
    assert_eq!(ended, Err("BIND_HOST is invalid: not an IP address".into()));
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port().to_string();
    let env = HashMap::from([("PORT".to_string(), port.clone())]);
    let ended = setup_mode(home.path(), &env, &mut out, &mut stopped, SETUP_TIMEOUT).await;
    assert!(
        ended
            .unwrap_err()
            .starts_with(&format!("cannot listen on 127.0.0.1:{port}: ")),
    );
}

// SPDX-License-Identifier: AGPL-3.0-or-later

//! `reference/comment-post/golden.sh`'s cases, each in a transaction on the
//! fixture database that is rolled back: the answer compared with PHP's,
//! and the rows with the write oracle's; then the route itself.

use axum::body::Body;
use hyscribe_config::Mode;
use hyscribe_db::{Connection, Writer};
use hyscribe_testkit::fixture_database;
use hyscribe_testkit::writes::check_writes;
use sqlx::Connection as _;
use tower::ServiceExt;

use super::*;
use crate::admin::tests::{logged_in, page_of, salted, send, session_nonce, unwritable};
use crate::{Peer, router};

const GOLDEN: &str = include_str!("../../tests/golden/comment-post.tsv");
const CASES: &str = include_str!("../../../../reference/comment-post/cases.list");
const FORM: &str = "application/x-www-form-urlencoded";
const BOUNDARY: &str = "----HyScribeBoundary";

/// A case of `cases.list`.
struct Case {
    name: &'static str,
    method: &'static str,
    admin: bool,
    cookie: &'static str,
    setup: &'static str,
    first: &'static str,
    body: &'static str,
}

fn cases() -> Vec<Case> {
    let lines = CASES.lines().filter(|line| !line.starts_with('#'));
    lines
        .map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            let [name, method, user, cookie, setup, _, first, body] = fields[..] else {
                panic!("{line}");
            };
            Case {
                name,
                method,
                admin: user == "1",
                cookie,
                setup,
                first,
                body,
            }
        })
        .collect()
}

/// PHP's answer to `case`: status, `Location`, cookies and body.
fn php(case: &str) -> (u16, String, String, Vec<u8>) {
    let row = GOLDEN
        .lines()
        .find(|row| row.starts_with(&format!("{case}\t")));
    let fields: Vec<&str> = row.unwrap().split('\t').collect();
    let [_, status, to, cookies, body] = fields[..] else {
        panic!("{case}");
    };
    let body = hyscribe_testkit::unhex(body);
    (status.parse().unwrap(), to.into(), cookies.into(), body)
}

/// The body of a `MULTIPART` case: its pairs as `multipart/form-data`
/// parts, a name starting with `@` a file.
fn multipart(body: &str) -> String {
    let mut out = String::new();
    for pair in body.split('&') {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        let decode = |text: &str| String::from_utf8(hyscribe_phpser::urldecode(text.as_bytes()));
        let (name, value) = (decode(name).unwrap(), decode(value).unwrap());
        let disposition = match name.strip_prefix('@') {
            Some(file) => {
                format!("name=\"{file}\"; filename=\"picture.txt\"\r\nContent-Type: text/plain")
            }
            None => format!("name=\"{name}\""),
        };
        out += &format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; {disposition}\r\n\r\n{value}\r\n"
        );
    }
    out + &format!("--{BOUNDARY}--\r\n")
}

/// The content type of a body made by [`multipart`] or a form.
fn kind_of(body: &str) -> String {
    match body.starts_with("--") {
        true => format!("multipart/form-data; boundary={BOUNDARY}"),
        false => FORM.into(),
    }
}

/// A body with `%aN%` as N letters a and `%nonce%` as `nonce`.
fn expand(body: &str, nonce: &str) -> String {
    let mut out = String::new();
    let mut rest = body.replace("%nonce%", nonce);
    while let Some((before, tail)) = rest.split_once("%a") {
        let (digits, after) = tail.split_once('%').unwrap();
        out = out + before + &"a".repeat(digits.parse().unwrap());
        rest = after.to_string();
    }
    out + &rest
}

/// A response as the golden records it: the new comment's ID as `%id%`,
/// `hash` as `%hash%`, hs-admin as wp-admin, cookies' expiry as `%expires%`
/// and without the attributes HyScribe adds.
async fn recorded(response: Response, id: u64, hash: &str) -> (u16, String, String, Vec<u8>) {
    let status = response.status().as_u16();
    let to = response
        .headers()
        .get(header::LOCATION)
        .map_or("", |to| to.to_str().unwrap());
    let to = to
        .replace(&format!("comment-{id}"), "comment-%id%")
        .replace(&format!("unapproved={id}"), "unapproved=%id%")
        .replace(&format!("moderation-hash={hash}"), "moderation-hash=%hash%")
        .replace("/hs-admin/", "/wp-admin/");
    let cookies: Vec<String> = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|cookie| {
            let cookie = cookie
                .to_str()
                .unwrap()
                .replace("; HttpOnly; SameSite=Lax", "");
            let (name, rest) = cookie.split_once("expires=").unwrap();
            let (_, path) = rest.split_once("; path=").unwrap();
            format!("{name}%expires%; path={path}")
        })
        .collect();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await;
    (status, to, cookies.join("|"), body.unwrap().to_vec())
}

/// The answer to `body` as one action on `conn`, whose statements after
/// `budget` fail, from 192.0.2.1 with `cookie`.
async fn post(
    (site, writer): (&Site, &Writer),
    conn: &mut Connection,
    (body, cookie): (&str, &str),
    budget: Option<usize>,
) -> Response {
    let mut headers = HeaderMap::new();
    let kind = kind_of(body);
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_str(&kind).unwrap());
    headers.insert(
        header::USER_AGENT,
        HeaderValue::from_static("HyScribe oracle"),
    );
    headers.insert(header::COOKIE, HeaderValue::from_str(cookie).unwrap());
    let asked = Asked {
        post: true,
        headers,
        body: body.to_string().into(),
        peer: Some([192, 0, 2, 1].into()),
    };
    let form = read_form(kind.as_bytes(), body.as_bytes()).unwrap();
    tokio::task::block_in_place(|| {
        let begin: Begin<'_> = Box::pin(async move {
            let mut write = writer.begin_on(conn).await?;
            if let Some(budget) = budget {
                write.fail_after(budget);
            }
            Ok(write)
        });
        answer(site, &asked, &form, (now(), Handle::current()), begin)
    })
}

/// The last comment's ID and moderation hash under the test salts.
async fn last_comment(conn: &mut Connection) -> (u64, String) {
    let sql = "SELECT comment_ID, CAST(comment_date_gmt AS CHAR) FROM wp_comments ORDER BY comment_ID DESC LIMIT 1";
    let (id, date): (u64, String) = sqlx::query_as(sql).fetch_one(conn).await.unwrap();
    let hash = hyscribe_phpser::hash_hmac_md5(date.as_bytes(), b"AUTH_KEYAUTH_SALT");
    (id, hash)
}

async fn comment_count(conn: &mut Connection) -> i64 {
    let sql = "SELECT COUNT(*) FROM wp_comments";
    sqlx::query_scalar(sql).fetch_one(conn).await.unwrap()
}

/// Every POST of `cases.list` answers as PHP answered it, and writes the
/// rows PHP wrote, from the state its setup SQL and first body leave, the
/// page of a comment on paged comments and a multipart body included.
#[tokio::test(flavor = "multi_thread")]
async fn posts_answer_and_write_as_php() {
    // The admin saves' content shares the users' meta rows, which deadlock.
    let _turn = crate::admin::saves::tests::turn().await;
    let _writes = crate::admin::tests::COMMENT_ROWS.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let (site, logged_in) = logged_in("wordpress", dir.path()).await;
    let nonce = session_nonce(&site, &logged_in, "unfiltered-html-comment_1").await;
    let writer = Writer::open(&fixture_database(), Mode::Primary).await;
    let writer = writer.unwrap().unwrap();
    let mut pretty = salted(Arc::into_inner(crate::tests::oracle_site().await).unwrap());
    // The site reads its options once, so a case that sets the permalink
    // structure is served by a site that has it set.
    pretty
        .options
        .insert("permalink_structure".into(), b"/%postname%/".to_vec());
    pretty.build_rewriter();
    let mut failures = Vec::new();
    for case in cases().iter().filter(|case| case.method != "GET") {
        let on = match case.setup.contains("permalink_structure") {
            true => (&pretty, &writer),
            false => (&*site, &writer),
        };
        let mut cookie: Vec<&str> = Vec::new();
        if case.admin {
            cookie.push(&logged_in);
        }
        if case.cookie != "-" {
            cookie.push(case.cookie);
        }
        let cookie = cookie.join("; ");
        let mut body = expand(case.body, &nonce);
        if case.method == "MULTIPART" {
            body = multipart(&body);
        }
        let setup = async |conn: &mut Connection| {
            for query in case.setup.split("; ").filter(|_| case.setup != "-") {
                sqlx::query(query).execute(&mut *conn).await.unwrap();
            }
            if case.first != "-" {
                post(on, conn, (&expand(case.first, &nonce), &cookie), None).await;
            }
            Ok::<_, String>(())
        };
        let php = php(case.name);
        let mut action = async |conn: &mut Connection, ()| {
            let response = post(on, conn, (&body, &cookie), None).await;
            let (id, hash) = last_comment(conn).await;
            let ours = recorded(response, id, &hash).await;
            if ours != php {
                failures.push(format!("{}: {:?}\n  php {:?}", case.name, ours.0, php.0));
                failures.push(format!(
                    "  {}\n  {}\n  {}\n  {}",
                    ours.1, php.1, ours.2, php.2
                ));
            }
            Ok(())
        };
        if php.0 == 302 {
            let golden = format!("comment-post-{}", case.name);
            let rows = check_writes(&fixture_database(), &golden, setup, action).await;
            if let Err(rows) = rows {
                failures.push(rows);
            }
            continue;
        }
        // A refusal writes nothing.
        let mut conn = connect().await;
        let mut tx = conn.begin().await.unwrap();
        setup(&mut tx).await.unwrap();
        let before = comment_count(&mut tx).await;
        action(&mut tx, ()).await.unwrap();
        assert_eq!(comment_count(&mut tx).await, before, "{}", case.name);
        tx.rollback().await.unwrap();
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

async fn connect() -> sqlx::MySqlConnection {
    let db = fixture_database();
    let options = sqlx::mysql::MySqlConnectOptions::new()
        .host(&db.host)
        .port(db.port)
        .database(&db.name)
        .username(&db.user)
        .password(db.password.expose())
        .charset(&db.charset);
    sqlx::MySqlConnection::connect_with(&options).await.unwrap()
}

/// A statement that fails part way keeps nothing and shows the database
/// error page: a visitor's reply, on paged comments too, and an
/// administrator's comment, under every budget short of success.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_statement_keeps_nothing() {
    let _writes = crate::admin::tests::COMMENT_ROWS.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let (site, logged_in) = logged_in("wordpress", dir.path()).await;
    let writer = Writer::open(&fixture_database(), Mode::Primary).await;
    let writer = writer.unwrap().unwrap();
    let held =
        "comment_post_ID=1&author=Ann&email=ann%40example.test&comment=A+reply.&comment_parent=1";
    let admin = "comment_post_ID=1&comment=An+administrator%27s+comment.";
    let paged = [
        "UPDATE wp_options SET option_value = '1' WHERE option_name = 'page_comments'",
        "UPDATE wp_options SET option_value = '2' WHERE option_name = 'comments_per_page'",
    ];
    let posts = [
        (held, "", false),
        (admin, logged_in.as_str(), false),
        (held, "", true),
    ];
    for (body, cookie, on) in posts {
        let mut conn = connect().await;
        let mut tx = conn.begin().await.unwrap();
        for sql in paged.iter().filter(|_| on) {
            sqlx::query(*sql).execute(&mut *tx).await.unwrap();
        }
        let before = comment_count(&mut tx).await;
        let mut budget = 0;
        loop {
            let response = post((&site, &writer), &mut tx, (body, cookie), Some(budget)).await;
            if response.status() == StatusCode::FOUND {
                break;
            }
            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
            assert_eq!(comment_count(&mut tx).await, before, "{body} {budget}");
            budget += 1;
        }
        assert_eq!(comment_count(&mut tx).await, before + 1);
        tx.rollback().await.unwrap();
    }
}

fn request(body: &str, kind: &str) -> axum::http::Request<Body> {
    axum::http::Request::builder()
        .method("POST")
        .uri(PATH)
        .header(header::CONTENT_TYPE, kind)
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// The route: any other method is not allowed; shadow mode refuses every
/// post as read only; a body only PHP reads, or an array to redirect to,
/// is not ported; a refusal is PHP's page; and a comment the database
/// refuses, or a database HyScribe does not write to, is its error page.
#[tokio::test(flavor = "multi_thread")]
async fn the_route_answers_as_php() {
    let dir = tempfile::tempdir().unwrap();
    let (site, _) = logged_in("wordpress", dir.path()).await;
    for method in ["GET", "HEAD", "PUT"] {
        let response = send(&site, method, PATH, "", None).await;
        assert_eq!(response.status().as_u16(), php("get").0);
        assert_eq!(response.headers()[header::ALLOW], "POST");
        assert_eq!(response.headers()[header::CONTENT_TYPE], "text/plain");
        assert_eq!(page_of(response).await, "");
    }
    let closed = cases()
        .into_iter()
        .find(|case| case.name == "closed")
        .unwrap();
    let response = send(&site, "POST", PATH, "", Some((FORM, closed.body))).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(page_of(response).await.contains(READ_ONLY));
    let site = unwritable(site, "wp_").await;
    let response = send(&site, "POST", PATH, "", Some((FORM, closed.body))).await;
    let (status, _, _, body) = php("closed");
    assert_eq!(response.status().as_u16(), status);
    let headers = response.headers();
    assert_eq!(headers[header::CONTENT_TYPE], "text/html; charset=UTF-8");
    assert!(
        headers[header::CACHE_CONTROL]
            .to_str()
            .unwrap()
            .contains("no-store")
    );
    assert_eq!(page_of(response).await.into_bytes(), body);
    let multipart_kind = "multipart/form-data; boundary=x";
    // `get_post( -1 )` finds nothing, and PHP exits.
    let negative = "comment_post_ID=-1&author=Ann&email=ann%40example.test&comment=Hi.";
    let response = send(&site, "POST", PATH, "", Some((FORM, negative))).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(page_of(response).await, "");
    for (body, kind) in [("garbage", multipart_kind), ("redirect_to[]=x", FORM)] {
        let response = send(&site, "POST", PATH, "", Some((kind, body))).await;
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED, "{body}");
    }
    // The connection's address is the comment's, which the database refuses.
    let held = cases()
        .into_iter()
        .find(|case| case.name == "held")
        .unwrap();
    let mut held = request(held.body, FORM);
    held.extensions_mut().insert(Peer([192, 0, 2, 1].into()));
    let response = router(Arc::clone(&site)).oneshot(held).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let site = unwritable(site, "missing_").await;
    let response = send(&site, "POST", PATH, "", Some((FORM, closed.body))).await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let page = page_of(response).await;
    assert!(
        page.contains("Error establishing a database connection"),
        "{page}"
    );
}

/// `REMOTE_ADDR` is the connection's address, or behind a proxy the last
/// entry of every `X-Forwarded-For` line, with or without a port; none,
/// never the proxy's, where that entry names no address.
#[tokio::test(flavor = "multi_thread")]
async fn the_client_is_the_proxy_s_last_hop() {
    let dir = tempfile::tempdir().unwrap();
    let (site, _) = logged_in("wordpress", dir.path()).await;
    let asked = |lines: &[&[u8]], peer: Option<IpAddr>| {
        let mut headers = HeaderMap::new();
        for line in lines {
            headers.append("x-forwarded-for", HeaderValue::from_bytes(line).unwrap());
        }
        Asked {
            post: true,
            headers,
            body: axum::body::Bytes::new(),
            peer,
        }
    };
    let peer = Some([192, 0, 2, 1].into());
    let direct = asked(&[b"198.51.100.7"], peer);
    assert_eq!(remote_addr(&site, &direct), b"192.0.2.1");
    assert_eq!(remote_addr(&site, &asked(&[], None)), b"");
    let site = Arc::try_unwrap(site)
        .ok()
        .unwrap()
        .with_proxy(crate::tests::trusted("192.0.2.0/24"));
    assert_eq!(remote_addr(&site, &direct), b"198.51.100.7");
    assert_eq!(remote_addr(&site, &asked(&[], peer)), b"192.0.2.1");
    // A peer outside `TRUSTED_PROXIES`, or none, names itself, not the header.
    let stranger = Some([203, 0, 113, 50].into());
    let forged = asked(&[b"198.51.100.7"], stranger);
    assert_eq!(remote_addr(&site, &forged), b"203.0.113.50");
    assert_eq!(remote_addr(&site, &asked(&[b"198.51.100.7"], None)), b"");
    let mapped = Some("::ffff:192.0.2.9".parse().unwrap());
    assert_eq!(
        remote_addr(&site, &asked(&[b"198.51.100.7"], mapped)),
        b"198.51.100.7"
    );
    let cases: [(&[&str], &str); 8] = [
        (&["203.0.113.9, 2001:db8::1 "], "2001:db8::1"),
        (
            &["203.0.113.9", "198.51.100.2, 198.51.100.3"],
            "198.51.100.3",
        ),
        (&["198.51.100.4:5678"], "198.51.100.4"),
        (&["[2001:db8::2]"], "2001:db8::2"),
        (&["x, [2001:db8::3]:443"], "2001:db8::3"),
        (&["198.51.100.5", "unknown"], ""),
        (&["198.51.100.6", ""], ""),
        (&["[198.51.100.7"], ""),
    ];
    for (lines, client) in cases {
        let lines: Vec<&[u8]> = lines.iter().map(|line| line.as_bytes()).collect();
        let found = remote_addr(&site, &asked(&lines, peer));
        assert_eq!(found, client.as_bytes(), "{lines:?}");
    }
    let not_text = asked(&[&b"198.51.100.8"[..], b"\xff"], peer);
    assert_eq!(remote_addr(&site, &not_text), b"");
    let unnamed = client_ip(&site, &asked(&[b"a, unknown "], peer).headers, peer);
    assert_eq!(unnamed, Err(Unnamed("unknown".into())));
}

/// A content type no header can carry reads as none, and a body without a
/// form as only PHP reads it.
#[test]
fn a_form_needs_its_content_type() {
    assert_eq!(read_form(b"\n", b"author=Ann"), None);
    assert_eq!(
        read_form(b"text/plain", b""),
        Some(Value::Array(Vec::new()))
    );
}

/// A read that fails on the way, here the current user's, shows the
/// database error page.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_read_is_the_database_error_page() {
    let mut config = crate::tests::fixture_config();
    config.prefix = hyscribe_config::Prefix::new("missing_").unwrap();
    let db = hyscribe_db::Db::connect(&config).await.unwrap();
    let writer = Writer::open(&fixture_database(), Mode::Primary).await;
    let dir = tempfile::tempdir().unwrap();
    let site = Arc::new(Site {
        db,
        options: std::collections::HashMap::new(),
        home: String::new(),
        theme: Arc::new(hyscribe_render::theme::twentytwentyfive()),
        styled: Arc::new(tokio::sync::Mutex::new((
            "twentytwentyfive".to_owned(),
            None,
            Arc::new(hyscribe_render::theme::twentytwentyfive()),
        ))),
        rewriter: Arc::default(),
        compiled: Arc::default(),
        salts: Vec::new(),
        sessions: Some(Arc::new(hyscribe_auth::sessions::Sessions::new(dir.path()))),
        writer: writer.unwrap().map(Arc::new),
        defined: Vec::new(),
        public_users: true,
        proxies: None,
        time_limit: crate::DEFAULT_TIME_LIMIT,
        constants: hyscribe_config::Constants::default(),
        limiter: Arc::new(crate::login::limits::Limiter::new(Default::default())),
        uploads: None,
        extensions: None,
        registry: None,
        running: None,
        install: crate::unconfigured(),
        fetch: None,
        feed_hosts: Arc::default(),
        embed_hosts: Arc::default(),
        mailer: Arc::new(crate::mail_off()),
        hooks: Arc::new(()),
    });
    let cookie = format!(
        "hyscribe_logged_in_{}=admin%7C9999999999%7Ctoken%7Chmac",
        hyscribe_phpser::md5(b"")
    );
    let closed = cases()
        .into_iter()
        .find(|case| case.name == "closed")
        .unwrap();
    let response = send(&site, "POST", PATH, &cookie, Some((FORM, closed.body))).await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let page = page_of(response).await;
    assert!(
        page.contains("Error establishing a database connection"),
        "{page}"
    );
}

/// The link a held comment's author is sent to shows nothing held while
/// its comment is not held (`hyscribe-render` renders a held one).
#[tokio::test(flavor = "multi_thread")]
async fn only_a_held_comment_is_previewed() {
    let site = crate::tests::oracle_site().await;
    let uri = "/?p=1&unapproved=1&moderation-hash=x";
    let response = send(&site, "GET", uri, "", None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let page = page_of(response).await;
    assert!(page.contains("id=\"comment-1\""), "{page}");
    assert!(!page.contains("awaiting moderation"));
}

/// A visitor who does not agree to cookies has them removed, else kept a
/// year; a `redirect_to` of `0` is empty, so the comment's link is used.
#[tokio::test(flavor = "multi_thread")]
async fn cookies_last_a_year_or_go() {
    let _writes = crate::admin::tests::COMMENT_ROWS.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let (site, _) = logged_in("wordpress", dir.path()).await;
    let writer = Writer::open(&fixture_database(), Mode::Primary).await;
    let writer = writer.unwrap().unwrap();
    let body = "comment_post_ID=1&author=Ann&email=ann%40example.test&comment=Here.&redirect_to=0";
    let consent = format!("{body}&wp-comment-cookies-consent=yes");
    for (body, age) in [(body, "Max-Age=0;"), (&consent, "Max-Age=31536000;")] {
        let mut conn = connect().await;
        let mut tx = conn.begin().await.unwrap();
        let response = post((&site, &writer), &mut tx, (body, ""), None).await;
        let to = response.headers()[header::LOCATION].to_str().unwrap();
        assert!(to.starts_with("http://127.0.0.1:8081/?p=1"), "{to}");
        // The author's cookie: an empty URL's is deleted either way.
        let author = response.headers().get(header::SET_COOKIE).unwrap();
        assert!(author.to_str().unwrap().contains(age), "{author:?}");
        // Removed a year back, as PHP's `time() - YEAR_IN_SECONDS`.
        assert!(!author.to_str().unwrap().contains(" 1970 "), "{author:?}");
        tx.rollback().await.unwrap();
    }
}

/// Capability checks see the post the action read in place of the site's.
#[tokio::test(flavor = "multi_thread")]
async fn capability_checks_see_the_action_s_post() {
    let site = crate::tests::oracle_site().await;
    let handle = Handle::current();
    tokio::task::spawn_blocking(move || {
        let mut reads = LoginReads::new(&site, handle, now());
        reads.action_post = Some(CapPost {
            id: 1,
            author: 1,
            status: "private".into(),
            post_type: "post".into(),
            parent: 0,
        });
        let status = |id| hyscribe_auth::caps::CapabilityData::post(&reads, id).map(|p| p.status);
        assert_eq!(status(1).as_deref(), Some("private"));
        assert_eq!(status(2).as_deref(), Some("publish"));
    })
    .await
    .unwrap();
}

/// The front end reads a held comment, here the Theme Test Data's.
#[tokio::test(flavor = "multi_thread")]
async fn held_comments_are_read() {
    let config = hyscribe_config::Database {
        name: "wordpress_ttd".into(),
        ..fixture_database()
    };
    let db = hyscribe_db::Db::connect(&config).await.unwrap();
    let site = Arc::new(Site::load(db, None, None).await.unwrap());
    let handle = Handle::current();
    tokio::task::spawn_blocking(move || {
        let reads = crate::reads::Reads::new(&site, handle);
        assert_eq!(reads.held_comment(27).map(|c| c.post_id), Some(1148));
        assert!(reads.held_comment(1).is_none());
    })
    .await
    .unwrap();
}

/// Unpaged, a comment's link is its post's, with the trailing slash of a
/// pretty permalink structure that has one.
#[tokio::test(flavor = "multi_thread")]
async fn a_comment_links_to_its_post() {
    let _writes = crate::admin::tests::COMMENT_ROWS.lock().await;
    let writer = Writer::open(&fixture_database(), Mode::Primary).await;
    let writer = writer.unwrap().unwrap();
    let mut conn = connect().await;
    let mut tx = conn.begin().await.unwrap();
    let sql = "UPDATE wp_options SET option_value = '/%postname%/' WHERE option_name = 'permalink_structure'";
    sqlx::query(sql).execute(&mut *tx).await.unwrap();
    let mut write = writer.begin_on(&mut tx).await.unwrap();
    let permalink = b"http://127.0.0.1:8081/hello-world";
    let link = comment_link(&mut write, permalink, 5, None).await.unwrap();
    assert_eq!(link, "http://127.0.0.1:8081/hello-world/#comment-5");
}

/// A comment whose thread has no top, as it or a parent is missing, has no
/// page, as PHP's `get_page_of_comment()` finds none; a page size below 1
/// puts every comment on page 1.
#[tokio::test(flavor = "multi_thread")]
async fn a_comment_without_a_thread_top_has_no_page() {
    let _writes = crate::admin::tests::COMMENT_ROWS.lock().await;
    let writer = Writer::open(&fixture_database(), Mode::Primary).await;
    let writer = writer.unwrap().unwrap();
    let mut conn = connect().await;
    let mut tx = conn.begin().await.unwrap();
    let mut write = writer.begin_on(&mut tx).await.unwrap();
    let orphan = hyscribe_db::CommentFields {
        post_id: 1,
        parent: 999_999,
        date: "2026-10-27 09:00:00".into(),
        date_gmt: "2026-10-27 09:00:00".into(),
        approved: "1".into(),
        comment_type: "comment".into(),
        ..Default::default()
    };
    let id = write.insert_comment(&orphan).await.unwrap();
    let mut page = async |id, size, threaded| {
        page_of_comment(&mut write, id, (size, threaded), None)
            .await
            .unwrap()
    };
    assert_eq!(page(999_998, 2, true).await, None);
    assert_eq!(page(id, 2, true).await, None);
    // Unthreaded, the orphan counts from its own date: one older comment.
    assert_eq!(page(id, 1, false).await, Some(2));
    assert_eq!(page(id, 0, true).await, Some(1));
}

/// Once saved, a held comment is mailed to its moderator and an approved
/// one to the post's author (ADR 0016).
#[tokio::test(flavor = "multi_thread")]
async fn saved_comments_are_mailed() {
    let _writes = crate::admin::tests::COMMENT_ROWS.lock().await;
    let db = hyscribe_db::Db::connect(&fixture_database()).await.unwrap();
    let home = Some("http://127.0.0.1:8081".to_string());
    let site = Site::load(db, home.clone(), home).await.unwrap();
    let recorder = Arc::new(crate::mail::tests::Recorder::default());
    let (site, _) = site.with_mail(&hyscribe_config::Mail::default(), recorder.clone());
    let writer = Writer::open(&fixture_database(), Mode::Primary).await;
    let writer = writer.unwrap().unwrap();
    let mut conn = connect().await;
    let mut tx = conn.begin().await.unwrap();
    let held = "comment_post_ID=1&author=Ann&email=ann%40example.test&comment=Held.";
    post((&site, &writer), &mut tx, (held, ""), None).await;
    // Past the flood check, a commenter approved before is approved again.
    let sql = "UPDATE wp_comments SET comment_date_gmt = '2000-01-01 00:00:00'";
    sqlx::query(sql).execute(&mut *tx).await.unwrap();
    let known = "comment_post_ID=1&author=A+WordPress+Commenter\
                 &email=wapuu%40wordpress.example&comment=Approved.";
    post((&site, &writer), &mut tx, (known, ""), None).await;
    let subjects: Vec<String> = recorder
        .taken()
        .into_iter()
        .map(|mail| String::from_utf8(mail.subject).unwrap())
        .collect();
    let title = "[HyScribe Default Install]";
    assert_eq!(
        subjects,
        [
            format!("{title} Please moderate: \"Hello world!\""),
            format!("{title} Comment: \"Hello world!\""),
        ]
    );
    tx.rollback().await.unwrap();
}

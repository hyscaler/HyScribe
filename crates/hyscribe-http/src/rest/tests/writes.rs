// SPDX-License-Identifier: AGPL-3.0-or-later

//! The posts' and pages' writes (MVP 4.3): PHP's answers to
//! `reference/rest/post-writes.list` sent over HTTP to `wordpress_saves`
//! with a writer, as a visitor and each role, the database loaded afresh
//! after each request that wrote; and the rows each write makes, against
//! the write oracle on the fixture.

use hyscribe_config::{Database, Mode};
use hyscribe_db::Writer;
use hyscribe_render::date::{Zone, stored_unix};
use hyscribe_rest::{Outcome, Pending, Plan};
use hyscribe_testkit::writes::{check_writes, load_default_install};
use hyscribe_testkit::{fixture_database, rest_cases};
use sqlx::Connection as _;

use super::*;

/// The scratch database the writing tests reload.
fn saves() -> Database {
    Database {
        name: "wordpress_saves".into(),
        ..fixture_database()
    }
}

/// `wordpress_saves` as the default install with `reference/rest/users.sql`'s
/// users, as the oracle recorded over it.
async fn fresh() {
    fresh_with("").await;
}

/// [`fresh`] with a list's `state`, its statements before `-- cleanup`.
async fn fresh_with(state: &str) {
    let db = saves();
    load_default_install(&db).await.unwrap();
    let options = sqlx::mysql::MySqlConnectOptions::new()
        .host(&db.host)
        .port(db.port)
        .database(&db.name)
        .username(&db.user)
        .password(db.password.expose());
    let mut conn = sqlx::MySqlConnection::connect_with(&options).await.unwrap();
    let users = include_str!("../../../../../reference/rest/users.sql");
    let state = state.split("\n-- cleanup\n").next().unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe([users, state].concat()))
        .execute(&mut conn)
        .await
        .unwrap();
}

/// The oracle's site over `wordpress_saves`, with sessions in `dir` and a
/// writer.
async fn saves_site(dir: &std::path::Path) -> Arc<Site> {
    let db = crate::Db::connect(&saves()).await.unwrap();
    let home = Some("http://127.0.0.1:8081".to_string());
    let site = Arc::new(Site::load(db, home.clone(), home).await.unwrap());
    let site = Arc::into_inner(on(site, false, dir)).unwrap();
    let writer = Writer::open(&saves(), Mode::Primary).await.unwrap();
    Arc::new(site.with_writer(writer))
}

/// The dates of `body` from `start` to `end` as `%now%`, as the golden
/// keeps them.
fn now_masked(body: &str, (start, end): (i64, i64)) -> String {
    let shaped = |date: &[u8]| {
        date.iter().enumerate().all(|(i, b)| match i {
            4 | 7 => *b == b'-',
            10 => *b == b'T',
            13 | 16 => *b == b':',
            _ => b.is_ascii_digit(),
        })
    };
    let mut out = String::new();
    let mut rest = body;
    while let Some(date) = rest.get(..19) {
        let unix = stored_unix(&date.replace('T', " "), Zone::Utc);
        if shaped(date.as_bytes()) && (start..=end).contains(&unix) {
            out.push_str("%now%");
            rest = rest.get(19..).unwrap_or_default();
            continue;
        }
        let next = rest.chars().next().unwrap();
        out.push(next);
        rest = rest.get(next.len_utf8()..).unwrap_or_default();
    }
    out + rest
}

/// PHP's answers to the posts' and pages' writes, for a visitor and every
/// role: those that write leave the rows PHP leaves (the write oracle
/// tests below), so the next request starts from the database afresh. The
/// body PHP takes with `meta` HyScribe answers 501.
#[tokio::test(flavor = "multi_thread")]
async fn post_writes_match_php() {
    let golden = include_str!("../../../../hyscribe-rest/tests/golden/post-writes.tsv");
    let meta = r#"{"title":"Meta","meta":{"footnotes":""}}"#;
    assert_eq!(writes_match_php(golden, "", &[meta]).await, (6 * 31, 6));
}

/// Form posts whose text is not valid UTF-8 are refused as PHP's `wpdb`
/// refuses them, and an `id` below 1 is not found
/// (`reference/rest/invalid-input.list`).
#[tokio::test(flavor = "multi_thread")]
async fn invalid_input_is_refused_as_php() {
    let golden = include_str!("../../../../hyscribe-rest/tests/golden/invalid-input.tsv");
    assert_eq!(writes_match_php(golden, "", &[]).await, (6 * 10, 0));
}

/// PHP's answers to the navigation menus' and synced patterns' writes, over
/// `reference/rest/site-writes.sql`, those flagged `refused` answered 501.
#[tokio::test(flavor = "multi_thread")]
async fn site_writes_match_php() {
    let golden = include_str!("../../../../hyscribe-rest/tests/golden/site-writes.tsv");
    let state = include_str!("../../../../../reference/rest/site-writes.sql");
    assert_eq!(writes_match_php(golden, state, &[]).await, (6 * 20, 6 * 3));
}

/// The cases of `golden`, recorded over `state`, sent over HTTP as each
/// identity: how many there are and how many answered 501, as those
/// `unported` and flagged `refused` must.
async fn writes_match_php(golden: &str, state: &str, unported: &[&str]) -> (usize, usize) {
    let _saves = crate::admin::tests::SAVES.lock().await;
    fresh_with(state).await;
    let dir = tempfile::tempdir().unwrap();
    let mut site = saves_site(dir.path()).await;
    let mut logins = Vec::new();
    for (who, login, id) in [
        ("administrator", "admin", 1),
        ("editor", "editor", 701),
        ("author", "author", 702),
        ("contributor", "contributor", 703),
        ("subscriber", "subscriber", 704),
    ] {
        logins.push((who, log_in_as(&site, login, id).await));
    }
    let cases = rest_cases(golden);
    let mut refused = 0;
    for case in &cases {
        let sent = String::from_utf8(case.sent.clone()).unwrap();
        let mut request = axum::http::Request::builder()
            .method(case.method.as_str())
            .uri(case.target.as_str())
            .header(header::HOST, "127.0.0.1:8081");
        let login = logins.iter().find(|(who, _)| *who == case.who);
        let nonce = match login {
            Some((_, (cookie, nonce))) => {
                request = request
                    .header(header::COOKIE, cookie)
                    .header("x-wp-nonce", nonce);
                nonce.clone()
            }
            None => String::from("%nonce%"),
        };
        for (flag, kind) in [
            ("json", "application/json"),
            ("form", "application/x-www-form-urlencoded"),
        ] {
            if case.flag(flag) {
                request = request.header(header::CONTENT_TYPE, kind);
            }
        }
        let start = now();
        let response = send(&site, request, &sent).await;
        let what = format!("{} {} {} {sent}", case.who, case.method, case.target);
        if case.flag("refused") || unported.contains(&sent.as_str()) {
            assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED, "{what}");
            refused += 1;
            continue;
        }
        let status = response.status();
        assert_eq!(status.as_u16(), case.status, "{what}");
        let mut php = case.headers.clone();
        php.sort();
        assert_eq!(kept(response.headers(), &nonce), php, "{what}");
        let ours = body(response).await.replace(&nonce, "%nonce%");
        let ours = now_masked(&ours, (start, now()));
        let php = String::from_utf8_lossy(&case.body).replace(CREDIT[0], CREDIT[1]);
        let at = ours
            .bytes()
            .zip(php.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        let from = at.saturating_sub(80);
        assert_eq!(ours.get(from..), php.get(from..), "{what}");
        // Connections opened before the tables were loaded again can
        // refuse them (1412), so the site connects afresh too.
        if status.is_success() {
            fresh_with(state).await;
            site = saves_site(dir.path()).await;
        }
    }
    (cases.len(), refused)
}

/// `target` asked of `site` by user 1 with the JSON `body`, past its
/// checks: the write it plans.
async fn planned(site: &Arc<Site>, method: &'static str, target: &str, body: &str) -> Pending {
    let (site, handle) = (Arc::clone(site), Handle::current());
    let (target, body) = (target.to_string(), body.to_string());
    tokio::task::spawn_blocking(move || {
        let mut reads = LoginReads::new(&site, handle, now());
        reads.act_as(1, Vec::new());
        let nonce = reads.nonce("wp_rest");
        let rest = Rest {
            reads,
            site: &site,
            user: 1,
            caps: OnceCell::new(),
        };
        let (path, query) = target.split_once('?').unwrap_or((&target, ""));
        let get = parse_str(query.as_bytes()).as_array().unwrap().to_vec();
        let route = hyscribe_phpser::lookup(&get, "rest_route")
            .unwrap()
            .to_php_string();
        let request = hyscribe_rest::Request {
            method,
            route: &route,
            uri: path.as_bytes(),
            get: &get,
            post: &[],
            cookie_user: 1,
            origin: None,
            nonce: Some(nonce.as_bytes()),
            method_override: None,
            content_type: Some(b"application/json"),
            body: body.as_bytes(),
            host: None,
            now: 0,
        };
        match hyscribe_rest::answer(&rest, &request) {
            hyscribe_rest::Answer::Write(pending) => *pending,
            hyscribe_rest::Answer::Served(served) => {
                panic!("{}", String::from_utf8_lossy(&served.body))
            }
        }
    })
    .await
    .unwrap()
}

/// What `plan` writes on `conn`, once each statement budget short of it
/// has failed and rolled back.
async fn acted(
    conn: &mut hyscribe_db::Connection,
    plan: &Plan,
    ctx: &hyscribe_write::posts::Context,
) -> Outcome {
    let fixture = fixture_database();
    let writer = Writer::open(&fixture, Mode::Primary)
        .await
        .unwrap()
        .unwrap();
    let mut budget = 0;
    loop {
        let mut write = writer.begin_on(conn).await;
        if let Ok(write) = &mut write {
            write.fail_after(budget);
        }
        match crate::rest::writes::act(write, plan, ctx).await {
            Err(hyscribe_write::Error::Db(err)) if err.to_string().contains("budget is spent") => {
                budget += 1;
            }
            outcome => return outcome.unwrap(),
        }
    }
}

/// Each write leaves the rows PHP's leaves (`reference/writes/actions-rest.php`),
/// and nothing when a statement fails part way.
#[tokio::test(flavor = "multi_thread")]
async fn post_writes_are_php_s() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(false, dir.path()).await;
    let writes = [
        (
            "rest-posts-create",
            "POST",
            "/?rest_route=/wp/v2/posts",
            r#"{"title":"<b>Bold</b> <script>x()</script>title","content":"<p onclick=\"y()\">Para</p><script>bad()</script>","excerpt":"<em>Kept</em><iframe src=\"https://example.test/\"></iframe>","status":"publish","slug":"rest slug","comment_status":"closed","ping_status":"closed","categories":[1],"tags":[],"date":"2026-01-02T03:04:05","featured_media":0,"template":""}"#,
        ),
        (
            "rest-posts-create-draft",
            "POST",
            "/?rest_route=/wp/v2/posts",
            r#"{"title":"A REST draft","status":"draft","slug":"hello-world"}"#,
        ),
        (
            "rest-pages-create",
            "POST",
            "/?rest_route=/wp/v2/pages",
            r#"{"title":"A REST page","content":"<!-- wp:template-part {\"slug\":\"header\"} /-->","parent":2,"menu_order":3,"status":"publish"}"#,
        ),
        (
            "rest-posts-update",
            "POST",
            "/?rest_route=/wp/v2/posts/1",
            r#"{"title":"Hello again","content":"<!-- wp:paragraph -->\n<p>Changed</p>\n<!-- /wp:paragraph -->\n\n<!-- wp:core/template-part {\"slug\":\"footer\"}   /-->","sticky":true,"categories":[1,999],"tags":[],"date_gmt":"2026-02-03T04:05:06Z"}"#,
        ),
        (
            "rest-posts-update-draft",
            "PATCH",
            "/?rest_route=/wp/v2/posts/1",
            r#"{"status":"draft","slug":"sample-page","date":null}"#,
        ),
        (
            "rest-pages-update",
            "PUT",
            "/?rest_route=/wp/v2/pages/2",
            r#"{"title":"Sample renamed","parent":0,"menu_order":5,"status":"private","password":"dropped","featured_media":1}"#,
        ),
        (
            "rest-posts-trash",
            "DELETE",
            "/?rest_route=/wp/v2/posts/1",
            "",
        ),
        (
            "rest-posts-delete",
            "DELETE",
            "/?rest_route=/wp/v2/posts/1&force=true",
            "",
        ),
        (
            "rest-pages-delete",
            "DELETE",
            "/?rest_route=/wp/v2/pages/2&force=1",
            "",
        ),
    ];
    for (golden, method, target, body) in writes {
        let pending = planned(&site, method, target, body).await;
        let nothing = async |_: &mut hyscribe_db::Connection| Ok::<_, String>(());
        let action = async |conn: &mut hyscribe_db::Connection, ()| {
            // The clock is read once the oracle's has started, as `%now%` counts from it.
            let ctx = crate::rest::writes::context(&site, &pending.plan, now());
            let outcome = acted(conn, &pending.plan, &ctx).await;
            assert!(matches!(outcome, Outcome::Written(_)), "{golden}");
            Ok(())
        };
        let report = check_writes(&fixture_database(), golden, nothing, action).await;
        report.unwrap();
    }
}

/// The navigation menus' and synced patterns' writes leave the rows PHP's
/// leave, planned over `wordpress_saves` and written on the fixture, each
/// with `reference/rest/site-writes.sql`'s posts.
#[tokio::test(flavor = "multi_thread")]
async fn site_writes_are_php_s() {
    let state = include_str!("../../../../../reference/rest/site-writes.sql");
    let insert = state
        .lines()
        .find(|line| line.starts_with("INSERT INTO `wp_posts`"))
        .unwrap();
    let footer = r#"{"title":"Footer","content":"<!-- wp:navigation-link {\"label\":\"Home\",\"url\":\"http://127.0.0.1:8081/\",\"kind\":\"custom\"} /-->","status":"publish"}"#;
    let header = r#"{"title":"Header renamed","content":"<!-- wp:navigation-link {\"label\":\"Only\",\"url\":\"https://example.test/\",\"kind\":\"custom\"} /-->"}"#;
    let pattern = r#"{"title":"A pattern","content":"<!-- wp:paragraph -->\n<p>Reused</p>\n<!-- /wp:paragraph -->","status":"publish","date":"2026-01-02T03:04:05"}"#;
    let renamed = r#"{"title":"Renamed pattern","content":"<!-- wp:paragraph -->\n<p>Changed</p>\n<!-- /wp:paragraph -->"}"#;
    let navigation = "/?rest_route=/wp/v2/navigation";
    let blocks = "/?rest_route=/wp/v2/blocks";
    let writes = [
        ("rest-navigation-create", "POST", navigation.into(), footer),
        (
            "rest-navigation-update",
            "POST",
            format!("{navigation}/851"),
            header,
        ),
        (
            "rest-navigation-publish",
            "PUT",
            format!("{navigation}/852"),
            r#"{"status":"publish","slug":"header"}"#,
        ),
        (
            "rest-navigation-trash",
            "DELETE",
            format!("{navigation}/851"),
            "",
        ),
        (
            "rest-navigation-delete",
            "DELETE",
            format!("{navigation}/852&force=true"),
            "",
        ),
        ("rest-blocks-create", "POST", blocks.into(), pattern),
        (
            "rest-blocks-update",
            "POST",
            format!("{blocks}/853"),
            renamed,
        ),
        (
            "rest-blocks-publish",
            "PATCH",
            format!("{blocks}/854"),
            r#"{"status":"publish"}"#,
        ),
        ("rest-blocks-trash", "DELETE", format!("{blocks}/853"), ""),
        (
            "rest-blocks-delete",
            "DELETE",
            format!("{blocks}/854&force=1"),
            "",
        ),
    ];
    let _saves = crate::admin::tests::SAVES.lock().await;
    fresh_with(state).await;
    let dir = tempfile::tempdir().unwrap();
    let site = saves_site(dir.path()).await;
    for (golden, method, target, body) in writes {
        let pending = planned(&site, method, &target, body).await;
        let setup = async |conn: &mut hyscribe_db::Connection| {
            if !golden.ends_with("-create") {
                let insert = sqlx::raw_sql(sqlx::AssertSqlSafe(insert));
                insert.execute(&mut *conn).await.unwrap();
            }
            Ok::<_, String>(())
        };
        let action = async |conn: &mut hyscribe_db::Connection, ()| {
            // The clock is read once the oracle's has started, as `%now%` counts from it.
            let ctx = crate::rest::writes::context(&site, &pending.plan, now());
            let outcome = acted(conn, &pending.plan, &ctx).await;
            assert!(matches!(outcome, Outcome::Written(_)), "{golden}");
            Ok(())
        };
        let report = check_writes(&fixture_database(), golden, setup, action).await;
        report.unwrap();
    }
}

/// Without a writer, a write past its checks is refused as hs-admin
/// refuses one.
#[tokio::test(flavor = "multi_thread")]
async fn shadow_mode_refuses_writes() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(false, dir.path()).await;
    let (cookie, nonce) = log_in(&site).await;
    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/?rest_route=/wp/v2/posts")
        .header(header::COOKIE, cookie)
        .header("x-wp-nonce", nonce)
        .header(header::CONTENT_TYPE, "application/json");
    let response = send(&site, request, r#"{"title":"Kept out"}"#).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let message = hyscribe_admin::login::READ_ONLY;
    let json = format!(
        r#"{{"code":"wp_die","message":"{message}","data":{{"status":403}},"additional_errors":[]}}"#
    );
    assert_eq!(body(response).await, json);
}

/// A write `hyscribe-write` does not port answers 501, one the database
/// refuses the error page, and a trash or deletion the post is already
/// past writes nothing: each leaves the database as it was.
#[tokio::test(flavor = "multi_thread")]
async fn writes_stop_short() {
    let _saves = crate::admin::tests::SAVES.lock().await;
    fresh().await;
    let dir = tempfile::tempdir().unwrap();
    let site = saves_site(dir.path()).await;
    let (cookie, nonce) = log_in(&site).await;
    let ask = |method: &str, target: &str| {
        axum::http::Request::builder()
            .method(method)
            .uri(target)
            .header(header::COOKIE, &cookie)
            .header("x-wp-nonce", &nonce)
            .header(header::CONTENT_TYPE, "application/json")
    };
    // kses refuses global styles content it does not port.
    let styles = r#"{"title":"Styles","content":"isGlobalStylesUserThemeJSON"}"#;
    let response = send(&site, ask("POST", "/?rest_route=/wp/v2/posts"), styles).await;
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    // Planned, then trashed and deleted by another request first.
    let trash = planned(&site, "DELETE", "/?rest_route=/wp/v2/posts/1", "").await;
    let delete = planned(&site, "DELETE", "/?rest_route=/wp/v2/posts/1&force=1", "").await;
    let gone = send(
        &site,
        ask("DELETE", "/?rest_route=/wp/v2/posts/1&force=1"),
        "",
    )
    .await;
    assert_eq!(gone.status(), StatusCode::OK);
    let writer = site.writer.as_ref().unwrap();
    for pending in [trash, delete] {
        let ctx = crate::rest::writes::context(&site, &pending.plan, now());
        let outcome = crate::rest::writes::act(writer.begin().await, &pending.plan, &ctx).await;
        assert_eq!(outcome.unwrap(), Outcome::Refused("rest_cannot_delete"));
    }
    // A database the writer refuses: its version is not supported.
    let mut write = writer.begin().await.unwrap();
    write.update_option("db_version", b"1").await.unwrap();
    write.commit().await.unwrap();
    let response = send(
        &site,
        ask("POST", "/?rest_route=/wp/v2/posts"),
        r#"{"title":"x"}"#,
    )
    .await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    fresh().await;
}

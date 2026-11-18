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
use hyscribe_write::terms::Taxonomy::PostTag;
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

/// A connection to `wordpress_saves`.
async fn saves_conn() -> sqlx::MySqlConnection {
    let db = saves();
    let options = sqlx::mysql::MySqlConnectOptions::new()
        .host(&db.host)
        .port(db.port)
        .database(&db.name)
        .username(&db.user)
        .password(db.password.expose());
    sqlx::MySqlConnection::connect_with(&options).await.unwrap()
}

/// The one value `sql` selects from `wordpress_saves`.
async fn stored(sql: &'static str, id: u64) -> String {
    let query = sqlx::query_scalar::<_, Vec<u8>>(sql).bind(id);
    let value = query.fetch_one(&mut saves_conn().await).await.unwrap();
    String::from_utf8(value).unwrap()
}

/// [`fresh`] with a list's `state`, its statements before `-- cleanup`.
async fn fresh_with(state: &str) {
    load_default_install(&saves()).await.unwrap();
    let mut conn = saves_conn().await;
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

/// `body` with each preview link's nonce as `%preview_nonce%`, as the
/// golden keeps it.
fn preview_masked(body: &str) -> String {
    let key = "preview_nonce=";
    let mut parts = body.split(key);
    let mut out = parts.next().unwrap_or_default().to_string();
    for part in parts {
        out.push_str(key);
        out.push_str("%preview_nonce%");
        out.push_str(part.get(10..).unwrap_or_default());
    }
    out
}

/// PHP's answers to the posts' and pages' writes, for a visitor and every
/// role: those that write leave the rows PHP leaves (the write oracle
/// tests below), so the next request starts from the database afresh. The
/// body PHP takes with `meta` HyScribe answers 501.
#[tokio::test(flavor = "multi_thread")]
async fn post_writes_match_php() {
    let golden = include_str!("../../../../hyscribe-rest/tests/golden/post-writes.tsv");
    let meta = r#"{"title":"Meta","meta":{"footnotes":""}}"#;
    assert_eq!(
        writes_match_php(golden, "", &[(meta, &[])]).await,
        (6 * 31, 6)
    );
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
    assert_eq!(writes_match_php(golden, state, &[]).await, (6 * 30, 2 * 3));
}

/// PHP's answers to the block editor's saves as its JavaScript sends them,
/// over `reference/rest/editor-saves.sql` (MVP 5.2).
#[tokio::test(flavor = "multi_thread")]
async fn editor_saves_match_php() {
    let golden = include_str!("../../../../hyscribe-rest/tests/golden/editor-saves.tsv");
    let state = include_str!("../../../../../reference/rest/editor-saves.sql");
    assert_eq!(writes_match_php(golden, state, &[]).await, (6 * 34, 0));
}

/// PHP's answers to the autosaves, over `reference/rest/autosaves.sql`,
/// with `meta`, not ported for posts, answered 501.
#[tokio::test(flavor = "multi_thread")]
async fn autosaves_match_php() {
    let golden = include_str!("../../../../hyscribe-rest/tests/golden/autosaves.tsv");
    let state = include_str!("../../../../../reference/rest/autosaves.sql");
    assert_eq!(writes_match_php(golden, state, &[]).await, (6 * 10, 6));
}

/// PHP's answers to the templates' and template parts' writes, over
/// `reference/rest/templates-custom.sql`.
#[tokio::test(flavor = "multi_thread")]
async fn template_writes_match_php() {
    let golden = include_str!("../../../../hyscribe-rest/tests/golden/template-writes.tsv");
    let state = include_str!("../../../../../reference/rest/template-writes.sql");
    assert_eq!(writes_match_php(golden, state, &[]).await, (6 * 23, 0));
}

/// The cases of `golden`, recorded over `state`, sent over HTTP as each
/// identity: how many there are and how many answered 501, as those
/// flagged `refused` and the bodies of `unported` must, to the identities
/// listed with them or to all for none.
async fn writes_match_php(
    golden: &str,
    state: &str,
    unported: &[(&str, &[&str])],
) -> (usize, usize) {
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
        for (flag, method) in [("override", "DELETE"), ("put", "PUT")] {
            if case.flag(flag) {
                request = request.header("x-http-method-override", method);
            }
        }
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
        let unported = unported.iter().any(|(body, whos)| {
            *body == sent && (whos.is_empty() || whos.contains(&case.who.as_str()))
        });
        // A request refused past its permission check is PHP's before it,
        // unless PHP answers 501 too (a trash its type does not support).
        if response.status() == StatusCode::NOT_IMPLEMENTED && case.status != 501 {
            assert!(case.flag("refused") || unported, "{what}");
            refused += 1;
            continue;
        }
        let status = response.status();
        assert_eq!(status.as_u16(), case.status, "{what}");
        let mut php = case.headers.clone();
        php.sort();
        assert_eq!(kept(response.headers(), &nonce), php, "{what}");
        let ours = body(response).await.replace(&nonce, "%nonce%");
        let ours = preview_masked(&now_masked(&ours, (start, now())));
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

/// `target` asked of `site` by user 1 with the JSON `body`, from the
/// address and user agent of `client`: what `then` makes of the request.
async fn as_admin<T: Send + 'static>(
    site: &Arc<Site>,
    asked: (&'static str, &str, &str),
    client: (&'static [u8], Option<&'static [u8]>),
    then: impl FnOnce(&Rest, &hyscribe_rest::Request) -> T + Send + 'static,
) -> T {
    as_user(site, 1, asked, client, then).await
}

/// [`as_admin`], asked by the user `user`.
async fn as_user<T: Send + 'static>(
    site: &Arc<Site>,
    user: u64,
    (method, target, body): (&'static str, &str, &str),
    client: (&'static [u8], Option<&'static [u8]>),
    then: impl FnOnce(&Rest, &hyscribe_rest::Request) -> T + Send + 'static,
) -> T {
    let (site, handle) = (Arc::clone(site), Handle::current());
    let (target, body) = (target.to_string(), body.to_string());
    tokio::task::spawn_blocking(move || {
        let mut reads = LoginReads::new(&site, handle, now());
        reads.act_as(user, Vec::new());
        let nonce = reads.nonce("wp_rest");
        let rest = Rest {
            reads,
            site: &site,
            user,
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
            cookie_user: user,
            origin: None,
            nonce: Some(nonce.as_bytes()),
            host: None,
            now: 0,
            remote_addr: client.0,
            user_agent: client.1,
            method_override: None,
            content_type: Some(b"application/json"),
            body: body.as_bytes(),
        };
        then(&rest, &request)
    })
    .await
    .unwrap()
}

/// `target` asked of `site` by user 1 with the JSON `body`, past its
/// checks: the write it plans.
async fn planned(site: &Arc<Site>, method: &'static str, target: &str, body: &str) -> Pending {
    let then = |rest: &Rest, request: &hyscribe_rest::Request| match hyscribe_rest::answer(
        rest, request,
    ) {
        hyscribe_rest::Answer::Write(pending) => *pending,
        hyscribe_rest::Answer::Served(served) => {
            panic!("{}", String::from_utf8_lossy(&served.body))
        }
    };
    as_admin(site, (method, target, body), (b"", None), then).await
}

/// The answer to `target` asked as [`planned`] once the write came to
/// `outcome`: its status and body.
async fn answered(
    site: &Arc<Site>,
    asked: (&'static str, &str, &str),
    outcome: Outcome,
) -> (u16, String) {
    let then = move |rest: &Rest, request: &hyscribe_rest::Request| {
        let hyscribe_rest::Answer::Write(pending) = hyscribe_rest::answer(rest, request) else {
            panic!("served at once")
        };
        let served = hyscribe_rest::written(rest, request, *pending, outcome);
        (served.status, String::from_utf8(served.body).unwrap())
    };
    as_admin(site, asked, (b"", None), then).await
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

/// The write oracle's `%now%` spans its action alone, so a write dated
/// when it was planned, a second or more before its action, is not PHP's:
/// the rows tests date each write as its action runs.
#[tokio::test(flavor = "multi_thread")]
async fn writes_are_dated_as_their_action_runs() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(false, dir.path()).await;
    let body = r#"{"title":"A REST draft","status":"draft","slug":"hello-world"}"#;
    let pending = planned(&site, "POST", "/?rest_route=/wp/v2/posts", body).await;
    for (shift, dated) in [(-2, false), (0, true)] {
        let nothing = async |_: &mut hyscribe_db::Connection| Ok::<_, String>(());
        let action = async |conn: &mut hyscribe_db::Connection, ()| {
            let ctx = crate::rest::writes::context(&site, &pending.plan, now() + shift);
            acted(conn, &pending.plan, &ctx).await;
            Ok(())
        };
        let golden = "rest-posts-create-draft";
        let report = check_writes(&fixture_database(), golden, nothing, action).await;
        assert_eq!(report.is_ok(), dated, "{shift}");
    }
}

/// Each write leaves the rows PHP's leaves (`reference/writes/actions-rest.php`),
/// and nothing when a statement fails part way.
#[tokio::test(flavor = "multi_thread")]
async fn post_writes_are_php_s() {
    // The editor's saves write the same rows of the fixture.
    let _saves = crate::admin::tests::SAVES.lock().await;
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
        // Dated as the action runs, which the golden's `%now%` spans.
        let action = async |conn: &mut hyscribe_db::Connection, ()| {
            let ctx = crate::rest::writes::context(&site, &pending.plan, now());
            let outcome = acted(conn, &pending.plan, &ctx).await;
            assert!(matches!(outcome, Outcome::Written(_)), "{golden}");
            Ok(())
        };
        let report = check_writes(&fixture_database(), golden, nothing, action).await;
        report.unwrap();
    }
}

/// What a write oracle action inserts of its list's state first.
#[derive(Clone, Copy, PartialEq)]
enum Setup {
    Nothing,
    Posts,
    All,
}

/// Each of `writes` leaves the rows PHP's leaves, planned over
/// `wordpress_saves` with `state` and written on the fixture after the
/// INSERTs of `state` its [`Setup`] names.
async fn writes_are_php_s(state: &str, writes: Vec<(&str, &'static str, String, &str, Setup)>) {
    let _saves = crate::admin::tests::SAVES.lock().await;
    fresh_with(state).await;
    let dir = tempfile::tempdir().unwrap();
    let site = saves_site(dir.path()).await;
    for (golden, method, target, body, inserted) in writes {
        let pending = planned(&site, method, &target, body).await;
        let inserts: Vec<String> = state
            .lines()
            .filter(|line| match inserted {
                Setup::Nothing => false,
                Setup::Posts => line.starts_with("INSERT INTO `wp_posts`"),
                Setup::All => line.starts_with("INSERT INTO "),
            })
            .map(String::from)
            .collect();
        let setup = async |conn: &mut hyscribe_db::Connection| {
            for insert in inserts {
                let insert = sqlx::raw_sql(sqlx::AssertSqlSafe(insert));
                insert.execute(&mut *conn).await.unwrap();
            }
            Ok::<_, String>(())
        };
        // Dated as the action runs, which the golden's `%now%` spans.
        let action = async |conn: &mut hyscribe_db::Connection, ()| {
            let ctx = crate::rest::writes::context(&site, &pending.plan, now());
            let outcome = acted(conn, &pending.plan, &ctx).await;
            assert!(matches!(outcome, Outcome::Written(_)), "{golden}");
            Ok(())
        };
        let report = check_writes(&fixture_database(), golden, setup, action).await;
        report.unwrap();
    }
}

/// The navigation menus' and synced patterns' writes leave the rows PHP's
/// leave, over `reference/rest/site-writes.sql`.
#[tokio::test(flavor = "multi_thread")]
async fn site_writes_are_php_s() {
    let state = include_str!("../../../../../reference/rest/site-writes.sql");
    let footer = r#"{"title":"Footer","content":"<!-- wp:navigation-link {\"label\":\"Home\",\"url\":\"http://127.0.0.1:8081/\",\"kind\":\"custom\"} /-->","status":"publish"}"#;
    let header = r#"{"title":"Header renamed","content":"<!-- wp:navigation-link {\"label\":\"Only\",\"url\":\"https://example.test/\",\"kind\":\"custom\"} /-->"}"#;
    let pattern = r#"{"title":"A pattern","content":"<!-- wp:paragraph -->\n<p>Reused</p>\n<!-- /wp:paragraph -->","status":"publish","date":"2026-01-02T03:04:05"}"#;
    let renamed = r#"{"title":"Renamed pattern","content":"<!-- wp:paragraph -->\n<p>Changed</p>\n<!-- /wp:paragraph -->"}"#;
    let in_two = r#"{"title":"In two","content":"<!-- wp:paragraph -->\n<p>Two</p>\n<!-- /wp:paragraph -->","status":"publish","wp_pattern_category":[861,862,999],"meta":{"other":"x","wp_pattern_sync_status":"partial"}}"#;
    let navigation = |path: &str| format!("/?rest_route=/wp/v2/navigation{path}");
    let blocks = |path: &str| format!("/?rest_route=/wp/v2/blocks{path}");
    let (nothing, posts, all) = (Setup::Nothing, Setup::Posts, Setup::All);
    let writes = vec![
        (
            "rest-navigation-create",
            "POST",
            navigation(""),
            footer,
            nothing,
        ),
        (
            "rest-navigation-update",
            "POST",
            navigation("/851"),
            header,
            posts,
        ),
        (
            "rest-navigation-publish",
            "PUT",
            navigation("/852"),
            r#"{"status":"publish","slug":"header"}"#,
            posts,
        ),
        (
            "rest-navigation-trash",
            "DELETE",
            navigation("/851"),
            "",
            posts,
        ),
        (
            "rest-navigation-delete",
            "DELETE",
            navigation("/852&force=true"),
            "",
            posts,
        ),
        ("rest-blocks-create", "POST", blocks(""), pattern, nothing),
        ("rest-blocks-update", "POST", blocks("/853"), renamed, posts),
        (
            "rest-blocks-publish",
            "PATCH",
            blocks("/854"),
            r#"{"status":"publish"}"#,
            posts,
        ),
        ("rest-blocks-trash", "DELETE", blocks("/853"), "", posts),
        (
            "rest-blocks-delete",
            "DELETE",
            blocks("/854&force=1"),
            "",
            posts,
        ),
        (
            "rest-blocks-create-unsynced",
            "POST",
            blocks(""),
            r#"{"title":"Unsynced","meta":{"wp_pattern_sync_status":"unsynced"}}"#,
            nothing,
        ),
        (
            "rest-blocks-create-categorised",
            "POST",
            blocks(""),
            in_two,
            all,
        ),
        (
            "rest-blocks-categories",
            "POST",
            blocks("/853"),
            r#"{"wp_pattern_category":[862]}"#,
            all,
        ),
        (
            "rest-blocks-draft-meta",
            "POST",
            blocks("/854"),
            r#"{"meta":{"wp_pattern_sync_status":"partial"},"wp_pattern_category":[861]}"#,
            all,
        ),
        (
            "rest-blocks-delete-categorised",
            "DELETE",
            blocks("/853&force=true"),
            "",
            all,
        ),
    ];
    writes_are_php_s(state, writes).await;
}

/// Each of `writes`, a golden of the write oracle and the request that
/// makes it, leaves the rows PHP's leaves, and nothing when a statement
/// fails part way.
async fn planned_writes_are_php_s(site: &Arc<Site>, writes: &[(&str, &'static str, &str, &str)]) {
    for &(golden, method, target, body) in writes {
        let pending = planned(site, method, target, body).await;
        let ctx = crate::rest::writes::context(site, &pending.plan, now());
        let nothing = async |_: &mut hyscribe_db::Connection| Ok::<_, String>(());
        let action = async |conn: &mut hyscribe_db::Connection, ()| {
            let outcome = acted(conn, &pending.plan, &ctx).await;
            let written = matches!(outcome, Outcome::Written(_) | Outcome::Cached(_));
            assert!(written, "{golden}");
            Ok(())
        };
        let report = check_writes(&fixture_database(), golden, nothing, action).await;
        report.unwrap();
    }
}

/// PHP's answers to the categories' and tags' writes, as
/// [`writes_match_php`] replays them.
#[tokio::test(flavor = "multi_thread")]
async fn term_writes_match_php() {
    let golden = include_str!("../../../../hyscribe-rest/tests/golden/term-writes.tsv");
    let state = include_str!("../../../../../reference/rest/term-writes.sql");
    let unported = [(r#"{"name":"Meta","meta":{}}"#, &[][..])];
    assert_eq!(
        writes_match_php(golden, state, &unported).await,
        (6 * 31, 6)
    );
}

/// PHP's answers to the comments' writes, as [`post_writes_match_php`]
/// replays them, from 127.0.0.1 with no user agent.
#[tokio::test(flavor = "multi_thread")]
async fn comment_writes_match_php() {
    let golden = include_str!("../../../../hyscribe-rest/tests/golden/comment-writes.tsv");
    let state = include_str!("../../../../../reference/rest/comment-writes.sql");
    let unported = [
        r#"{"post":1,"content":"Note","type":"note"}"#,
        r#"{"post":1,"content":"Addressed","author_ip":"10.0.0.1"}"#,
        r#"{"post":1,"content":"Meta","meta":{"_wp_note_status":"resolved"}}"#,
    ];
    let unported = unported.map(|body| (body, &[][..]));
    assert_eq!(
        writes_match_php(golden, state, &unported).await,
        (6 * 37, 18)
    );
}

/// PHP's answers to the users' writes, as [`post_writes_match_php`]
/// replays them: `meta` and a password of one's own answer 501.
#[tokio::test(flavor = "multi_thread")]
async fn user_writes_match_php() {
    let golden = include_str!("../../../../hyscribe-rest/tests/golden/user-writes.tsv");
    let logged_in = [
        "administrator",
        "editor",
        "author",
        "contributor",
        "subscriber",
    ];
    let unported = [
        (
            r#"{"username":"meta","email":"meta@example.test","password":"pw","meta":{}}"#,
            &[][..],
        ),
        (r#"{"meta":{"persisted_preferences":{}}}"#, &[][..]),
        (r#"{"password":"my-new-secret"}"#, &logged_in[..]),
        (r#"{"password":"new-secret"}"#, &["author"][..]),
    ];
    assert_eq!(writes_match_php(golden, "", &unported).await, (6 * 33, 18));
}

/// PHP's answers to the settings' writes, as [`post_writes_match_php`]
/// replays them.
#[tokio::test(flavor = "multi_thread")]
async fn settings_writes_match_php() {
    let golden = include_str!("../../../../hyscribe-rest/tests/golden/settings-writes.tsv");
    assert_eq!(writes_match_php(golden, "", &[]).await, (6 * 17, 0));
}

/// The comments' writes leave the rows PHP's leave.
#[tokio::test(flavor = "multi_thread")]
async fn comment_writes_are_php_s() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(false, dir.path()).await;
    let writes = [
        (
            "rest-comments-create",
            "POST",
            "/?rest_route=/wp/v2/comments",
            r#"{"post":1,"parent":1,"content":"<p onclick=\"x()\">Hi</p> <a href=\"https://example.test/\">link</a>","author_name":"Named <b>one</b>","author_email":"named@example.test","author_url":"https://named.example.test/","date":"2026-01-02T03:04:05","status":"hold"}"#,
        ),
        (
            "rest-comments-update",
            "POST",
            "/?rest_route=/wp/v2/comments/1",
            r#"{"content":"Edited <script>x()</script>","author_name":"Renamed","status":"spam","date_gmt":"2026-02-03T04:05:06Z"}"#,
        ),
        (
            "rest-comments-trash",
            "DELETE",
            "/?rest_route=/wp/v2/comments/1",
            "",
        ),
        (
            "rest-comments-delete",
            "DELETE",
            "/?rest_route=/wp/v2/comments/1&force=true",
            "",
        ),
    ];
    planned_writes_are_php_s(&site, &writes).await;
}

/// The users' writes leave the rows PHP's leave; the update and delete
/// are of an author the setup adds, the delete's post passed to user 1.
#[tokio::test(flavor = "multi_thread")]
async fn user_writes_are_php_s() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(false, dir.path()).await;
    let create = (
        "rest-users-create",
        "POST",
        "/?rest_route=/wp/v2/users",
        r#"{"username":"newbie","email":"newbie@example.test","password":"secret-pass","name":"New Bie","first_name":"New","description":"<b>Hi</b><script>x()</script>","slug":"New Bie","roles":["editor","author"]}"#,
    );
    planned_writes_are_php_s(&site, &[create]).await;
    let writer = Writer::open(&fixture_database(), Mode::Primary)
        .await
        .unwrap()
        .unwrap();
    // The oracle's author, the author of post 1 for the delete.
    let author = async |conn: &mut hyscribe_db::Connection, post: bool| {
        let mut write = writer.begin_on(conn).await.unwrap();
        let some = |text: &str| Some(text.as_bytes().to_vec());
        let data = hyscribe_write::users::UserData {
            user_login: some("oracle-author"),
            user_pass: some("A pass phrase"),
            user_email: some("oracle-author@example.test"),
            role: some("author"),
            ..hyscribe_write::users::UserData::default()
        };
        let id = hyscribe_write::users::wp_insert_user(&mut write, &data);
        let id = id.await.unwrap();
        write.commit().await.unwrap();
        if post {
            let author = sqlx::query("UPDATE wp_posts SET post_author = ? WHERE ID = 1").bind(id);
            author.execute(&mut *conn).await.unwrap();
        }
        Ok::<_, String>(id)
    };
    let pending = planned(&site, "PUT", "/?rest_route=/wp/v2/categories/1", "{}").await;
    let some = |text: &str| Some(text.as_bytes().to_vec());
    let update = |id| hyscribe_rest::UserSave {
        id,
        display_name: some("Author Renamed"),
        nickname: some("Nick"),
        email: some("renamed@example.test"),
        url: some("https://a.example.test"),
        description: some("Bio <em>here</em>"),
        pass: some("new-secret"),
        roles: Some(vec![b"subscriber".to_vec()]),
        ..hyscribe_rest::UserSave::default()
    };
    for (golden, post) in [("rest-users-update", false), ("rest-users-delete", true)] {
        let mut plan = pending.plan.clone();
        let action = async |conn: &mut hyscribe_db::Connection, id| {
            plan.write = match post {
                false => hyscribe_rest::Write::User(Box::new(update(id))),
                true => hyscribe_rest::Write::DeleteUser(id, Some(1)),
            };
            let ctx = crate::rest::writes::context(&site, &plan, now());
            assert_eq!(acted(conn, &plan, &ctx).await, Outcome::Written(id));
            Ok(())
        };
        let setup = async |conn: &mut hyscribe_db::Connection| author(conn, post).await;
        let report = check_writes(&fixture_database(), golden, setup, action).await;
        report.unwrap();
    }
}

/// The write a request plans.
fn plan_of(rest: &Rest, request: &hyscribe_rest::Request) -> hyscribe_rest::Write {
    match hyscribe_rest::answer(rest, request) {
        hyscribe_rest::Answer::Write(pending) => pending.plan.write,
        hyscribe_rest::Answer::Served(served) => {
            panic!("{}", String::from_utf8_lossy(&served.body))
        }
    }
}

/// A comment stored without a type.
const UNTYPED: &str = "INSERT INTO wp_comments (comment_ID, comment_post_ID, comment_author, comment_author_email, comment_author_url, comment_author_IP, comment_date, comment_date_gmt, comment_content, comment_approved, comment_agent, comment_type) VALUES (90, 1, '', '', '', '', '2026-11-02 00:00:00', '2026-11-02 00:00:00', 'Untyped', '1', '', '');";

/// A comment's author URL longer than its column takes.
const LONG_URL: &str = concat!(
    r#"{"author_url":"https://x.test/"#,
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    r#""}"#
);

/// The errors PHP answers where a write function refused, which no
/// request of the goldens reaches.
#[tokio::test(flavor = "multi_thread")]
async fn refused_writes_are_answered_as_php_answers_them() {
    let _saves = crate::admin::tests::SAVES.lock().await;
    let terms = include_str!("../../../../../reference/rest/term-writes.sql");
    fresh_with(&format!("{terms}\n{UNTYPED}")).await;
    let dir = tempfile::tempdir().unwrap();
    let site = saves_site(dir.path()).await;
    let error = |code: &str, message: &str, status: Option<u16>| {
        let data = status.map_or("null".to_string(), |status| {
            format!(r#"{{"status":{status}}}"#)
        });
        let code = format!(r#"{{"code":"{code}","message":"{message}","data":{data}}}"#);
        (status.unwrap_or(500), code)
    };
    let term = ("POST", "/?rest_route=/wp/v2/tags/2", r#"{"name":"x"}"#);
    for (code, message) in [
        ("invalid_term_name", "Invalid term name."),
        ("missing_parent", "Parent term does not exist."),
        ("invalid_term", "Empty Term."),
    ] {
        let refused = answered(&site, term, Outcome::Refused(code)).await;
        assert_eq!(refused, error(code, message, None));
    }
    let deleted = ("DELETE", "/?rest_route=/wp/v2/tags/2&force=1", "");
    let cannot = |what: &str| {
        error(
            "rest_cannot_delete",
            &format!("The {what} cannot be deleted."),
            Some(500),
        )
    };
    let refused = answered(&site, deleted, Outcome::Refused("rest_cannot_delete")).await;
    assert_eq!(refused, cannot("term"));
    let comment = (
        "POST",
        "/?rest_route=/wp/v2/comments",
        r#"{"post":1,"content":"x"}"#,
    );
    let flood = "You are posting comments too quickly. Slow down.";
    let refused = answered(&site, comment, Outcome::Refused("comment_flood")).await;
    assert_eq!(refused, error("comment_flood", flood, Some(400)));
    let edit = (
        "POST",
        "/?rest_route=/wp/v2/comments/1",
        r#"{"content":"x"}"#,
    );
    let refused = answered(&site, edit, Outcome::Refused("rest_comment_failed_edit")).await;
    assert_eq!(
        refused,
        error(
            "rest_comment_failed_edit",
            "Updating comment failed.",
            Some(500)
        )
    );
    let trash = ("DELETE", "/?rest_route=/wp/v2/comments/1", "");
    let refused = answered(&site, trash, Outcome::Refused("rest_cannot_delete")).await;
    assert_eq!(refused, cannot("comment"));
    let user = ("POST", "/?rest_route=/wp/v2/users/702", r#"{"name":"x"}"#);
    for (code, message) in [
        (
            "empty_user_login",
            "Cannot create a user with an empty login name.",
        ),
        (
            "user_login_too_long",
            "Username may not be longer than 60 characters.",
        ),
        (
            "empty_user_nicename",
            "Cannot create a user with an empty nicename.",
        ),
        (
            "user_nicename_too_long",
            "Nicename may not be longer than 50 characters.",
        ),
        (
            "user_url_too_long",
            "User URL may not be longer than 100 characters.",
        ),
        ("invalid_user_id", "Invalid user ID."),
    ] {
        let refused = answered(&site, user, Outcome::Refused(code)).await;
        assert_eq!(refused, error(code, message, None));
    }
    let delete = (
        "DELETE",
        "/?rest_route=/wp/v2/users/704&force=1&reassign=1",
        "",
    );
    let refused = answered(&site, delete, Outcome::Refused("rest_cannot_delete")).await;
    assert_eq!(refused, cannot("user"));
    // Content without a raw text gives none, so an update keeps its own.
    let raw = (
        "POST",
        "/?rest_route=/wp/v2/comments/1",
        r#"{"content":{"rendered":"x"}}"#,
    );
    let write = as_admin(&site, raw, (b"", None), plan_of).await;
    let hyscribe_rest::Write::Comment(save) = write else {
        unreachable!()
    };
    assert_eq!(save.content, None);
    // Answered at once: an author by URL alone, and a slug another user has.
    for (asked, code) in [
        (
            (
                "POST",
                "/?rest_route=/wp/v2/comments",
                r#"{"post":1,"content":"x","author_name":"","author_url":"https://x.test"}"#,
            ),
            "rest_comment_author_data_required",
        ),
        (
            (
                "PATCH",
                "/?rest_route=/wp/v2/users/702",
                r#"{"slug":"editor"}"#,
            ),
            "rest_user_invalid_slug",
        ),
        (
            (
                "DELETE",
                "/?rest_route=/wp/v2/users/me&force=true&reassign=1&id=x",
                "",
            ),
            "rest_user_invalid_reassign",
        ),
        (
            ("DELETE", "/?rest_route=/wp/v2/tags/2&force=maybe", ""),
            "rest_invalid_param",
        ),
        (
            ("DELETE", "/?rest_route=/wp/v2/posts/1&force=maybe", ""),
            "rest_invalid_param",
        ),
        (
            ("DELETE", "/?rest_route=/wp/v2/comments/1&force=maybe", ""),
            "rest_invalid_param",
        ),
        (
            (
                "POST",
                "/?rest_route=/wp/v2/comments",
                r#"{"post":1,"content":"x","author_name":"","author_email":"e@example.test"}"#,
            ),
            "rest_comment_author_data_required",
        ),
        (
            (
                "POST",
                "/?rest_route=/wp/v2/comments/1",
                r#"{"author":999}"#,
            ),
            "rest_comment_author_invalid",
        ),
        (
            ("POST", "/?rest_route=/wp/v2/comments/1", LONG_URL),
            "comment_author_url_column_length",
        ),
        (
            (
                "POST",
                "/?rest_route=/wp/v2/categories",
                r#"{"name":"x","parent":-1}"#,
            ),
            "rest_term_invalid",
        ),
    ] {
        let served = |rest: &Rest, request: &hyscribe_rest::Request| match hyscribe_rest::answer(
            rest, request,
        ) {
            hyscribe_rest::Answer::Served(served) => String::from_utf8(served.body).unwrap(),
            hyscribe_rest::Answer::Write(_) => unreachable!(),
        };
        let body = as_admin(&site, asked, (b"", None), served).await;
        assert!(body.contains(code), "{body}");
    }
    // A slug no other user has is planned.
    let slug = (
        "PATCH",
        "/?rest_route=/wp/v2/users/702",
        r#"{"slug":"fresh-slug"}"#,
    );
    as_admin(&site, slug, (b"", None), plan_of).await;
    // One's own role may be a role that edits users.
    let roles = (
        "POST",
        "/?rest_route=/wp/v2/users/1",
        r#"{"roles":["administrator"]}"#,
    );
    as_admin(&site, roles, (b"", None), plan_of).await;
    // An author URL as long as its column takes is planned.
    let url = format!(r#"{{"author_url":"https://x.test/{}"}}"#, "a".repeat(185));
    let long = ("POST", "/?rest_route=/wp/v2/comments/1", url.as_str());
    as_admin(&site, long, (b"", None), plan_of).await;
    // An editor's comment skips the flood check, as a moderator's.
    let comment = (
        "POST",
        "/?rest_route=/wp/v2/comments",
        r#"{"post":1,"content":"x"}"#,
    );
    let hyscribe_rest::Write::Comment(save) =
        as_user(&site, 701, comment, (b"", None), plan_of).await
    else {
        unreachable!()
    };
    assert!(save.skip_flood);
    // A comment stored without a type is a comment.
    let untyped = (
        "POST",
        "/?rest_route=/wp/v2/comments/90",
        r#"{"type":"comment"}"#,
    );
    as_admin(&site, untyped, (b"", None), plan_of).await;
    // A comment takes the request's address and user agent.
    let client = (&b"192.0.2.1"[..], Some(&b"Agent/2.0"[..]));
    let plan = |rest: &Rest, request: &hyscribe_rest::Request| match hyscribe_rest::answer(
        rest, request,
    ) {
        hyscribe_rest::Answer::Write(pending) => pending.plan.write,
        hyscribe_rest::Answer::Served(_) => unreachable!(),
    };
    let write = as_admin(&site, comment, client, plan).await;
    let hyscribe_rest::Write::Comment(save) = write else {
        unreachable!()
    };
    assert_eq!(
        (&save.author_ip[..], save.agent.as_deref()),
        (&b"192.0.2.1"[..], Some(&b"Agent/2.0"[..]))
    );
}

/// What the write functions refuse past the checks, as another request
/// gets in first, and each status a comment's write may ask for.
#[tokio::test(flavor = "multi_thread")]
async fn writes_refused_past_their_checks() {
    let _saves = crate::admin::tests::SAVES.lock().await;
    fresh().await;
    let dir = tempfile::tempdir().unwrap();
    let site = saves_site(dir.path()).await;
    let pending = planned(&site, "PUT", "/?rest_route=/wp/v2/categories/1", "{}").await;
    let writer = site.writer.as_ref().unwrap();
    let act = async |write: hyscribe_rest::Write| {
        let mut plan = pending.plan.clone();
        plan.write = write;
        let ctx = crate::rest::writes::context(&site, &plan, now());
        crate::rest::writes::act(writer.begin().await, &plan, &ctx)
            .await
            .unwrap()
    };
    let term = |id, name: &str, parent| {
        let save = hyscribe_rest::TermSave {
            taxonomy: "category",
            id,
            name: Some(name.as_bytes().to_vec()),
            parent,
            ..hyscribe_rest::TermSave::default()
        };
        hyscribe_rest::Write::Term(Box::new(save))
    };
    for (write, refused) in [
        (term(0, "<b></b>", None), "invalid_term_name"),
        (term(0, "Orphan", Some(999)), "missing_parent"),
        (term(999, "Gone", None), "invalid_term"),
    ] {
        assert_eq!(act(write).await, Outcome::Refused(refused));
    }
    let comment = |id, status: &str| {
        let save = hyscribe_rest::CommentSave {
            id,
            post_id: Some(1),
            author: Some(b"Reader".to_vec()),
            author_email: Some(b"reader@example.test".to_vec()),
            author_ip: b"192.0.2.9".to_vec(),
            content: (id == 0).then(|| format!("A comment to {status}").into_bytes()),
            status: Some(status.as_bytes().to_vec()),
            ..hyscribe_rest::CommentSave::default()
        };
        hyscribe_rest::Write::Comment(Box::new(save))
    };
    let failed = Outcome::Refused("rest_comment_failed_edit");
    assert_eq!(act(comment(999, "hold")).await, failed);
    let Outcome::Written(id) = act(comment(0, "trash")).await else {
        panic!("not written")
    };
    let approved = "SELECT comment_approved FROM wp_comments WHERE comment_ID = ?";
    assert_eq!(stored(approved, id).await, "trash");
    for (status, now) in [
        ("untrash", "0"),
        ("spam", "spam"),
        ("unspam", "0"),
        ("approved", "1"),
        ("approved", "1"),
        ("hold", "0"),
        ("approve", "1"),
        ("trash", "trash"),
        ("bogus", "trash"),
    ] {
        assert_eq!(act(comment(id, status)).await, Outcome::Written(id));
        assert_eq!(stored(approved, id).await, now, "{status}");
    }
    // An update's agent, parent and author, and a new comment's dates.
    let edit = hyscribe_rest::CommentSave {
        id,
        parent: Some(1),
        user_id: Some(704),
        agent: Some(b"Agent/3.0".to_vec()),
        ..hyscribe_rest::CommentSave::default()
    };
    act(hyscribe_rest::Write::Comment(Box::new(edit))).await;
    let row = "SELECT CONCAT_WS('|', comment_agent, comment_parent, user_id) FROM wp_comments WHERE comment_ID = ?";
    assert_eq!(stored(row, id).await, "Agent/3.0|1|704");
    let dated = hyscribe_rest::CommentSave {
        post_id: Some(1),
        author: Some(b"Dated".to_vec()),
        author_email: Some(b"dated@example.test".to_vec()),
        content: Some(b"Dated".to_vec()),
        date: Some(("2026-01-02 05:04:05".into(), "2026-01-02 03:04:05".into())),
        skip_flood: true,
        ..hyscribe_rest::CommentSave::default()
    };
    let Outcome::Written(dated) = act(hyscribe_rest::Write::Comment(Box::new(dated))).await else {
        panic!("not written")
    };
    let dates = "SELECT CONCAT_WS('|', comment_date, comment_date_gmt) FROM wp_comments WHERE comment_ID = ?";
    assert_eq!(
        stored(dates, dated).await,
        "2026-01-02 05:04:05|2026-01-02 03:04:05"
    );
    // A user's password and locale.
    let user = hyscribe_rest::UserSave {
        id: 704,
        pass: Some(b"new-secret".to_vec()),
        locale: Some(b"en_US".to_vec()),
        ..hyscribe_rest::UserSave::default()
    };
    assert_eq!(
        act(hyscribe_rest::Write::User(Box::new(user))).await,
        Outcome::Written(704)
    );
    let pass = stored("SELECT user_pass FROM wp_users WHERE ID = ?", 704).await;
    assert!(hyscribe_auth::check_password(
        b"new-secret",
        pass.as_bytes()
    ));
    let locale = "SELECT meta_value FROM wp_usermeta WHERE user_id = ? AND meta_key = 'locale'";
    assert_eq!(stored(locale, 704).await, "en_US");
    let flood = Outcome::Refused("comment_flood");
    assert_eq!(act(comment(0, "hold")).await, flood);
    // A text setting given an array, which sanitize_option() does not port.
    let setting = hyscribe_rest::Setting {
        option: "date_format".into(),
        value: Some(hyscribe_phpser::Value::Array(Vec::new())),
        default: None,
    };
    let save = hyscribe_rest::SettingsSave {
        options: vec![setting],
        invalid: None,
    };
    let mut plan = pending.plan.clone();
    plan.write = hyscribe_rest::Write::Settings(Box::new(save));
    let ctx = crate::rest::writes::context(&site, &plan, now());
    let unported = crate::rest::writes::act(writer.begin().await, &plan, &ctx).await;
    assert!(matches!(unported, Err(hyscribe_write::Error::Unported(_))));
    fresh().await;
}

/// A setting not stored reads as its registered default, which a value
/// equal to it leaves unwritten, as `update_option()` compares them.
#[tokio::test(flavor = "multi_thread")]
async fn settings_not_stored_read_as_their_default() {
    let _saves = crate::admin::tests::SAVES.lock().await;
    fresh_with("DELETE FROM wp_options WHERE option_name = 'use_smilies';").await;
    let dir = tempfile::tempdir().unwrap();
    let site = saves_site(dir.path()).await;
    let body = r#"{"use_smilies":true}"#;
    let pending = planned(&site, "POST", "/?rest_route=/wp/v2/settings", body).await;
    let ctx = crate::rest::writes::context(&site, &pending.plan, now());
    let writer = site.writer.as_ref().unwrap();
    let outcome = crate::rest::writes::act(writer.begin().await, &pending.plan, &ctx).await;
    assert_eq!(outcome.unwrap(), Outcome::Cached(Vec::new()));
    fresh().await;
}

/// The terms' and settings' writes leave the rows PHP's leave.
#[tokio::test(flavor = "multi_thread")]
async fn term_and_settings_writes_are_php_s() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(false, dir.path()).await;
    let writes = [
        (
            "rest-categories-create",
            "POST",
            "/?rest_route=/wp/v2/categories",
            r#"{"name":"News","description":"<b>Fresh</b> <script>x()</script>","slug":"the news","parent":1}"#,
        ),
        (
            "rest-categories-update",
            "PUT",
            "/?rest_route=/wp/v2/categories/1",
            r#"{"name":"Renamed","description":"Desc","slug":"renamed"}"#,
        ),
        (
            "rest-settings-update",
            "POST",
            "/?rest_route=/wp/v2/settings",
            r#"{"title":"New <b>title</b> & co","posts_per_page":null,"use_smilies":false,"start_of_week":"3","timezone":"Europe/Paris","site_logo":5,"language":""}"#,
        ),
    ];
    planned_writes_are_php_s(&site, &writes).await;
    // A tag the setup adds to post 1, deleted.
    let mut pending = planned(&site, "PUT", "/?rest_route=/wp/v2/categories/1", "{}").await;
    let writer = Writer::open(&fixture_database(), Mode::Primary)
        .await
        .unwrap()
        .unwrap();
    let setup = async |conn: &mut hyscribe_db::Connection| {
        let mut write = writer.begin_on(conn).await.unwrap();
        let args = hyscribe_write::terms::TermArgs::default();
        let tag = hyscribe_write::terms::wp_insert_term(&mut write, PostTag, b"Tagged", &args);
        let tag = tag.await.unwrap().term_id;
        let terms = [hyscribe_write::terms::TermRef::Id(tag)];
        let set = hyscribe_write::terms::wp_set_object_terms(&mut write, 1, &terms, PostTag, false);
        set.await.unwrap();
        write.commit().await.unwrap();
        Ok::<_, String>(tag)
    };
    let action = async |conn: &mut hyscribe_db::Connection, tag| {
        pending.plan.write = hyscribe_rest::Write::DeleteTerm("post_tag", tag);
        let ctx = crate::rest::writes::context(&site, &pending.plan, now());
        let outcome = acted(conn, &pending.plan, &ctx).await;
        assert_eq!(outcome, Outcome::Written(tag));
        Ok(())
    };
    let report = check_writes(&fixture_database(), "rest-tags-delete", setup, action).await;
    report.unwrap();
}

/// The block editor's saves leave the rows PHP's leave
/// (`reference/writes/actions-editor.php`), each over
/// `reference/rest/editor-saves.sql`.
#[tokio::test(flavor = "multi_thread")]
async fn editor_saves_are_php_s() {
    let state = include_str!("../../../../../reference/rest/editor-saves.sql");
    let para =
        |text: &str| format!(r#"<!-- wp:paragraph -->\n<p>{text}</p>\n<!-- /wp:paragraph -->"#);
    let hello = para(
        "Welcome to WordPress. This is your first post. Edit or delete it, then start writing!",
    );
    let (drafted, friends) = (para("Drafted in the editor."), para("For friends."));
    let post = |id: u64| format!("/?rest_route=/wp/v2/posts/{id}");
    let saves = [
        (
            "editor-save-draft",
            post(9601),
            format!(
                r#"{{"id":9601,"title":"First draft","content":"{drafted}","status":"draft"}}"#
            ),
        ),
        (
            "editor-publish",
            post(9601),
            format!(
                r#"{{"id":9601,"title":"Published from the editor","content":"{drafted}","status":"publish"}}"#
            ),
        ),
        (
            "editor-schedule",
            post(9601),
            format!(
                r#"{{"id":9601,"title":"Scheduled","content":"{drafted}","date":"2030-06-01T09:30:00","status":"future"}}"#
            ),
        ),
        (
            "editor-update",
            post(1),
            format!(
                r#"{{"id":1,"title":"Hello edited","content":"{}"}}"#,
                para("Edited in the editor.")
            ),
        ),
        (
            "editor-unschedule",
            post(9604),
            format!(
                r#"{{"id":9604,"status":"draft","date":null,"content":"{}"}}"#,
                para("Coming up.")
            ),
        ),
        (
            "editor-private",
            post(9606),
            format!(r#"{{"id":9606,"status":"private","password":"","content":"{friends}"}}"#),
        ),
        (
            "editor-featured-image",
            post(1),
            format!(r#"{{"id":1,"featured_media":9605,"content":"{hello}"}}"#),
        ),
        (
            "editor-format",
            post(1),
            format!(r#"{{"id":1,"format":"gallery","content":"{hello}"}}"#),
        ),
        (
            "editor-format-standard",
            post(9606),
            format!(r#"{{"id":9606,"format":"standard","content":"{friends}"}}"#),
        ),
        (
            "editor-terms",
            post(1),
            format!(r#"{{"id":1,"categories":[1,9602],"tags":[9601],"content":"{hello}"}}"#),
        ),
        (
            "editor-page-template",
            "/?rest_route=/wp/v2/pages/2".into(),
            r#"{"id":2,"template":"page-no-title","parent":9607}"#.into(),
        ),
    ];
    let writes = saves
        .iter()
        .map(|(golden, target, body)| (*golden, "PUT", target.clone(), body.as_str(), Setup::All))
        .collect();
    writes_are_php_s(state, writes).await;
}

/// The templates' and parts' writes leave the rows PHP's leave, over
/// `reference/rest/templates-custom.sql`.
#[tokio::test(flavor = "multi_thread")]
async fn template_writes_are_php_s() {
    let state = include_str!("../../../../../reference/rest/template-writes.sql");
    let paragraph =
        |text: &str| format!(r#""<!-- wp:paragraph -->\n<p>{text}</p>\n<!-- /wp:paragraph -->""#);
    let templates = |path: &str| format!("/?rest_route=/wp/v2/templates{path}");
    let parts = |path: &str| format!("/?rest_route=/wp/v2/template-parts{path}");
    let mine = format!(
        r#"{{"slug":"my-template","title":"My template","content":{},"description":"Made here"}}"#,
        paragraph("Mine")
    );
    let side = format!(
        r#"{{"slug":"my-sidebar","title":"My sidebar","area":"header","content":{}}}"#,
        paragraph("Side")
    );
    let customised = format!(r#"{{"content":{}}}"#, paragraph("Customised"));
    let foot = format!(r#"{{"content":{}}}"#, paragraph("Foot"));
    let again = format!(
        r#"{{"title":"Index again","content":{}}}"#,
        paragraph("Changed")
    );
    let index = "/twentytwentyfive//index";
    let writes = vec![
        (
            "rest-templates-create",
            "POST",
            templates(""),
            mine.as_str(),
        ),
        (
            "rest-templates-create-suggested",
            "POST",
            templates(""),
            r#"{"slug":"page-about","title":{"raw":"About"},"content":{"raw":"<!-- wp:post-content /-->"},"author":1,"is_wp_suggestion":true}"#,
        ),
        (
            "rest-templates-create-taken",
            "POST",
            templates(""),
            r#"{"slug":"my-custom","title":"Again","content":"<!-- wp:post-title /-->"}"#,
        ),
        ("rest-template-parts-create", "POST", parts(""), &side),
        (
            "rest-template-parts-create-area",
            "POST",
            parts(""),
            r#"{"slug":"odd-part","title":"Odd","area":"bogus"}"#,
        ),
        (
            "rest-templates-customise",
            "POST",
            templates("/twentytwentyfive//page"),
            &customised,
        ),
        (
            "rest-template-parts-customise",
            "PUT",
            parts("/twentytwentyfive//footer"),
            &foot,
        ),
        ("rest-templates-update", "POST", templates(index), &again),
        (
            "rest-template-parts-update",
            "PATCH",
            parts("/twentytwentyfive//header"),
            r#"{"area":"footer","description":"Moved"}"#,
        ),
        (
            "rest-templates-revert",
            "POST",
            templates("/twentytwentyfive//page-no-title"),
            r#"{"source":"theme"}"#,
        ),
        (
            "rest-templates-trash",
            "DELETE",
            templates("/twentytwentyfive//my-custom"),
            "",
        ),
        (
            "rest-templates-delete",
            "DELETE",
            templates("/twentytwentyfive//my-custom&force=true"),
            "",
        ),
        (
            "rest-template-parts-delete",
            "DELETE",
            parts("/twentytwentyfive//header&force=1"),
            "",
        ),
    ];
    let writes = writes
        .into_iter()
        .map(|(golden, method, target, body)| (golden, method, target, body, Setup::All))
        .collect();
    writes_are_php_s(state, writes).await;
}

/// The autosaves leave the rows PHP's leave, over
/// `reference/rest/autosaves.sql`: a draft of the user's updated, their
/// old autosave updated, and a new one.
#[tokio::test(flavor = "multi_thread")]
async fn autosaves_are_php_s() {
    let state = include_str!("../../../../../reference/rest/autosaves.sql");
    let saved = |title: &str, text: &str, excerpt: &str| {
        format!(
            r#"{{"title":"{title}","content":"<!-- wp:paragraph -->\n<p>{text}</p>\n<!-- /wp:paragraph -->"{excerpt}}}"#
        )
    };
    let autosaves = |path: &str| format!("/?rest_route=/wp/v2/{path}/autosaves");
    let page = saved("Page autosaved", "Page words", "");
    let held = saved("Held, autosaved", "Held", "");
    let other = saved("Draft autosaved", "Later words", r#","excerpt":"""#);
    let again = saved("Published", "Out there, changed", r#","excerpt":"""#);
    let writes = vec![
        ("rest-autosaves-draft", autosaves("pages/863"), &page),
        ("rest-autosaves-new", autosaves("posts/866"), &held),
        ("rest-autosaves-other", autosaves("posts/861"), &other),
        ("rest-autosaves-again", autosaves("posts/862"), &again),
    ];
    let writes = writes
        .into_iter()
        .map(|(golden, target, body)| (golden, "POST", target, body.as_str(), Setup::All))
        .collect();
    writes_are_php_s(state, writes).await;
}

/// The answers to what a write comes to past the goldens: a created
/// template not found, a template gone before its answer, a refusal of an
/// update, a revert or a delete, and an autosave of an empty draft, which
/// PHP refuses with an error of no status; and the plans not ported.
#[tokio::test(flavor = "multi_thread")]
async fn written_answers_past_the_goldens() {
    let state = include_str!("../../../../../reference/rest/template-writes.sql");
    let _saves = crate::admin::tests::SAVES.lock().await;
    fresh_with(state).await;
    let dir = tempfile::tempdir().unwrap();
    let site = saves_site(dir.path()).await;
    let templates = |path: &str| format!("/?rest_route=/wp/v2/templates{path}");
    let index = templates("/twentytwentyfive//index");
    let cases = [
        (
            ("POST", templates(""), r#"{"slug":"lost","title":"Lost"}"#),
            Outcome::Written(1),
            (400, "rest_template_insert_error"),
        ),
        (
            ("POST", templates(""), r#"{"slug":"plain"}"#),
            Outcome::Refused("empty_content"),
            (400, "empty_content"),
        ),
        (
            (
                "POST",
                templates("/twentytwentyfive//my-custom"),
                r#"{"title":"x"}"#,
            ),
            Outcome::Refused("rest_cannot_delete"),
            (500, "rest_cannot_delete"),
        ),
        // Text `wpdb` refuses, which PHP answers as no template found.
        (
            (
                "POST",
                templates("/twentytwentyfive//my-custom"),
                r#"{"title":"x"}"#,
            ),
            Outcome::InvalidText(String::new()),
            (501, ""),
        ),
        (
            ("DELETE", templates("/twentytwentyfive//my-custom"), ""),
            Outcome::Refused("rest_cannot_delete"),
            (500, "rest_cannot_delete"),
        ),
        (
            ("POST", index.clone(), r#"{"source":"theme"}"#),
            Outcome::Written(711),
            (200, r#""source":"custom""#),
        ),
    ];
    for ((method, target, body), outcome, (status, code)) in cases {
        let served = answered(&site, (method, &target, body), outcome).await;
        let what = format!("{method} {target} {body}");
        assert_eq!(served.0, status, "{what}");
        assert!(served.1.contains(code), "{what}");
    }
    let unported = [
        // A template its slug would title after the page it names.
        (
            "POST",
            templates(""),
            r#"{"slug":"page-sample-page","content":"x"}"#,
        ),
        // Reverting a template with no customised post, or no theme file.
        (
            "POST",
            templates("/twentytwentyfive//search"),
            r#"{"source":"theme"}"#,
        ),
        (
            "POST",
            templates("/twentytwentyfive//my-custom"),
            r#"{"source":"theme"}"#,
        ),
        // A pattern the theme does not have.
        (
            "POST",
            templates(""),
            r#"{"slug":"x","content":"<!-- wp:pattern {\"slug\":\"other/x\"} /-->"}"#,
        ),
    ];
    let (cookie, nonce) = log_in(&site).await;
    for (method, target, body) in unported {
        let request = axum::http::Request::builder()
            .method(method)
            .uri(&target)
            .header(header::COOKIE, &cookie)
            .header("x-wp-nonce", &nonce)
            .header(header::CONTENT_TYPE, "application/json");
        let response = send(&site, request, body).await;
        assert_eq!(
            response.status(),
            StatusCode::NOT_IMPLEMENTED,
            "{target} {body}"
        );
    }
    let autosaves = include_str!("../../../../../reference/rest/autosaves.sql");
    fresh_with(autosaves).await;
    let site = saves_site(dir.path()).await;
    let page = "/?rest_route=/wp/v2/pages/863/autosaves";
    let empty = ("POST", page, "{}");
    let empty = answered(&site, empty, Outcome::Refused("empty_content")).await;
    let not_ported = hyscribe_rest::Served::not_ported();
    assert_eq!(
        empty,
        (
            not_ported.status,
            String::from_utf8(not_ported.body).unwrap()
        )
    );
    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/?rest_route=/wp/v2/blocks/853/autosaves")
        .header(header::COOKIE, &cookie)
        .header("x-wp-nonce", &nonce)
        .header(header::CONTENT_TYPE, "application/json");
    let meta = r#"{"meta":{"wp_pattern_sync_status":"partial"}}"#;
    fresh_with(include_str!(
        "../../../../../reference/rest/site-writes.sql"
    ))
    .await;
    let site = saves_site(dir.path()).await;
    let response = send(&site, request, meta).await;
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    // A pattern, which is not viewable, has no preview link.
    let pattern = r#"{"title":"Pattern autosaved"}"#;
    let target = "/?rest_route=/wp/v2/blocks/853/autosaves";
    let login = (cookie.clone(), nonce.clone());
    let (status, text) = sent_as(&site, &login, ("POST", target, pattern)).await;
    assert_eq!(status, StatusCode::OK, "{text}");
    fresh().await;
}

/// The administrator's `method target` with the JSON `body` sent to
/// `site`, logged in as the cookie and nonce say: its status and body.
async fn sent_as(
    site: &Arc<Site>,
    (cookie, nonce): &(String, String),
    (method, target, sent): (&str, &str, &str),
) -> (StatusCode, String) {
    let request = axum::http::Request::builder()
        .method(method)
        .uri(target)
        .header(header::COOKIE, cookie)
        .header("x-wp-nonce", nonce)
        .header(header::CONTENT_TYPE, "application/json");
    let response = send(site, request, sent).await;
    let status = response.status();
    (status, body(response).await)
}

/// Templates past the goldens, over `reference/rest/templates-custom.sql`
/// with a template its slug would title, which reads 501, and one naming a
/// pattern the theme does not have: what PHP answers with its errors, and
/// what is not ported, 501, before anything is written.
#[tokio::test(flavor = "multi_thread")]
async fn template_writes_past_the_goldens() {
    let state = include_str!("../../../../../reference/rest/template-writes.sql");
    let more = "INSERT INTO wp_posts (ID, post_author, post_content, post_title, post_excerpt, \
        post_status, post_name, post_type, to_ping, pinged, post_content_filtered) VALUES \
        (760, 1, 'x', '', '', 'publish', 'single-post-hello-world', 'wp_template', '', '', ''), \
        (761, 1, '<!-- wp:pattern {\"slug\":\"other/x\"} /-->', 'Odd', '', 'publish', 'my-odd', \
        'wp_template', '', '', ''); \
        INSERT INTO wp_term_relationships VALUES (760, 711, 0), (761, 711, 0);\n-- cleanup\n";
    let _saves = crate::admin::tests::SAVES.lock().await;
    fresh_with(&format!(
        "{}{more}",
        state.split("\n-- cleanup\n").next().unwrap()
    ))
    .await;
    let dir = tempfile::tempdir().unwrap();
    let site = saves_site(dir.path()).await;
    let login = log_in(&site).await;
    let at = |path: &str| format!("/?rest_route=/wp/v2/{path}");
    let foreign = r#"{"content":"<!-- wp:pattern {\"slug\":\"other/x\"} /-->"}"#;
    let cases = [
        (
            "POST",
            at("templates/twentytwentyfive//single-post-hello-world"),
            r#"{"title":"x"}"#,
            501,
        ),
        (
            "DELETE",
            at("templates/twentytwentyfive//single-post-hello-world"),
            "",
            501,
        ),
        (
            "DELETE",
            at("templates/twentytwentyfive//my-odd&force=true"),
            "",
            501,
        ),
        (
            "POST",
            at("templates/twentytwentyfive//my-custom"),
            foreign,
            501,
        ),
        (
            "PATCH",
            at("templates/twentytwentyfive//my-custom"),
            r#"{"author":999}"#,
            400,
        ),
        (
            "POST",
            at("templates/twentytwentyfive//my-custom"),
            r#"{"content":{}}"#,
            200,
        ),
        (
            "PATCH",
            at("template-parts/twentytwentyfive//header"),
            r#"{"title":"Head"}"#,
            200,
        ),
    ];
    for (method, target, body, status) in cases {
        let (found, text) = sent_as(&site, &login, (method, &target, body)).await;
        assert_eq!(found.as_u16(), status, "{method} {target} {body}: {text}");
    }
    fresh().await;
}

/// Autosaves past the goldens: a template, which an autosave does not
/// port, and the author's draft emptied, which PHP refuses with an error of
/// no status; an empty template is no template.
#[tokio::test(flavor = "multi_thread")]
async fn autosaves_past_the_goldens() {
    let state = include_str!("../../../../../reference/rest/autosaves.sql");
    let unlocked = format!(
        "{}\nDELETE FROM wp_postmeta WHERE post_id = 866;\n-- cleanup\n",
        state.split("\n-- cleanup\n").next().unwrap()
    );
    let _saves = crate::admin::tests::SAVES.lock().await;
    fresh_with(&unlocked).await;
    let dir = tempfile::tempdir().unwrap();
    let site = saves_site(dir.path()).await;
    let login = log_in(&site).await;
    let autosaves = "/?rest_route=/wp/v2/posts/866/autosaves";
    for (body, status) in [
        (r#"{"template":"single"}"#, 501),
        (r#"{"title":""}"#, 501),
        (r#"{"template":"","title":"Kept"}"#, 200),
    ] {
        let (found, text) = sent_as(&site, &login, ("POST", autosaves, body)).await;
        assert_eq!(found.as_u16(), status, "{body}: {text}");
    }
    // A form's text that is not UTF-8, which `wpdb` refuses.
    let (cookie, nonce) = &login;
    let request = axum::http::Request::builder()
        .method("POST")
        .uri(autosaves)
        .header(header::COOKIE, cookie)
        .header("x-wp-nonce", nonce)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    let response = send(&site, request, "title=a%FFb").await;
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    fresh().await;
}

/// Without a writer, a write past its checks is refused as hs-admin
/// refuses one.
#[tokio::test(flavor = "multi_thread")]
async fn shadow_mode_refuses_writes() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(false, dir.path()).await;
    let (cookie, nonce) = log_in(&site).await;
    let ask = |target: &str| {
        axum::http::Request::builder()
            .method("POST")
            .uri(target)
            .header(header::COOKIE, &cookie)
            .header("x-wp-nonce", &nonce)
            .header(header::CONTENT_TYPE, "application/json")
    };
    // A settings update giving none writes nothing, so it is answered.
    let none = send(&site, ask("/?rest_route=/wp/v2/settings"), "{}").await;
    assert_eq!(none.status(), StatusCode::OK);
    let response = send(
        &site,
        ask("/?rest_route=/wp/v2/posts"),
        r#"{"title":"Kept out"}"#,
    )
    .await;
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

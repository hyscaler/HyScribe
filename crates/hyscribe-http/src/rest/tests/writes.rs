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
/// users and `state`'s rows (a list's `.sql` up to its cleanup), as the
/// oracle recorded over it.
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
    let state = state.split("-- cleanup").next().unwrap_or_default();
    for sql in [users, state].into_iter().filter(|sql| !sql.is_empty()) {
        sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
            .execute(&mut conn)
            .await
            .unwrap();
    }
}

/// [`fresh_with`] no rows of a list's own.
async fn fresh() {
    fresh_with("").await;
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

/// The bodies PHP takes that HyScribe answers 501: `meta`, and JSON PHP
/// refuses (`rest_invalid_json`).
const UNPORTED: [&str; 2] = [
    r#"{"title":"Meta","meta":{"footnotes":""}}"#,
    r#"{"title":"#,
];

/// PHP's answers to the posts' and pages' writes, for a visitor and every
/// role: those that write leave the rows PHP leaves (the write oracle
/// tests below), so the next request starts from the database afresh.
#[tokio::test(flavor = "multi_thread")]
async fn post_writes_match_php() {
    let golden = include_str!("../../../../hyscribe-rest/tests/golden/post-writes.tsv");
    let unported = UNPORTED.map(|body| (body, &[][..]));
    assert_eq!(replay_writes(golden, "", &unported).await, 6 * 31);
}

/// PHP's answers to `golden`'s writes over the default install with the
/// rows of `state`, as [`post_writes_match_php`] replays them: the bodies
/// of `unported` answer 501 to the identities listed with them, or to all
/// for none. The number of cases.
pub(super) async fn replay_writes(
    golden: &str,
    state: &str,
    unported: &[(&str, &[&str])],
) -> usize {
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
    let mut skipped = 0;
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
        let unported = unported.iter().any(|(body, whos)| {
            *body == sent && (whos.is_empty() || whos.contains(&case.who.as_str()))
        });
        if unported {
            assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED, "{what}");
            skipped += 1;
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
    let listed: usize = unported
        .iter()
        .map(|(_, whos)| if whos.is_empty() { 6 } else { whos.len() })
        .sum();
    assert_eq!(skipped, listed);
    cases.len()
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
            remote_addr: b"",
            user_agent: None,
            method_override: None,
            content_type: Some(b"application/json"),
            body: body.as_bytes(),
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
    oracle_writes(&site, &writes).await;
}

/// Each of `writes`, a golden of the write oracle and the request that
/// makes it, leaves the rows PHP's leaves, and nothing when a statement
/// fails part way.
async fn oracle_writes(site: &Arc<Site>, writes: &[(&str, &'static str, &str, &str)]) {
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
/// [`post_writes_match_php`] replays them.
#[tokio::test(flavor = "multi_thread")]
async fn term_writes_match_php() {
    let golden = include_str!("../../../../hyscribe-rest/tests/golden/term-writes.tsv");
    let state = include_str!("../../../../../reference/rest/term-writes.sql");
    let unported = [(r#"{"name":"Meta","meta":{}}"#, &[][..])];
    assert_eq!(replay_writes(golden, state, &unported).await, 6 * 31);
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
    assert_eq!(replay_writes(golden, state, &unported).await, 6 * 37);
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
    assert_eq!(replay_writes(golden, "", &unported).await, 6 * 33);
}

/// PHP's answers to the settings' writes, as [`post_writes_match_php`]
/// replays them.
#[tokio::test(flavor = "multi_thread")]
async fn settings_writes_match_php() {
    let golden = include_str!("../../../../hyscribe-rest/tests/golden/settings-writes.tsv");
    assert_eq!(replay_writes(golden, "", &[]).await, 6 * 17);
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
    oracle_writes(&site, &writes).await;
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
    oracle_writes(&site, &[create]).await;
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
    oracle_writes(&site, &writes).await;
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

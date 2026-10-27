// SPDX-License-Identifier: AGPL-3.0-or-later

//! Requests against the default install fixture in MariaDB:
//! `reference/default-install/db.sh up` first (CI does).

use super::*;
use axum::body::Body;
use axum::http::Request;
use hyscribe_config::Database;
use tower::ServiceExt;

fn fixture_config() -> Database {
    hyscribe_testkit::fixture_database()
}

async fn fixture_site(home: Option<&str>) -> Arc<Site> {
    let db = Db::connect(&fixture_config())
        .await
        .expect(hyscribe_testkit::NO_FIXTURE_DATABASE);
    Arc::new(Site::load(db, home.map(String::from)).await.unwrap())
}

struct Reply {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: String,
}

async fn request(site: &Arc<Site>, method: &str, uri: &str) -> Reply {
    let response = router(site.clone())
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    Reply {
        status,
        headers,
        body: String::from_utf8(bytes.to_vec()).unwrap(),
    }
}

fn title(body: &str) -> &str {
    let start = body.find("<title>").unwrap() + 7;
    body.get(start..start + body.get(start..).unwrap().find("</title>").unwrap())
        .unwrap()
}

#[tokio::test]
async fn home_page_lists_the_posts() {
    let site = fixture_site(None).await;
    let reply = request(&site, "GET", "/").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(title(&reply.body), "HyScribe Default Install");
    assert!(
        reply
            .body
            .contains("<a href=\"http://default-install.test/?p=1\">Hello world!</a>")
    );
    assert!(
        reply
            .body
            .contains("<p>Welcome to WordPress. This is your first post.")
    );
    // Pages and drafts are not on the blog home page.
    assert!(!reply.body.contains("Sample Page"));
    assert_eq!(reply.headers["content-type"], "text/html; charset=UTF-8");
    for (name, value) in SECURITY_HEADERS {
        assert_eq!(reply.headers[name], value);
    }
}

#[tokio::test]
async fn single_posts_and_pages() {
    let site = fixture_site(Some("http://127.0.0.1:8080/")).await;
    let reply = request(&site, "GET", "/?p=1").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        title(&reply.body),
        "Hello world! &#8211; HyScribe Default Install"
    );
    assert!(
        reply
            .body
            .contains("<a href=\"http://127.0.0.1:8080/?p=1\">")
    );
    assert!(
        reply
            .body
            .contains("<a href=\"http://127.0.0.1:8080/\">HyScribe Default Install</a>")
    );

    let reply = request(&site, "GET", "/?page_id=2").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        title(&reply.body),
        "Sample Page &#8211; HyScribe Default Install"
    );
    assert!(reply.body.contains("<blockquote class=\"wp-block-quote\">"));
    assert!(
        reply
            .body
            .contains("href=\"http://127.0.0.1:8080/?page_id=2\"")
    );

    // Like (int) in PHP, leading digits count.
    assert_eq!(
        request(&site, "GET", "/?p=1abc").await.status,
        StatusCode::OK
    );
    assert_eq!(request(&site, "HEAD", "/?p=1").await.status, StatusCode::OK);
}

#[tokio::test]
async fn missing_drafts_and_unknown_paths_are_not_found() {
    let site = fixture_site(None).await;
    // Drafts, missing IDs, and author archives (off for privacy).
    for uri in [
        "/?page_id=3",
        "/?p=3",
        "/?p=999",
        "/?p=abc",
        "/?author=1",
        "/?p=1&author=1",
    ] {
        let reply = request(&site, "GET", uri).await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(
            title(&reply.body),
            "Page not found &#8211; HyScribe Default Install"
        );
    }
    assert_eq!(
        request(&site, "POST", "/").await.status,
        StatusCode::METHOD_NOT_ALLOWED
    );
}

#[tokio::test]
async fn the_tagline_joins_the_home_title() {
    let db = Db::connect(&fixture_config()).await.unwrap();
    let mut site = Site::load(db, None).await.unwrap();
    site.options
        .insert("blogdescription".into(), b"Just <another> site".to_vec());
    let reply = request(&Arc::new(site), "GET", "/").await;
    assert_eq!(
        title(&reply.body),
        "HyScribe Default Install &#8211; Just &lt;another&gt; site"
    );
}

#[tokio::test]
async fn database_errors_show_no_details() {
    let mut config = fixture_config();
    config.prefix = hyscribe_config::Prefix::new("missing_").unwrap();
    let db = Db::connect(&config).await.unwrap();
    assert!(Site::load(db.clone(), None).await.is_err());
    let site = Arc::new(Site {
        db,
        options: HashMap::new(),
        home: String::new(),
    });
    for uri in ["/", "/?p=1", "/?cat=1"] {
        let reply = request(&site, "GET", uri).await;
        assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(
            reply
                .body
                .contains("Error establishing a database connection")
        );
        assert!(!reply.body.contains("missing_"));
    }
}

#[tokio::test]
async fn serves_until_shutdown() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let site = fixture_site(None).await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve(listener, site, async {
        let _ = stopped.await;
    }));
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    stream
        .write_all(b"GET /?p=1 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert!(response.contains("Hello world!"));
    stop.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[test]
fn escaping() {
    assert_eq!(
        esc_html("a < b > c \" ' &"),
        "a &lt; b &gt; c &quot; &#039; &amp;"
    );
    assert_eq!(
        esc_html("&amp; &#8211; &#x2014; &copy; & ;"),
        "&amp; &#8211; &#x2014; &copy; &amp; ;"
    );
    assert_eq!(esc_html("&;"), "&amp;;");
    assert_eq!(esc_html("&nbsp"), "&amp;nbsp");
    assert_eq!(esc_attr("\"x\""), "&quot;x&quot;");
}

#[test]
fn absint_takes_leading_digits() {
    assert_eq!(absint("42"), 42);
    assert_eq!(absint(" 7up"), 7);
    assert_eq!(absint("-3"), 0);
    assert_eq!(absint(""), 0);
    assert_eq!(absint("99999999999999999999999"), i64::MAX.unsigned_abs());
}

async fn location(site: &Arc<Site>, uri: &str) -> String {
    let reply = request(site, "GET", uri).await;
    assert_eq!(reply.status, StatusCode::MOVED_PERMANENTLY, "{uri}");
    assert!(reply.body.is_empty());
    reply.headers["location"].to_str().unwrap().to_string()
}

#[tokio::test]
async fn canonical_redirects_match_wordpress() {
    let site = fixture_site(Some("http://127.0.0.1:8081")).await;
    let home = "http://127.0.0.1:8081";
    // Locations recorded from PHP WordPress on the same database.
    for (uri, expected) in [
        ("/?p=2", "/?page_id=2"),
        ("/?p=2&x=1", "/?page_id=2&x=1"),
        ("/?x=1&p=2&&y", "/?page_id=2&x=1&y"),
        ("/?page_id=1", "/?p=1"),
        ("/no/such/path", "/no/such/path/"),
        ("/robots.txt", "/robots.txt/"),
        ("/robots.txt?a=b", "/robots.txt/?a=b"),
        ("/index.php", "/"),
        ("/index.php?p=1", "/?p=1"),
        ("/blog/index.php", "/blog/"),
    ] {
        assert_eq!(
            location(&site, uri).await,
            format!("{home}{expected}"),
            "{uri}"
        );
    }
    // A trailing slash makes any path the home page.
    let reply = request(&site, "GET", "/no/such/path/").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(title(&reply.body), "HyScribe Default Install");
    let reply = request(&site, "GET", "/xindex.php/").await;
    assert_eq!(reply.status, StatusCode::OK);
}

#[tokio::test]
async fn archives_and_searches_match_wordpress_titles() {
    let site = fixture_site(None).await;
    let name = "HyScribe Default Install";
    // Titles recorded from PHP WordPress on the same database.
    for (uri, expected) in [
        ("/?s=hello", "Search Results for &#8220;hello&#8221;"),
        ("/?s=", "Search Results for &#8220;&#8221;"),
        (
            "/?s=sample+page",
            "Search Results for &#8220;sample page&#8221;",
        ),
        (
            "/?s=nothingmatches",
            "Search Results for &#8220;nothingmatches&#8221;",
        ),
        ("/?s=%3Cb%3E", "Search Results for &#8220;&lt;b&gt;&#8221;"),
        ("/?cat=1", "Uncategorized"),
        ("/?s=hello&cat=1", "Search Results for &#8220;hello&#8221;"),
        ("/?cat=1&m=2026", "Uncategorized"),
    ] {
        let reply = request(&site, "GET", uri).await;
        assert_eq!(reply.status, StatusCode::OK, "{uri}");
        assert_eq!(
            title(&reply.body),
            format!("{expected} &#8211; {name}"),
            "{uri}"
        );
    }
    let hello = site.db.post(1).await.unwrap().unwrap();
    let year = hello.date.get(0..4).unwrap();
    let month = hello.date.get(5..7).unwrap();
    let day = hello.date.get(8..10).unwrap();
    for (m, format) in [
        (year.to_string(), "Y"),
        (format!("{year}{month}"), "F Y"),
        (format!("{year}-{month}-{day}"), "F j, Y"),
    ] {
        let reply = request(&site, "GET", &format!("/?m={m}")).await;
        assert_eq!(reply.status, StatusCode::OK, "{m}");
        assert_eq!(
            title(&reply.body),
            format!("{} &#8211; {name}", php_date(format, &hello.date))
        );
    }
    // The search results list pages too; the home page does not.
    let reply = request(&site, "GET", "/?s=").await;
    assert!(reply.body.contains("Sample Page") && reply.body.contains("Hello world!"));
    for uri in ["/?cat=999", "/?m=1999", "/?m=12"] {
        assert_eq!(
            request(&site, "GET", uri).await.status,
            StatusCode::NOT_FOUND,
            "{uri}"
        );
    }
    // Digits only: `abc` is no date at all.
    assert_eq!(title(&request(&site, "GET", "/?m=abc").await.body), name);
    assert_eq!(
        request(&site, "GET", "/?cat=0").await.status,
        StatusCode::OK
    );
}

#[test]
fn date_parts_follow_wp_query() {
    assert_eq!(date_parts("2026"), (Some(2026), None, None));
    assert_eq!(date_parts("2026-10"), (Some(2026), Some(10), None));
    assert_eq!(date_parts("20261027"), (Some(2026), Some(10), Some(27)));
    assert_eq!(date_parts("202610271200"), (Some(2026), Some(10), Some(27)));
    assert_eq!(date_parts("12"), (Some(12), None, None));
    assert_eq!(date_parts("abc"), (None, None, None));
    assert_eq!(date_parts("20261"), (Some(2026), None, None));
}

#[test]
fn php_date_formats() {
    let stored = "2026-03-07 08:09:10";
    assert_eq!(php_date("F j, Y", stored), "March 7, 2026");
    assert_eq!(php_date("M d y, n/m", stored), "Mar 07 26, 3/03");
    assert_eq!(php_date("\\Y\\e\\a\\r Y", stored), "Year 2026");
    assert_eq!(php_date("F", "2026-12-01"), "December");
    assert_eq!(php_date("F|M|j", ""), "||0");
    // `n` is a format character, as in PHP.
    assert_eq!(php_date("trailing\\", stored), "traili3g");
}

#[test]
fn search_terms_split_like_wordpress() {
    assert_eq!(search_terms("a b,c+d\te"), ["a", "b", "c", "d", "e"]);
    assert!(search_terms("  ,+ ").is_empty());
}

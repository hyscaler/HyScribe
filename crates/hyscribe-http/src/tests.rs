// SPDX-License-Identifier: AGPL-3.0-or-later

//! Requests against the default install fixture in MariaDB:
//! `reference/default-install/db.sh up` first (CI does).

use super::*;
use axum::body::Body;
use axum::http::Request;
use hyscribe_config::{Database, Secret};
use tower::ServiceExt;

fn fixture_config() -> Database {
    let env = |key: &str, default: &str| std::env::var(key).unwrap_or_else(|_| default.to_string());
    Database {
        host: env("HYSCRIBE_TEST_DB_HOST", "127.0.0.1"),
        port: env("HYSCRIBE_TEST_DB_PORT", "33061").parse().unwrap(),
        name: "wordpress".into(),
        user: "wordpress".into(),
        password: Secret::new("reference-only"),
        prefix: "wp_".into(),
        charset: "utf8mb4".into(),
    }
}

async fn fixture_site(home: Option<&str>) -> Arc<Site> {
    let db = Db::connect(&fixture_config())
        .await
        .expect("no fixture database: run reference/default-install/db.sh up");
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
    for uri in [
        "/?page_id=3",
        "/?p=2",
        "/?p=999",
        "/?p=abc",
        "/wp-admin/",
        "/no/such/path",
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
    config.prefix = "missing_".into();
    let db = Db::connect(&config).await.unwrap();
    assert!(Site::load(db.clone(), None).await.is_err());
    let site = Arc::new(Site {
        db,
        options: HashMap::new(),
        home: String::new(),
    });
    for uri in ["/", "/?p=1"] {
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

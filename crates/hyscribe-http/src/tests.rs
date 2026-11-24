// SPDX-License-Identifier: AGPL-3.0-or-later

//! Requests against the default install fixture in MariaDB:
//! `reference/default-install/db.sh up` first (CI does).

use super::*;
use axum::body::Body;
use axum::http::Request;
use hyscribe_config::Database;
use hyscribe_model::Term;
use hyscribe_phpser::{Key, Value};
use tower::ServiceExt;

/// The `TRUSTED_PROXIES` list `text` makes, for a site behind a proxy.
pub(crate) fn trusted(text: &str) -> Option<Vec<hyscribe_config::proxies::Cidr>> {
    hyscribe_config::proxies::parse_list(text)
}

pub(crate) fn fixture_config() -> Database {
    hyscribe_testkit::fixture_database()
}

async fn fixture_site(home: Option<&str>) -> Arc<Site> {
    site_at(home, None).await
}

async fn site_at(home: Option<&str>, siteurl: Option<&str>) -> Arc<Site> {
    let db = Db::connect(&fixture_config())
        .await
        .expect(hyscribe_testkit::NO_FIXTURE_DATABASE);
    Arc::new(
        Site::load(db, home.map(String::from), siteurl.map(String::from))
            .await
            .unwrap(),
    )
}

/// The fixture seen from the PHP oracle's address, as its goldens are.
pub(crate) async fn oracle_site() -> Arc<Site> {
    site_at(Some("http://127.0.0.1:8081"), Some("http://127.0.0.1:8081")).await
}

struct Reply {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: String,
}

async fn request(site: &Arc<Site>, method: &str, uri: &str) -> Reply {
    answer(router(site.clone()), method, uri).await
}

async fn answer(app: Router, method: &str, uri: &str) -> Reply {
    answer_with(app, method, uri, "").await
}

/// [`answer`] with a `Cookie` header, none when `cookie` is empty.
async fn answer_with(app: Router, method: &str, uri: &str, cookie: &str) -> Reply {
    let mut asked = Request::builder().method(method).uri(uri);
    if !cookie.is_empty() {
        asked = asked.header(header::COOKIE, cookie);
    }
    let response = app
        .oneshot(asked.body(Body::empty()).unwrap())
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
    assert!(reply.body.contains(
        "<a href=\"http://default-install.test/?p=1\" target=\"_self\" >Hello world!</a>"
    ));
    assert!(reply.body.contains(
        "<p class=\"wp-block-paragraph\">Welcome to WordPress. This is your first post."
    ));
    // Pages are not among the posts; the header's fallback menu lists them.
    assert_eq!(reply.body.matches("Sample Page").count(), 1);
    assert!(reply.body.contains(
        "<a class=\"wp-block-pages-list__item__link wp-block-navigation-item__content\" href=\"http://default-install.test/?page_id=2\">Sample Page</a>"
    ));
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
            .contains("<h1 class=\"wp-block-post-title\">Hello world!</h1>")
    );
    // The fixture's one comment, threaded at the top level.
    assert!(
        reply
            .body
            .contains("<li id=\"comment-1\" class=\"comment even thread-even depth-1\">")
    );
    // The author's display name, read from the users table.
    assert!(reply.body.contains(
        "<a href=\"http://127.0.0.1:8080/?author=1\" target=\"_self\" class=\"wp-block-post-author-name__link\">admin</a>"
    ));
    assert!(
        reply
            .body
            .contains("<a href=\"http://127.0.0.1:8080\" target=\"_self\" rel=\"home\">HyScribe Default Install</a>")
    );
    assert!(
        reply.body.contains(
            "<body class=\"wp-singular post-template-default single single-post postid-1"
        )
    );

    let reply = request(&site, "GET", "/?page_id=2").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        title(&reply.body),
        "Sample Page &#8211; HyScribe Default Install"
    );
    assert!(reply.body.contains(
        "<blockquote class=\"wp-block-quote is-layout-flow wp-block-quote-is-layout-flow\">"
    ));
    assert!(
        reply
            .body
            .contains("<body class=\"wp-singular page-template-default page page-id-2 ")
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
    for method in ["POST", "PUT"] {
        assert_eq!(
            request(&site, method, "/").await.status,
            StatusCode::METHOD_NOT_ALLOWED
        );
    }
}

/// `WP_HOME` and `WP_SITEURL` replace their options, untrailingslashed, so
/// the comment form posts to the configured site address.
#[tokio::test]
async fn configured_addresses_replace_their_options() {
    let db = Db::connect(&fixture_config()).await.unwrap();
    let site = Site::load(
        db,
        Some("http://home.test//".into()),
        Some("http://site.test/wp\\/".into()),
    )
    .await
    .unwrap();
    assert_eq!(site.options["home"], b"http://home.test");
    assert_eq!(site.home, "http://home.test");
    let reply = request(&Arc::new(site), "GET", "/?p=1&replytocom=1").await;
    assert!(
        reply
            .body
            .contains("<form action=\"http://site.test/wp/wp-comments-post.php\""),
        "{}",
        reply.body
    );
    assert!(
        reply
            .body
            .contains("Leave a Reply to <a href=\"#comment-1\">")
    );
    // The cancel link is the request URI without the reply.
    assert!(
        reply
            .body
            .contains("id=\"cancel-comment-reply-link\" href=\"/?p=1#respond\">")
    );
}

#[tokio::test]
async fn the_tagline_joins_the_home_title() {
    let db = Db::connect(&fixture_config()).await.unwrap();
    let mut site = Site::load(db, None, None).await.unwrap();
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
    assert!(Site::load(db.clone(), None, None).await.is_err());
    let site = Arc::new(Site {
        db,
        // Sitemaps on, so their queries run.
        options: HashMap::from([
            ("blog_public".into(), b"1".to_vec()),
            ("show_on_front".into(), b"posts".to_vec()),
        ]),
        home: String::new(),
        theme: Arc::new(hyscribe_render::theme::twentytwentyfive()),
        classic_styled: Arc::default(),
        styled: Arc::new(tokio::sync::Mutex::new((
            "twentytwentyfive".to_owned(),
            None,
            Arc::new(hyscribe_render::theme::twentytwentyfive()),
        ))),
        rewriter: Arc::default(),
        compiled: Arc::default(),
        salts: Vec::new(),
        sessions: None,
        writer: None,
        defined: Vec::new(),
        public_users: true,
        proxies: None,
        time_limit: crate::DEFAULT_TIME_LIMIT,
        constants: hyscribe_config::Constants::default(),
        limiter: Arc::new(login::limits::Limiter::new(Default::default())),
        uploads: None,
        extensions: None,
        registry: None,
        running: None,
        install: unconfigured(),
        fetch: None,
        feed_hosts: Arc::default(),
        embed_hosts: Arc::default(),
        mailer: Arc::new(crate::mail_off()),
        hooks: Arc::new(()),
    });
    // The 404 page fails while rendering, reading an option it has not loaded.
    for uri in [
        "/",
        "/?p=1",
        "/?cat=1",
        "/?author=1",
        "/?feed=rss2",
        "/?feed=rss2&cat=1",
        "/?feed=comments-rss2",
        "/?p=-1&page_id=2",
        "/?error=404&p=1&feed=rss2",
        "/?name=x",
        "/?pagename=x",
        "/?attachment=x",
        "/?subpost=x",
        "/?name=x&post_type=attachment",
        "/?attachment_id=1",
        // Past a route that reads nothing, sitemaps fail.
        "/?author=1&sitemap=index",
    ] {
        let reply = request(&site, "GET", uri).await;
        assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR);
        // Served, the options fail to load first, with the same page.
        let served = answer(live(site.clone()), "GET", uri).await;
        assert_eq!(
            (served.status, served.body),
            (reply.status, reply.body.clone())
        );
        assert!(
            reply
                .body
                .contains("Error establishing a database connection")
        );
        assert!(!reply.body.contains("missing_"));
    }
    // Sitemaps come after the page's own queries, so their failures show
    // only on their own.
    for query in [
        "sitemap=index",
        "sitemap=posts&sitemap-subtype=page",
        "sitemap=posts&sitemap-subtype=post",
        "sitemap=taxonomies&sitemap-subtype=category",
    ] {
        let query = query
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let page = Outcome::Page(Box::new(not_found(&site, false)));
        assert!(sitemaps::serve(&site, &query, page).await.is_err());
    }
    // A trackback's post is read after the page's own queries.
    let reply = trackback::respond(&site, 1, StatusCode::OK, &Value::Array(Vec::new())).await;
    assert_eq!(reply.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let uri = Uri::from_static("/wp-trackback.php");
    assert!(
        trackback::outcome(&site, &uri, &HashMap::new())
            .await
            .is_err()
    );
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
    server.await.unwrap();
}

/// A client that leaves a request's headers unfinished, or a kept-alive
/// connection idle, is cut off after [`HEADER_READ_TIMEOUT`], so slow
/// clients cannot hold connections open.
#[tokio::test]
async fn unfinished_headers_time_out() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let site = fixture_site(None).await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(serve(listener, site, std::future::pending()));
    // Before the server can start its timer.
    let started = tokio::time::Instant::now();
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: loc")
        .await
        .unwrap();
    // The server reads the start of the headers, and its timer runs, before
    // the paused clock may jump ahead to the next timer.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    tokio::time::pause();
    let mut rest = Vec::new();
    let read = tokio::time::timeout(2 * HEADER_READ_TIMEOUT, stream.read_to_end(&mut rest)).await;
    assert!(read.is_ok(), "still open");
    assert!(started.elapsed() >= HEADER_READ_TIMEOUT);
}

/// A client that leaves a request's body unfinished gets a 408 after
/// [`BODY_READ_TIMEOUT`] and its connection closes, as Apache's
/// `RequestReadTimeout` answers; a body over [`MAX_BODY`] gets a 413.
#[tokio::test]
async fn unfinished_bodies_time_out() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let echo = |body: axum::body::Bytes| async move { body };
    let app = Router::new().route("/", axum::routing::post(echo));
    tokio::spawn(serve_connections(listener, app, 2, std::future::pending()));
    let started = tokio::time::Instant::now();
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    let head = "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 100\r\n\r\n";
    stream.write_all(head.as_bytes()).await.unwrap();
    stream.write_all(b"0123456789").await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    tokio::time::pause();
    let mut answer = Vec::new();
    let read = tokio::time::timeout(2 * BODY_READ_TIMEOUT, stream.read_to_end(&mut answer));
    assert!(read.await.is_ok(), "still open");
    assert!(started.elapsed() >= BODY_READ_TIMEOUT);
    assert!(answer.starts_with(b"HTTP/1.1 408 Request Timeout"));
    tokio::time::resume();
    // A whole body is served, and one too large is refused.
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    let head =
        "POST / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: 2\r\n\r\nok";
    stream.write_all(head.as_bytes()).await.unwrap();
    let mut answer = Vec::new();
    stream.read_to_end(&mut answer).await.unwrap();
    assert!(answer.starts_with(b"HTTP/1.1 200 OK") && answer.ends_with(b"\r\n\r\nok"));
    let app = Router::new()
        .route("/", axum::routing::post(echo))
        .layer(axum::middleware::from_fn(body_deadline));
    let large = axum::http::Request::post("/").body(Body::from(vec![b'x'; MAX_BODY + 1]));
    let response = app.oneshot(large.unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

/// Past the connection limit a client waits until a connection closes.
#[tokio::test]
async fn connections_past_the_limit_wait() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let site = fixture_site(None).await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(serve_connections(listener, router(site), 1, async {
        let _ = stopped.await;
    }));
    let first = tokio::net::TcpStream::connect(address).await.unwrap();
    let mut second = tokio::net::TcpStream::connect(address).await.unwrap();
    second
        .write_all(b"GET /?p=1 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut byte = [0];
    let waiting = tokio::time::timeout(
        std::time::Duration::from_millis(200),
        second.read(&mut byte),
    );
    assert!(waiting.await.is_err(), "answered past the limit");
    drop(first);
    let mut response = Vec::new();
    second.read_to_end(&mut response).await.unwrap();
    assert!(response.starts_with(b"HTTP/1.1 200 OK"));
    stop.send(()).unwrap();
    server.await.unwrap();
}

/// Once shutdown starts the server stops listening, so new clients are
/// refused at once while the connections open finish.
#[tokio::test]
async fn shutdown_refuses_new_connections() {
    use tokio::io::AsyncWriteExt;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    // A request that is never answered keeps the server finishing it.
    let app = Router::new().route("/", axum::routing::get(std::future::pending::<()>));
    tokio::spawn(serve_connections(listener, app, 2, async {
        let _ = stopped.await;
    }));
    let mut open = tokio::net::TcpStream::connect(address).await.unwrap();
    open.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    stop.send(()).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    // Kept listening, the kernel would take the connection into the
    // backlog for no one to accept.
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
    drop(open);
}

/// Every case of `reference/request-line/golden.sh`: a request line longer
/// than Apache's limit gets Apache's 414, without the server signature, and
/// one at the limit is served. The limit also keeps every query var's lists
/// far below the bound values a database query takes.
#[tokio::test]
async fn long_request_lines_match_apache() {
    let site = fixture_site(None).await;
    let golden = include_str!("../tests/golden/request-line.tsv");
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let [method, version, length, status, content_type, body] = fields[..] else {
            panic!("{line}");
        };
        let length: usize = length.parse().unwrap();
        let filler = length - method.len() - 1 - 4 - 1 - "HTTP/1.1".len();
        let uri = format!("/?q={}", "a".repeat(filler));
        let version = match version {
            "1.0" => axum::http::Version::HTTP_10,
            _ => axum::http::Version::HTTP_11,
        };
        let response = router(site.clone())
            .oneshot(
                Request::builder()
                    .method(method)
                    .version(version)
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        if status != "414" {
            assert_ne!(
                response.status(),
                StatusCode::URI_TOO_LONG,
                "{method} {length}"
            );
            continue;
        }
        assert_eq!(
            response.status(),
            StatusCode::URI_TOO_LONG,
            "{method} {length}"
        );
        let headers = response.headers().clone();
        assert_eq!(headers["content-type"], content_type);
        assert_eq!(headers["x-content-type-options"], "nosniff");
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        // HEAD's body is left out on the wire, by hyper as by Apache.
        if method != "HEAD" {
            assert_eq!(bytes, hyscribe_testkit::unhex(body));
        }
    }
    assert_eq!(golden.lines().count(), 18);
}

#[test]
fn absint_casts_like_php() {
    assert_eq!(absint("42"), 42);
    assert_eq!(absint(" 7up"), 7);
    // absint() in PHP WordPress: the absolute value of the (int) cast.
    assert_eq!(absint("-3"), 3);
    assert_eq!(absint("1e3"), 1000);
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
        // robots.txt and sitemaps wait for the redirects.
        ("/?robots=1&p=2", "/?page_id=2&robots=1"),
        ("/?p=1&feed=rss2&robots=1", "/?feed=rss2&p=1&robots=1"),
        ("/?p=2&sitemap=index", "/?page_id=2&sitemap=index"),
        (
            "/?p=1&feed=rss2&sitemap=index",
            "/?feed=rss2&p=1&sitemap=index",
        ),
        ("/?robots=1&p=1.", "/?robots=1&p=1"),
        // An attachment ID or name that is a post's or a page's.
        ("/?attachment_id=1", "/?p=1"),
        ("/?attachment_id=2", "/?page_id=2"),
        ("/?attachment_id=-1", "/?p=1"),
        ("/?attachment_id=1x", "/?p=1"),
        ("/?attachment_id=1&x=y", "/?p=1&x=y"),
        ("/?attachment_id=1&cpage=2", "/?p=1&cpage=2"),
        ("/?attachment_id=1&s=x", "/?p=1&s=x"),
        ("/?attachment_id=1&cat=1", "/?p=1&cat=1"),
        ("/?attachment_id=1&paged=2", "/?p=1&paged=2"),
        ("/?attachment_id=1&page=2", "/?p=1&page=2"),
        ("/?subpost_id=1", "/?p=1&subpost_id=1"),
        ("/?subpost_id=2", "/?page_id=2&subpost_id=2"),
        ("/?subpost_id=1&attachment_id=2", "/?p=1&subpost_id=1"),
        ("/?attachment=hello-world", "/?p=1&attachment=hello-world"),
        (
            "/?attachment=sample-page",
            "/?page_id=2&attachment=sample-page",
        ),
        ("/?attachment=sam", "/?page_id=2&attachment=sam"),
        ("/?subpost=hello-world", "/?p=1&subpost=hello-world"),
        (
            "/?attachment=a/hello-world",
            "/?p=1&attachment=a%2Fhello-world",
        ),
        ("/?x=1&p=2&&y", "/?page_id=2&x=1&y"),
        ("/?page_id=1", "/?p=1"),
        ("/no/such/path", "/no/such/path/"),
        ("/robots.txt", "/robots.txt/"),
        ("/robots.txt?a=b", "/robots.txt/?a=b"),
        ("/index.php", "/"),
        ("/index.php?p=1", "/?p=1"),
        ("/blog/index.php", "/blog/"),
        // A post's feed goes to its comments feed link.
        ("/?p=1&feed=rss2", "/?feed=rss2&p=1"),
        ("/?page_id=2&feed=rss2", "/?feed=rss2&page_id=2"),
        ("/?p=1&feed=atom", "/?feed=atom&p=1"),
        ("/?x=1&p=1&feed=rss2", "/?feed=rss2&p=1&x=1"),
        ("/?p=1&feed=rss", "/?feed=rss2&p=1"),
        ("/?feed=rss&p=1", "/?feed=rss2&p=1"),
        (
            "/?page_id=2&feed=rss2&cpage=2",
            "/?feed=rss2&page_id=2&cpage=2",
        ),
        ("/?p=1&feed=rdf", "/?feed=rdf&p=1"),
        ("/?p=2&feed=rss2", "/?page_id=2&feed=rss2"),
        ("/?page_id=1&feed=rss2", "/?p=1&feed=rss2"),
        ("/?p=1&feed=rss2&x=a+b", "/?feed=rss2&p=1&x=a%20b"),
        ("/?p=1&feed=comments-rss2", "/?feed=comments-rss2&p=1"),
        ("/?p=01&feed=rss2", "/?feed=rss2&p=01"),
        ("/?p=1&feed=rss2&x[]=1", "/?feed=rss2&p=1&x%5B0%5D=1"),
        (
            "/?p=1&withoutcomments=1&feed=rss2",
            "/?feed=rss2&p=1&withoutcomments=1",
        ),
        ("/?p=1&feed=feed", "/?feed=feed&p=1"),
        ("/?p=1&&feed=rss2", "/?feed=rss2&p=1"),
        ("/?p=1&feed=rss2&x=%7E", "/?feed=rss2&p=1&x=~"),
        ("/?p=1&page_id=0&feed=rss2", "/?feed=rss2&p=1"),
        ("/?p=1&feed=rss2&preview=1", "/?feed=rss2&p=1"),
        // The last clean up of the query, and `page_id` over `p`.
        ("/?p=1.", "/?p=1"),
        ("/?p=1%20", "/?p=1"),
        ("/?cat=1%2E", "/?cat=1"),
        ("/?page_id=2)", "/?page_id=2"),
        ("/?p=1%E2%80%9D", "/?p=1"),
        ("/?x=1&p=1.", "/?x=1&p=1"),
        ("/?cat=&p=1", "/?p=1"),
        ("/?x=1&tag=", "/?x=1"),
        ("/?p=", "/"),
        ("/?page_id=&x=1", "/?x=1"),
        ("/?p=&p=1", "/?p=1"),
        ("/?p=1&page_id=2", "/?page_id=2"),
        ("/?p=2&page_id=2", "/?page_id=2"),
        ("/?p=999&page_id=2", "/?page_id=2"),
        ("/?p=2&cat=", "/?page_id=2"),
        ("/?feed=rss2&p=1.", "/?feed=rss2&p=1"),
        ("/?p=1.&feed=rss2", "/?feed=rss2&p=1"),
        ("/?cat=&feed=rss2", "/?feed=rss2"),
        ("/?p=1&s=x&cat=", "/?p=1&s=x"),
        ("/?page_id=2&s=x&tag=", "/?page_id=2&s=x"),
        // Invalid dates go to their year or month, a negative `p` to the
        // `page_id` with it.
        ("/?year=2026&monthnum=13", "/?m=2026"),
        ("/?year=2026&monthnum=13&x=1", "/?m=2026&x=1"),
        ("/?year=2026&monthnum=2&day=30", "/?m=202602"),
        ("/?x=1&year=2026&monthnum=2&day=30", "/?m=202602&x=1"),
        ("/?p=-1&page_id=2", "/?page_id=2"),
        ("/?year=2026&monthnum=13&day=5", "/?m=202613"),
        ("/?year=2026&monthnum=4&day=31", "/?m=202604"),
        ("/?year=40000&monthnum=1&day=1", "/?m=4000001"),
        ("/?year=2026&monthnum=2&day=29", "/?m=202602"),
        ("/?year=1900&monthnum=2&day=29", "/?m=190002"),
        // A `name` or `pagename` that finds nothing goes where
        // `redirect_guess_404_permalink()` guesses.
        ("/?name=sample-page", "/?page_id=2"),
        ("/?pagename=hello-world", "/?p=1"),
        ("/?name=hel", "/?p=1"),
        ("/?pagename=sam", "/?page_id=2"),
        ("/?pagename=a/sample-page", "/?page_id=2"),
        ("/?name=sam&post_type=nope", "/?page_id=2"),
        ("/?name=hel&post_type=page", "/?p=1"),
        ("/?name=hello&year=2026&x[]=1", "/?p=1&year=2026&x%5B0%5D=1"),
        ("/?name=hello-world&page=2", "/?p=1"),
        ("/?name=hel&year=0", "/?p=1&year=0"),
        ("/?name=hel&page=1", "/?p=1"),
        ("/?name=hel&page_id=0", "/?p=1"),
        ("/?pagename=sam&page_id=0", "/?page_id=0"),
        (
            "/?year=2026&monthnum=2&day=31&name=&x=1",
            "/?m=202602&name&x=1",
        ),
        ("/?feed=&name=hello", "/?p=1"),
        ("/?page=&name=hello", "/?p=1"),
        // The other arguments parsed and encoded again, the link's own in
        // place, an array `p` among them.
        ("/?p=2&x[]=1", "/?page_id=2&x%5B0%5D=1"),
        ("/?page_id=1&p[]=3", "/?p%5B0%5D=3"),
        ("/?p[x]=1&page_id=2", "/?page_id=2"),
        ("/?p=2&preview=1", "/?page_id=2"),
        ("/?year=2026&monthnum=2&day=30&m=2026", "/?m=2026"),
        ("/?x[]=1&year=2026&monthnum=13", "/?m=2026&x%5B0%5D=1"),
        ("/?p=-1&year=2026&monthnum=2&day=30", "/?m=202602&p=-1"),
    ] {
        assert_eq!(
            location(&site, uri).await,
            format!("{home}{expected}"),
            "{uri}"
        );
    }
    // At its link, or with no such post, a feed is served; a clean up that
    // would clean up again, an ID of 0 and a search are not redirected.
    for (uri, status) in [
        ("/?feed=rss2&p=1", StatusCode::OK),
        ("/?feed=rss2&p=1&x=1", StatusCode::OK),
        ("/?p=999&feed=rss2", StatusCode::OK),
        ("/?p=&cat=", StatusCode::OK),
        ("/?p=&cat=&x=1", StatusCode::OK),
        ("/?p=0", StatusCode::OK),
        ("/?p=abc", StatusCode::OK),
        ("/?page_id=2&p=0", StatusCode::OK),
        ("/?p=1&page_id=1", StatusCode::OK),
        ("/?p=1.&x=1", StatusCode::OK),
        ("/?s=&cat=", StatusCode::OK),
        ("/?s=a&cat=1,", StatusCode::OK),
        ("/?p=1&page_id=999", StatusCode::NOT_FOUND),
        ("/?feed=rss&cat=1.&page_id=", StatusCode::OK),
        ("/wp-content/x?cat=", StatusCode::NOT_FOUND),
        ("/hs-admin/?p=", StatusCode::FOUND),
        ("/?year=2026&monthnum=4&day=30", StatusCode::NOT_FOUND),
        ("/?p=-1&page_id=999", StatusCode::OK),
        ("/?p[]=1", StatusCode::OK),
        ("/?name=sam&feed=rss2", StatusCode::OK),
        ("/?pagename=privacy-policy", StatusCode::NOT_FOUND),
        (
            "/?pagename=sample-page&post_type=post",
            StatusCode::NOT_FOUND,
        ),
        ("/?name=hello-world&feed=rss2", StatusCode::OK),
    ] {
        assert_eq!(request(&site, "GET", uri).await.status, status, "{uri}");
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
            format!(
                "{} &#8211; {name}",
                hyscribe_render::date::format(
                    format,
                    &hello.date,
                    hyscribe_render::date::Zone::Utc
                )
            )
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
    let parts = |m| query::date_parts(m);
    assert_eq!(parts("2026"), [Some(2026), None, None, None, None, None]);
    assert_eq!(parts("0"), [None; 6]);
    assert_eq!(parts("00")[0], Some(0));
    assert_eq!(parts("2026-10")[..3], [Some(2026), Some(10), None]);
    assert_eq!(
        parts("20261027")[..4],
        [Some(2026), Some(10), Some(27), None]
    );
    assert_eq!(
        parts("202610271200"),
        [Some(2026), Some(10), Some(27), Some(12), Some(0), None]
    );
    assert_eq!(parts("20261027080815")[5], Some(15));
    assert_eq!(parts("12")[..2], [Some(12), None]);
    assert_eq!(parts("abc"), [None; 6]);
    assert_eq!(parts("20261")[..2], [Some(2026), None]);
}

#[tokio::test]
async fn admin_and_login_urls_are_clean() {
    let site = fixture_site(Some("http://127.0.0.1:8080")).await;
    let (temporary, permanent, moved) = (
        StatusCode::TEMPORARY_REDIRECT,
        StatusCode::PERMANENT_REDIRECT,
        StatusCode::MOVED_PERMANENTLY,
    );
    for (method, uri, status, expected) in [
        ("GET", "/wp-login.php", temporary, "/hs-login"),
        (
            "GET",
            "/wp-login.php?action=lostpassword",
            temporary,
            "/hs-login?action=lostpassword",
        ),
        ("POST", "/wp-login.php", temporary, "/hs-login"),
        ("GET", "/wp-admin", temporary, "/hs-admin/"),
        ("GET", "/wp-admin/", temporary, "/hs-admin/"),
        ("GET", "/wp-admin/index.php", temporary, "/hs-admin/"),
        (
            "GET",
            "/wp-admin/edit.php?post_type=page",
            temporary,
            "/hs-admin/edit?post_type=page",
        ),
        (
            "GET",
            "/wp-admin/admin.php?page=x&y=1",
            temporary,
            "/hs-admin/admin?page=x&y=1",
        ),
        (
            "POST",
            "/wp-admin/admin-ajax.php",
            temporary,
            "/hs-admin/admin-ajax",
        ),
        (
            "GET",
            "/wp-admin/css/forms.css",
            temporary,
            "/hs-admin/css/forms.css",
        ),
        ("GET", "/hs-login.php", permanent, "/hs-login"),
        (
            "GET",
            "/hs-admin/edit.php?post_type=page",
            moved,
            "/hs-admin/edit?post_type=page",
        ),
        (
            "HEAD",
            "/hs-admin/admin-ajax.php?action=x",
            moved,
            "/hs-admin/admin-ajax?action=x",
        ),
        (
            "GET",
            "/hs-admin/load-styles.php",
            moved,
            "/hs-admin/load-styles",
        ),
        ("GET", "/hs-admin/index.php", moved, "/hs-admin/"),
    ] {
        let reply = request(&site, method, uri).await;
        assert_eq!(reply.status, status, "{method} {uri}");
        assert_eq!(
            reply.headers["location"],
            format!("http://127.0.0.1:8080{expected}"),
            "{uri}"
        );
    }
    // The login screen answers; the admin's screens send visitors to it.
    assert_eq!(
        request(&site, "GET", "/hs-login").await.status,
        StatusCode::OK
    );
    for uri in ["/hs-admin", "/hs-admin/", "/hs-admin/edit"] {
        let reply = request(&site, "GET", uri).await;
        assert_eq!(reply.status, StatusCode::FOUND, "{uri}");
        let location = reply.headers["location"].to_str().unwrap();
        assert!(location.contains("/hs-login?redirect_to="), "{location}");
    }
    // Look-alikes are ordinary paths.
    for uri in ["/wp-adminx", "/wp-login.phpx", "/x/wp-admin/", "/hs-loginx"] {
        let status = request(&site, "GET", uri).await.status;
        assert!(status != temporary && status != permanent, "{uri}");
    }
    // Other methods are served in place, as the clean form would be, with
    // the query kept: a redirect would drop the body.
    for (php, clean) in [
        (
            "/hs-admin/edit.php?post_type=page",
            "/hs-admin/edit?post_type=page",
        ),
        ("/hs-admin/index.php", "/hs-admin/index"),
        (
            "/hs-admin/load-styles.php?load%5Bchunk_0%5D=forms",
            "/hs-admin/load-styles?load%5Bchunk_0%5D=forms",
        ),
        (
            "/hs-admin/admin-ajax.php?action=x",
            "/hs-admin/admin-ajax?action=x",
        ),
    ] {
        for method in ["POST", "PUT"] {
            let (old, new) = (
                request(&site, method, php).await,
                request(&site, method, clean).await,
            );
            assert_eq!(old.status, new.status, "{method} {clean}");
            assert_ne!(old.status, moved, "{method} {clean}");
            let location = |reply: &Reply| reply.headers.get("location").cloned();
            assert_eq!(location(&old), location(&new), "{method} {clean}");
            assert_eq!(old.body, new.body, "{method} {clean}");
        }
    }
}

#[test]
fn hs_admin_php_urls_are_served_in_place() {
    for (uri, served) in [
        (
            "/hs-admin/edit.php?post_type=page&a=%22",
            "/hs-admin/edit?post_type=page&a=%22",
        ),
        ("/hs-admin/admin-post.php", "/hs-admin/admin-post"),
        ("/hs-admin/edit", "/hs-admin/edit"),
        ("/hs-admin/css/x.css", "/hs-admin/css/x.css"),
        ("/wp-admin/edit.php", "/wp-admin/edit.php"),
    ] {
        assert_eq!(in_place(uri.parse().unwrap()), served, "{uri}");
    }
}

#[tokio::test]
async fn static_paths_keep_their_urls() {
    let site = fixture_site(None).await;
    for uri in [
        "/wp-content/uploads/2026/10/photo.jpg",
        "/wp-content/themes/twentytwentyfive/",
        "/wp-includes/js/nope.min.js",
        "/wp-content/plugins/x/",
    ] {
        // Never redirected; 404 where no file is served.
        let reply = request(&site, "GET", uri).await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{uri}");
        assert!(!reply.headers.contains_key("location"), "{uri}");
    }
}

#[tokio::test]
async fn the_front_page_links_to_home() {
    let base = fixture_site(Some("http://example.test")).await;
    let mut options = base.options.clone();
    options.insert("show_on_front".into(), b"page".to_vec());
    options.insert("page_on_front".into(), b"2".to_vec());
    let site = Arc::new(Site {
        db: base.db.clone(),
        options,
        home: base.home.clone(),
        theme: Arc::new(hyscribe_render::theme::twentytwentyfive()),
        classic_styled: Arc::default(),
        styled: Arc::new(tokio::sync::Mutex::new((
            "twentytwentyfive".to_owned(),
            None,
            Arc::new(hyscribe_render::theme::twentytwentyfive()),
        ))),
        rewriter: Arc::default(),
        compiled: Arc::default(),
        salts: Vec::new(),
        sessions: None,
        writer: None,
        defined: Vec::new(),
        public_users: true,
        proxies: None,
        time_limit: crate::DEFAULT_TIME_LIMIT,
        constants: hyscribe_config::Constants::default(),
        limiter: Arc::new(login::limits::Limiter::new(Default::default())),
        uploads: None,
        extensions: None,
        registry: None,
        running: None,
        install: unconfigured(),
        fetch: None,
        feed_hosts: Arc::default(),
        embed_hosts: Arc::default(),
        mailer: Arc::new(crate::mail_off()),
        hooks: Arc::new(()),
    });
    // Search results list the front page with a link to it.
    let reply = request(&site, "GET", "/?s=sample").await;
    assert!(
        reply
            .body
            .contains(r#"<a href="http://example.test/" target="_self" >Sample Page</a>"#),
        "{}",
        reply.body
    );
}

#[tokio::test]
async fn a_render_that_panics_is_a_server_error() {
    let site = fixture_site(None).await;
    let panicked = tokio::spawn(async { panic!("render bug") })
        .await
        .unwrap_err();
    let page = Page {
        status: StatusCode::OK,
        title: "x".into(),
        candidates: Vec::new(),
        request: hyscribe_render::site::Request::default(),
        hidden: false,
    };
    let rendered = rendered(&site, page, Err(panicked));
    assert_eq!(rendered.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(!rendered.document.contains("render bug"));
}

#[tokio::test]
async fn templates_follow_the_hierarchy() {
    let site = fixture_site(None).await;
    // The single template shows the post date; the page template does not.
    assert!(
        request(&site, "GET", "/?p=1")
            .await
            .body
            .contains("wp-block-post-date")
    );
    assert!(
        !request(&site, "GET", "/?page_id=2")
            .await
            .body
            .contains("wp-block-post-date")
    );
    let search = request(&site, "GET", "/?s=hello").await;
    assert!(
        search
            .body
            .contains("<body class=\"search search-results wp-embed-responsive")
    );
    assert!(
        search
            .body
            .contains("Search results for: &#8220;hello&#8221;")
    );
    let category = request(&site, "GET", "/?cat=1").await;
    assert!(
        category
            .body
            .contains("Category: <span>Uncategorized</span>")
    );
    // Post classes come from the post's terms.
    assert!(category.body.contains("hentry category-uncategorized"));
    // A category with a date is both archives, as PHP's body_class() says.
    let dated = request(&site, "GET", "/?cat=1&m=2026").await;
    assert!(
        dated
            .body
            .contains("<body class=\"archive date category category-uncategorized ")
    );
    assert!(!category.body.contains("<body class=\"archive date"));
    let missing = request(&site, "GET", "/?p=999").await;
    assert!(missing.body.contains("<body class=\"error404 "));
}

/// The list page of `query`.
async fn listed(site: &Site, query: &HashMap<String, String>) -> Page {
    let main = main_query(site, query, false).await.unwrap();
    list(site, query, false, main, None).await.unwrap().0
}

#[tokio::test]
async fn routes_give_the_request_state() {
    let site = fixture_site(None).await;
    let recent = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        - 60;
    let home = listed(&site, &HashMap::new()).await;
    assert!(home.request.front_page && home.request.home && home.request.now > recent);
    assert_eq!(home.request.post, Some(1));
    let search = HashMap::from([("s".to_string(), "a".to_string())]);
    let search = listed(&site, &search).await;
    assert_eq!(
        (search.request.found_posts, search.request.max_pages),
        (2, 1)
    );
    let post = site.db.post(1).await.unwrap().unwrap();
    let single = single_page(&site, post, Paging::default());
    assert_eq!(
        (single.request.queried_object_id, single.request.post),
        (1, Some(1))
    );
    assert!(single.request.singular && single.request.now > recent);
    assert_eq!(single.request.found_posts, 1);
    let missing = not_found(&site, false);
    assert!(missing.request.not_found && missing.request.now > recent);
}

/// `get_attachment_template()`'s names lead an attachment's hierarchy.
#[tokio::test]
async fn an_attachments_template_names_follow_its_mime_type() {
    let site = fixture_site(None).await;
    let post = site.db.post(1).await.unwrap().unwrap();
    let names = |post: Post| single_page(&site, post, Paging::default()).candidates;
    let tail = ["single-attachment-hello-world", "single-attachment"];
    let mut attachment = post.clone();
    attachment.post_type = "attachment".into();
    attachment.name = b"hello-world".to_vec();
    attachment.mime_type = "image/png".into();
    let want = ["image-png", "png", "image", "attachment"];
    assert_eq!(names(attachment.clone())[..4], want);
    assert_eq!(names(attachment.clone())[4..6], tail);
    attachment.mime_type = "image".into();
    let got = names(attachment.clone());
    assert_eq!(got[..2], ["image", "attachment"]);
    assert_eq!(got[2..4], tail);
    assert!(!names(post).contains(&"attachment".to_string()));
}

/// `get_post_meta( $id, $key )` answers every value, as the feed's
/// enclosures read it.
#[tokio::test(flavor = "multi_thread")]
async fn reads_give_every_post_meta_value() {
    use hyscribe_render::site::Site as _;
    let site = fixture_site(None).await;
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let reads = Reads::new(&site, handle);
        assert_eq!(
            reads.post_meta_all(2, "_wp_page_template"),
            [b"default".to_vec()]
        );
        assert!(reads.post_meta_all(1, "_thumbnail_id").is_empty());
    })
    .await
    .unwrap();
}

/// A read that fails answers its type's default and keeps the error; the
/// site's global styles fail to load.
#[tokio::test(flavor = "multi_thread")]
async fn failed_reads_answer_defaults() {
    use hyscribe_render::site::Site as _;
    let mut config = fixture_config();
    config.prefix = hyscribe_config::Prefix::new("missing_").unwrap();
    let site = Arc::new(Site {
        db: Db::connect(&config).await.unwrap(),
        options: HashMap::new(),
        home: String::new(),
        theme: Arc::new(hyscribe_render::theme::twentytwentyfive()),
        classic_styled: Arc::default(),
        styled: Arc::new(tokio::sync::Mutex::new((
            "twentytwentyfive".to_owned(),
            None,
            Arc::new(hyscribe_render::theme::twentytwentyfive()),
        ))),
        rewriter: Arc::default(),
        compiled: Arc::default(),
        salts: Vec::new(),
        sessions: None,
        writer: None,
        defined: Vec::new(),
        public_users: true,
        proxies: None,
        time_limit: crate::DEFAULT_TIME_LIMIT,
        constants: hyscribe_config::Constants::default(),
        limiter: Arc::new(login::limits::Limiter::new(Default::default())),
        uploads: None,
        extensions: None,
        registry: None,
        running: None,
        install: unconfigured(),
        fetch: None,
        feed_hosts: Arc::default(),
        embed_hosts: Arc::default(),
        mailer: Arc::new(crate::mail_off()),
        hooks: Arc::new(()),
    });
    // The site's global styles, read on each reload.
    assert!(site.styled_theme("twentytwentyfive").await.is_err());
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let reads = Reads::new(&site, handle);
        assert!(!reads.has_published_posts());
        assert!(reads.finish().is_err());
    })
    .await
    .unwrap();
}

/// `wp_hash()` signs with `AUTH_KEY` and `AUTH_SALT`; the default install
/// stores no salts to fall back on.
#[tokio::test(flavor = "multi_thread")]
async fn wp_hash_signs_with_the_configured_salts() {
    use hyscribe_config::Secret;
    use hyscribe_render::site::Site as _;
    let db = Db::connect(&fixture_config()).await.unwrap();
    let salted = Site::load(db.clone(), None, None)
        .await
        .unwrap()
        .with_salts(vec![
            ("AUTH_KEY", Secret::new("key")),
            ("AUTH_SALT", Secret::new("salt")),
        ]);
    let bare = Site::load(db, None, None).await.unwrap();
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let reads = Reads::new(&salted, handle.clone());
        let expected = hyscribe_phpser::hash_hmac_md5(b"data", b"keysalt");
        assert_eq!(reads.wp_hash(b"data"), Some(expected));
        assert_eq!(Reads::new(&bare, handle).wp_hash(b"data"), None);
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn reads_come_from_the_database_once() {
    use hyscribe_render::site::Site as _;
    let site = fixture_site(None).await;
    let handle = tokio::runtime::Handle::current();
    let reading = Arc::clone(&site);
    tokio::task::spawn_blocking(move || {
        let reads = Reads::new(&reading, handle);
        assert_eq!(
            reads.post_meta(2, "_wp_page_template"),
            Some(b"default".to_vec())
        );
        assert_eq!(
            reads.post_meta(2, "_wp_page_template"),
            Some(b"default".to_vec())
        );
        assert_eq!(reads.post_terms(1, "category")[0].slug, b"uncategorized");
        assert_eq!(reads.recent_comments(5)[0].id, 1);
        let uncategorized = &reads.taxonomy_terms("category")[0];
        assert_eq!(uncategorized.slug, b"uncategorized");
        assert_eq!(reads.term_posts("category"), [(1, 1)]);
        assert!(reads.term_posts("post_tag").is_empty());
        assert!(reads.has_published_posts());
        assert_eq!(reads.calendar_month(2026, 10, 31).days, [27]);
        let category = &reads.cloud_terms("category", 45)[0];
        assert_eq!(
            (&category.slug[..], category.count),
            (&b"uncategorized"[..], 1)
        );
        for _ in 0..2 {
            let months = reads.date_counts(hyscribe_model::Period::Month);
            assert_eq!(
                (months[0].year, months[0].month, months[0].posts),
                (2026, 10, 1)
            );
        }
        let query = PostQuery {
            post_types: vec!["page".into()],
            limit: 5,
            ..PostQuery::default()
        };
        assert_eq!(
            reads.query(&query).iter().map(|p| p.id).collect::<Vec<_>>(),
            [2]
        );
        let both = PostQuery {
            post_types: vec!["post".into(), "page".into()],
            limit: 1,
            ..PostQuery::default()
        };
        assert_eq!(reads.count_posts(&both), 2);
        let later = hyscribe_model::Post {
            id: 999,
            date: "2099-01-01 00:00:00".into(),
            post_type: "post".into(),
            ..hyscribe_model::Post::default()
        };
        assert_eq!(
            reads.adjacent_post(&later, true, None).map(|p| p.id),
            Some(1)
        );
        reads.finish().unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn list_filters_narrow_the_query() {
    let site = fixture_site(None).await;
    let nothing = request(&site, "GET", "/?s=nothingmatches").await;
    assert!(
        nothing
            .body
            .contains("<body class=\"search search-no-results ")
    );
    // The one post is from October 27, 2026.
    assert_eq!(
        request(&site, "GET", "/?m=202601").await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(&site, "GET", "/?m=20261001").await.status,
        StatusCode::NOT_FOUND
    );
}

/// The posts feeds and their caching headers against PHP's, from
/// `reference/feed/golden.sh`, with HyScribe as the generator.
#[tokio::test]
async fn feeds_match_php() {
    let site = oracle_site().await;
    let golden = include_str!("../../hyscribe-render/tests/golden/feeds-http.tsv");
    for line in golden.lines() {
        let fields: Vec<String> = line
            .split('\t')
            .map(|f| String::from_utf8(hyscribe_testkit::unhex(f)).unwrap())
            .collect();
        let (path, sent, kept, body) = (&fields[0], &fields[1], &fields[2], &fields[3]);
        let mut request = axum::http::Request::get(path.as_str());
        for header in sent.lines() {
            let (name, value) = header.split_once(": ").unwrap();
            request = request.header(name, value);
        }
        let response = router(site.clone())
            .oneshot(request.body(axum::body::Body::empty()).unwrap())
            .await
            .unwrap();
        let header = |name| {
            response
                .headers()
                .get(name)
                .map_or("", |v| v.to_str().unwrap())
                .to_string()
        };
        let ours = [
            response.status().as_u16().to_string(),
            header(header::CONTENT_TYPE),
            header(header::LAST_MODIFIED),
            header(header::ETAG),
            header(header::LOCATION),
        ]
        .join("\n");
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let (repository, version) = (env!("CARGO_PKG_REPOSITORY"), env!("CARGO_PKG_VERSION"));
        let php = body
            .replace(
                "https://wordpress.org/?v=7.1.3",
                &format!("{repository}?v={version}"),
            )
            .replace(
                "<generator uri=\"https://wordpress.org/\" version=\"7.1.3\">WordPress</generator>",
                &format!(
                    "<generator uri=\"{repository}\" version=\"{version}\">HyScribe</generator>"
                ),
            )
            .replace(
                "<!-- generator=\"WordPress/7.1.3\" -->",
                &format!("<!-- generator=\"HyScribe/{version}\" -->"),
            );
        assert_eq!(&ours, kept, "{path} {sent:?}");
        assert_eq!(
            String::from_utf8(bytes.to_vec()).unwrap(),
            php,
            "{path} {sent:?}"
        );
    }
    assert_eq!(golden.lines().count(), 76);
}

/// Every page of `reference/head/golden.sh`, status and HTML, against
/// PHP's, with HyScribe as the generator and without the tags held
/// back.
#[tokio::test]
async fn pages_match_php() {
    let golden = include_str!("../../hyscribe-render/tests/golden/pages.tsv");
    assert_pages(&oracle_site().await, golden).await;
    assert_eq!(golden.lines().count(), 196);
}

/// Every case of `reference/loader/golden.sh`: the favicon, trackback and
/// preview query vars, with an HTML body reduced to its robots meta tag
/// through its title and the IDs of the posts it lists.
#[tokio::test]
async fn loader_vars_match_php() {
    let site = oracle_site().await;
    let golden = include_str!("../tests/golden/loader.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let path = String::from_utf8(hyscribe_testkit::unhex(fields[0])).unwrap();
        let php_body = as_hyscribe(String::from_utf8(hyscribe_testkit::unhex(fields[4])).unwrap());
        let reply = request(&site, "GET", &path).await;
        let header = |name| reply.headers.get(name).map_or("", |v| v.to_str().unwrap());
        let html = header("content-type").starts_with("text/html");
        let body = if html {
            let from = reply.body.find("<meta name='robots'");
            let to = reply.body.find("</title>").map(|at| at + "</title>".len());
            let ids: Vec<&str> = reply
                .body
                .split("wp-block-post post-")
                .skip(1)
                .map(|rest| rest.split(|c: char| !c.is_ascii_digit()).next().unwrap())
                .collect();
            from.zip(to).map_or(String::new(), |(from, to)| {
                format!(
                    "{}\nposts: {}",
                    reply.body.get(from..to).unwrap(),
                    ids.join(",")
                )
            })
        } else {
            reply.body.clone()
        };
        // A redirect's or empty page's content type is not compared.
        let typed = !php_body.is_empty();
        let ours = (
            reply.status.as_u16().to_string(),
            header("location"),
            typed.then(|| header("content-type")),
            body,
        );
        let php = (
            fields[1].to_string(),
            fields[2],
            typed.then_some(fields[3]),
            php_body,
        );
        if ours != php {
            failures.push(format!("{path}\n  php:  {php:?}\n  ours: {ours:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(golden.lines().count(), 270);
    // Recorded from PHP: a HEAD request ends before the favicon or the
    // trackback. PHP links the draft page 3, which HyScribe keeps hidden.
    for (method, path) in [
        ("HEAD", "/?favicon=1"),
        ("HEAD", "/?tb=1&p=1"),
        ("GET", "/3?tb=1"),
    ] {
        let reply = request(&site, method, path).await;
        assert_eq!(reply.status, StatusCode::OK, "{path}");
        assert!(
            reply.headers.get("location").is_none() && reply.body.is_empty(),
            "{path}"
        );
    }
}

/// Every case of `reference/trackback/golden.sh`: `wp-trackback.php` and
/// trackback POSTs, with trackbacks closed.
#[tokio::test]
async fn trackbacks_match_php() {
    let site = oracle_site().await;
    let golden = include_str!("../tests/golden/trackback.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let path = String::from_utf8(hyscribe_testkit::unhex(fields[1])).unwrap();
        let form = hyscribe_testkit::unhex(fields[3]);
        let mut builder = Request::builder().method(fields[0]).uri(&path);
        if !fields[2].is_empty() {
            builder = builder.header("content-type", fields[2]);
        }
        let response = router(site.clone())
            .oneshot(builder.body(Body::from(form.clone())).unwrap())
            .await
            .unwrap();
        let header = |name| {
            response
                .headers()
                .get(name)
                .map_or(String::new(), |v| v.to_str().unwrap().to_string())
        };
        let redirect = response.status().is_redirection();
        let ours = (
            response.status().as_u16().to_string(),
            header("location"),
            (!redirect).then(|| header("content-type")),
        );
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let php = (
            fields[4].to_string(),
            fields[5].to_string(),
            (!redirect).then(|| fields[6].to_string()),
        );
        let php_body = hyscribe_testkit::unhex(fields[7]);
        // HEAD bodies are dropped by the server.
        let bodies = fields[0] == "HEAD" || *body == *php_body;
        if ours != php || !bodies {
            failures.push(format!(
                "{} {path} {:?}\n  php:  {php:?}\n  ours: {ours:?} {:?}",
                fields[0],
                String::from_utf8_lossy(&form),
                String::from_utf8_lossy(&body)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(golden.lines().count(), 107);
    // PHP links the draft page 3, which HyScribe keeps hidden.
    let reply = request(&site, "GET", "/wp-trackback.php/3").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert!(reply.headers.get("location").is_none() && reply.body.is_empty());
}

/// Every case of `reference/links/golden.sh`: the REST API and shortlink
/// `Link` headers.
#[tokio::test]
async fn link_headers_match_php() {
    let site = oracle_site().await;
    let golden = include_str!("../tests/golden/links.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let path = String::from_utf8(hyscribe_testkit::unhex(fields[1])).unwrap();
        let reply = request(&site, fields[0], &path).await;
        let links: Vec<&str> = reply
            .headers
            .get_all("link")
            .iter()
            .map(|v| v.to_str().unwrap())
            .collect();
        let ours = (reply.status.as_u16().to_string(), links.join(" | "));
        if ours != (fields[2].to_string(), fields[3].to_string()) {
            failures.push(format!(
                "{} {path}\n  php:  {:?}\n  ours: {ours:?}",
                fields[0],
                &fields[2..]
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(golden.lines().count(), 69);
    // Author archives are off, so no user's route is linked.
    let reply = request(&site, "GET", "/?author=1").await;
    assert_eq!(reply.headers.get_all("link").iter().count(), 1);
}

/// A static file as served: status, content type (empty for none), the
/// SHA-256 of the body, `Last-Modified` and `ETag`.
async fn served(site: &Arc<Site>, path: &str) -> [String; 5] {
    use sha2::Digest;
    let response = router(site.clone())
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status().as_u16().to_string();
    let header = |name| {
        response
            .headers()
            .get(name)
            .map_or("", |v| v.to_str().unwrap())
            .to_string()
    };
    let content_type = header(header::CONTENT_TYPE);
    let (modified, etag) = (header(header::LAST_MODIFIED), header(header::ETAG));
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let sha = sha2::Sha256::digest(&body)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    [status, content_type, sha, modified, etag]
}

/// The fixture child's files (10.3), which are not in the release: status,
/// type and body are PHP's, and only its parent's files are not at its URL.
#[tokio::test]
async fn a_child_theme_s_files_match_php() {
    let site = fixture_site(None).await;
    let golden = include_str!("../tests/golden/theme-files-hyscribe-fixture-child.tsv");
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let ours = served(&site, fields[0]).await;
        assert_eq!(
            ours[..3],
            [fields[1], fields[2], fields[3]],
            "{}",
            fields[0]
        );
    }
    assert_eq!(golden.lines().count(), 10);
    let parents = "/wp-content/themes/hyscribe-fixture-child/style.min.css";
    assert_eq!(
        request(&site, "GET", parents).await.status,
        StatusCode::NOT_FOUND
    );
}

/// Every file of `reference/themes/files-golden.sh`: each embedded theme's
/// files as the web server sends them, whichever theme is active.
#[tokio::test]
async fn theme_files_match_php() {
    let site = fixture_site(None).await;
    let goldens = [
        (include_str!("../tests/golden/theme-files.tsv"), 136),
        (
            include_str!("../tests/golden/theme-files-twentytwentyfour.tsv"),
            49,
        ),
        (
            include_str!("../tests/golden/theme-files-twentytwentythree.tsv"),
            44,
        ),
        (
            include_str!("../tests/golden/theme-files-twentytwentytwo.tsv"),
            69,
        ),
    ];
    for (golden, count) in goldens {
        for line in golden.lines() {
            let fields: Vec<&str> = line.split('\t').collect();
            // The ETag is the SHA-256 of what PHP serves: the files are the
            // release's own.
            let etag = format!("\"{}\"", fields[3]);
            let expected = [fields[1], fields[2], fields[3], fields[4], &etag];
            assert_eq!(served(&site, fields[0]).await, expected, "{}", fields[0]);
        }
        assert_eq!(golden.lines().count(), count);
    }
    // No PHP, no data files of the port's own, and only GET and HEAD.
    for (method, path) in [
        ("GET", "/wp-content/themes/twentytwentyfive/functions.php"),
        (
            "GET",
            "/wp-content/themes/twentytwentyfive/patterns/footer.php",
        ),
        ("GET", "/wp-content/themes/twentytwentyfive/patterns.json"),
        ("GET", "/wp-content/themes/twentytwentyfive/nope.css"),
        ("GET", "/wp-content/themes/twentytwentyfour/functions.php"),
        ("GET", "/wp-content/themes/twentytwentyfour/registered.json"),
        ("GET", "/wp-content/themes/nope/style.css"),
        ("GET", "/wp-content/themes/twentytwentyfour"),
    ] {
        assert_eq!(
            request(&site, method, path).await.status,
            StatusCode::NOT_FOUND,
            "{path}"
        );
    }
    let path = "/wp-content/themes/twentytwentyfive/style.css";
    assert_eq!(request(&site, "HEAD", path).await.status, StatusCode::OK);
    assert_eq!(
        request(&site, "POST", path).await.status,
        StatusCode::METHOD_NOT_ALLOWED
    );
}

/// Every case of `reference/themes/conditional-golden.sh`: conditional
/// requests for a theme file, answered as Apache answers them.
#[tokio::test]
async fn conditional_requests_match_apache() {
    let site = fixture_site(None).await;
    let path = "/wp-content/themes/twentytwentyfive/style.css";
    let [.., modified, etag] = served(&site, path).await;
    let golden = include_str!("../tests/golden/conditional.tsv");
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let fill = |v: &str| v.replace("ETAG", &etag).replace("MODIFIED", &modified);
        let mut builder = Request::builder().method(fields[0]).uri(path);
        for pair in [(fields[1], fields[2]), (fields[3], fields[4])] {
            if !pair.0.is_empty() {
                builder = builder.header(pair.0, fill(pair.1));
            }
        }
        let response = router(site.clone())
            .oneshot(builder.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        assert_eq!(status.as_str(), fields[5], "{line}");
        let headers = response.headers();
        assert_eq!(headers[header::ETAG], etag.as_str());
        assert_eq!(headers[header::LAST_MODIFIED], modified.as_str());
        let typed = headers.contains_key(header::CONTENT_TYPE);
        assert_eq!(typed, status != StatusCode::NOT_MODIFIED, "{line}");
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(body.is_empty(), status != StatusCode::OK, "{line}");
    }
    assert_eq!(golden.lines().count(), 37);
}

/// Whether a registered style or script names the file, which is served
/// even with a `.min` twin.
fn registered(path: &str) -> bool {
    let dependencies = include_str!("../../../wordpress/wp-includes/dependencies.json");
    dependencies.contains(&format!("\"src\": \"{path}\""))
}

/// Every file of `reference/wordpress/files-golden.sh`: `wp-includes` as
/// the web server sends it, skipping PHP and the unminified files with a
/// `.min` twin, which are not bundled.
#[tokio::test]
async fn wp_includes_files_match_php() {
    let site = fixture_site(None).await;
    let golden = include_str!("../tests/golden/wp-includes-files.tsv");
    let paths: Vec<&str> = golden
        .lines()
        .filter_map(|l| l.split('\t').next())
        .collect();
    let mut unminified = 0;
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let path = fields[0];
        let served = served(&site, path).await;
        let twin = [".js", ".css"].iter().find_map(|ext| {
            let stem = path.strip_suffix(ext)?;
            (!stem.ends_with(".min")).then(|| format!("{stem}.min{ext}"))
        });
        if twin.is_some_and(|twin| paths.contains(&twin.as_str())) && !registered(fields[0]) {
            unminified += 1;
            assert_eq!(served[0], "404", "{path}");
            continue;
        }
        let etag = format!("\"{}\"", fields[3]);
        let expected = [fields[1], fields[2], fields[3], fields[4], &etag];
        assert_eq!(served, expected, "{path}");
    }
    assert_eq!((golden.lines().count(), unminified), (1670, 640));
    for path in [
        "/wp-includes/version.php",
        "/wp-includes/nope.js",
        "/wp-includes/",
    ] {
        assert_eq!(
            request(&site, "GET", path).await.status,
            StatusCode::NOT_FOUND,
            "{path}"
        );
    }
}

/// Every file of `reference/wordpress/files-golden.sh wp-admin`: the admin's
/// CSS, scripts and images, served under `/hs-admin/` (ADR 0008) as the web
/// server sends them under `/wp-admin/`, skipping the unminified twins.
#[tokio::test]
async fn wp_admin_files_match_php() {
    let site = fixture_site(None).await;
    let golden = include_str!("../tests/golden/wp-admin-files.tsv");
    let paths: Vec<&str> = golden
        .lines()
        .filter_map(|l| l.split('\t').next())
        .collect();
    let mut unminified = 0;
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let path = fields[0].replacen("/wp-admin/", "/hs-admin/", 1);
        let served = served(&site, &path).await;
        let twin = [".js", ".css"].iter().find_map(|ext| {
            let stem = fields[0].strip_suffix(ext)?;
            (!stem.ends_with(".min")).then(|| format!("{stem}.min{ext}"))
        });
        if twin.is_some_and(|twin| paths.contains(&twin.as_str())) && !registered(fields[0]) {
            unminified += 1;
            assert_eq!(served[0], "404", "{path}");
            continue;
        }
        let etag = format!("\"{}\"", fields[3]);
        let expected = [fields[1], fields[2], fields[3], fields[4], &etag];
        assert_eq!(served, expected, "{path}");
    }
    assert!(unminified > 0);
    assert_eq!(golden.lines().count(), 351);
}

/// Every request of `reference/loaders/golden.sh`: `load-styles.php` and
/// `load-scripts.php` at `/hs-admin/load-styles` and `/hs-admin/load-scripts`,
/// headers and bodies as PHP sends them (`Expires` follows the clock).
#[tokio::test]
async fn loaders_match_php() {
    use sha2::Digest;
    let site = fixture_site(None).await;
    let golden = include_str!("../tests/golden/loaders.tsv");
    for line in golden.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let mut request = Request::get(format!("/hs-admin/{}?{}", f[0], f[1]));
        if !f[2].is_empty() {
            request = request.header(header::IF_NONE_MATCH, f[2]);
        }
        let response = router(site.clone())
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let header = |name| {
            response
                .headers()
                .get(name)
                .map_or("", |v| v.to_str().unwrap())
                .to_string()
        };
        let headers = [
            response.status().as_u16().to_string(),
            header(header::CONTENT_TYPE),
            header(header::ETAG),
            header(header::CACHE_CONTROL),
        ];
        // `Expires` is a year from now, as PHP dates it.
        let expires = response.headers().get(header::EXPIRES).map(|date| {
            let date = httpdate::parse_http_date(date.to_str().unwrap()).unwrap();
            let ahead = date
                .duration_since(std::time::SystemTime::now())
                .unwrap()
                .as_secs();
            assert!((31_535_990..=31_536_000).contains(&ahead), "{ahead}");
        });
        let expires = expires.is_some();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let sha: String = sha2::Sha256::digest(&body)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(headers, [f[3], f[4], f[5], f[6]], "{line}");
        assert_eq!(expires, f[3] == "200", "{line}");
        assert_eq!(sha, f[7], "{line}");
    }
    assert_eq!(golden.lines().count(), 416);
    // The WordPress URL redirects, as every admin URL does.
    let moved = request(&site, "GET", "/wp-admin/load-styles.php?load=login").await;
    assert_eq!(
        moved.headers[header::LOCATION],
        "http://default-install.test/hs-admin/load-styles?load=login"
    );
}

/// With a page on the front, its link and shortlink are the home page.
#[tokio::test]
async fn the_front_page_links_to_the_home_page() {
    let mut site = Arc::into_inner(oracle_site().await).unwrap();
    site.options
        .insert("show_on_front".into(), b"page".to_vec());
    site.options.insert("page_on_front".into(), b"2".to_vec());
    let reply = request(&Arc::new(site), "GET", "/?tb=1&page_id=2").await;
    assert_eq!(reply.headers["location"], "http://127.0.0.1:8081/");
    let links: Vec<_> = reply.headers.get_all("link").iter().collect();
    assert_eq!(links[2], "<http://127.0.0.1:8081/>; rel=shortlink");
}

/// Every case of `reference/embed/golden.sh`: embed pages, with the
/// sharing dialog's random secret and ID suffix made the golden's.
#[tokio::test]
#[allow(clippy::string_slice)]
async fn embed_pages_match_php() {
    let site = oracle_site().await;
    let golden = include_str!("../tests/golden/embed.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let path = String::from_utf8(hyscribe_testkit::unhex(fields[0])).unwrap();
        let reply = request(&site, "GET", &path).await;
        let header = |name| reply.headers.get(name).map_or("", |v| v.to_str().unwrap());
        let mut body = String::new();
        if !fields[3].is_empty() {
            body = without_share_draws(reply.body.clone());
        }
        let ours = (
            reply.status.as_u16().to_string(),
            header("location"),
            header("x-wp-embed"),
            body,
        );
        let php_body = as_hyscribe(String::from_utf8(hyscribe_testkit::unhex(fields[4])).unwrap());
        let php = (fields[1].to_string(), fields[2], fields[3], php_body);
        if ours != php {
            failures.push(format!(
                "{path}\n  php:  {:?}\n  ours: {:?}",
                (&php.0, php.1, php.2),
                (&ours.0, ours.1, ours.2)
            ));
            if ours.3 != php.3 {
                let common = ours
                    .3
                    .bytes()
                    .zip(php.3.bytes())
                    .take_while(|(a, b)| a == b)
                    .count();
                let from = common.saturating_sub(80);
                failures.push(format!(
                    "  at {common}: php {:?}\n  ours {:?}",
                    php.3.get(from..common + 200),
                    ours.3.get(from..common + 200)
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(golden.lines().count(), 27);
}

/// An embed page's sharing dialog with its random draws as the embed
/// golden records them: the secret as `0123456789`, the ID suffixes as
/// `RAND`.
#[allow(clippy::string_slice)]
fn without_share_draws(mut body: String) -> String {
    if let Some(at) = body.find("data-secret=&quot;") {
        let secret = body[at + 18..at + 28].to_string();
        assert!(
            secret.bytes().all(|b| b.is_ascii_alphanumeric()),
            "{secret}"
        );
        body = body.replace(&secret, "0123456789");
    }
    for kind in [
        "tab-wordpress",
        "tab-html",
        "description-wordpress",
        "description-html",
    ] {
        let prefix = format!("wp-embed-share-{kind}-");
        let mut out = String::new();
        let mut rest = body.as_str();
        while let Some(at) = rest.find(&prefix) {
            let after = &rest[at + prefix.len()..];
            let id = after.split('-').next().unwrap();
            let digits = after[id.len() + 1..]
                .bytes()
                .take_while(u8::is_ascii_digit)
                .count();
            out.push_str(&rest[..at + prefix.len()]);
            out.push_str(&format!("{id}-RAND"));
            rest = &after[id.len() + 1 + digits..];
        }
        out.push_str(rest);
        body = out;
    }
    body
}

/// Recorded from PHP: an embed of a 404 of the query's own is an embed
/// page, whose card is not ported; and the dialog's random draws.
#[tokio::test]
async fn embeds_of_query_404s_and_share_draws() {
    let site = oracle_site().await;
    for (path, status) in [
        ("/?error=404&embed=true", StatusCode::NOT_FOUND),
        ("/?p=-1&embed=true", StatusCode::OK),
    ] {
        let reply = request(&site, "GET", path).await;
        assert_eq!(reply.status, status, "{path}");
        assert_eq!(reply.headers["x-wp-embed"], "true", "{path}");
        assert!(!reply.headers.contains_key("content-security-policy"));
        assert_eq!(reply.headers["x-content-type-options"], "nosniff");
    }
    let mut upper = false;
    for _ in 0..20 {
        let Share { secret, suffix } = share();
        upper |= secret.bytes().any(|b| b.is_ascii_uppercase());
        assert_eq!(secret.len(), 10);
        assert!(
            secret.bytes().all(|b| b.is_ascii_alphanumeric()),
            "{secret}"
        );
        assert!(
            secret.bytes().any(|b| b != secret.as_bytes()[0]),
            "{secret}"
        );
        assert!(suffix.parse::<u32>().is_ok(), "{suffix}");
    }
    assert!(upper);
}

/// Term archives from the same script, over the fixture with
/// `reference/default-install/terms.sql`, which `db.sh` loads as the
/// `wordpress_terms` database.
#[tokio::test]
async fn term_archives_match_php() {
    let config = Database {
        name: "wordpress_terms".into(),
        ..fixture_config()
    };
    let db = Db::connect(&config)
        .await
        .expect(hyscribe_testkit::NO_FIXTURE_DATABASE);
    let oracle = Some("http://127.0.0.1:8081".to_string());
    let site = Site::load(db, oracle.clone(), oracle).await.unwrap();
    let golden = include_str!("../../hyscribe-render/tests/golden/terms.tsv");
    assert_pages(&Arc::new(site), golden).await;
    assert_eq!(golden.lines().count(), 36);
}

/// `redirect_canonical()` under pretty permalinks, past what the PHP
/// goldens ask: dates by their parts, and terms by their query vars.
#[tokio::test]
async fn pretty_canonical_redirects() {
    let site = pretty_site("/%postname%/", None).await;
    let location = |reply: &Reply| {
        reply
            .headers
            .get("location")
            .map(|v| v.to_str().unwrap().to_string())
    };
    let reply = request(&site, "GET", "/?m=2026&cat=1").await;
    assert_eq!(
        location(&reply).as_deref(),
        Some("http://default-install.test/2026/?cat=1")
    );
    let reply = request(&site, "GET", "/?year=2026&monthnum=10&day=27").await;
    assert_eq!(
        location(&reply).as_deref(),
        Some("http://default-install.test/2026/10/27/")
    );
    // A month without its year, and a date with no posts, stay as asked.
    for path in ["/?monthnum=10", "/?m=2030"] {
        assert_eq!(location(&request(&site, "GET", path).await), None, "{path}");
    }
    // Recorded from PHP under the same structure.
    for (path, to) in [
        ("/hello-world/?x[]=1", None),
        ("/?preview=0", None),
        ("/hello-world/?preview=0", None),
        ("/?m=20261", None),
        ("/hello-world/comment-page-2", None),
        ("/?m=2026-10", Some("/2026/10/")),
        (
            "/?category_name=uncategorized",
            Some("/category/uncategorized/"),
        ),
        ("/nope.", Some("/nope")),
        ("/index.php", Some("/")),
        ("/hello-world.", Some("/hello-world/")),
        ("/?p=1&tag=x+", Some("/hello-world/?tag=x")),
        ("/hello-world//", Some("/hello-world/")),
        ("/?day=27", None),
        ("/?year=2026&day=27", None),
        ("/?monthnum=10&day=27", None),
        ("/2026/?day=27", None),
        ("/hello-world/?category_name=nope", None),
        ("/2026/10/?day=27", Some("/2026/10/27/")),
        ("/2026/?monthnum=10", Some("/2026/10/")),
        ("/?monthnum=10&year=2026", Some("/2026/10/")),
        (
            "/?year=2026&monthnum=10&day=27&x=1",
            Some("/2026/10/27/?x=1"),
        ),
    ] {
        let to = to.map(|to| format!("http://default-install.test{to}"));
        assert_eq!(location(&request(&site, "GET", path).await), to, "{path}");
    }
    let config = Database {
        name: "wordpress_terms".into(),
        ..fixture_config()
    };
    let mut terms = Site::load(Db::connect(&config).await.unwrap(), None, None)
        .await
        .unwrap();
    terms
        .options
        .insert("permalink_structure".into(), b"/%postname%/".to_vec());
    terms.build_rewriter();
    let terms = Arc::new(terms);
    for (path, to) in [
        ("/?tag=foo", "http://default-install.test/tag/foo/"),
        (
            "/?post_format=aside",
            "http://default-install.test/type/aside/",
        ),
    ] {
        let reply = request(&terms, "GET", path).await;
        assert_eq!(location(&reply).as_deref(), Some(to), "{path}");
    }
    // A post or page under the category it is in, asked for by its own
    // vars, stays.
    let category = pretty_site("/%category%/%postname%/", None).await;
    for path in [
        "/sample-page/?category_name=nope",
        "/uncategorized/hello-world/?category_name=uncategorized",
        // An empty one names none, and the query is left as it is.
        "/uncategorized/hello-world/?category_name=&x[]=1",
    ] {
        assert_eq!(
            location(&request(&category, "GET", path).await),
            None,
            "{path}"
        );
    }
}

/// The page on the front is titled as the home page, tagline and all.
#[tokio::test]
async fn the_page_on_the_front_takes_the_home_title() {
    let mut site = Arc::into_inner(fixture_site(None).await).unwrap();
    for (name, value) in [
        ("show_on_front", "page"),
        ("page_on_front", "2"),
        ("blogdescription", "Tag"),
    ] {
        site.options.insert(name.into(), value.as_bytes().to_vec());
    }
    let site = Arc::new(site);
    let reply = request(&site, "GET", "/").await;
    assert_eq!(title(&reply.body), "HyScribe Default Install &#8211; Tag");
    let reply = request(&site, "GET", "/?paged=2").await;
    assert_eq!(
        title(&reply.body),
        "HyScribe Default Install &#8211; Page 2 &#8211; Tag"
    );
    // No page to show: the posts are the home page.
    let mut site = Arc::into_inner(site).unwrap();
    site.options.insert("page_on_front".into(), b"0".to_vec());
    let reply = request(&Arc::new(site), "GET", "/").await;
    assert!(reply.body.contains("Hello world!"));
}

/// The posts whose canonical redirect is cancelled, as PHP answered in the
/// loader golden of da35edd for the draft page and for the `wp_navigation`
/// post its first page view writes, which the fixture lacks. A feed hides
/// any post a visitor is not shown.
#[test]
fn hidden_posts_are_the_ones_visitors_are_not_shown() {
    let post = |post_type: &str, status: &str| Post {
        post_type: post_type.into(),
        status: status.into(),
        ..Post::default()
    };
    assert!(hides(&post("page", "draft"), false));
    assert!(!hides(&post("page", "publish"), false));
    assert!(!hides(&post("post", "auto-draft"), false));
    assert!(!hides(&post("wp_navigation", "publish"), false));
    assert!(hides(&post("wp_navigation", "publish"), true));
}

/// A page for posts that does not exist still lists the posts, under the
/// site's name alone and with no redirect (checked against PHP).
#[tokio::test]
async fn a_missing_page_for_posts_lists_the_posts() {
    for structure in ["", "/%postname%/"] {
        let mut site = Arc::into_inner(fixture_site(None).await).unwrap();
        for (name, value) in [
            ("show_on_front", "page"),
            ("page_for_posts", "999"),
            ("permalink_structure", structure),
        ] {
            site.options.insert(name.into(), value.as_bytes().to_vec());
        }
        site.build_rewriter();
        let reply = request(&Arc::new(site), "GET", "/?page_id=999").await;
        assert_eq!(reply.status, StatusCode::OK, "{structure}");
        assert_eq!(title(&reply.body), "HyScribe Default Install");
        assert!(reply.body.contains("Hello world!"));
    }
}

/// A name's date vars must match the post it names, not another post
/// (the `later` post of `terms.sql` is from the 28th, `hello-world` from
/// the 27th); checked against PHP on the default install.
#[tokio::test]
async fn a_name_and_date_must_match_the_same_post() {
    let config = Database {
        name: "wordpress_terms".into(),
        ..fixture_config()
    };
    let db = Db::connect(&config).await.unwrap();
    let site = Arc::new(Site::load(db, None, None).await.unwrap());
    let reply = request(&site, "GET", "/?name=hello-world&day=28").await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    let reply = request(&site, "GET", "/?name=later&day=28").await;
    assert_eq!(reply.status, StatusCode::OK);
}

/// A post with no category takes the default one's slug in its link.
#[tokio::test]
async fn links_name_the_default_category() {
    let config = Database {
        name: "wordpress_terms".into(),
        ..fixture_config()
    };
    let db = Db::connect(&config).await.unwrap();
    let mut site = Site::load(db, None, None).await.unwrap();
    site.options.insert(
        "permalink_structure".into(),
        b"/%category%/%postname%/".to_vec(),
    );
    site.build_rewriter();
    let reply = request(&Arc::new(site), "GET", "/wp-sitemap-posts-post-1.xml").await;
    assert!(
        reply
            .body
            .contains("<loc>http://default-install.test/uncategorized/later/</loc>"),
        "{}",
        reply.body
    );
}

/// Week archives under other first days of the week, from the same script.
#[tokio::test]
async fn week_archives_follow_the_start_of_the_week() {
    let golden = include_str!("../../hyscribe-render/tests/golden/weeks.tsv");
    for day in ["0", "3"] {
        let mut site = Arc::into_inner(oracle_site().await).unwrap();
        site.options
            .insert("start_of_week".into(), day.as_bytes().to_vec());
        let lines: Vec<&str> = golden
            .lines()
            .filter_map(|line| line.strip_prefix(&format!("{day}\t")))
            .collect();
        assert_eq!(lines.len(), 3);
        assert_pages(&Arc::new(site), &lines.join("\n")).await;
    }
}

/// Pages past the first, with one post a page, from the same script.
#[tokio::test]
async fn paged_pages_match_php() {
    let mut site = Arc::into_inner(oracle_site().await).unwrap();
    site.options.insert("posts_per_page".into(), b"1".to_vec());
    let golden = include_str!("../../hyscribe-render/tests/golden/paged.tsv");
    assert_pages(&Arc::new(site), golden).await;
    assert_eq!(golden.lines().count(), 6);
}

/// WordPress's name in a page, feed or sitemap, and HyScribe's in its
/// place: as the generator and in the footer credit.
fn generator_names() -> [(&'static str, String); 7] {
    let version = env!("CARGO_PKG_VERSION");
    [
        (
            "<admin:generatorAgent rdf:resource=\"https://wordpress.org/?v=7.1.3\" />",
            format!(
                "<admin:generatorAgent rdf:resource=\"https://github.com/hyscaler/HyScribe?v={version}\" />"
            ),
        ),
        (
            "<meta name=\"generator\" content=\"WordPress 7.1.3\" />",
            format!("<meta name=\"generator\" content=\"HyScribe {version}\" />"),
        ),
        (
            "<generator>https://wordpress.org/?v=7.1.3</generator>",
            format!("<generator>https://github.com/hyscaler/HyScribe?v={version}</generator>"),
        ),
        (
            "<!-- generator=\"WordPress/7.1.3\" -->",
            format!("<!-- generator=\"HyScribe/{version}\" -->"),
        ),
        (
            "<generator uri=\"https://wordpress.org/\" version=\"7.1.3\">WordPress</generator>",
            format!(
                "<generator uri=\"{}\" version=\"{version}\">HyScribe</generator>",
                env!("CARGO_PKG_REPOSITORY")
            ),
        ),
        ("generated by WordPress", "generated by HyScribe".into()),
        (
            "Designed with <a href=\"https://wordpress.org\" rel=\"nofollow\">WordPress</a>",
            "Designed with <a href=\"https://github.com/hyscaler/HyScribe\" rel=\"nofollow\">HyScribe</a>".into(),
        ),
    ]
}

/// PHP's page, feed or sitemap as HyScribe sends it: HyScribe as the
/// generator and in the footer credit, and without the RSD tag.
#[allow(clippy::string_slice)]
pub(crate) fn as_hyscribe(mut php: String) -> String {
    while let Some(start) = php.find("<link rel=\"EditURI\"") {
        let end = start + php[start..].find("/>\n").unwrap() + 3;
        php.replace_range(start..end, "");
    }
    // No users sitemap: author archives are off (docs/SECURITY_MODEL.md).
    for users in [
        "<sitemap><loc>http://127.0.0.1:8081/?sitemap=users&amp;paged=1</loc></sitemap>",
        "<sitemap><loc>http://127.0.0.1:8081/wp-sitemap-users-1.xml</loc></sitemap>",
    ] {
        php = php.replace(users, "");
    }
    for (wordpress, hyscribe) in generator_names() {
        php = php.replace(wordpress, &hyscribe);
    }
    php
}

/// Our page as a golden's hash takes PHP's: with WordPress's names, and
/// without the RSD tag, which the golden script takes out.
fn as_wordpress(mut ours: String) -> String {
    for (wordpress, hyscribe) in generator_names() {
        ours = ours.replace(&hyscribe, wordpress);
    }
    ours
}

/// Where two pages first differ, with the text around it.
pub(crate) fn first_difference(php: &str, ours: &str) -> String {
    let at = ours
        .bytes()
        .zip(php.bytes())
        .position(|(a, b)| a != b)
        .unwrap_or(ours.len().min(php.len()));
    let around = |page: &str| {
        let bytes = page.as_bytes();
        let from = at.saturating_sub(200).min(bytes.len());
        let end = (at + 300).min(bytes.len());
        String::from_utf8_lossy(bytes.get(from..end).unwrap_or_default()).into_owned()
    };
    format!(
        "at byte {at}\n  php:  {:?}\n  ours: {:?}",
        around(php),
        around(ours)
    )
}

/// Each line of `golden`, a path, PHP's status and its page, against ours.
async fn assert_pages(site: &Arc<Site>, golden: &str) {
    let mut failures = Vec::new();
    for line in golden.lines() {
        let mut fields = line.split('\t');
        let path = String::from_utf8(hyscribe_testkit::unhex(fields.next().unwrap())).unwrap();
        let status: u16 = fields.next().unwrap().parse().unwrap();
        let php = as_hyscribe(
            String::from_utf8(hyscribe_testkit::unhex(fields.next().unwrap())).unwrap(),
        );
        let reply = request(site, "GET", &path).await;
        if reply.status.as_u16() != status || reply.body != php {
            failures.push(format!(
                "{path} {status}/{} {}",
                reply.status,
                first_difference(&php, &reply.body)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// A post in pages, by the page asked for, from `reference/head/golden.sh`,
/// which inserts the post in PHP for these requests.
#[tokio::test]
async fn pages_of_a_post_match_php() {
    let site = oracle_site().await;
    let golden = include_str!("../../hyscribe-render/tests/golden/singular-pages.tsv");
    let mut lines = golden.lines();
    let (id, content) = lines.next().unwrap().split_once('\t').unwrap();
    let post = Post {
        id: id.parse().unwrap(),
        title: b"Paged".to_vec(),
        name: b"paged".to_vec(),
        content: hyscribe_testkit::unhex(content),
        status: "publish".into(),
        post_type: "post".into(),
        ..Post::default()
    };
    let mut count = 0;
    for line in lines {
        let fields: Vec<&str> = line.split('\t').collect();
        let raw = String::from_utf8(hyscribe_testkit::unhex(fields[0])).unwrap();
        let query: HashMap<String, String> = raw
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let title = String::from_utf8(hyscribe_testkit::unhex(fields[3])).unwrap();
        let link = site.permalink(&post).await;
        let ours = match paged_single(&site, &query, &query, Some(&raw), &link, "p", post.clone()) {
            Outcome::Page(page) => {
                // The page shown, which the title names past the first;
                // `paged` names one in the title only.
                let second = title.contains("Page 2") && !raw.contains("&paged=");
                assert_eq!(page.request.page == 2, second, "{raw}");
                (page.status.as_u16(), String::new(), page.title)
            }
            Outcome::Redirect(location) => (301, location, String::new()),
            _ => unreachable!(),
        };
        assert_eq!(
            ours,
            (fields[1].parse().unwrap(), fields[2].to_string(), title),
            "{raw}"
        );
        count += 1;
    }
    assert_eq!(count, 11);
}

/// WordPress sorts a search by relevance only outside feeds.
#[tokio::test]
async fn search_feeds_are_not_sorted_by_relevance() {
    let site = fixture_site(None).await;
    let query = HashMap::from([("s".to_string(), "hello".to_string())]);
    assert_eq!(
        post_query(&site, &query, false, vec![], false).order,
        [(OrderBy::Relevance, false), (OrderBy::Date, false)]
    );
    assert!(
        post_query(&site, &query, true, vec![], false)
            .order
            .is_empty()
    );
    assert!(
        post_query(&site, &HashMap::new(), false, vec![], false)
            .order
            .is_empty()
    );
}

/// parse_search() runs only for a search with something in it, so an
/// empty `sentence` search asks for no term.
#[tokio::test]
async fn an_empty_sentence_search_has_no_terms() {
    let site = fixture_site(None).await;
    let query = |s: &str| {
        HashMap::from([
            ("s".to_string(), s.to_string()),
            ("sentence".to_string(), "1".to_string()),
        ])
    };
    assert!(
        post_query(&site, &query(""), false, vec![], false)
            .search
            .is_empty()
    );
    assert_eq!(
        post_query(&site, &query("a\r\nb"), false, vec![], false).search,
        ["ab"]
    );
    // A sentence is one word to the relevance order; a search counts its
    // words, and one over 1600 bytes is none.
    let words = post_query(&site, &query("red fox"), false, vec![], false);
    assert_eq!(
        (words.search_count, words.sentence.as_str()),
        (1, "red fox")
    );
    let plain = |s: &str| HashMap::from([("s".to_string(), s.to_string())]);
    assert_eq!(
        post_query(&site, &plain("red fox"), false, vec![], false).search_count,
        2
    );
    let long = post_query(&site, &plain(&"x".repeat(1600)), false, vec![], false);
    assert_eq!(long.sentence.len(), 1600);
    let longer = post_query(&site, &plain(&"x".repeat(1601)), false, vec![], false);
    assert_eq!(longer.sentence, "");
}

/// Sitemaps are XML, off for a site search engines may not index, and have
/// no users sitemap, which would list author archives; robots.txt is text,
/// naming the sitemap only for a public site.
#[tokio::test]
async fn sitemaps_and_robots_follow_blog_public() {
    let site = oracle_site().await;
    let index = request(&site, "GET", "/?sitemap=index").await;
    assert_eq!(
        index.headers[header::CONTENT_TYPE],
        "application/xml; charset=UTF-8"
    );
    assert!(!index.body.contains("users"));
    let users = request(&site, "GET", "/?sitemap=users&paged=1").await;
    assert_eq!(users.status, StatusCode::NOT_FOUND);
    let robots = request(&site, "GET", "/?robots=1").await;
    assert_eq!(
        robots.headers[header::CONTENT_TYPE],
        "text/plain; charset=utf-8"
    );
    let mut hidden = Arc::into_inner(site).unwrap();
    hidden.options.insert("blog_public".into(), b"0".to_vec());
    // do_robots() names the admin under the site URL's path.
    hidden
        .options
        .insert("siteurl".into(), b"http://127.0.0.1:8081/wp".to_vec());
    let hidden = Arc::new(hidden);
    for uri in ["/?sitemap=index", "/?sitemap-stylesheet=index"] {
        assert_eq!(
            request(&hidden, "GET", uri).await.status,
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        request(&hidden, "GET", "/?robots=1").await.body,
        "User-agent: *\nDisallow: /wp/wp-admin/\nAllow: /wp/wp-admin/admin-ajax.php\n"
    );
}

/// Sticky posts lead the home list's first page only.
#[tokio::test]
async fn the_home_list_puts_sticky_posts_first() {
    let mut site = fixture_site(None).await;
    Arc::get_mut(&mut site)
        .unwrap()
        .options
        .insert("sticky_posts".into(), b"a:1:{i:0;i:1;}".to_vec());
    let sticky = async |pairs: &[(&str, &str)], feed| {
        let query = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        main_query(&site, &query, feed).await.unwrap().0.sticky
    };
    assert_eq!(sticky(&[], false).await, [1]);
    assert_eq!(sticky(&[("paged", "1")], false).await, [1]);
    for pairs in [
        &[("paged", "2")][..],
        &[("s", "x")],
        &[("cat", "1")],
        &[("tag", "x")],
        &[("m", "2026")],
    ] {
        assert!(sticky(pairs, false).await.is_empty(), "{pairs:?}");
    }
    assert!(sticky(&[], true).await.is_empty());
}

/// Every case of `reference/tax-query/golden.sh`: the archive, its term
/// and the posts the filters match, over the terms and posts it inserts.
/// Singular requests leave the tax vars alone, and searches' posts are
/// left to the search tests.
#[test]
fn tax_queries_match_php() {
    let golden = include_str!("../tests/golden/tax-query.tsv");
    let mut lines = golden.lines();
    let data = lines.next().unwrap().split_once('\t').unwrap().1;
    let data = hyscribe_phpser::json_decode(&hyscribe_testkit::unhex(data)).unwrap();
    let hierarchy = data.get_str("hierarchy").unwrap().to_php_string();
    let terms: Vec<(String, Term)> = data
        .get_str("terms")
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .map(|(_, row)| {
            let field = |i: i64| row.get(&Key::Int(i)).unwrap().to_php_string();
            let taxonomy = String::from_utf8(field(0)).unwrap();
            let term = Term {
                id: row.get(&Key::Int(1)).unwrap().to_int().unsigned_abs(),
                slug: field(2),
                ..Term::default()
            };
            (taxonomy, term)
        })
        .collect();
    let posts: Vec<(u64, Vec<u64>)> = data
        .get_str("posts")
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .map(|(id, ids)| {
            let id = match id {
                Key::Int(id) => id.unsigned_abs(),
                Key::String(id) => String::from_utf8_lossy(id).parse().unwrap(),
            };
            let ids = ids
                .as_array()
                .unwrap()
                .iter()
                .map(|(_, t)| t.to_int().unsigned_abs());
            (id, ids.collect())
        })
        .collect();
    let mut count = 0;
    for line in lines {
        let (case, state) = line.split_once('\t').unwrap();
        let case = String::from_utf8(hyscribe_testkit::unhex(case)).unwrap();
        let state = hyscribe_phpser::json_decode(&hyscribe_testkit::unhex(state)).unwrap();
        let uri: axum::http::Uri = format!("/?{case}").parse().unwrap();
        let query = axum::extract::Query::<HashMap<String, String>>::try_from_uri(&uri)
            .unwrap()
            .0;
        if query.contains_key("p") {
            continue;
        }
        let tax = query::tax_query(&query);
        let (slugs, ids) = tax.lookups();
        // What find_terms() selects.
        let found: Vec<(String, Term)> = terms
            .iter()
            .filter(|(taxonomy, term)| {
                slugs.contains(&(taxonomy.clone(), term.slug.clone()))
                    || taxonomy == "category" && ids.contains(&term.id)
            })
            .cloned()
            .collect();
        let resolved = tax.resolve(&found, &hierarchy);
        let flag = |name| state.get_str(name) == Some(&Value::Bool(true));
        let kind = if flag("category") {
            Some(Taxonomy::Category)
        } else if flag("tag") {
            Some(Taxonomy::Tag)
        } else if flag("tax") {
            Some(Taxonomy::Format)
        } else {
            None
        };
        let queried = state
            .get_str("queried")
            .and_then(Value::as_array)
            .map(|q| q[1].1.to_int().unsigned_abs());
        let ours = resolved.archive.as_ref().map(|archive| match archive {
            Archive::Term(kind, term) => (*kind, term.as_ref().map(|t| t.id)),
            _ => unreachable!(),
        });
        assert_eq!(ours, kind.map(|kind| (kind, queried)), "{case}");
        // The templates tried up to the first Twenty Twenty-Five has.
        let php: Vec<String> = state
            .get_str("templates")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .map(|(_, t)| String::from_utf8(t.to_php_string()).unwrap())
            .collect();
        if !php.is_empty() {
            let mut templates = list_templates(
                &resolved.kinds,
                resolved.archive.as_ref(),
                query.contains_key("s"),
                query.contains_key("m"),
                query.contains_key("post_format"),
            );
            let end = templates
                .iter()
                .position(|t| t == "search" || t == "archive");
            templates.truncate(end.unwrap() + 1);
            assert_eq!(templates, php, "{case}");
        }
        if !query.contains_key("s") {
            let taxonomy_of = |id: &u64| {
                terms
                    .iter()
                    .find(|(_, t)| t.id == *id)
                    .map(|(tax, _)| tax.clone())
            };
            let mut matched: Vec<u64> = posts
                .iter()
                .filter(|(_, post_terms)| {
                    resolved.filters.iter().all(|filter| {
                        let has = post_terms.iter().any(|id| {
                            filter.terms.contains(id)
                                && taxonomy_of(id).as_deref() == Some(&filter.taxonomy)
                        });
                        has != filter.exclude
                    })
                })
                .map(|(id, _)| *id)
                .collect();
            let mut php: Vec<u64> = state
                .get_str("posts")
                .and_then(Value::as_array)
                .unwrap()
                .iter()
                .map(|(_, id)| id.to_int().unsigned_abs())
                .collect();
            matched.sort_unstable();
            php.sort_unstable();
            assert_eq!(matched, php, "{case}");
        }
        count += 1;
    }
    assert_eq!(count, 105);
}

/// Every case of `reference/category-query/golden.sh`: the categories in and
/// out, children included, and whether the list stays the home list.
#[test]
fn category_lists_match_php() {
    let golden = include_str!("../tests/golden/category-query.tsv");
    for line in golden.lines() {
        let mut fields = line.split('\t').map(hyscribe_testkit::unhex);
        let (cat, state) = (fields.next().unwrap(), fields.next().unwrap());
        let cat = String::from_utf8(cat).unwrap();
        let state = hyscribe_phpser::json_decode(&state).unwrap();
        let hierarchy = state.get_str("hierarchy").unwrap().to_php_string();
        let query = HashMap::from([("cat".to_string(), cat.clone())]);
        let filters = query::tax_query(&query).resolve(&[], &hierarchy).filters;
        let ids = |exclude: bool| {
            let mut ids: Vec<u64> = filters
                .iter()
                .filter(|f| f.exclude == exclude)
                .flat_map(|f| f.terms.clone())
                .collect();
            ids.sort_unstable();
            ids.dedup();
            ids
        };
        let php = |name| {
            state
                .get_str(name)
                .and_then(hyscribe_phpser::Value::as_array)
                .unwrap()
                .iter()
                .map(|(_, id)| id.to_int().unsigned_abs())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            (ids(false), ids(true)),
            (php("in"), php("not_in")),
            "{cat:?}"
        );
        let flag = |name| state.get_str(name) == Some(&hyscribe_phpser::Value::Bool(true));
        assert_eq!(filters.iter().all(|f| f.exclude), flag("home"), "{cat:?}");
        assert_eq!(
            filters.iter().any(|f| !f.exclude),
            flag("category"),
            "{cat:?}"
        );
    }
    assert_eq!(golden.lines().count(), 28);
}

/// `WP_Tax_Query::clean_query()` keeps each asked category once
/// (`array_unique()`) before adding its children, so a list repeating one
/// ID takes linear time, not a scan of the list for every entry.
#[test]
fn repeated_categories_count_once() {
    let cat = "5,".repeat(100_000);
    let query = HashMap::from([("cat".to_string(), cat)]);
    let hierarchy = b"a:1:{i:5;a:1:{i:0;i:6;}}";
    let filters = query::tax_query(&query).resolve(&[], hierarchy).filters;
    assert_eq!(filters.len(), 1);
    assert_eq!(filters[0].terms, [5, 6]);
}

/// Many tags take linear time to keep each once.
#[test]
fn long_tag_lists_count_each_once() {
    let tags: Vec<String> = (0..100_000).map(|n| format!("t{n}")).collect();
    let tag = [tags.join(","), ",t0".to_string()].concat();
    let query = HashMap::from([("tag".to_string(), tag)]);
    let (slugs, _) = query::tax_query(&query).lookups();
    assert_eq!(
        slugs
            .iter()
            .filter(|(taxonomy, _)| taxonomy == "post_tag")
            .count(),
        100_000 + 1
    );
}

/// Every case of `reference/canonical/clean-golden.php`.
#[test]
fn query_clean_up_matches_php() {
    let golden = include_str!("../tests/golden/clean-query.tsv");
    for line in golden.lines() {
        let mut fields = line.split('\t').map(hyscribe_testkit::unhex);
        let query = String::from_utf8(fields.next().unwrap()).unwrap();
        let php = String::from_utf8(fields.next().unwrap()).unwrap();
        assert_eq!(query::clean_query(&query), php, "{query:?}");
    }
    assert_eq!(golden.lines().count(), 48);
}

/// Every case of `reference/page-path/golden.php`: the page
/// `get_page_by_path()` picks from the rows it selects.
#[test]
fn page_paths_match_php() {
    let golden = include_str!("../tests/golden/page-path.tsv");
    for line in golden.lines() {
        let mut fields = line.split('\t').map(hyscribe_testkit::unhex);
        let path = String::from_utf8(fields.next().unwrap()).unwrap();
        let state = hyscribe_phpser::json_decode(&fields.next().unwrap()).unwrap();
        let text = |row: &hyscribe_phpser::Value, name| row.get_str(name).unwrap().to_php_string();
        let rows: Vec<Post> = state
            .get_str("rows")
            .and_then(hyscribe_phpser::Value::as_array)
            .unwrap()
            .iter()
            .map(|(_, row)| {
                let number = |name| row.get_str(name).unwrap().to_int().unsigned_abs();
                Post {
                    id: number("ID"),
                    name: text(row, "post_name"),
                    parent: number("post_parent"),
                    post_type: String::from_utf8(text(row, "post_type")).unwrap(),
                    ..Post::default()
                }
            })
            .collect();
        let php = state.get_str("id").unwrap().to_int().unsigned_abs();
        let ours = page_by_path(&page_path(&path), &rows).map_or(0, |page| page.id);
        assert_eq!(ours, php, "{path:?}");
    }
    assert_eq!(golden.lines().count(), 26);
    assert_eq!(hyscribe_phpser::wp_basename(b"a/b\\c//"), b"c");
}

#[test]
fn visitors_see_published_posts_of_the_types_served() {
    let post = |status: &str, post_type: &str| Post {
        status: status.into(),
        post_type: post_type.into(),
        ..Post::default()
    };
    assert!(viewable(&post("publish", "page"), &["page"]));
    assert!(!viewable(&post("draft", "page"), &["page"]));
    assert!(!viewable(&post("publish", "attachment"), &["post", "page"]));
}

/// The fixture with a pretty permalink structure.
async fn pretty_site(structure: &str, home: Option<&str>) -> Arc<Site> {
    let mut site = Arc::into_inner(fixture_site(home).await).unwrap();
    site.options
        .insert("permalink_structure".into(), structure.as_bytes().to_vec());
    site.build_rewriter();
    Arc::new(site)
}

#[tokio::test]
async fn pretty_permalinks_route_requests() {
    let site = pretty_site("/%postname%/", None).await;
    for (path, status, title_text) in [
        (
            "/hello-world/",
            StatusCode::OK,
            "Hello world! &#8211; HyScribe Default Install",
        ),
        (
            "/sample-page/",
            StatusCode::OK,
            "Sample Page &#8211; HyScribe Default Install",
        ),
        (
            "/category/uncategorized/",
            StatusCode::OK,
            "Uncategorized &#8211; HyScribe Default Install",
        ),
        (
            "/nope/deeper/",
            StatusCode::NOT_FOUND,
            "Page not found &#8211; HyScribe Default Install",
        ),
        // Author archives stay off.
        (
            "/author/admin/",
            StatusCode::NOT_FOUND,
            "Page not found &#8211; HyScribe Default Install",
        ),
    ] {
        let reply = request(&site, "GET", path).await;
        assert_eq!(reply.status, status, "{path}");
        assert_eq!(title(&reply.body), title_text, "{path}");
        assert!(!reply.headers.contains_key("location"), "{path}");
    }
    // The request's own query wins over the path's.
    let reply = request(&site, "GET", "/hello-world/?name=nope").await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    // The request's `error` goes, as `WP::parse_request()` drops it.
    let reply = request(&site, "GET", "/hello-world/?error=403").await;
    assert_eq!(reply.status, StatusCode::OK);
    let reply = request(&site, "GET", "/nope/deeper/?error=500").await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    // A parsed path is not sent to plain permalinks' canonical URL.
    let reply = request(&site, "GET", "/tag/foo./").await;
    assert!(!reply.headers.contains_key("location"));
    // Empty path vars stay out of the query a redirect keeps, which goes to
    // the page's link.
    let ids = pretty_site("/archives/%post_id%", None).await;
    let reply = request(&ids, "GET", "/archives/2?x=1").await;
    assert_eq!(
        reply.headers["location"],
        "http://default-install.test/sample-page?x=1"
    );
    // An author in the structure names a post's author, not an archive.
    let site = pretty_site("/%author%/%postname%/", None).await;
    let reply = request(&site, "GET", "/admin/hello-world/").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        request(&site, "GET", "/author/admin/").await.status,
        StatusCode::NOT_FOUND
    );
    let feed = request(&site, "GET", "/feed/").await;
    assert!(
        feed.headers["content-type"]
            .to_str()
            .unwrap()
            .starts_with("application/rss+xml")
    );
    let robots = request(&site, "GET", "/robots.txt").await;
    assert_eq!(robots.headers["content-type"], "text/plain; charset=utf-8");
    let sitemap = request(&site, "GET", "/wp-sitemap.xml").await;
    assert!(sitemap.body.contains("<sitemapindex"), "{}", sitemap.body);
    let terms = request(&site, "GET", "/wp-sitemap-taxonomies-category-1.xml").await;
    assert!(
        terms
            .body
            .contains("<loc>http://default-install.test/category/uncategorized/</loc>"),
        "{}",
        terms.body
    );
    // PHP files and reserved paths keep their own handling.
    let file = request(&site, "GET", "/wp-trackback.php?p=1").await;
    assert_eq!(file.status, StatusCode::FOUND);
    assert_eq!(
        request(&site, "GET", "/hs-admin/").await.status,
        StatusCode::FOUND
    );
}

#[tokio::test]
async fn pretty_permalinks_edge_cases() {
    let site = pretty_site("/%postname%/", None).await;
    // The home page has no path to parse.
    let reply = request(&site, "GET", "/").await;
    assert_eq!(title(&reply.body), "HyScribe Default Install");
    for (status, matchable) in [
        ("publish", true),
        ("draft", true),
        ("inherit", true),
        ("trash", false),
        ("auto-draft", false),
        ("request-pending", false),
        ("request-other", true),
    ] {
        assert_eq!(matchable_status(status), matchable, "{status}");
    }
    // A database failure while matching shows the error page.
    let mut config = fixture_config();
    config.prefix = hyscribe_config::Prefix::new("missing_").unwrap();
    let db = Db::connect(&config).await.unwrap();
    let mut broken = Arc::into_inner(pretty_site("/%postname%/", None).await).unwrap();
    broken.db = db;
    let reply = request(&Arc::new(broken), "GET", "/sample-page/").await;
    assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn pretty_permalinks_under_a_folder() {
    let site = pretty_site("/%postname%/", Some("http://example.test/blog")).await;
    let reply = request(&site, "GET", "/blog/hello-world/").await;
    assert_eq!(
        title(&reply.body),
        "Hello world! &#8211; HyScribe Default Install"
    );
    let site = pretty_site("/index.php/%postname%/", Some("http://example.test/blog")).await;
    let reply = request(&site, "GET", "/blog/index.php/hello-world/").await;
    assert_eq!(
        title(&reply.body),
        "Hello world! &#8211; HyScribe Default Install"
    );
    // PHP files under the folder are not rewritten, which would make them
    // a post name and a 404.
    let reply = request(&site, "GET", "/blog/wp-cron.php").await;
    assert_eq!(reply.status, StatusCode::MOVED_PERMANENTLY);
}

/// Every request of `reference/rewrite/pages-golden.sh`: pages and
/// redirects under pretty permalinks.
#[tokio::test]
async fn pretty_pages_match_php() {
    let golden = include_str!("../tests/golden/pretty-pages.tsv");
    let mut failures = Vec::new();
    let mut redirects = Vec::new();
    let mut attachments = 0;
    let mut sites: HashMap<&str, Arc<Site>> = HashMap::new();
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        if !sites.contains_key(fields[0]) {
            let mut site = Arc::into_inner(oracle_site().await).unwrap();
            let setup: Vec<&str> = fields[0].split('|').collect();
            for (name, value) in [
                ("permalink_structure", setup[0]),
                ("category_base", setup[1]),
                ("tag_base", setup[2]),
            ] {
                site.options.insert(name.into(), value.as_bytes().to_vec());
            }
            let posts = setup.get(4).copied().unwrap_or_default();
            if !setup[3].is_empty() || !posts.is_empty() {
                for (name, value) in [
                    ("page_on_front", setup[3]),
                    ("page_for_posts", posts),
                    ("show_on_front", "page"),
                ] {
                    site.options.insert(name.into(), value.as_bytes().to_vec());
                }
            }
            site.build_rewriter();
            sites.insert(fields[0], Arc::new(site));
        }
        let site = &sites[fields[0]];
        let path = String::from_utf8(hyscribe_testkit::unhex(fields[1])).unwrap();
        let php = as_hyscribe(String::from_utf8(hyscribe_testkit::unhex(fields[4])).unwrap());
        let reply = request(site, "GET", &path).await;
        // An embed page's random draws.
        let (php, body) = if path.contains("embed=true") {
            (
                without_share_draws(php),
                without_share_draws(reply.body.clone()),
            )
        } else {
            (php, reply.body.clone())
        };
        let location = reply
            .headers
            .get("location")
            .map_or("", |v| v.to_str().unwrap());
        // Under a category base the old category path's feed is an
        // attachment's, and attachments are not routed.
        if fields[0].contains("|topics|") && path == "/category/uncategorized/feed/" {
            attachments += 1;
            continue;
        }
        let redirected = fields[2].starts_with('3') || reply.status.is_redirection();
        if redirected && (reply.status.as_str() != fields[2] || location != fields[3]) {
            redirects.push(format!("{} {path} {}/{location}", fields[0], fields[3]));
            continue;
        }
        if reply.status.as_str() != fields[2] || location != fields[3] || body != php {
            let ours = &body;
            let at = ours
                .bytes()
                .zip(php.bytes())
                .position(|(a, b)| a != b)
                .unwrap_or(ours.len().min(php.len()));
            let from = at.saturating_sub(100);
            failures.push(format!(
                "{} {path} {}/{} {:?}/{:?}\n  php:  {:?}\n  ours: {:?}",
                fields[0],
                fields[2],
                reply.status.as_str(),
                fields[3],
                location,
                php.get(from..(at + 200).min(php.len())).unwrap_or_default(),
                ours.get(from..(at + 200).min(ours.len()))
                    .unwrap_or_default()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!(redirects.len(), 0, "{}", redirects.join("\n"));
    assert_eq!(attachments, 1);
    assert_eq!(golden.lines().count(), 727);
}

tokio::task_local! {
    /// The time `now()` gives a request answered inside its scope.
    pub(crate) static PINNED_NOW: i64;
}

/// Noon UTC of the day `reference/pages/golden.sh` recorded the hashes of
/// a list, from its `.clock` file's text.
fn recorded_at(clock: &str) -> i64 {
    clock.trim().parse().unwrap()
}

/// The clock of `pages-ttd.tsv`.
fn ttd_recorded_at() -> i64 {
    recorded_at(include_str!("../tests/golden/pages-ttd.clock"))
}

/// A pinned time is what a request reads inside its scope, the clock's
/// outside it: the Theme Test Data hashes hold the day they were recorded.
#[tokio::test]
async fn a_pinned_time_is_read_only_inside_its_scope() {
    assert_eq!(PINNED_NOW.scope(7, async { now() }).await, 7);
    assert!(now() > ttd_recorded_at());
}

/// The pages `reference/pages/golden.sh` keeps for reading the differences
/// of a hashed list.
fn kept_pages(list: &str) -> String {
    format!(
        "{}/../../target/pages/{list}.tsv",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// Each request of a `reference/pages/golden.sh` list over `database`,
/// served with the options it names, against PHP's status, location and
/// page, or with `hashed` (a list's name and the time it was recorded at)
/// the page's hash, the page itself in `kept_pages`.
/// The requests of `known`, `options<TAB>path<TAB>why` lines, still differ.
/// Author archives are off (docs/SECURITY_MODEL.md): their count is returned.
async fn assert_listed_pages(
    database: &str,
    golden: &str,
    hashed: Option<(&str, i64)>,
    known: &str,
) -> usize {
    let known: Vec<(&str, &str)> = known
        .lines()
        .map(|line| {
            let mut fields = line.split('\t');
            (fields.next().unwrap(), fields.next().unwrap())
        })
        .collect();
    let mut sites: HashMap<String, Arc<Site>> = HashMap::new();
    let (mut failures, mut authors, mut kept) = (Vec::new(), 0, None);
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        // An option named `_cookie` is the cookies the request sends.
        let mut cookie = String::new();
        let mut named = Vec::new();
        for pair in fields[0].split('&').filter(|pair| !pair.is_empty()) {
            match pair.strip_prefix("_cookie=") {
                Some(value) => {
                    cookie =
                        String::from_utf8(hyscribe_phpser::urldecode(value.as_bytes())).unwrap();
                }
                None => named.push(pair),
            }
        }
        let named = named.join("&");
        if !sites.contains_key(&named) {
            let config = Database {
                name: database.into(),
                ..fixture_config()
            };
            let db = Db::connect(&config).await.unwrap();
            let oracle = Some("http://127.0.0.1:8081".to_string());
            let mut site = Site::load(db, oracle.clone(), oracle).await.unwrap();
            // Names from `_` are the golden script's, not options.
            let options = named.split('&').filter_map(|o| o.split_once('='));
            for (name, value) in options.filter(|(name, _)| !name.starts_with('_')) {
                let value = hyscribe_phpser::urldecode(value.as_bytes());
                if name == "home" {
                    site.home = String::from_utf8(value.clone()).unwrap();
                }
                site.options.insert(name.into(), value);
            }
            // The theme a `stylesheet` option names, as every request's reload picks it.
            if let Some(stylesheet) = site.options.get("stylesheet") {
                let slug = hyscribe_render::theme::resolve(&String::from_utf8_lossy(stylesheet));
                site.theme = site.styled_theme(slug).await.unwrap();
            }
            // `options.php` declines the fallback post unless `_fallback` is set.
            if !named
                .split('&')
                .any(|option| option.starts_with("_fallback="))
            {
                site.hooks = Arc::new(NoFallback);
            }
            site.build_rewriter();
            // Feeds and nothing else are fetched, from the test site.
            let pages = Arc::new(crate::rest::tests::url_details::Pages::default());
            let client = hyscribe_fetch::Client::new(
                hyscribe_fetch::RootCertStore::empty(),
                pages,
                Arc::new(()),
            );
            let site = site
                .with_fetch(Some(client))
                .with_feed_hosts(&["pages.test".to_string()]);
            sites.insert(named.clone(), Arc::new(site));
        }
        let path = String::from_utf8(hyscribe_testkit::unhex(fields[1])).unwrap();
        let app = router(sites[&named].clone());
        // Hashes were recorded on one day: the calendar's today, among others.
        let request = answer_with(app, "GET", &path, &cookie);
        let reply = match hashed {
            Some((_, clock)) => PINNED_NOW.scope(clock, request).await,
            None => request.await,
        };
        if path.contains("author=") {
            assert_eq!(reply.status, StatusCode::NOT_FOUND, "{path}");
            authors += 1;
            continue;
        }
        let location = reply
            .headers
            .get("location")
            .map_or("", |v| v.to_str().unwrap());
        let php = |hex: &str| as_hyscribe(String::from_utf8(hyscribe_testkit::unhex(hex)).unwrap());
        let same = if hashed.is_some() {
            // A page with the gallery of `orderby="rand"` is compared by its
            // items in any order, nothing else loosened.
            let (hash, page) = match fields[4].strip_prefix("unshuffled:") {
                Some(hash) => {
                    let page = as_wordpress(reply.body.clone());
                    let sorted = hyscribe_testkit::unshuffle::unshuffle(page.as_bytes());
                    (hash, String::from_utf8(sorted).unwrap())
                }
                None => (fields[4], as_wordpress(reply.body.clone())),
            };
            admin::tests::sha256(&page) == hash
        } else {
            php(fields[4]) == reply.body
        };
        let same = same && reply.status.as_str() == fields[2] && location == fields[3];
        let listed = known
            .iter()
            .any(|(options, listed)| *options == fields[0] && *listed == path);
        if listed {
            if same {
                failures.push(format!("{} {path} matches: no longer known", fields[0]));
            }
        } else if !same {
            let page = if let Some((list, _)) = hashed {
                let kept: &String = kept.get_or_insert_with(|| {
                    std::fs::read_to_string(kept_pages(list)).unwrap_or_default()
                });
                let key = format!("{}\t{}\t", fields[0], fields[1]);
                let line = kept.lines().find_map(|l| l.strip_prefix(&key));
                line.and_then(|l| l.rsplit('\t').next())
                    .map_or_else(String::new, php)
            } else {
                php(fields[4])
            };
            failures.push(format!(
                "{} {path} {}/{} {:?}/{location:?} {}",
                fields[0],
                fields[2],
                reply.status.as_str(),
                fields[3],
                first_difference(&page, &reply.body)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
    authors
}

/// The default install's requests of `reference/pages/default.list`: query
/// variants, previews as a visitor, comment pages, and redirects of slashes
/// and case under plain and pretty permalinks, and a bad preview nonce, which
/// PHP refuses on every request (`_show_post_preview()`). Nothing differs.
#[tokio::test]
async fn listed_pages_match_php() {
    let golden = include_str!("../tests/golden/pages-default.tsv");
    let known = include_str!("../tests/golden/pages-default-known.tsv");
    assert_eq!(
        assert_listed_pages("wordpress", golden, None, known).await,
        0
    );
    assert_eq!(golden.lines().count(), 101);
}

/// The Theme Test Data fixture's requests of `reference/pages/ttd.list`,
/// which `reference/pages/ttd-list.sh` builds from its database: every
/// post, page, attachment and archive, searches, feeds, comment pages and
/// both kinds of front page, by hash, as the fixture's content is not ours
/// to commit, loaded into `wordpress_saves` with the terms of one name told
/// apart (`reference/pages/ttd-ties.sql`), as MariaDB orders them
/// differently from run to run. The known differences name what is not
/// ported: oEmbed, image loading priorities, the player of an attachment
/// page for audio, the navigation fallback from a classic menu. The RSS
/// blocks read the test site both sides fetch from, and the gallery
/// `orderby="rand"`, which no two runs repeat, is compared by its items in
/// any order.
#[tokio::test(flavor = "multi_thread")]
async fn theme_test_data_pages_match_php() {
    let golden = include_str!("../tests/golden/pages-ttd.tsv");
    let known = include_str!("../tests/golden/pages-ttd-known.tsv");
    assert_ttd_pages("ttd", golden, ttd_recorded_at(), known).await;
}

/// The requests of a Theme Test Data list `list` recorded at `clock`, by
/// hash: ten author archives are off and 747 requests are listed.
async fn assert_ttd_pages(list: &str, golden: &str, clock: i64, known: &str) {
    let _saves = admin::tests::SAVES.lock().await;
    let db = hyscribe_config::Database {
        name: "wordpress_saves".into(),
        ..fixture_config()
    };
    let dump = std::fs::read_to_string(hyscribe_testkit::writes::THEME_TEST_DATA).unwrap();
    hyscribe_testkit::writes::load_theme_test_data(&db, &dump)
        .await
        .unwrap();
    hyscribe_testkit::writes::point_feeds_at_test_site(&db)
        .await
        .unwrap();
    let hashed = Some((list, clock));
    assert_eq!(
        assert_listed_pages("wordpress_saves", golden, hashed, known).await,
        10
    );
    assert_eq!(golden.lines().count(), 747);
}

/// The lists of a bundled theme, which `reference/pages/theme-list.sh`
/// builds from the default theme's: the default install's pages and the
/// Theme Test Data's, both by hash, with the theme's own known differences
/// (`pages-default-<short>-known.tsv`), none left.
macro_rules! theme_pages {
    ($default:ident, $ttd:ident, $short:literal) => {
        #[tokio::test]
        async fn $default() {
            let list = concat!("default-", $short);
            let golden = include_str!(concat!("../tests/golden/pages-default-", $short, ".tsv"));
            let known = include_str!(concat!(
                "../tests/golden/pages-default-",
                $short,
                "-known.tsv"
            ));
            let clock = recorded_at(include_str!(concat!(
                "../tests/golden/pages-default-",
                $short,
                ".clock"
            )));
            let hashed = Some((list, clock));
            assert_eq!(
                assert_listed_pages("wordpress", golden, hashed, known).await,
                0
            );
            assert_eq!(golden.lines().count(), 101);
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn $ttd() {
            let golden = include_str!(concat!("../tests/golden/pages-ttd-", $short, ".tsv"));
            let known = include_str!(concat!("../tests/golden/pages-ttd-", $short, "-known.tsv"));
            let clock = recorded_at(include_str!(concat!(
                "../tests/golden/pages-ttd-",
                $short,
                ".clock"
            )));
            assert_ttd_pages(concat!("ttd-", $short), golden, clock, known).await;
        }
    };
}

theme_pages!(
    twenty_twenty_four_default_pages_match_php,
    twenty_twenty_four_theme_test_data_pages_match_php,
    "tt4"
);
theme_pages!(
    twenty_twenty_three_default_pages_match_php,
    twenty_twenty_three_theme_test_data_pages_match_php,
    "tt3"
);
theme_pages!(
    twenty_twenty_two_default_pages_match_php,
    twenty_twenty_two_theme_test_data_pages_match_php,
    "tt2"
);
// The fixture child of Twenty Twenty-Five (10.3), built into the tests only.
theme_pages!(
    child_default_pages_match_php,
    child_theme_test_data_pages_match_php,
    "child"
);

/// A visitor's page view over the Theme Test Data, which has classic menus
/// and no `wp_navigation` post, in primary mode: the navigation block's
/// fallback shows the newest classic menu and writes the post PHP writes
/// for it, once (the write itself is the oracle's `navigation-fallback-*`).
#[tokio::test(flavor = "multi_thread")]
async fn a_page_view_writes_the_classic_menu_fallback() {
    use sqlx::Connection;
    let _saves = admin::tests::SAVES.lock().await;
    let db = hyscribe_config::Database {
        name: "wordpress_saves".into(),
        ..fixture_config()
    };
    let dump = std::fs::read_to_string(hyscribe_testkit::writes::THEME_TEST_DATA).unwrap();
    hyscribe_testkit::writes::load_theme_test_data(&db, &dump)
        .await
        .unwrap();
    let oracle = Some("http://127.0.0.1:8081".to_string());
    let site = Site::load(Db::connect(&db).await.unwrap(), oracle.clone(), oracle)
        .await
        .unwrap();
    let writer = hyscribe_db::Writer::open(&db, hyscribe_config::Mode::Primary).await;
    let site = Arc::new(site.with_writer(writer.unwrap()));
    let options = sqlx::mysql::MySqlConnectOptions::new()
        .host(&db.host)
        .port(db.port)
        .database(&db.name)
        .username(&db.user)
        .password(db.password.expose());
    let mut conn = sqlx::MySqlConnection::connect_with(&options).await.unwrap();
    let posts = "SELECT CONCAT_WS('|', post_author, post_status, post_title, post_name, \
        post_content) FROM wp_posts WHERE post_type = 'wp_navigation'";
    for visit in 0..2 {
        let reply = request(&site, "GET", "/").await;
        assert_eq!(reply.status, StatusCode::OK);
        assert!(
            reply
                .body
                .contains("<span class=\"wp-block-navigation-item__label\">twitter.com</span>")
        );
        let written: Vec<String> = sqlx::query_scalar(posts)
            .fetch_all(&mut conn)
            .await
            .unwrap();
        // The first view wrote the one post; the second found it.
        assert_eq!(written.len(), 1, "view {visit}");
        let content = "<!-- wp:navigation-link {\"className\":\" menu-item menu-item-type-custom menu-item-object-custom\"";
        let label = "\"label\":\"twitter.com\"";
        assert!(written[0].starts_with(&format!("0|publish|Social menu|social-menu|{content}")));
        assert!(written[0].contains(label), "{}", written[0]);
    }
}

/// The theme a request is served with is the one the `stylesheet` option
/// names as the database has it now, an unknown one falling back to the
/// default theme, whose bundled files are all served.
#[tokio::test(flavor = "multi_thread")]
async fn the_stylesheet_option_picks_the_theme() {
    use sqlx::Connection;
    let _saves = admin::tests::SAVES.lock().await;
    let db = hyscribe_config::Database {
        name: "wordpress_saves".into(),
        ..fixture_config()
    };
    hyscribe_testkit::writes::load_default_install(&db)
        .await
        .unwrap();
    let options = sqlx::mysql::MySqlConnectOptions::new()
        .host(&db.host)
        .port(db.port)
        .database(&db.name)
        .username(&db.user)
        .password(db.password.expose());
    let mut conn = sqlx::MySqlConnection::connect_with(&options).await.unwrap();
    let mut site = Site::load(Db::connect(&db).await.unwrap(), None, None)
        .await
        .unwrap();
    assert_eq!(site.theme.slug(), "twentytwentyfive");
    let update = "UPDATE wp_options SET option_value = ? WHERE option_name = 'stylesheet'";
    for (stylesheet, served) in [
        ("twentytwentyfour", "twentytwentyfour"),
        ("twentytwentythree", "twentytwentythree"),
        ("twentytwentytwo", "twentytwentytwo"),
        ("missing", "twentytwentyfive"),
        ("twentytwentyfour", "twentytwentyfour"),
    ] {
        sqlx::query(update)
            .bind(stylesheet)
            .execute(&mut conn)
            .await
            .unwrap();
        site = site.reloaded().await.unwrap();
        assert_eq!(site.theme.slug(), served);
    }
    let site = Arc::new(site);
    let page = request(&site, "GET", "/").await;
    assert!(page.body.contains("wp-theme-twentytwentyfour"));
}

/// A visitor's page view over the default install, which has no classic
/// menu and no `wp_navigation` post, in primary mode: the navigation block's
/// fallback writes the page list post PHP writes (`create_default_fallback()`,
/// the oracle's `navigation-fallback-default`), once.
#[tokio::test(flavor = "multi_thread")]
async fn a_page_view_writes_the_default_fallback() {
    use sqlx::Connection;
    let _saves = admin::tests::SAVES.lock().await;
    let db = hyscribe_config::Database {
        name: "wordpress_saves".into(),
        ..fixture_config()
    };
    hyscribe_testkit::writes::load_default_install(&db)
        .await
        .unwrap();
    let delete = "DELETE FROM wp_posts WHERE post_type = 'wp_navigation'";
    let options = sqlx::mysql::MySqlConnectOptions::new()
        .host(&db.host)
        .port(db.port)
        .database(&db.name)
        .username(&db.user)
        .password(db.password.expose());
    let mut conn = sqlx::MySqlConnection::connect_with(&options).await.unwrap();
    sqlx::query(delete).execute(&mut conn).await.unwrap();
    let oracle = Some("http://127.0.0.1:8081".to_string());
    let site = Site::load(Db::connect(&db).await.unwrap(), oracle.clone(), oracle)
        .await
        .unwrap();
    let writer = hyscribe_db::Writer::open(&db, hyscribe_config::Mode::Primary).await;
    let site = Arc::new(site.with_writer(writer.unwrap()));
    let posts = "SELECT CONCAT_WS('|', post_author, post_status, post_title, post_name, \
        post_content) FROM wp_posts WHERE post_type = 'wp_navigation'";
    for visit in 0..2 {
        let reply = request(&site, "GET", "/").await;
        assert_eq!(reply.status, StatusCode::OK);
        assert!(reply.body.contains("wp-block-page-list"));
        let written: Vec<String> = sqlx::query_scalar(posts)
            .fetch_all(&mut conn)
            .await
            .unwrap();
        // The first view wrote the one post; the second found it.
        let expected = "0|publish|Navigation|navigation|<!-- wp:page-list /-->";
        assert_eq!(written, [expected], "view {visit}");
    }
}

/// `wp_navigation_should_create_fallback` returning false, as the page
/// goldens' `options.php` has it unless a request sets `_fallback`.
#[derive(Debug)]
pub(crate) struct NoFallback;

impl hyscribe_render::site::Hooks for NoFallback {
    fn wp_navigation_should_create_fallback(&self, _create: bool) -> bool {
        false
    }
}

/// The pages of `reference/pages/site-edits.list` over the site editor's
/// saves as PHP wrote them (`reference/site-edits/content.sql`): edited
/// templates and template parts, a custom template, global styles and a
/// navigation menu, in `wordpress_saves`.
#[tokio::test(flavor = "multi_thread")]
async fn edited_site_pages_match_php() {
    use sqlx::Connection;
    let _saves = admin::tests::SAVES.lock().await;
    let db = hyscribe_config::Database {
        name: "wordpress_saves".into(),
        ..fixture_config()
    };
    hyscribe_testkit::writes::load_default_install(&db)
        .await
        .unwrap();
    let options = sqlx::mysql::MySqlConnectOptions::new()
        .host(&db.host)
        .port(db.port)
        .database(&db.name)
        .username(&db.user)
        .password(db.password.expose());
    let mut conn = sqlx::MySqlConnection::connect_with(&options).await.unwrap();
    let edits = include_str!("../../../reference/site-edits/content.sql");
    sqlx::raw_sql(sqlx::AssertSqlSafe(edits))
        .execute(&mut conn)
        .await
        .unwrap();
    let golden = include_str!("../tests/golden/pages-site-edits.tsv");
    assert_eq!(
        assert_listed_pages("wordpress_saves", golden, None, "").await,
        0
    );
    assert_eq!(golden.lines().count(), 12);
    // Options without posts: the global styles post cannot be read.
    let broken = "CREATE TABLE broken_options AS SELECT * FROM wp_options";
    sqlx::raw_sql(broken).execute(&mut conn).await.unwrap();
    let config = Database {
        prefix: hyscribe_config::Prefix::new("broken_").unwrap(),
        ..db
    };
    let site = Site::load(Db::connect(&config).await.unwrap(), None, None).await;
    let drop = "DROP TABLE broken_options";
    sqlx::raw_sql(drop).execute(&mut conn).await.unwrap();
    assert!(site.is_err());
}

/// A site not at the oracle's address: the 404 page's bundled theme image
/// is under the site's own URL, not the exported one.
#[tokio::test(flavor = "multi_thread")]
async fn bundled_pattern_urls_follow_the_site() {
    let own = "http://127.0.0.1:8180";
    let site = site_at(Some(own), Some(own)).await;
    let reply = request(&site, "GET", "/?p=999999").await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    let image = format!("{own}/wp-content/themes/twentytwentyfive/assets/images/404-image.webp");
    assert!(reply.body.contains(&image), "{}", reply.body);
    assert!(!reply.body.contains("127.0.0.1:8081"));
}

/// The pages of `reference/pages/folder.list` for a home URL in a folder,
/// with plain and pretty permalinks: the canonical redirects keep the
/// folder once, in `wordpress_saves`.
#[tokio::test(flavor = "multi_thread")]
async fn folder_pages_match_php() {
    let _saves = admin::tests::SAVES.lock().await;
    let db = hyscribe_config::Database {
        name: "wordpress_saves".into(),
        ..fixture_config()
    };
    hyscribe_testkit::writes::load_default_install(&db)
        .await
        .unwrap();
    let golden = include_str!("../tests/golden/pages-folder.tsv");
    assert_eq!(
        assert_listed_pages("wordpress_saves", golden, None, "").await,
        0
    );
    assert_eq!(golden.lines().count(), 10);
    // The canonical address itself is served, as PHP does (its body names
    // the oracle's root content URL, so it is not in the golden).
    let home = "http://127.0.0.1:8180/blog";
    let site = site_at(Some(home), Some(home)).await;
    assert_eq!(request(&site, "GET", "/blog/").await.status, StatusCode::OK);
    let reply = request(&site, "GET", "/blog").await;
    assert_eq!(reply.headers["location"], format!("{home}/"));
    // A page on the front takes the slash too, none without a page chosen.
    for (page, status) in [("2", StatusCode::MOVED_PERMANENTLY), ("0", StatusCode::OK)] {
        let db = Db::connect(&fixture_config()).await.unwrap();
        let mut site = Site::load(db, Some(home.into()), Some(home.into()))
            .await
            .unwrap();
        site.options
            .insert("show_on_front".into(), b"page".to_vec());
        site.options.insert("page_on_front".into(), page.into());
        assert_eq!(
            request(&Arc::new(site), "GET", "/blog").await.status,
            status
        );
    }
}

/// The pages of `reference/pages/comments.list` over the comments of
/// `reference/pages/comments.sql`: paged comments, comment pages and their
/// redirects, and the held comments, comment form and links the commenter
/// cookies change, in `wordpress_saves`.
#[tokio::test(flavor = "multi_thread")]
async fn commenter_pages_match_php() {
    use sqlx::Connection;
    let _saves = admin::tests::SAVES.lock().await;
    let db = hyscribe_config::Database {
        name: "wordpress_saves".into(),
        ..fixture_config()
    };
    hyscribe_testkit::writes::load_default_install(&db)
        .await
        .unwrap();
    let options = sqlx::mysql::MySqlConnectOptions::new()
        .host(&db.host)
        .port(db.port)
        .database(&db.name)
        .username(&db.user)
        .password(db.password.expose());
    let mut conn = sqlx::MySqlConnection::connect_with(&options).await.unwrap();
    let comments = include_str!("../../../reference/pages/comments.sql");
    sqlx::raw_sql(sqlx::AssertSqlSafe(comments))
        .execute(&mut conn)
        .await
        .unwrap();
    let golden = include_str!("../tests/golden/pages-comments.tsv");
    assert_eq!(
        assert_listed_pages("wordpress_saves", golden, None, "").await,
        0
    );
    assert_eq!(golden.lines().count(), 95);
}

/// The pages of `reference/pages/titles.list` over the posts and pages of
/// `reference/pages/titles.sql`: the browser title of untitled posts and
/// pages and of titles with quotes, dashes and entities, and of the posts
/// page, in `wordpress_saves`.
#[tokio::test(flavor = "multi_thread")]
async fn title_pages_match_php() {
    use sqlx::Connection;
    let _saves = admin::tests::SAVES.lock().await;
    let db = hyscribe_config::Database {
        name: "wordpress_saves".into(),
        ..fixture_config()
    };
    hyscribe_testkit::writes::load_default_install(&db)
        .await
        .unwrap();
    let options = sqlx::mysql::MySqlConnectOptions::new()
        .host(&db.host)
        .port(db.port)
        .database(&db.name)
        .username(&db.user)
        .password(db.password.expose());
    let mut conn = sqlx::MySqlConnection::connect_with(&options).await.unwrap();
    let titles = include_str!("../../../reference/pages/titles.sql");
    sqlx::raw_sql(sqlx::AssertSqlSafe(titles))
        .execute(&mut conn)
        .await
        .unwrap();
    let golden = include_str!("../tests/golden/pages-titles.tsv");
    assert_eq!(
        assert_listed_pages("wordpress_saves", golden, None, "").await,
        0
    );
    assert_eq!(golden.lines().count(), 13);
}

/// Every case of `reference/term-template/golden.sh` over the Theme Test
/// Data fixture's database: `WP_Term_Query`'s terms and whole terms query
/// blocks, by hash, as the fixture's content is not ours to commit.
#[tokio::test(flavor = "multi_thread")]
async fn term_template_matches_php() {
    use hyscribe_render::site::{Archive, Site as _, Taxonomy};
    use hyscribe_render::term_template::{TermArgs, get_terms};
    use sha2::{Digest, Sha256};
    let mut config = fixture_config();
    config.name = "wordpress_ttd".into();
    let db = Db::connect(&config)
        .await
        .expect(hyscribe_testkit::NO_FIXTURE_DATABASE);
    let home = Some("http://127.0.0.1:8081".to_string());
    let site = Arc::new(Site::load(db, home.clone(), home).await.unwrap());
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let reads = Reads::new(&site, handle);
        let hash = |bytes: &[u8]| -> String {
            Sha256::digest(bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect()
        };
        let golden = include_str!("../tests/golden/term-template.tsv");
        let mut failures = Vec::new();
        for line in golden.lines() {
            let fields: Vec<&str> = line.split('\t').collect();
            let (name, ours) = if fields[0] == "query" {
                let json = hyscribe_testkit::unhex(fields[1]);
                let args = hyscribe_phpser::json_decode(&json).unwrap();
                let get = |key| args.get_str(key).cloned().unwrap_or(Value::Null);
                let args = TermArgs {
                    taxonomy: String::from_utf8(get("taxonomy").to_php_string()).unwrap(),
                    number: get("number").to_int().unsigned_abs(),
                    order: get("order"),
                    orderby: get("orderby").to_php_string(),
                    hide_empty: !get("hide_empty").is_empty(),
                    parent: args.get_str("parent").map(Value::to_int),
                    child_of: get("child_of").to_int(),
                    include: get("include")
                        .as_array()
                        .unwrap_or_default()
                        .iter()
                        .map(|(_, id)| id.to_int().unsigned_abs())
                        .collect(),
                };
                let ids: Vec<String> = get_terms(&reads, &args)
                    .iter()
                    .map(|term| term.id.to_string())
                    .collect();
                (json, ids.join(",").into_bytes())
            } else {
                let query = String::from_utf8(hyscribe_testkit::unhex(fields[1])).unwrap();
                let mut request = hyscribe_render::site::Request::default();
                if let Some((key, id)) = query.split_once('=') {
                    let (taxonomy, name) = match key {
                        "cat" => (Taxonomy::Category, "category"),
                        _ => (Taxonomy::Tag, "post_tag"),
                    };
                    let term = reads.term(id.parse().unwrap(), name);
                    request.archive = Some(Archive::Term(taxonomy, term));
                }
                let markup = hyscribe_testkit::unhex(fields[2]);
                let html = hyscribe_render::template::render_content(
                    &site.theme,
                    &reads,
                    &request,
                    &markup,
                );
                ([query.as_bytes(), b" ", &markup].concat(), html)
            };
            if hash(&ours) != fields[fields.len() - 1] {
                // Only names and hashes: `RAW=1` on the golden script shows PHP's.
                failures.push(String::from_utf8_lossy(&name).into_owned());
            }
        }
        assert!(
            failures.is_empty(),
            "{} differ:\n{}",
            failures.len(),
            failures.join("\n")
        );
        assert_eq!(golden.lines().count(), 355);
        // Beyond PHP, which reads a null queried object there: an inheriting
        // template on a term archive without its term lists nothing.
        let request = hyscribe_render::site::Request {
            archive: Some(Archive::Term(Taxonomy::Category, None)),
            ..Default::default()
        };
        let inherit = b"<!-- wp:terms-query {\"termQuery\":{\"inherit\":true}} --><div><!-- wp:term-template --><!-- wp:term-name /--><!-- /wp:term-template --></div><!-- /wp:terms-query -->";
        let html = hyscribe_render::template::render_content(&site.theme, &reads, &request, inherit);
        assert_eq!(
            String::from_utf8(html).unwrap(),
            "<div class=\"is-layout-flow wp-block-terms-query-is-layout-flow\"></div>"
        );
        // Outside a template and a term archive, there is no description.
        let request = hyscribe_render::site::Request::default();
        let html = hyscribe_render::template::render_content(
            &site.theme,
            &reads,
            &request,
            b"<!-- wp:term-description /-->",
        );
        assert!(html.is_empty());
    })
    .await
    .unwrap();
}

/// A change to the options, by PHP or anything else, shows on the next
/// request (ADR 0012): the title, and a new permalink structure's rules.
#[tokio::test(flavor = "multi_thread")]
async fn each_request_reads_the_options_as_they_are_now() {
    let _saves = admin::tests::SAVES.lock().await;
    let db = hyscribe_config::Database {
        name: "wordpress_saves".into(),
        ..fixture_config()
    };
    hyscribe_testkit::writes::load_default_install(&db)
        .await
        .unwrap();
    let site = Site::load(Db::connect(&db).await.unwrap(), None, None);
    let app = live(Arc::new(site.await.unwrap()));
    let home = answer(app.clone(), "GET", "/").await;
    assert!(title(&home.body).starts_with("HyScribe Default Install"));
    // Plain permalinks have no rules: the path is the home page.
    let pretty = answer(app.clone(), "GET", "/hello-world/").await;
    assert_eq!(title(&pretty.body), title(&home.body));
    let writer = hyscribe_db::Writer::open(&db, hyscribe_config::Mode::Primary);
    let writer = writer.await.unwrap().unwrap();
    let mut write = writer.begin().await.unwrap();
    write.update_option("blogname", b"Renamed").await.unwrap();
    write
        .update_option("home", b"http://moved.test")
        .await
        .unwrap();
    let structure = b"/%postname%/";
    write
        .update_option("permalink_structure", structure)
        .await
        .unwrap();
    write.commit().await.unwrap();
    let home = answer(app.clone(), "GET", "/").await;
    assert!(title(&home.body).starts_with("Renamed"), "{}", home.body);
    let pretty = answer(app, "GET", "/hello-world/").await;
    assert_eq!(title(&pretty.body), "Hello world! &#8211; Renamed");
    assert!(
        pretty
            .body
            .contains("href=\"http://moved.test/hello-world/\"")
    );
}

/// The rules are compiled again only for other permalink options.
#[test]
fn the_rules_compile_once_per_options() {
    let compiled = rewrite::Compiled::default();
    let settings = |structure| rewrite::Settings {
        structure,
        ..rewrite::Settings::default()
    };
    let postname = compiled.rewriter(&settings("/%postname%/"));
    let again = compiled.rewriter(&settings("/%postname%/"));
    assert!(Arc::ptr_eq(&postname, &again));
    let dated = compiled.rewriter(&settings("/%year%/%postname%/"));
    assert!(!Arc::ptr_eq(&postname, &dated));
}

/// The pages of `reference/attachment-page/golden.sh`: attachments by ID
/// and by name, with attachment pages off and on, over the default install
/// with the attachments of `reference/attachment-edit/content.sql` and
/// `reference/attachment-page/extra.sql`, in `wordpress_saves`.
#[tokio::test(flavor = "multi_thread")]
async fn attachment_pages_match_php() {
    use sqlx::Connection;
    let _saves = admin::tests::SAVES.lock().await;
    let db = hyscribe_config::Database {
        name: "wordpress_saves".into(),
        ..fixture_config()
    };
    hyscribe_testkit::writes::load_default_install(&db)
        .await
        .unwrap();
    let options = sqlx::mysql::MySqlConnectOptions::new()
        .host(&db.host)
        .port(db.port)
        .database(&db.name)
        .username(&db.user)
        .password(db.password.expose());
    let mut conn = sqlx::MySqlConnection::connect_with(&options).await.unwrap();
    for rows in [
        include_str!("../../../reference/attachment-edit/content.sql"),
        include_str!("../../../reference/attachment-page/extra.sql"),
    ] {
        sqlx::raw_sql(sqlx::AssertSqlSafe(rows))
            .execute(&mut conn)
            .await
            .unwrap();
    }
    let golden = include_str!("../tests/golden/attachment-pages.tsv");
    // The players of audio and video files need MediaElement.js.
    let known = ["818", "821", "825"]
        .map(|id| format!("wp_attachment_pages_enabled=1\t/?attachment_id={id}\tmediaelement\n"))
        .concat();
    assert_eq!(
        assert_listed_pages("wordpress_saves", golden, None, &known).await,
        0
    );
    assert_eq!(golden.lines().count(), 110);
    // Without the table of its meta the file of an attachment is unread.
    let tables = [
        "options",
        "posts",
        "terms",
        "term_taxonomy",
        "term_relationships",
        "users",
        "usermeta",
        "comments",
        "commentmeta",
        "termmeta",
    ];
    for table in tables {
        let copy = format!("CREATE TABLE broken_{table} AS SELECT * FROM wp_{table}");
        sqlx::raw_sql(sqlx::AssertSqlSafe(copy))
            .execute(&mut conn)
            .await
            .unwrap();
    }
    let config = Database {
        prefix: hyscribe_config::Prefix::new("broken_").unwrap(),
        ..db
    };
    let site = Site::load(Db::connect(&config).await.unwrap(), None, None).await;
    let reply = match site {
        Ok(site) => Some(request(&Arc::new(site), "GET", "/?attachment_id=801").await),
        Err(_) => None,
    };
    for table in tables {
        let drop = format!("DROP TABLE broken_{table}");
        sqlx::raw_sql(sqlx::AssertSqlSafe(drop))
            .execute(&mut conn)
            .await
            .unwrap();
    }
    assert_eq!(
        reply.expect("the site loads").status,
        StatusCode::INTERNAL_SERVER_ERROR
    );
}

/// The pages of `reference/duotone/page-golden.sh`: covers and an image with
/// every kind of duotone filter, the footer's filters and the global
/// styles' custom properties of presets, over the default install with the
/// posts of `reference/duotone/page.sql`, in `wordpress_saves`.
#[tokio::test(flavor = "multi_thread")]
async fn duotone_pages_match_php() {
    use sqlx::Connection;
    let _saves = admin::tests::SAVES.lock().await;
    let db = hyscribe_config::Database {
        name: "wordpress_saves".into(),
        ..fixture_config()
    };
    hyscribe_testkit::writes::load_default_install(&db)
        .await
        .unwrap();
    let options = sqlx::mysql::MySqlConnectOptions::new()
        .host(&db.host)
        .port(db.port)
        .database(&db.name)
        .username(&db.user)
        .password(db.password.expose());
    let mut conn = sqlx::MySqlConnection::connect_with(&options).await.unwrap();
    let rows = include_str!("../../../reference/duotone/page.sql");
    sqlx::raw_sql(sqlx::AssertSqlSafe(rows))
        .execute(&mut conn)
        .await
        .unwrap();
    let golden = include_str!("../tests/golden/duotone-pages.tsv");
    assert_eq!(
        assert_listed_pages("wordpress_saves", golden, None, "").await,
        0
    );
    assert_eq!(golden.lines().count(), 2);
}

/// A request still running past `REQUEST_TIME_LIMIT` answers 503.
#[tokio::test(flavor = "multi_thread")]
async fn a_request_past_the_time_limit_answers_503() {
    let site = Arc::try_unwrap(fixture_site(None).await).ok().unwrap();
    let site = Arc::new(site.with_time_limit(Duration::ZERO));
    let request = Request::get("/").body(Body::empty()).unwrap();
    let response = router(site).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()[header::CONNECTION], "close");
    assert!(response.headers().contains_key("x-content-type-options"));
    let default = fixture_site(None).await;
    let request = Request::get("/").body(Body::empty()).unwrap();
    let response = router(default).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

/// A write held past the limit is cut off and rolls back: nothing of it is
/// left, and the row it had written is free for the next write at once.
#[tokio::test(flavor = "multi_thread")]
async fn a_write_cut_off_by_the_time_limit_rolls_back() {
    let _saves = admin::tests::SAVES.lock().await;
    let db = hyscribe_config::Database {
        name: "wordpress_saves".into(),
        ..fixture_config()
    };
    hyscribe_testkit::writes::load_default_install(&db)
        .await
        .unwrap();
    let writer = hyscribe_db::Writer::open(&db, hyscribe_config::Mode::Primary)
        .await
        .unwrap()
        .unwrap();
    let writer = Arc::new(writer);
    let held = Arc::clone(&writer);
    let slow = move || {
        let writer = Arc::clone(&held);
        async move {
            let mut write = writer.begin().await.unwrap();
            let added = write.add_option("cut_off_option", b"half", "no");
            added.await.unwrap();
            tokio::time::sleep(Duration::from_secs(30)).await;
            write.commit().await.unwrap();
            "done"
        }
    };
    let app = Router::new().route("/", axum::routing::get(slow)).layer(
        axum::middleware::from_fn_with_state(Duration::from_millis(500), time_limited),
    );
    let request = Request::get("/").body(Body::empty()).unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let mut write = writer.begin().await.unwrap();
    let ten = Duration::from_secs(10);
    let found = tokio::time::timeout(ten, write.option_value("cut_off_option"));
    assert_eq!(found.await.unwrap().unwrap(), None);
    let again = write.add_option("cut_off_option", b"whole", "no");
    tokio::time::timeout(ten, again).await.unwrap().unwrap();
    write.commit().await.unwrap();
}

pub(crate) mod classic;

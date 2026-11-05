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

fn fixture_config() -> Database {
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
async fn oracle_site() -> Arc<Site> {
    site_at(Some("http://127.0.0.1:8081"), Some("http://127.0.0.1:8081")).await
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
    assert_eq!(
        request(&site, "POST", "/").await.status,
        StatusCode::METHOD_NOT_ALLOWED
    );
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
        theme: hyscribe_render::theme::twentytwentyfive(),
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
        "/?name=x",
        "/?pagename=x",
        "/?attachment_id=1",
        // Past a route that reads nothing, sitemaps fail.
        "/?author=1&sitemap=index",
    ] {
        let reply = request(&site, "GET", uri).await;
        assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR);
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
    let reply = trackback(&site, 1).await;
    assert_eq!(reply.status(), StatusCode::INTERNAL_SERVER_ERROR);
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
        ("/hs-admin/?p=", StatusCode::NOT_FOUND),
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
    let (temporary, permanent) = (
        StatusCode::TEMPORARY_REDIRECT,
        StatusCode::PERMANENT_REDIRECT,
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
            permanent,
            "/hs-admin/edit?post_type=page",
        ),
        ("GET", "/hs-admin/index.php", permanent, "/hs-admin/"),
    ] {
        let reply = request(&site, method, uri).await;
        assert_eq!(reply.status, status, "{method} {uri}");
        assert_eq!(
            reply.headers["location"],
            format!("http://127.0.0.1:8080{expected}"),
            "{uri}"
        );
    }
    // Clean URLs are reserved (no login or admin yet), never redirected.
    for uri in ["/hs-login", "/hs-admin", "/hs-admin/", "/hs-admin/edit"] {
        let reply = request(&site, "GET", uri).await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{uri}");
    }
    // Look-alikes are ordinary paths.
    for uri in ["/wp-adminx", "/wp-login.phpx", "/x/wp-admin/", "/hs-loginx"] {
        let status = request(&site, "GET", uri).await.status;
        assert!(status != temporary && status != permanent, "{uri}");
    }
}

#[tokio::test]
async fn static_paths_keep_their_urls() {
    let site = fixture_site(None).await;
    for uri in [
        "/wp-content/uploads/2026/10/photo.jpg",
        "/wp-content/themes/twentytwentyfive/style.css",
        "/wp-includes/js/wp-emoji-release.min.js",
        "/wp-content/plugins/x/",
    ] {
        // Never redirected; 404 until static files are served.
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
        theme: hyscribe_render::theme::twentytwentyfive(),
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
    };
    let rendered = rendered(&site, page, Err(panicked));
    assert_eq!(rendered.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(!rendered.body.contains("render bug"));
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
    list(site, query, false, main).await.unwrap()
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
    assert_eq!(golden.lines().count(), 67);
}

/// Every page of `reference/head/golden.sh`, status and HTML, against
/// PHP's, with HyScribe as the generator and without the tags an open question holds
/// back.
#[tokio::test]
async fn pages_match_php() {
    let golden = include_str!("../../hyscribe-render/tests/golden/pages.tsv");
    assert_pages(&oracle_site().await, golden).await;
    assert_eq!(golden.lines().count(), 196);
}

/// Every case of `reference/loader/golden.sh`: the favicon and trackback
/// query vars, with an HTML body reduced to its title.
#[tokio::test]
async fn favicons_and_trackbacks_match_php() {
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
            reply
                .body
                .lines()
                .find(|l| l.starts_with("<title>"))
                .unwrap_or_default()
                .to_string()
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
    assert_eq!(golden.lines().count(), 65);
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
    assert_eq!(golden.lines().count(), 34);
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

/// PHP's page, feed or sitemap as HyScribe sends it: HyScribe as the
/// generator and without the tags an open question holds back.
#[allow(clippy::string_slice)]
fn as_hyscribe(mut php: String) -> String {
    for prefix in [
        "<link rel=\"https://api.w.org/\"",
        "<link rel=\"alternate\" title=\"JSON\"",
        "<link rel=\"EditURI\"",
        "<link rel=\"alternate\" title=\"oEmbed",
    ] {
        while let Some(start) = php.find(prefix) {
            let end = start + php[start..].find("/>").unwrap() + 2;
            let end = if php[end..].starts_with('\n') {
                end + 1
            } else {
                end
            };
            php.replace_range(start..end, "");
        }
    }
    let version = env!("CARGO_PKG_VERSION");
    php.replace(
        "<meta name=\"generator\" content=\"WordPress 7.1.3\" />",
        &format!("<meta name=\"generator\" content=\"HyScribe {version}\" />"),
    )
    .replace(
        "<generator>https://wordpress.org/?v=7.1.3</generator>",
        &format!("<generator>https://github.com/hyscaler/HyScribe?v={version}</generator>"),
    )
    // No users sitemap: author archives are off (docs/SECURITY_MODEL.md).
    .replace(
        "<sitemap><loc>http://127.0.0.1:8081/?sitemap=users&amp;paged=1</loc></sitemap>",
        "",
    )
    .replace(
        "<!-- generator=\"WordPress/7.1.3\" -->",
        &format!("<!-- generator=\"HyScribe/{version}\" -->"),
    )
    .replace("generated by WordPress", "generated by HyScribe")
}

/// Each line of `golden`, a path, PHP's status and its page, against ours.
#[allow(clippy::string_slice)]
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
        let ours = reply.body;
        if reply.status.as_u16() != status || ours != php {
            let at = ours
                .bytes()
                .zip(php.bytes())
                .position(|(a, b)| a != b)
                .unwrap_or(ours.len().min(php.len()));
            let from = at.saturating_sub(200);
            failures.push(format!(
                "{path} {status}/{}\n  php:  {:?}\n  ours: {:?}",
                reply.status,
                &php[from.min(php.len())..(at + 300).min(php.len())],
                &ours[from.min(ours.len())..(at + 300).min(ours.len())]
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
        let ours = match paged_single(&site, &query, Some(&raw), "p", post.clone()) {
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
    assert_eq!(query::basename(b"a/b\\c//"), b"c");
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

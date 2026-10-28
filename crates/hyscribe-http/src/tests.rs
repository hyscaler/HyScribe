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
        options: HashMap::new(),
        home: String::new(),
        theme: hyscribe_render::theme::twentytwentyfive(),
    });
    // The 404 page fails while rendering, reading an option it has not loaded.
    for uri in ["/", "/?p=1", "/?cat=1", "/?author=1"] {
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
    assert_eq!(date_parts("2026"), (Some(2026), None, None));
    assert_eq!(date_parts("2026-10"), (Some(2026), Some(10), None));
    assert_eq!(date_parts("20261027"), (Some(2026), Some(10), Some(27)));
    assert_eq!(date_parts("202610271200"), (Some(2026), Some(10), Some(27)));
    assert_eq!(date_parts("12"), (Some(12), None, None));
    assert_eq!(date_parts("abc"), (None, None, None));
    assert_eq!(date_parts("20261"), (Some(2026), None, None));
}

#[test]
fn search_terms_split_like_wordpress() {
    assert_eq!(search_terms("a b,c+d\te"), ["a", "b", "c", "d", "e"]);
    assert!(search_terms("  ,+ ").is_empty());
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

#[tokio::test]
async fn routes_give_the_request_state() {
    let site = fixture_site(None).await;
    let recent = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        - 60;
    let home = list(&site, &HashMap::new()).await.unwrap();
    assert!(home.request.front_page && home.request.home && home.request.now > recent);
    assert_eq!(home.request.post, Some(1));
    let post = site.db.post(1).await.unwrap().unwrap();
    let single = single_page(&site, post);
    assert_eq!(
        (single.request.queried_object_id, single.request.post),
        (1, Some(1))
    );
    assert!(single.request.singular && single.request.now > recent);
    let missing = not_found(&site);
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

/// What every page prints after the theme's footer (`wp_footer()`)
/// against PHP's, from `reference/head/golden.sh`.
#[tokio::test]
async fn footers_match_php() {
    let site = oracle_site().await;
    let golden = include_str!("../../hyscribe-render/tests/golden/footer.tsv");
    for line in golden.lines() {
        let (path, php) = line.split_once('\t').unwrap();
        let path = String::from_utf8(hyscribe_testkit::unhex(path)).unwrap();
        let php = String::from_utf8(hyscribe_testkit::unhex(php)).unwrap();
        let body = request(&site, "GET", &path).await.body;
        let marker = "</footer>\n</div>\n";
        let start = body.rfind(marker).unwrap() + marker.len();
        assert_eq!(
            body.get(start..body.rfind("</body>").unwrap()),
            Some(php.as_str()),
            "{path}"
        );
    }
    assert_eq!(golden.lines().count(), 16);
}

/// Every page's `<head>` against PHP's, from `reference/head/golden.sh`,
/// with HyScribe as the generator and without the tags an open question holds back.
#[tokio::test]
#[allow(clippy::string_slice)]
async fn heads_match_php() {
    let site = oracle_site().await;
    let golden = include_str!("../../hyscribe-render/tests/golden/head.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let (path, php) = line.split_once('\t').unwrap();
        let path = String::from_utf8(hyscribe_testkit::unhex(path)).unwrap();
        let mut php = String::from_utf8(hyscribe_testkit::unhex(php)).unwrap();
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
        let php = php.replace(
            "<meta name=\"generator\" content=\"WordPress 7.1.3\" />",
            &format!(
                "<meta name=\"generator\" content=\"HyScribe {}\" />",
                env!("CARGO_PKG_VERSION")
            ),
        );
        let body = request(&site, "GET", &path).await.body;
        let ours = &body[body.find("<head>").unwrap()..body.find("</head>").unwrap() + 7];
        if ours != php {
            let at = ours
                .bytes()
                .zip(php.bytes())
                .position(|(a, b)| a != b)
                .unwrap_or(ours.len().min(php.len()));
            let from = at.saturating_sub(200);
            failures.push(format!(
                "{path}\n  php:  {}\n  ours: {}",
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

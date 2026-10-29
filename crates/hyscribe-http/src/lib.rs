// SPDX-License-Identifier: AGPL-3.0-or-later

//! The HTTP server (M2.1 and M3, first slice): serves a site read only from
//! its WordPress database.
//!
//! This first slice covers what the default install needs with plain
//! permalinks: the home page with the latest posts, `?p=ID` for posts and
//! `?page_id=ID` for pages, search, category and date archives, and a 404
//! for everything else, rendered through the theme's block templates.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use axum::Router;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use hyscribe_db::{Db, DbError};
use hyscribe_kses::{esc_attr, esc_html};
use hyscribe_model::{Post, PostQuery};
use hyscribe_phpser::parse_str;
use hyscribe_render::date::{self, Zone};
use hyscribe_render::head::{wp_footer, wp_head};
use hyscribe_render::link;
use hyscribe_render::post::body_class;
use hyscribe_render::site::{Archive, Request};
use hyscribe_render::template::render_template;
use hyscribe_render::theme::{Theme, twentytwentyfive};
use tokio::net::TcpListener;

mod feed;
pub mod query;
mod reads;

use reads::Reads;

use query::{absint, date_parts, other_args, search_terms};

/// Headers on every response (docs/SECURITY_MODEL.md, Web).
const SECURITY_HEADERS: [(&str, &str); 3] = [
    ("x-content-type-options", "nosniff"),
    ("referrer-policy", "strict-origin-when-cross-origin"),
    ("content-security-policy", "frame-ancestors 'self'"),
];

/// What a request needs: the database and the options WordPress loads on
/// every request.
#[derive(Debug)]
pub struct Site {
    db: Db,
    options: HashMap<String, Vec<u8>>,
    home: String,
    theme: Theme,
}

impl Site {
    /// Loads the autoloaded options. `home` and `siteurl` override those
    /// options, untrailingslashed, as the `WP_HOME` and `WP_SITEURL`
    /// constants do.
    pub async fn load(
        db: Db,
        home: Option<String>,
        siteurl: Option<String>,
    ) -> Result<Site, DbError> {
        let mut options: HashMap<String, Vec<u8>> =
            db.autoload_options().await?.into_iter().collect();
        for (name, url) in [("home", home), ("siteurl", siteurl)] {
            if let Some(url) = url {
                let url = url.trim_end_matches(['/', '\\']).as_bytes().to_vec();
                options.insert(name.to_string(), url);
            }
        }
        let home = String::from_utf8_lossy(options.get("home").map_or(&[][..], Vec::as_slice))
            .trim_end_matches('/')
            .to_string();
        Ok(Site {
            db,
            options,
            home,
            theme: twentytwentyfive(),
        })
    }

    fn option(&self, name: &str) -> String {
        String::from_utf8_lossy(self.options.get(name).map_or(&[][..], Vec::as_slice)).into_owned()
    }

    /// The site's time zone, `wp_timezone()`.
    fn zone(&self) -> Zone {
        let option = |name| self.options.get(name).map_or(&[][..], Vec::as_slice);
        date::site_zone(option("timezone_string"), option("gmt_offset"))
    }

    /// The site title, as `wp_get_document_title()` prints it.
    fn name(&self) -> String {
        esc_html(self.option("blogname").as_bytes())
    }

    fn permalink(&self, post: &Post) -> String {
        let option = |name| self.options.get(name).map_or(&[][..], Vec::as_slice);
        let front_page = link::front_page(option("show_on_front"), option("page_on_front"));
        String::from_utf8_lossy(&link::permalink(self.home.as_bytes(), front_page, post))
            .into_owned()
    }
}

/// The router for `site`.
pub fn router(site: Arc<Site>) -> Router {
    Router::new()
        .fallback(page)
        .with_state(site)
        .layer(axum::middleware::map_response(security_headers))
}

/// Serves `site` on `listener` until `shutdown` completes, then finishes the
/// requests in flight.
pub async fn serve(
    listener: TcpListener,
    site: Arc<Site>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    axum::serve(listener, router(site))
        .with_graceful_shutdown(shutdown)
        .await
}

async fn security_headers(mut response: Response) -> Response {
    let headers = response.headers_mut();
    for (name, value) in SECURITY_HEADERS {
        headers.insert(
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        );
    }
    response
}

/// What a request gets: a page, or a permanent redirect to its canonical
/// URL.
enum Outcome {
    Page(Box<Page>),
    /// The feed of the page's posts or of comments.
    Feed(Box<Page>, feed::Feed),
    Redirect(String),
}

/// One page to render: the status, the document title, and the request
/// rendered with the first of the template slugs the theme has.
struct Page {
    status: StatusCode,
    title: String,
    candidates: Vec<String>,
    request: Request,
}

/// A page as sent: the status, the `<head>` and the `<body>`.
struct Rendered {
    status: StatusCode,
    head: String,
    body: String,
}

async fn page(
    State(site): State<Arc<Site>>,
    method: axum::http::Method,
    uri: Uri,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    // Before the method check, so a login form's POST is redirected too.
    if let Some((status, location)) = admin_redirect(&site, uri.path(), uri.query()) {
        return (status, [(header::LOCATION, location)]).into_response();
    }
    if method != axum::http::Method::GET && method != axum::http::Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let page = match route(&site, uri.path(), uri.query(), &query).await {
        Ok(Outcome::Redirect(location)) => {
            return (
                StatusCode::MOVED_PERMANENTLY,
                [(header::LOCATION, location)],
            )
                .into_response();
        }
        Ok(Outcome::Feed(page, feed)) => {
            return feed::respond(site, requested(*page, &uri), feed, &headers).await;
        }
        Ok(Outcome::Page(page)) => render(&site, requested(*page, &uri)).await,
        Err(err) => database_error(&site, &err),
    };
    html(&page)
}

/// `page` with the request's URI and query.
fn requested(page: Page, uri: &Uri) -> Page {
    Page {
        request: Request {
            uri: request_uri(uri),
            get: parse_str(uri.query().unwrap_or_default().as_bytes()),
            ..page.request
        },
        ..page
    }
}

/// `page` rendered through its template.
async fn render(site: &Arc<Site>, page: Page) -> Rendered {
    let rendering = Arc::clone(site);
    let (candidates, request, title) = (
        page.candidates.clone(),
        page.request.clone(),
        page.title.clone(),
    );
    let body =
        tokio::task::spawn_blocking(move || render_page(&rendering, &candidates, &request, &title))
            .await;
    rendered(site, page, body)
}

/// A rendered page as the response.
fn html(page: &Rendered) -> Response {
    (
        page.status,
        [(header::CONTENT_TYPE, "text/html; charset=UTF-8")],
        layout(&page.head, &page.body),
    )
        .into_response()
}

/// `page` with its rendered body, or the 500 page when rendering met a
/// database error or panicked.
fn rendered(
    site: &Site,
    page: Page,
    body: Result<Result<(String, String), DbError>, tokio::task::JoinError>,
) -> Rendered {
    match finished(site, body) {
        Ok((head, body)) => Rendered {
            status: page.status,
            head,
            body,
        },
        Err(error) => error,
    }
}

/// What a rendering task made, or the 500 page when it met a database
/// error or panicked.
fn finished<T>(
    site: &Site,
    body: Result<Result<T, DbError>, tokio::task::JoinError>,
) -> Result<T, Rendered> {
    match body {
        Ok(Ok(made)) => Ok(made),
        Ok(Err(err)) => Err(database_error(site, &err)),
        Err(err) => Err(database_error(site, &err)),
    }
}

/// The 500 page, with the details in the log and never shown to visitors.
fn database_error(site: &Site, err: &dyn std::fmt::Display) -> Rendered {
    eprintln!("database error: {err}");
    Rendered {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        head: format!(
            "<head>\n<meta charset=\"UTF-8\" />\n<title>Error &#8211; {}</title>\n</head>",
            site.name()
        ),
        body: "<body>\n<p>Error establishing a database connection</p>\n</body>".to_string(),
    }
}

/// The `<head>` and `<body>` elements: the first template in `candidates`
/// the theme has, rendered for `request` with the database read on this
/// thread, and the head its blocks enqueued styles for.
fn render_page(
    site: &Site,
    candidates: &[String],
    request: &Request,
    title: &str,
) -> Result<(String, String), DbError> {
    let reads = Reads::new(site, tokio::runtime::Handle::current());
    // Every block theme has an index template.
    let template = site
        .theme
        .template(candidates)
        .map_or(&[][..], |(_, template)| template);
    let (html, assets) = render_template(&site.theme, &reads, request, template);
    let classes = body_class(&reads, request, site.theme.slug());
    let head = wp_head(&site.theme, &reads, request, &assets, title.as_bytes());
    let footer = wp_footer(&reads, &assets);
    reads.finish()?;
    Ok((
        lossy(&head),
        format!(
            "<body class=\"{}\">\n\n{}\n{}</body>",
            String::from_utf8_lossy(&classes),
            String::from_utf8_lossy(&html),
            String::from_utf8_lossy(&footer)
        ),
    ))
}

/// The time relative dates are measured from.
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// The canonical form of `path` with plain permalinks, as
/// `redirect_canonical()` gives it: `/index.php` goes to `/`, and other
/// paths gain a trailing slash. `None` when `path` is canonical.
fn canonical_path(site: &Site, path: &str, raw_query: Option<&str>) -> Option<String> {
    let query = raw_query.map(|q| format!("?{q}")).unwrap_or_default();
    if let Some(folder) = path.strip_suffix("index.php").filter(|f| f.ends_with('/')) {
        return Some(format!("{}{folder}{query}", site.home));
    }
    (!path.ends_with('/')).then(|| format!("{}{path}/{query}", site.home))
}

/// HyScribe's admin and login URLs have no `.php` (ADR 0008):
/// `/hs-login` and `/hs-admin/<screen>`. The WordPress forms redirect there
/// with the query kept: `/wp-login.php`, and `/wp-admin/<screen>.php` with
/// `index.php` as `/hs-admin/`.
///
/// Those redirects are 307: browsers do not cache them, so a site switched
/// back to PHP WordPress gets its own admin back, and a POST stays a POST.
/// HyScribe's own URLs with `.php` get a permanent 308 to the clean form.
fn admin_redirect(
    site: &Site,
    path: &str,
    raw_query: Option<&str>,
) -> Option<(StatusCode, String)> {
    let (status, target) = match path {
        "/wp-login.php" => (StatusCode::TEMPORARY_REDIRECT, "/hs-login".to_string()),
        "/hs-login.php" => (StatusCode::PERMANENT_REDIRECT, "/hs-login".to_string()),
        "/wp-admin" => (StatusCode::TEMPORARY_REDIRECT, "/hs-admin/".to_string()),
        _ => {
            let (status, screen) = match (
                path.strip_prefix("/wp-admin/"),
                path.strip_prefix("/hs-admin/"),
            ) {
                (Some(screen), _) => (StatusCode::TEMPORARY_REDIRECT, screen),
                (None, Some(screen)) if screen.ends_with(".php") => {
                    (StatusCode::PERMANENT_REDIRECT, screen)
                }
                _ => return None,
            };
            let screen = screen.strip_suffix(".php").unwrap_or(screen);
            let screen = if screen == "index" { "" } else { screen };
            (status, format!("/hs-admin/{screen}"))
        }
    };
    let query = raw_query.map(|q| format!("?{q}")).unwrap_or_default();
    Some((status, format!("{}{target}{query}", site.home)))
}

/// Static file trees: never redirected or rewritten, so their public URLs
/// stay as they are. Served as files once static serving lands (M3.9).
fn is_static(path: &str) -> bool {
    path.starts_with("/wp-content/") || path.starts_with("/wp-includes/")
}

async fn route(
    site: &Site,
    path: &str,
    raw_query: Option<&str>,
    query: &HashMap<String, String>,
) -> Result<Outcome, DbError> {
    // hs-login and hs-admin come with M4 and M5; until then their URLs are
    // reserved, so no canonical redirect touches them either.
    if is_static(path)
        || path == "/hs-login"
        || path == "/hs-admin"
        || path.starts_with("/hs-admin/")
    {
        return Ok(Outcome::Page(Box::new(not_found(site, false))));
    }
    if let Some(location) = canonical_path(site, path, raw_query) {
        return Ok(Outcome::Redirect(location));
    }
    // With plain permalinks the path is ignored. Author archives give away
    // login names, so they are off by default (docs/SECURITY_MODEL.md).
    if query.contains_key("author") {
        return Ok(Outcome::Page(Box::new(not_found(site, false))));
    }
    // RSS2, Atom, RDF and RSS 0.92 feeds. `feed=rss` alone redirects to
    // RSS2, as `redirect_canonical()` rewrites only that pair.
    let single_requested = query.contains_key("p") || query.contains_key("page_id");
    let flag = |name| query.get(name).is_some_and(|v| !v.is_empty() && v != "0");
    let redirect = (query.get("feed").map(String::as_str) == Some("rss"))
        .then(|| rss2_location(site, path, raw_query));
    let format = match query
        .get("feed")
        .map(|f| f.replace("comments-", ""))
        .as_deref()
    {
        Some("rss2" | "feed") => Some(feed::Format::Rss2),
        Some("atom") => Some(feed::Format::Atom),
        Some("rdf") => Some(feed::Format::Rdf),
        Some("rss") => Some(feed::Format::Rss092),
        _ => None,
    };
    let with_comments =
        query.get("feed").is_some_and(|f| f.contains("comments-")) || flag("withcomments");
    let comment_feed = with_comments || (single_requested && !flag("withoutcomments"));
    let archive = ["s", "cat", "m"].iter().any(|k| query.contains_key(*k));
    if let Some(format) = format.filter(|_| !single_requested) {
        // A search's or archive's comment feed lists the comments on the
        // posts it matches.
        let comments = comment_feed.then(|| {
            if archive {
                feed::CommentsOf::Posts(post_query(site, query, true))
            } else {
                feed::CommentsOf::Site
            }
        });
        let page = list(site, query, true).await?;
        // A paged query with nothing in it is a 404: a feed of no posts,
        // never of comments, titled as the 404 page.
        let missing = page.request.not_found && Paging::of(query).is_paged();
        let comments = comments.filter(|_| !missing);
        let page = std::future::ready(Ok(page));
        return feed::outcome(site, page, comments, format, redirect, missing).await;
    }
    let single = |name: &'static str, post_type: &'static str| {
        query
            .get(name)
            .map(|value| (name, absint(value), post_type))
    };
    match single("p", "post").or_else(|| single("page_id", "page")) {
        Some((name, id, post_type)) => {
            let post = site.db.post(id).await?;
            // `?p=` for a page or `?page_id=` for a post.
            let elsewhere = |post: &Post| {
                (post.post_type != post_type)
                    .then(|| format!("{}{}", site.permalink(post), other_args(raw_query, &[name])))
            };
            Ok(
                match (
                    format,
                    post.filter(|p| {
                        p.status == "publish" && (p.post_type == "post" || p.post_type == "page")
                    }),
                ) {
                    // A feed's redirect keeps the feed's headers.
                    (Some(format), found) => {
                        let redirect = found.as_ref().and_then(elsewhere).or(redirect);
                        let comments =
                            comment_feed.then_some(feed::CommentsOf::Post(id, name == "p"));
                        let page = async {
                            Ok(found.map_or_else(
                                || missing_single(site),
                                |p| single_page(site, p, Paging::default()),
                            ))
                        };
                        return feed::outcome(site, page, comments, format, redirect, false).await;
                    }
                    (None, Some(post)) => match elsewhere(&post) {
                        Some(location) => Outcome::Redirect(location),
                        None => paged_single(site, query, raw_query, name, post),
                    },
                    (None, None) => Outcome::Page(Box::new(not_found(site, false))),
                },
            )
        }
        None => list(site, query, false)
            .await
            .map(|page| Outcome::Page(Box::new(page))),
    }
}

/// The `paged` and `page` query vars, as `WP_Query` casts them.
#[derive(Clone, Copy, Default)]
struct Paging {
    paged: u64,
    page: u64,
}

impl Paging {
    fn of(query: &HashMap<String, String>) -> Paging {
        let var = |name, trim: &[char]| query.get(name).map_or(0, |v| absint(v.trim_matches(trim)));
        Paging {
            paged: var("paged", &[]),
            page: var("page", &['/']),
        }
    }

    fn is_paged(self) -> bool {
        self.paged > 1
    }

    /// `wp_get_document_title()`'s page part.
    fn title(self) -> Option<String> {
        let page = self.paged.max(self.page);
        (page >= 2).then(|| format!("Page {page}"))
    }
}

/// `wp_get_document_title()`: the parts there are, joined by dashes.
fn document_title(parts: [Option<String>; 3]) -> String {
    parts
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" &#8211; ")
}

/// A singular page with the `page` var: the page itself, or a 404 when the
/// var is no number. Posts split by `<!--nextpage-->` are not paged yet.
fn paged_single(
    site: &Site,
    query: &HashMap<String, String>,
    raw_query: Option<&str>,
    name: &str,
    post: Post,
) -> Outcome {
    let page = query.get("page").filter(|v| !v.is_empty() && *v != "0");
    match page {
        Some(_) if Paging::of(query).page >= 1 => {
            let args = other_args(raw_query, &[name, "page"]);
            Outcome::Redirect(format!("{}{args}", site.permalink(&post)))
        }
        Some(_) => Outcome::Page(Box::new(not_found(site, false))),
        None => Outcome::Page(Box::new(single_page(site, post, Paging::of(query)))),
    }
}

/// A post or page on its own, with its type's template hierarchy.
fn single_page(site: &Site, post: Post, paging: Paging) -> Page {
    let name = lossy(&post.name);
    let candidates = if post.post_type == "page" {
        vec![
            format!("page-{name}"),
            format!("page-{}", post.id),
            "page".into(),
            "singular".into(),
            "index".into(),
        ]
    } else {
        let kind = &post.post_type;
        vec![
            format!("single-{kind}-{name}"),
            format!("single-{kind}"),
            "single".into(),
            "singular".into(),
            "index".into(),
        ]
    };
    let request = Request {
        singular: true,
        queried_object_id: post.id,
        post: Some(post.id),
        paged: paging.paged,
        now: now(),
        ..Request::default()
    };
    Page {
        status: StatusCode::OK,
        title: document_title([
            Some(esc_html(&post.title)),
            paging.title(),
            Some(site.name()),
        ]),
        candidates,
        request: Request {
            posts: vec![post],
            ..request
        },
    }
}

/// The query of a list page: a search covers posts and pages, a feed lists
/// `posts_per_rss` posts.
fn post_query(site: &Site, query: &HashMap<String, String>, feed: bool) -> PostQuery {
    let search = query.get("s");
    let (year, month, day) = query.get("m").map_or((None, None, None), |m| date_parts(m));
    let per_page = if feed {
        absint(&site.option("posts_per_rss"))
    } else {
        absint(&site.option("posts_per_page"))
    }
    .clamp(1, 100);
    PostQuery {
        post_types: if search.is_some() {
            vec!["post".into(), "page".into()]
        } else {
            vec!["post".into()]
        },
        search: search.map_or_else(Vec::new, |s| search_terms(s)),
        category: query.get("cat").map(|c| absint(c)).filter(|&id| id > 0),
        year,
        month,
        day,
        limit: u32::try_from(per_page).unwrap_or(10),
        offset: u32::try_from(offset(Paging::of(query).paged, per_page)).unwrap_or(u32::MAX),
        ..PostQuery::default()
    }
}

/// `WP_Query`'s offset of page `paged`: past the integers, PHP's product is
/// a float, which `absint()` wraps.
fn offset(paged: u64, per_page: u64) -> u64 {
    let skipped = paged.max(1) - 1;
    skipped.checked_mul(per_page).unwrap_or_else(|| {
        let product = skipped as f64 * per_page as f64;
        hyscribe_phpser::absint(&hyscribe_phpser::Value::Float(
            hyscribe_phpser::Float::from_f64(product),
        ))
    })
}

/// A list page: the home page, a search, a category or a date archive; for
/// a feed, its posts, and no 404.
async fn list(site: &Site, query: &HashMap<String, String>, feed: bool) -> Result<Page, DbError> {
    let search = query.get("s");
    let post_query = post_query(site, query, feed);
    let (year, month, day) = (post_query.year, post_query.month, post_query.day);
    let category = match post_query.category {
        Some(id) => match site.db.category(id).await? {
            Some(term) => Some(term),
            // A category that does not exist.
            None => return Ok(not_found(site, feed && !Paging::of(query).is_paged())),
        },
        None => None,
    };
    let (posts, found) = tokio::join!(site.db.posts(&post_query), site.db.count_posts(&post_query));
    let (posts, found) = posts.and_then(|posts| found.map(|found| (posts, found)))?;
    let paging = Paging::of(query);
    // A date archive with nothing in it is a 404, a search only when paged.
    let empty_404 = paging.is_paged() || year.is_some() && search.is_none() && category.is_none();
    if empty_404 && posts.is_empty() {
        let mut page = not_found(site, feed && !paging.is_paged());
        page.request.search_query = search.map_or_else(Vec::new, |s| s.as_bytes().to_vec());
        return Ok(page);
    }
    let first_date = posts.first().map_or("", |post| post.date.as_str());
    let heading = if let Some(s) = search {
        Some(format!(
            "Search Results for &#8220;{}&#8221;",
            esc_attr(s.as_bytes())
        ))
    } else if let Some(term) = &category {
        Some(esc_html(&term.name))
    } else if year.is_some() {
        let format = if day.is_some() {
            site.option("date_format")
        } else if month.is_some() {
            "F Y".to_string()
        } else {
            "Y".to_string()
        };
        Some(date::format(&format, first_date, site.zone()))
    } else {
        None
    };
    let tagline = site.option("blogdescription");
    let title = match heading {
        Some(heading) => document_title([Some(heading), paging.title(), Some(site.name())]),
        None => document_title([
            Some(site.name()),
            paging.title(),
            Some(esc_html(tagline.as_bytes())).filter(|t| !t.is_empty()),
        ]),
    };
    let archive = match (&category, year, month, day) {
        (Some(term), ..) => Some(Archive::Category(term.clone())),
        (None, Some(_), None, _) => Some(Archive::Year),
        (None, Some(_), Some(_), None) => Some(Archive::Month),
        (None, Some(_), Some(_), Some(_)) => Some(Archive::Day),
        (None, None, ..) => None,
    };
    let candidates: Vec<String> = match (&archive, search) {
        (_, Some(_)) => vec!["search".into(), "index".into()],
        (Some(Archive::Category(term)), None) => vec![
            format!("category-{}", lossy(&term.slug)),
            format!("category-{}", term.id),
            "category".into(),
            "archive".into(),
            "index".into(),
        ],
        (Some(_), None) => vec!["date".into(), "archive".into(), "index".into()],
        (None, None) => vec!["front-page".into(), "home".into(), "index".into()],
    };
    // The home page lists the latest posts, the default install's setting.
    let home = archive.is_none() && search.is_none();
    let request = Request {
        front_page: home,
        home,
        post: posts.first().map(|post| post.id),
        archive,
        date: year.is_some(),
        search: search.map(|s| s.as_bytes().to_vec()),
        search_query: search.map_or_else(Vec::new, |s| s.as_bytes().to_vec()),
        paged: paging.paged,
        page: paging.page,
        max_pages: found.div_ceil(u64::from(post_query.limit)),
        now: now(),
        posts,
        ..Request::default()
    };
    Ok(Page {
        status: StatusCode::OK,
        title,
        candidates,
        request,
    })
}

/// `redirect_canonical()` for `feed=rss`: the same request with `feed=rss2`.
fn rss2_location(site: &Site, path: &str, raw_query: Option<&str>) -> String {
    let query = raw_query
        .unwrap_or_default()
        .split('&')
        .map(|pair| {
            if pair == "feed=rss" {
                "feed=rss2"
            } else {
                pair
            }
        })
        .collect::<Vec<_>>()
        .join("&");
    format!("{}{path}?{query}", site.home)
}

/// A feed's missing post or page: a singular query with nothing in it.
fn missing_single(site: &Site) -> Page {
    let mut page = not_found(site, true);
    page.request.singular = true;
    page
}

/// The 404 page; for a feed, an empty one titled with the site's name.
fn not_found(site: &Site, feed: bool) -> Page {
    let request = Request {
        not_found: true,
        now: now(),
        ..Request::default()
    };
    Page {
        status: StatusCode::NOT_FOUND,
        title: if feed {
            site.name()
        } else {
            format!("Page not found &#8211; {}", site.name())
        },
        candidates: vec!["404".into(), "index".into()],
        request,
    }
}

/// `$_SERVER['REQUEST_URI']`.
fn request_uri(uri: &Uri) -> Vec<u8> {
    uri.path_and_query()
        .map_or(uri.path(), |p| p.as_str())
        .as_bytes()
        .to_vec()
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The document around the head and body.
fn layout(head: &str, body: &str) -> String {
    format!("<!DOCTYPE html>\n<html lang=\"en-US\">\n{head}\n\n{body}\n</html>\n")
}

#[cfg(test)]
mod tests;

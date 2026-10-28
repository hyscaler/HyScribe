// SPDX-License-Identifier: AGPL-3.0-or-later

//! The HTTP server (M2.1 and M3, first slice): serves a site read only from
//! its WordPress database.
//!
//! This first slice covers what the default install needs with plain
//! permalinks: the home page with the latest posts, `?p=ID` for posts and
//! `?page_id=ID` for pages, and a 404 for everything else. Pages use a
//! minimal layout, not the active theme yet (M3.10).

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use axum::Router;
use axum::extract::{Query, State};
use axum::http::{HeaderName, HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use hyscribe_db::{Db, DbError};
use hyscribe_model::{Post, PostQuery};
use hyscribe_render::blocks::{parse_blocks, render};
use hyscribe_render::date::{self, Zone};
use hyscribe_render::link;
use tokio::net::TcpListener;

pub mod query;

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
}

impl Site {
    /// Loads the autoloaded options. `home` overrides the `home` option, as
    /// the `WP_HOME` constant does.
    pub async fn load(db: Db, home: Option<String>) -> Result<Site, DbError> {
        let options: HashMap<String, Vec<u8>> = db.autoload_options().await?.into_iter().collect();
        let home = home.unwrap_or_else(|| {
            String::from_utf8_lossy(options.get("home").map_or(&[][..], Vec::as_slice)).into_owned()
        });
        Ok(Site {
            db,
            options,
            home: home.trim_end_matches('/').to_string(),
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
        esc_html(&self.option("blogname"))
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
    Page(Page),
    Redirect(String),
}

/// One HTML page: the status, the document title and the main content.
struct Page {
    status: StatusCode,
    title: String,
    main: String,
}

async fn page(
    State(site): State<Arc<Site>>,
    method: axum::http::Method,
    uri: Uri,
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
        Ok(Outcome::Page(page)) => page,
        Err(err) => {
            // Details go to the log, never to the visitor.
            eprintln!("database error: {err}");
            Page {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                title: format!("Error &#8211; {}", site.name()),
                main: "<p>Error establishing a database connection</p>".to_string(),
            }
        }
    };
    let html = layout(&site, &page);
    (
        page.status,
        [(header::CONTENT_TYPE, "text/html; charset=UTF-8")],
        html,
    )
        .into_response()
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
        return Ok(Outcome::Page(not_found(site)));
    }
    if let Some(location) = canonical_path(site, path, raw_query) {
        return Ok(Outcome::Redirect(location));
    }
    // With plain permalinks the path is ignored. Author archives give away
    // login names, so they are off by default (docs/SECURITY_MODEL.md).
    if query.contains_key("author") {
        return Ok(Outcome::Page(not_found(site)));
    }
    let single = |name: &'static str, post_type: &'static str| {
        query
            .get(name)
            .map(|value| (name, absint(value), post_type))
    };
    match single("p", "post").or_else(|| single("page_id", "page")) {
        Some((name, id, post_type)) => {
            let post = site.db.post(id).await?;
            Ok(
                match post.filter(|p| {
                    p.status == "publish" && (p.post_type == "post" || p.post_type == "page")
                }) {
                    Some(post) if post.post_type == post_type => Outcome::Page(Page {
                        status: StatusCode::OK,
                        title: format!("{} &#8211; {}", esc_html(&lossy(&post.title)), site.name()),
                        main: article(site, &post),
                    }),
                    // `?p=` for a page or `?page_id=` for a post.
                    Some(post) => Outcome::Redirect(format!(
                        "{}{}",
                        site.permalink(&post),
                        other_args(raw_query, name)
                    )),
                    None => Outcome::Page(not_found(site)),
                },
            )
        }
        None => list(site, query).await.map(Outcome::Page),
    }
}

/// A list page: the home page, a search, a category or a date archive.
async fn list(site: &Site, query: &HashMap<String, String>) -> Result<Page, DbError> {
    let search = query.get("s");
    let category = query.get("cat").map(|c| absint(c)).filter(|&id| id > 0);
    let (year, month, day) = query.get("m").map_or((None, None, None), |m| date_parts(m));
    let per_page = absint(&site.option("posts_per_page")).clamp(1, 100);
    let post_query = PostQuery {
        // Searches cover posts and pages.
        post_types: if search.is_some() {
            vec!["post".into(), "page".into()]
        } else {
            vec!["post".into()]
        },
        search: search.map_or_else(Vec::new, |s| search_terms(s)),
        category,
        year,
        month,
        day,
        limit: u32::try_from(per_page).unwrap_or(10),
        ..PostQuery::default()
    };
    let category_name = match category {
        Some(id) => match site.db.category_name(id).await? {
            Some(name) => Some(lossy(&name)),
            // A category that does not exist.
            None => return Ok(not_found(site)),
        },
        None => None,
    };
    let posts = site.db.posts(&post_query).await?;
    // A date archive with nothing in it is a 404; a search never is.
    if year.is_some() && search.is_none() && category_name.is_none() && posts.is_empty() {
        return Ok(not_found(site));
    }
    let first_date = posts.first().map_or("", |post| post.date.as_str());
    let heading = if let Some(s) = search {
        Some(format!("Search Results for &#8220;{}&#8221;", esc_attr(s)))
    } else if let Some(name) = category_name {
        Some(esc_html(&name))
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
        Some(heading) => format!("{heading} &#8211; {}", site.name()),
        None if tagline.is_empty() => site.name(),
        None => format!("{} &#8211; {}", site.name(), esc_html(&tagline)),
    };
    Ok(Page {
        status: StatusCode::OK,
        title,
        main: posts.iter().map(|post| article(site, post)).collect(),
    })
}

fn not_found(site: &Site) -> Page {
    Page {
        status: StatusCode::NOT_FOUND,
        title: format!("Page not found &#8211; {}", site.name()),
        main: "<p>It looks like nothing was found at this location.</p>".to_string(),
    }
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A post or page. Titles and content are printed as stored, as
/// `the_title()` and `the_content()` do: WordPress filtered them on save.
fn article(site: &Site, post: &Post) -> String {
    let content = lossy(&render(&parse_blocks(&post.content)));
    format!(
        "<article id=\"post-{id}\" class=\"post-{id} {kind} type-{kind} status-{status}\">\
         <h2 class=\"wp-block-post-title\"><a href=\"{link}\">{title}</a></h2>\
         <div class=\"entry-content wp-block-post-content\">{content}</div></article>",
        id = post.id,
        kind = post.post_type,
        status = post.status,
        link = esc_attr(&site.permalink(post)),
        title = lossy(&post.title),
    )
}

fn layout(site: &Site, page: &Page) -> String {
    format!(
        "<!DOCTYPE html>\n<html lang=\"en-US\">\n<head>\n<meta charset=\"UTF-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{title}</title>\n</head>\n<body>\n<div class=\"wp-site-blocks\">\n\
         <header><p class=\"wp-block-site-title\"><a href=\"{home}/\">{name}</a></p></header>\n\
         <main>{main}</main>\n<footer><p>Served by HyScribe, read only.</p></footer>\n</div>\n</body>\n</html>\n",
        title = page.title,
        home = esc_attr(&site.home),
        name = site.name(),
        main = page.main,
    )
}

/// Escapes text for HTML, leaving existing character references alone as
/// `esc_html()` does. Exact kses entity rules come with M3.6.
fn esc_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (i, c) in text.char_indices() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#039;"),
            '&' if !is_reference(text.get(i + 1..).unwrap_or_default()) => out.push_str("&amp;"),
            c => out.push(c),
        }
    }
    out
}

/// `esc_attr()` escapes the same characters.
fn esc_attr(text: &str) -> String {
    esc_html(text)
}

/// Whether `rest` (after a `&`) starts a character reference.
fn is_reference(rest: &str) -> bool {
    let body = rest.strip_prefix('#').unwrap_or(rest);
    let len = body.chars().take_while(char::is_ascii_alphanumeric).count();
    len > 0 && body.get(len..).is_some_and(|after| after.starts_with(';'))
}

#[cfg(test)]
mod tests;

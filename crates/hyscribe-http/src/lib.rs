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
use std::ops::ControlFlow;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use hyper::server::conn::http1;
use hyper_util::rt::{TokioIo, TokioTimer};
use hyper_util::server::graceful::GracefulShutdown;
use hyper_util::service::TowerToHyperService;
use hyscribe_db::{Db, DbError};
use hyscribe_kses::{esc_attr, esc_html, esc_url_raw, strip_tags};
use hyscribe_model::{OrderBy, Post, PostQuery, Term, TermFilter, TimeCondition};
use hyscribe_phpser::{
    Key, Value, add_query_values, is_empty_string, parse_str, php_trim, rawurlencode,
    rawurlencode_deep, remove_query_arg, urldecode, urlencode,
};
use hyscribe_render::date::{self, Zone};
use hyscribe_render::formatting::{
    CORE_SHORTCODE_TAGS, capital_p_dangit, convert_chars, sanitize_title_for_query, wptexturize,
};
use hyscribe_render::head::{Share, embed_page, wp_footer, wp_head};
use hyscribe_render::link;
use hyscribe_render::permalinks::{self, Permalinks, home_path};
use hyscribe_render::post::body_class;
use hyscribe_render::site::{Archive, Request, Taxonomy};
use hyscribe_render::template::render_template;
use hyscribe_render::theme::{Theme, twentytwentyfive};
use hyscribe_render::{search_count, search_terms, sticky_posts};
use tokio::net::TcpListener;
use tokio::sync::Semaphore;

mod admin;
mod canonical;
mod comment_post;
mod feed;
mod files;
mod loaders;
mod login;
pub mod query;
mod reads;
mod redirect;
mod rest;
mod rewrite;
mod sitemaps;
mod trackback;

pub use hyscribe_admin::site_health::Install;

/// Parsers of request input that only the fuzz targets in `fuzz/` call
/// from outside.
#[doc(hidden)]
pub mod fuzzing {
    pub use crate::comment_post::read_form;
    pub use crate::files::http_date;
    pub use crate::login::cookies;
    pub use crate::redirect::{wp_sanitize_redirect, wp_validate_redirect};
}

use reads::Reads;

use query::{Resolved, absint, clean_query, date_query, tax_query};

/// Headers on every response (docs/SECURITY_MODEL.md, Web).
const SECURITY_HEADERS: [(&str, &str); 3] = [
    ("x-content-type-options", "nosniff"),
    ("referrer-policy", "strict-origin-when-cross-origin"),
    ("content-security-policy", "frame-ancestors 'self'"),
];

/// The options the `WP_HOME` and `WP_SITEURL` constants replace.
const URL_CONSTANTS: [(&str, &str); 2] = [("home", "WP_HOME"), ("siteurl", "WP_SITEURL")];

/// What a request needs: the database and the options WordPress loads on
/// every request.
#[derive(Debug)]
pub struct Site {
    db: Db,
    options: HashMap<String, Vec<u8>>,
    home: String,
    theme: Arc<Theme>,
    /// The rewrite rules of the permalink options.
    rewriter: Arc<rewrite::Rewriter>,
    /// The rules compiled for an earlier request.
    compiled: Arc<rewrite::Compiled>,
    /// The keys and salts set in the configuration.
    salts: Vec<(&'static str, hyscribe_config::Secret)>,
    /// The login sessions, in primary mode only.
    sessions: Option<Arc<hyscribe_auth::sessions::Sessions>>,
    /// The gated writer, in primary mode only (ADR 0010).
    writer: Option<Arc<hyscribe_db::Writer>>,
    /// The constants of `home` and `siteurl` the configuration sets.
    defined: Vec<&'static str>,
    /// `REST_PUBLIC_USERS`: the public users REST endpoint is on.
    public_users: bool,
    /// `BEHIND_PROXY`: the proxy's `X-Forwarded-For` names the client.
    behind_proxy: bool,
    /// The capability constants of the configuration.
    constants: hyscribe_config::Constants,
    /// The login rate limits and their counts, kept across reloads.
    limiter: Arc<login::limits::Limiter>,
    /// The installation as configured, which the admin shows.
    install: Arc<Install>,
    /// The outbound client, when `HTTP_OUTBOUND` is on (ADR 0019).
    fetch: Option<Arc<hyscribe_fetch::Client>>,
}

/// The address a connection comes from.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Peer(pub(crate) std::net::IpAddr);

/// An installation before [`Site::with_install`]: `.env`'s defaults.
pub(crate) fn unconfigured() -> Arc<Install> {
    Arc::new(Install {
        binary: String::new(),
        upload_max_size: hyscribe_config::DEFAULT_UPLOAD_MAX_SIZE,
        database_user: String::new(),
        database_name: String::new(),
        charset: String::new(),
    })
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
        let mut site = Site {
            db,
            options: HashMap::new(),
            home: String::new(),
            theme: Arc::new(twentytwentyfive()),
            rewriter: Arc::default(),
            compiled: Arc::default(),
            salts: Vec::new(),
            sessions: None,
            writer: None,
            defined: Vec::new(),
            public_users: true,
            behind_proxy: false,
            constants: hyscribe_config::Constants::default(),
            limiter: Arc::new(login::limits::Limiter::new(Default::default())),
            install: unconfigured(),
            fetch: None,
        };
        for ((name, constant), url) in URL_CONSTANTS.into_iter().zip([home, siteurl]) {
            if let Some(url) = url {
                let url = url.trim_end_matches(['/', '\\']).as_bytes().to_vec();
                site.options.insert(name.to_string(), url);
                site.defined.push(constant);
            }
        }
        site.reloaded().await
    }

    /// The site as the database has it now, which every request reads
    /// (ADR 0012): the autoloaded options in one query, as
    /// `wp_load_alloptions()` reads them, under the configured addresses.
    async fn reloaded(&self) -> Result<Site, DbError> {
        let mut options: HashMap<String, Vec<u8>> =
            self.db.autoload_options().await?.into_iter().collect();
        for (name, constant) in URL_CONSTANTS {
            if self.defined.contains(&constant) {
                options.insert(name.to_string(), self.raw_option(name).to_vec());
            }
        }
        let home = String::from_utf8_lossy(options.get("home").map_or(&[][..], Vec::as_slice))
            .trim_end_matches('/')
            .to_string();
        let mut site = Site {
            db: self.db.clone(),
            options,
            home,
            theme: Arc::clone(&self.theme),
            rewriter: Arc::default(),
            compiled: Arc::clone(&self.compiled),
            salts: self.salts.clone(),
            sessions: self.sessions.clone(),
            writer: self.writer.clone(),
            defined: self.defined.clone(),
            public_users: self.public_users,
            behind_proxy: self.behind_proxy,
            constants: self.constants,
            limiter: Arc::clone(&self.limiter),
            install: Arc::clone(&self.install),
            fetch: self.fetch.clone(),
        };
        site.build_rewriter();
        Ok(site)
    }

    /// The site with the keys and salts `wp_hash()` signs with.
    pub fn with_salts(self, salts: Vec<(&'static str, hyscribe_config::Secret)>) -> Site {
        Site { salts, ..self }
    }

    /// The site with the public users REST endpoint on or off.
    pub fn with_public_users(self, public_users: bool) -> Site {
        Site {
            public_users,
            ..self
        }
    }

    /// The site with the capability constants of the configuration.
    pub fn with_constants(self, constants: hyscribe_config::Constants) -> Site {
        Site { constants, ..self }
    }

    /// The site with the login rate limits of the configuration.
    pub fn with_login_limits(self, limits: hyscribe_config::LoginLimits) -> Site {
        Site {
            limiter: Arc::new(login::limits::Limiter::new(limits)),
            ..self
        }
    }

    /// The site of the installation `install` describes.
    pub fn with_install(self, install: Install) -> Site {
        Site {
            install: Arc::new(install),
            ..self
        }
    }

    /// The site behind a proxy or not.
    pub fn with_proxy(self, behind_proxy: bool) -> Site {
        Site {
            behind_proxy,
            ..self
        }
    }

    /// The site in primary mode, keeping login sessions in `sessions`.
    pub fn with_sessions(self, sessions: hyscribe_auth::sessions::Sessions) -> Site {
        Site {
            sessions: Some(Arc::new(sessions)),
            ..self
        }
    }

    /// The site with the outbound client, none while `HTTP_OUTBOUND` is off.
    pub fn with_fetch(self, fetch: Option<hyscribe_fetch::Client>) -> Site {
        let fetch = fetch.map(Arc::new);
        Site { fetch, ..self }
    }

    /// The site with the writer `Writer::open` gave (none in shadow mode),
    /// which takes the addresses the configuration sets.
    pub fn with_writer(self, writer: Option<hyscribe_db::Writer>) -> Site {
        let urls = URL_CONSTANTS
            .into_iter()
            .filter(|(_, constant)| self.defined.contains(constant))
            .map(|(name, _)| (name, self.option(name)))
            .collect();
        let writer = writer.map(|writer| Arc::new(writer.with_urls(urls)));
        Site { writer, ..self }
    }

    /// `blog_public`: whether search engines may index the site.
    fn public(&self) -> bool {
        !Value::String(self.raw_option("blog_public").to_vec()).is_empty()
    }

    /// `wp_salt( $scheme )`: the configured keys and salts, else `option`'s.
    fn salt(&self, scheme: &str, option: impl Fn(&str) -> Option<Vec<u8>>) -> Option<Vec<u8>> {
        let constants: Vec<(&str, &[u8])> = self
            .salts
            .iter()
            .map(|(name, secret)| (*name, secret.expose().as_bytes()))
            .collect();
        hyscribe_auth::wp_salt(scheme, &constants, option)
    }

    /// The permalink options.
    fn permalinks(&self) -> Permalinks {
        Permalinks::new(
            &self.option("permalink_structure"),
            &self.option("category_base"),
            &self.option("tag_base"),
        )
    }

    /// The rewrite rules for the permalink options as they are now.
    fn build_rewriter(&mut self) {
        self.rewriter = self.compiled.rewriter(&rewrite::Settings {
            structure: &self.option("permalink_structure"),
            category_base: &self.option("category_base"),
            tag_base: &self.option("tag_base"),
            page_on_front: &self.option("page_on_front"),
            home_path: home_path(&self.home),
        });
    }

    fn option(&self, name: &str) -> String {
        String::from_utf8_lossy(self.raw_option(name)).into_owned()
    }

    fn raw_option(&self, name: &str) -> &[u8] {
        self.options.get(name).map_or(&[][..], Vec::as_slice)
    }

    /// The site's time zone, `wp_timezone()`.
    fn zone(&self) -> Zone {
        date::site_zone(
            self.raw_option("timezone_string"),
            self.raw_option("gmt_offset"),
        )
    }

    /// The page on the front page, if any.
    fn front_page(&self) -> Option<u64> {
        link::front_page(
            self.raw_option("show_on_front"),
            self.raw_option("page_on_front"),
        )
    }

    /// `is_front_page()` for the posts listed on the home page.
    fn posts_on_front(&self) -> bool {
        self.raw_option("show_on_front") == b"posts"
    }

    /// `page_for_posts` when the front shows a page: the page that lists
    /// the posts.
    fn posts_page(&self) -> Option<u64> {
        link::front_page(
            self.raw_option("show_on_front"),
            self.raw_option("page_for_posts"),
        )
        .filter(|&id| id != 0)
    }

    /// The site title, as `wp_get_document_title()` prints it.
    fn name(&self) -> String {
        esc_html(self.option("blogname").as_bytes())
    }

    /// `get_permalink( $post )`; see [`Site::links`].
    async fn permalink(self: &Arc<Self>, post: &Post) -> String {
        let links = self.links(vec![post.clone()]).await;
        links.into_iter().next().unwrap_or_default()
    }

    /// `get_permalink()` of each of `posts`. Pretty links read what they
    /// name on one blocking task, as rendering reads; when a read fails,
    /// or under plain permalinks, they are the plain links, which always
    /// resolve.
    async fn links(self: &Arc<Self>, posts: Vec<Post>) -> Vec<String> {
        let plain: Vec<String> = posts
            .iter()
            .map(|post| {
                lossy(&link::permalink(
                    self.home.as_bytes(),
                    self.front_page(),
                    post,
                ))
            })
            .collect();
        self.read(plain, move |reads| {
            posts
                .iter()
                .map(|post| lossy(&permalinks::permalink(reads, post)))
                .collect()
        })
        .await
    }

    /// `get_term_link()` of each of `terms` of `taxonomy`; see [`Site::links`].
    async fn term_links(self: &Arc<Self>, taxonomy: &str, terms: Vec<Term>) -> Vec<String> {
        let plain: Vec<String> = terms
            .iter()
            .map(|term| lossy(&link::term_link(self.home.as_bytes(), taxonomy, term)))
            .collect();
        let taxonomy = taxonomy.to_string();
        self.read(plain, move |reads| {
            terms
                .iter()
                .map(|term| lossy(&permalinks::term_link(reads, &taxonomy, term)))
                .collect()
        })
        .await
    }

    /// `read`'s result under pretty permalinks, or `plain` without them or
    /// when a read fails.
    async fn read<T: Clone + Send + 'static>(
        self: &Arc<Self>,
        plain: T,
        read: impl FnOnce(&Reads) -> T + Send + 'static,
    ) -> T {
        if !self.permalinks().pretty() {
            return plain;
        }
        let (site, fallback) = (Arc::clone(self), plain.clone());
        tokio::task::spawn_blocking(move || {
            let reads = Reads::new(&site, tokio::runtime::Handle::current());
            let value = read(&reads);
            reads.finish().map_or(fallback, |()| value)
        })
        .await
        .unwrap_or(plain)
    }
}

/// The router [`serve`] runs: each request sees the site as the database
/// has it now (ADR 0012).
fn live(site: Arc<Site>) -> Router {
    layered(Router::new().fallback(current).with_state(site))
}

/// The router for `site` as it is, whatever the database holds now.
#[cfg(test)]
fn router(site: Arc<Site>) -> Router {
    layered(Router::new().fallback(page).with_state(site))
}

fn layered(router: Router) -> Router {
    router
        .layer(axum::middleware::from_fn(request_line_limit))
        .layer(axum::middleware::map_response(security_headers))
}

/// [`page`] for the site as the database has it now.
async fn current(
    State(site): State<Arc<Site>>,
    method: axum::http::Method,
    uri: Uri,
    headers: HeaderMap,
    query: Query<HashMap<String, String>>,
    peer: Option<axum::Extension<Peer>>,
    body: axum::body::Bytes,
) -> Response {
    match site.reloaded().await {
        Ok(now) => {
            page(
                State(Arc::new(now)),
                method,
                uri,
                headers,
                query,
                peer,
                body,
            )
            .await
        }
        Err(err) => html(database_error(&site, &err)),
    }
}

/// The longest request line (method, URL and protocol with their spaces)
/// Apache serves at its default `LimitRequestLine`, so the longest PHP
/// WordPress behind it is ever asked for. It also keeps every list in a
/// query string far below the values one database query can bind.
const MAX_REQUEST_LINE: usize = 8191;

/// Apache's 414 page, without the server signature it ends with.
const URI_TOO_LONG: &str =
    "<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.01//EN\" \"http://www.w3.org/TR/html4/strict.dtd\">
<html><head>
<title>414 Request-URI Too Long</title>
</head><body>
<h1>Request-URI Too Long</h1>
<p>The requested URL's length exceeds the capacity
limit for this server.<br />
</p>
</body></html>
";

/// A request line longer than [`MAX_REQUEST_LINE`] gets Apache's 414.
async fn request_line_limit(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let line = request.method().as_str().len()
        + 1
        + request.uri().to_string().len()
        + 1
        + "HTTP/1.1".len();
    if line <= MAX_REQUEST_LINE {
        return next.run(request).await;
    }
    (
        StatusCode::URI_TOO_LONG,
        [(header::CONTENT_TYPE, "text/html; charset=iso-8859-1")],
        URI_TOO_LONG,
    )
        .into_response()
}

/// Connections served at once. More wait in the listen backlog, so many
/// open connections cannot take every file descriptor, which the database
/// pool needs too.
const MAX_CONNECTIONS: usize = 1024;

/// How long a client has to send a request's headers, from when the
/// connection opens or its last response ends. hyper has this default but
/// keeps it only with a timer, which `axum::serve` does not give it.
const HEADER_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Serves `site` on `listener` until `shutdown` completes, then finishes the
/// requests in flight.
pub async fn serve(
    listener: TcpListener,
    site: Arc<Site>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) {
    serve_connections(listener, live(site), MAX_CONNECTIONS, shutdown).await;
}

/// Serves `app` as [`serve`] serves a site, with its own headers: setup
/// mode's wizard, before there is a site.
pub async fn serve_app(
    listener: TcpListener,
    app: Router,
    shutdown: impl Future<Output = ()> + Send + 'static,
) {
    serve_connections(listener, app, MAX_CONNECTIONS, shutdown).await;
}

/// [`serve`] with at most `limit` connections open.
async fn serve_connections(
    mut listener: TcpListener,
    app: Router,
    limit: usize,
    shutdown: impl Future<Output = ()>,
) {
    let mut http = http1::Builder::new();
    http.timer(TokioTimer::new())
        .header_read_timeout(HEADER_READ_TIMEOUT);
    let permits = Arc::new(Semaphore::new(limit));
    let graceful = GracefulShutdown::new();
    let mut shutdown = std::pin::pin!(shutdown);
    loop {
        // axum's accept logs and retries errors such as running out of file
        // descriptors.
        let accepted = async {
            let permit = permits.clone().acquire_owned().await.ok();
            (permit, axum::serve::Listener::accept(&mut listener).await)
        };
        let (permit, (stream, addr)) = tokio::select! {
            accepted = accepted => accepted,
            () = &mut shutdown => break,
        };
        let service = app.clone().layer(axum::Extension(Peer(addr.ip())));
        let connection = graceful
            .watch(http.serve_connection(TokioIo::new(stream), TowerToHyperService::new(service)));
        tokio::spawn(async move {
            let _ = connection.await;
            drop(permit);
        });
    }
    // New clients are refused at once while the open connections finish.
    drop(listener);
    graceful.shutdown().await;
}

/// Embed pages are made to be framed by other sites, so they go without
/// `frame-ancestors`, as WordPress sends them.
async fn security_headers(mut response: Response) -> Response {
    let headers = response.headers_mut();
    let embed = headers.contains_key("x-wp-embed");
    for (name, value) in SECURITY_HEADERS {
        if embed && name == "content-security-policy" {
            continue;
        }
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
    /// A sitemap or its stylesheet.
    Xml(Vec<u8>),
    /// `robots.txt`.
    Robots(Vec<u8>),
    /// `do_favicon()`.
    Favicon,
    /// `wp-trackback.php`: the post's ID, 0 for none, and the query's status.
    Trackback(i64, StatusCode),
}

/// One page to render: the status, the document title, and the request
/// rendered with the first of the template slugs the theme has.
struct Page {
    status: StatusCode,
    title: String,
    candidates: Vec<String>,
    request: Request,
    /// It asks for a post visitors cannot see, whose redirects
    /// `redirect_canonical()` cancels.
    hidden: bool,
}

/// A page as sent: the status and the document.
struct Rendered {
    status: StatusCode,
    document: String,
}

async fn page(
    State(site): State<Arc<Site>>,
    method: axum::http::Method,
    uri: Uri,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
    peer: Option<axum::Extension<Peer>>,
    body: axum::body::Bytes,
) -> Response {
    // Before the method check, so a login form's POST is redirected too.
    let read = method == Method::GET || method == Method::HEAD;
    if let Some((status, location)) = admin_redirect(&site, uri.path(), uri.query(), read) {
        return (status, [(header::LOCATION, location)]).into_response();
    }
    let uri = in_place(uri);
    let (head, post) = (method == Method::HEAD, method == Method::POST);
    let page_method = method == Method::GET || head || post;
    let file = files::static_file(uri.path(), &headers, now() as u64);
    if let Some(response) = file.filter(|_| page_method && !post) {
        return response;
    }
    let peer = peer.map(|axum::Extension(Peer(ip))| ip);
    if uri.path() == login::PATH && page_method {
        let form = if post {
            trackback::form(&headers, &body)
        } else {
            Value::Array(Vec::new())
        };
        let asked = login::Asked {
            post,
            query: uri.query().map(String::from),
            headers,
            form,
            peer,
        };
        return login::respond(&site, asked).await;
    }
    let siteurl = site.option("siteurl");
    if uri.path().strip_prefix(home_path(&siteurl)) == Some(comment_post::PATH) {
        let asked = comment_post::Asked {
            post,
            headers,
            body,
            peer,
        };
        return comment_post::respond(&site, asked).await;
    }
    // The loaders read only the query, whatever the method, as PHP's do.
    if let Some(loader) = loaders::Loader::at(uri.path()).filter(|_| page_method) {
        return loaders::load(loader, uri.query(), &headers, now() as u64);
    }
    if admin::is_admin(uri.path()) && page_method {
        // `$_POST`, unless the body is one only PHP reads.
        let form = if post {
            trackback::form(&headers, &body)
        } else {
            Value::Array(Vec::new())
        };
        let read = !post || body.is_empty() || form.as_array().is_some_and(|f| !f.is_empty());
        let form = read.then_some(form);
        return admin::respond(&site, uri, headers, (post, peer), form).await;
    }
    // The request as made is `$_SERVER['REQUEST_URI']` and `$_GET`.
    let original = uri.clone();
    let (uri, query) = match pretty_request(&site, uri, query).await {
        Ok(request) => request,
        Err(err) => return html(database_error(&site, &err)),
    };
    // `rest_api_loaded()`, for any method.
    match rest::route(&original, &query) {
        Some(Some(route)) => {
            let asked = rest::Asked {
                method,
                uri: original,
                headers,
                body,
                route,
            };
            return rest::respond(&site, asked).await;
        }
        Some(None) => return StatusCode::NOT_IMPLEMENTED.into_response(),
        None if !page_method => return StatusCode::METHOD_NOT_ALLOWED.into_response(),
        None => {}
    }
    // `WP::send_headers()` sends the status `error` asks for; these end the
    // request before any redirect.
    let status = error_status(&query);
    if let Some(status) = status.filter(|s| matches!(s.as_u16(), 403 | 500 | 502 | 503)) {
        return (status, [(header::CONTENT_TYPE, "text/html; charset=UTF-8")]).into_response();
    }
    let file = trackback::is_file(uri.path());
    let routed = if file {
        trackback::outcome(&site, &uri, &query)
            .await
            .map(|outcome| (outcome, Vec::new()))
    } else {
        outcome(&site, (&uri, &original), &query).await
    };
    let (outcome, links) = match routed {
        Ok(routed) => routed,
        Err(err) => return html(database_error(&site, &err)),
    };
    let mut response = match outcome {
        // The template loader ends a HEAD request before either.
        Outcome::Favicon | Outcome::Trackback(..) if head && !file => {
            [(header::CONTENT_TYPE, "text/html; charset=UTF-8")].into_response()
        }
        Outcome::Trackback(id, status) => {
            let form = if post {
                trackback::form(&headers, &body)
            } else {
                Value::Array(Vec::new())
            };
            trackback::respond(&site, id, status, &form).await
        }
        // Other forms are not handled yet.
        _ if post => StatusCode::METHOD_NOT_ALLOWED.into_response(),
        Outcome::Redirect(location) => (
            StatusCode::MOVED_PERMANENTLY,
            [(header::LOCATION, location)],
        )
            .into_response(),
        Outcome::Feed(page, feed) => {
            feed::respond(
                site.clone(),
                requested(*page, &original, &uri),
                feed,
                &headers,
            )
            .await
        }
        Outcome::Xml(body) => {
            let content_type = [(header::CONTENT_TYPE, "application/xml; charset=UTF-8")];
            (content_type, body).into_response()
        }
        Outcome::Robots(body) => {
            let content_type = [(header::CONTENT_TYPE, "text/plain; charset=utf-8")];
            (content_type, body).into_response()
        }
        Outcome::Favicon => {
            let icon = format!(
                "{}/wp-includes/images/w-logo-gray-white-bg.png",
                site.option("siteurl")
            );
            (StatusCode::FOUND, [(header::LOCATION, icon)]).into_response()
        }
        Outcome::Page(mut page) => {
            let embed = embeds(&query, &uri, page.request.singular);
            page.request.embed = embed;
            let mut response = html(render(&site, requested(*page, &original, &uri)).await);
            if embed {
                response
                    .headers_mut()
                    .insert("x-wp-embed", HeaderValue::from_static("true"));
            }
            response
        }
    };
    // Redirects and failures keep their own.
    let own = response.status().is_redirection() || response.status().is_server_error();
    if let Some(status) = status.filter(|_| !own) {
        *response.status_mut() = status;
    }
    for link in links
        .into_iter()
        .filter_map(|l| HeaderValue::try_from(l).ok())
    {
        response.headers_mut().append(header::LINK, link);
    }
    response
}

/// The status `WP::send_headers()` sends for a set `error`: its integer,
/// when `status_header()` knows it. Informational codes, which cannot end
/// a response, are left out.
fn error_status(query: &HashMap<String, String>) -> Option<StatusCode> {
    const KNOWN: [u16; 57] = [
        200, 201, 202, 203, 204, 205, 206, 207, 226, 300, 301, 302, 303, 304, 305, 306, 307, 308,
        400, 401, 402, 403, 404, 405, 406, 407, 408, 409, 410, 411, 412, 413, 414, 415, 416, 417,
        418, 421, 422, 423, 424, 425, 426, 428, 429, 431, 451, 500, 501, 502, 503, 504, 505, 506,
        507, 510, 511,
    ];
    let code = query
        .get("error")
        .filter(|v| !is_empty_string(v.as_bytes()))
        .map(|v| Value::String(v.as_bytes().to_vec()).to_int())?;
    u16::try_from(code)
        .ok()
        .filter(|code| KNOWN.contains(code))
        .and_then(|code| StatusCode::from_u16(code).ok())
}

/// `'404' == $error`, for which `WP_Query` is a 404 whatever it finds.
fn asks_404(query: &HashMap<String, String>) -> bool {
    query.get("error").is_some_and(|v| {
        let v = Value::String(v.as_bytes().to_vec());
        v.is_numeric() && v.to_float() == 404.0
    })
}

/// WordPress's own PHP files, which the web server runs without the
/// rewrite rules.
const PHP_FILES: [&str; 12] = [
    "/wp-activate.php",
    "/wp-blog-header.php",
    "/wp-config-sample.php",
    "/wp-config.php",
    "/wp-cron.php",
    "/wp-links-opml.php",
    "/wp-load.php",
    "/wp-mail.php",
    "/wp-settings.php",
    "/wp-signup.php",
    "/wp-trackback.php",
    "/xmlrpc.php",
];

/// A request under pretty permalinks as the query it stands for
/// (`WP::parse_request()`): the path's query vars join the request's own,
/// which win, and the path becomes the home page's.
async fn pretty_request(
    site: &Site,
    uri: Uri,
    mut query: HashMap<String, String>,
) -> Result<(Uri, HashMap<String, String>), DbError> {
    let home_path = home_path(&site.home);
    let local = uri.path().strip_prefix(home_path).unwrap_or(uri.path());
    if site.rewriter.is_empty() || reserved(uri.path()) || PHP_FILES.contains(&local) {
        return Ok((uri, query));
    }
    let parsed = rewrite::parse_request(site, &site.rewriter, home_path, uri.path()).await?;
    // The request's own `error` is dropped: a matched or empty path clears
    // it, and an unmatched one sets 404.
    query.remove("error");
    let mut raw = lossy(&remove_query_arg(
        &[b"error"],
        uri.query().unwrap_or_default().as_bytes(),
    ));
    for (name, value) in parsed.query_vars() {
        if query.contains_key(&name) {
            continue;
        }
        if !value.is_empty() {
            let sep = if raw.is_empty() { "" } else { "&" };
            raw.push_str(&format!("{sep}{name}={}", lossy(&urlencode(&value))));
        }
        query.insert(name, lossy(&value));
    }
    let path = format!("{home_path}/");
    let uri = if raw.is_empty() {
        path
    } else {
        format!("{path}?{raw}")
    };
    Ok((uri.parse().unwrap_or_default(), query))
}

/// What a request gets, from the route through `redirect_canonical()`,
/// sitemaps and robots.txt, and the `Link` headers `template_redirect`
/// sends after `redirect_canonical()`, which a sitemap never reaches.
async fn outcome(
    site: &Arc<Site>,
    (uri, original): (&Uri, &Uri),
    query: &HashMap<String, String>,
) -> Result<(Outcome, Vec<String>), DbError> {
    let outcome = route(site, uri.path(), uri.query(), query).await?;
    let links = link_headers(
        site,
        match &outcome {
            Outcome::Page(page) | Outcome::Feed(page, _) => Some(&page.request),
            _ => None,
        },
    );
    let get = parse_str(uri.query().unwrap_or_default().as_bytes());
    let outcome = match quiet(query, uri.query()) {
        Some(kind) => uncanonical(kind, uri, query, outcome),
        None => match preview(query, &get, outcome) {
            ControlFlow::Break(outcome) => outcome,
            ControlFlow::Continue(outcome) if site.permalinks().pretty() => {
                canonical::pretty(site, original, query, outcome).await
            }
            ControlFlow::Continue(outcome) => {
                // A preview a visitor is not shown drops `preview` from redirects.
                let strip = get.get_str("preview").is_some_and(|v| !v.is_empty());
                cleaned(site, uri.path(), uri.query(), query, outcome, strip)
            }
        },
    };
    let outcome = robots(site, query, sitemaps::serve(site, query, outcome).await?);
    let sent = match &outcome {
        Outcome::Redirect(_) | Outcome::Xml(_) => false,
        Outcome::Feed(_, feed) => feed.redirect.is_none(),
        _ => true,
    };
    Ok((outcome, if sent { links } else { Vec::new() }))
}

/// `rest_output_link_header()` and `wp_shortlink_header()`: the REST API
/// root, and for a page's request the REST route of what it found and a
/// singular post's shortlink.
fn link_headers(site: &Site, request: Option<&Request>) -> Vec<String> {
    let home = site.home.as_bytes();
    let links = site.permalinks();
    let (root, route) = request.map_or((link::rest_url(home, &links, "/"), None), |request| {
        link::rest_links(home, &links, request)
    });
    let mut links = vec![format!(
        "<{}>; rel=\"https://api.w.org/\"",
        lossy(&esc_url_raw(&root))
    )];
    if let Some(route) = route {
        links.push(format!(
            "<{}>; rel=\"alternate\"; title=\"JSON\"; type=\"application/json\"",
            lossy(&esc_url_raw(&route))
        ));
    }
    if let Some(shortlink) = request.and_then(|r| link::shortlink(home, site.front_page(), r)) {
        links.push(format!("<{}>; rel=shortlink", lossy(&shortlink)));
    }
    links
}

/// `is_embed()`: a set `embed` on a query `WP_Query` parses as singular
/// (or that is the page on the front), or as a 404 of its own. The
/// `attachment` var is not routed, so it is left out.
fn embeds(query: &HashMap<String, String>, uri: &Uri, singular: bool) -> bool {
    let get = parse_str(uri.query().unwrap_or_default().as_bytes());
    let text = |name| match get.get_str(name) {
        Some(Value::String(text)) => text.clone(),
        _ => Vec::new(),
    };
    let int = |name| Value::String(text(name)).to_int();
    let parsed = int("p") > 0
        || ["page_id", "attachment_id", "subpost_id"]
            .iter()
            .any(|name| int(name) != 0)
        || ["name", "pagename"]
            .iter()
            .any(|name| !php_trim(&text(name)).is_empty());
    php_set(&get, "embed") && (singular || parsed || bad_p(query, uri.query()) || asks_404(query))
}

/// Whether `$value` is set as `'' != $value` takes it, which an array is.
fn php_set(get: &Value, name: &str) -> bool {
    get.get_str(name)
        .is_some_and(|v| v.as_array().is_some() || !v.to_php_string().is_empty())
}

/// A preview, for which `redirect_canonical()` does nothing and the page
/// prints `wp_post_preview_js()`, or the outcome as it was to go on with.
/// `is_preview()` is a set `preview` that a visitor, who never previews,
/// keeps unless the query is a 404 or finds the published post its `p`
/// or `page_id` asks for.
fn preview(
    query: &HashMap<String, String>,
    get: &Value,
    outcome: Outcome,
) -> ControlFlow<Outcome, Outcome> {
    let set = php_set(get, "preview");
    let asked = ["p", "page_id"]
        .iter()
        .any(|k| query.get(*k).is_some_and(|v| absint(v) > 0));
    let shown = |page: &Page| set && !page.request.not_found && !(asked && page.request.singular);
    match outcome {
        Outcome::Page(mut page) if shown(&page) => {
            page.request.preview = true;
            ControlFlow::Break(Outcome::Page(page))
        }
        // A feed is never a 404 unless it is paged.
        Outcome::Feed(page, mut feed)
            if shown(&page) || set && page.request.not_found && !Paging::of(query).is_paged() =>
        {
            feed.redirect = None;
            ControlFlow::Break(Outcome::Feed(page, feed))
        }
        outcome => ControlFlow::Continue(outcome),
    }
}

/// A negative or array `p`.
fn bad_p(query: &HashMap<String, String>, raw_query: Option<&str>) -> bool {
    let negative = query
        .get("p")
        .is_some_and(|p| Value::String(p.as_bytes().to_vec()).to_int() < 0);
    negative
        || parse_str(raw_query.unwrap_or_default().as_bytes())
            .get_str("p")
            .is_some_and(|p| p.as_array().is_some())
}

/// The template loader's favicon and trackback, for which
/// `redirect_canonical()` does nothing.
#[derive(Clone, Copy)]
enum Quiet {
    Favicon,
    Trackback,
}

/// Which of [`Quiet`] the request is, as `WP_Query` sets its flags: the
/// favicon only without robots.txt, and neither when the query itself is
/// `error=404` (that var, a negative or array `p`, or an invalid date).
fn quiet(query: &HashMap<String, String>, raw_query: Option<&str>) -> Option<Quiet> {
    if bad_p(query, raw_query) || query::date_query(query).invalid || asks_404(query) {
        return None;
    }
    let get = parse_str(raw_query.unwrap_or_default().as_bytes());
    let set = |name| get.get_str(name).is_some_and(|v: &Value| !v.is_empty());
    if set("favicon") && !set("robots") {
        Some(Quiet::Favicon)
    } else {
        php_set(&get, "tb").then_some(Quiet::Trackback)
    }
}

/// `outcome` without `redirect_canonical()`'s redirects, as the template
/// loader sends a favicon (after robots.txt) or a trackback (after feeds).
/// `handle_404()` never sends a favicon's 404; a trackback's, or a paged
/// feed's, clears the flag, so its page and redirects stand.
fn uncanonical(
    kind: Quiet,
    uri: &Uri,
    query: &HashMap<String, String>,
    outcome: Outcome,
) -> Outcome {
    let id = match (kind, outcome) {
        (Quiet::Favicon, _) => return Outcome::Favicon,
        (_, Outcome::Page(page)) if page.request.not_found => return Outcome::Page(page),
        (_, Outcome::Feed(page, feed))
            if page.request.not_found && Paging::of(query).is_paged() =>
        {
            return Outcome::Feed(page, feed);
        }
        (_, Outcome::Feed(page, mut feed)) => {
            feed.redirect = None;
            return Outcome::Feed(page, feed);
        }
        (_, Outcome::Page(page)) => trackback::post_id(uri, trackback::singular(&page.request)),
        (_, outcome) => return outcome,
    };
    Outcome::Trackback(id, StatusCode::OK)
}

/// `outcome` with the query cleaned up as `redirect_canonical()` ends: a
/// redirect's location cleaned, and a page or feed sent where its query
/// cleans up to, unless cleaning that again would change it, which
/// `redirect_canonical()` takes for a chain of redirects. A search is left
/// alone.
fn cleaned(
    site: &Site,
    path: &str,
    raw_query: Option<&str>,
    query: &HashMap<String, String>,
    outcome: Outcome,
    strip_preview: bool,
) -> Outcome {
    let tidy = |query: &str| {
        if strip_preview {
            clean_query(&lossy(&remove_query_arg(&[b"preview"], query.as_bytes())))
        } else {
            clean_query(query)
        }
    };
    if let Outcome::Page(page) | Outcome::Feed(page, _) = &outcome
        && page.hidden
    {
        return outcome;
    }
    let raw = raw_query.unwrap_or_default();
    let clean = tidy(raw);
    // Any `s` makes a search, unless a post is asked for.
    let search = query.contains_key("s") && single_post(query).is_none();
    let redirect = (!search && !reserved(path) && clean != raw && stays_clean(&clean))
        .then(|| with_query(&format!("{}{path}", site.home), &clean));
    match outcome {
        Outcome::Redirect(location) => Outcome::Redirect(clean_location(&location, tidy)),
        Outcome::Feed(page, mut feed) => {
            feed.redirect = feed
                .redirect
                .map(|l| clean_location(&l, tidy))
                .filter(|l| stays_clean(l.split_once('?').map_or("", |(_, q)| q)))
                .or(redirect);
            Outcome::Feed(page, feed)
        }
        outcome => redirect.map_or(outcome, Outcome::Redirect),
    }
}

/// `outcome`, or for `robots`, robots.txt: the template loader serves it
/// in place of any page or feed that does not redirect, after sitemaps.
fn robots(site: &Site, query: &HashMap<String, String>, outcome: Outcome) -> Outcome {
    let asked = query
        .get("robots")
        .is_some_and(|v| !is_empty_string(v.as_bytes()))
        && !asks_404(query);
    match outcome {
        Outcome::Page(_) | Outcome::Feed(..) | Outcome::Favicon | Outcome::Trackback(..)
            if asked && !sitemaps::redirects(&outcome) =>
        {
            Outcome::Robots(sitemaps::robots(site))
        }
        outcome => outcome,
    }
}

/// Whether cleaning `query` again leaves it as it is, without which
/// `redirect_canonical()` takes a redirect for the first of a chain.
fn stays_clean(query: &str) -> bool {
    clean_query(query) == query
}

/// `location` with its query cleaned up.
fn clean_location(location: &str, tidy: impl Fn(&str) -> String) -> String {
    location.split_once('?').map_or_else(
        || location.to_string(),
        |(base, query)| with_query(base, &tidy(query)),
    )
}

/// `base`, then `?query` unless the query is empty.
fn with_query(base: &str, query: &str) -> String {
    if query.is_empty() {
        base.to_string()
    } else {
        format!("{base}?{query}")
    }
}

/// `page` with the request as made, `uri`, and `vars`, the query it stands
/// for with what its path gives.
fn requested(page: Page, uri: &Uri, vars: &Uri) -> Page {
    Page {
        request: Request {
            uri: request_uri(uri),
            get: parse_str(uri.query().unwrap_or_default().as_bytes()),
            vars: parse_str(vars.query().unwrap_or_default().as_bytes()),
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
fn html(page: Rendered) -> Response {
    (
        page.status,
        [(header::CONTENT_TYPE, "text/html; charset=UTF-8")],
        page.document,
    )
        .into_response()
}

/// `page` with its rendered body, or the 500 page when rendering met a
/// database error or panicked.
fn rendered(
    site: &Site,
    page: Page,
    body: Result<Result<String, DbError>, tokio::task::JoinError>,
) -> Rendered {
    match finished(site, body) {
        Ok(document) => Rendered {
            status: page.status,
            document,
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
        document: layout(
            &format!(
                "<head>\n<meta charset=\"UTF-8\" />\n<title>Error &#8211; {}</title>\n</head>",
                site.name()
            ),
            "<body>\n<p>Error establishing a database connection</p>\n</body>",
        ),
    }
}

/// The document: the first template in `candidates` the theme has,
/// rendered for `request` with the database read on this thread, and the
/// head its blocks enqueued styles for; or for an embed, the embed page.
fn render_page(
    site: &Site,
    candidates: &[String],
    request: &Request,
    title: &str,
) -> Result<String, DbError> {
    let reads = Reads::new(site, tokio::runtime::Handle::current());
    let document = if request.embed {
        lossy(&embed_page(
            &site.theme,
            &reads,
            request,
            title.as_bytes(),
            &share(),
        ))
    } else {
        // Every block theme has an index template.
        let template = site
            .theme
            .template(candidates)
            .map_or(&[][..], |(_, template)| template);
        let (html, assets) = render_template(&site.theme, &reads, request, template);
        let classes = body_class(&reads, request, site.theme.slug());
        let head = wp_head(&site.theme, &reads, request, &assets, title.as_bytes());
        let footer = wp_footer(&reads, &assets);
        layout(
            &lossy(&head),
            &format!(
                "<body class=\"{}\">\n\n{}\n{}</body>",
                String::from_utf8_lossy(&classes),
                String::from_utf8_lossy(&html),
                String::from_utf8_lossy(&footer)
            ),
        )
    };
    reads.finish()?;
    Ok(document)
}

/// The embed sharing dialog's secret, ten letters and digits, and ID
/// suffix, as large as `wp_rand()`'s: drawn from the standard library's
/// randomly keyed hasher, as they guard nothing.
fn share() -> Share {
    use std::hash::{BuildHasher, RandomState};
    let mut draw = RandomState::new().hash_one(0_u8);
    let mut secret = String::new();
    for _ in 0..10 {
        // Base 62: digits and lower case letters, then upper case ones.
        let digit = u32::try_from(draw % 62).unwrap_or_default();
        secret.push(char::from_digit(digit, 36).unwrap_or_else(|| {
            char::from_digit(digit - 26, 36)
                .unwrap_or_default()
                .to_ascii_uppercase()
        }));
        draw /= 62;
    }
    Share {
        secret,
        // Truncated to `wp_rand()`'s 32 bits.
        suffix: (RandomState::new().hash_one(1_u8) as u32).to_string(),
    }
}

/// The time relative dates are measured from.
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// The canonical form of `path` with plain permalinks, as
/// `redirect_canonical()` gives it: `/index.php` goes to `/`, a
/// `/index.php/` inside it to `/`, and other paths gain a trailing slash
/// on the front page. `None` when `path` is canonical.
fn canonical_path(site: &Site, path: &str, raw_query: Option<&str>) -> Option<String> {
    let query = raw_query.map(|q| format!("?{q}")).unwrap_or_default();
    if path.contains("/index.php/") {
        return Some(format!(
            "{}{}{query}",
            site.home,
            path.replace("/index.php/", "/")
        ));
    }
    if let Some(folder) = path.strip_suffix("index.php").filter(|f| f.ends_with('/')) {
        return Some(format!("{}{folder}{query}", site.home));
    }
    // The home page these paths show takes it only as `is_front_page()`.
    let front = site.posts_on_front() || site.front_page().is_some_and(|id| id != 0);
    (front && !path.ends_with('/')).then(|| format!("{}{path}/{query}", site.home))
}

/// HyScribe's admin and login URLs have no `.php` (ADR 0008):
/// `/hs-login` and `/hs-admin/<screen>`. The WordPress forms redirect there
/// with the query kept: `/wp-login.php`, and `/wp-admin/<screen>.php` with
/// `index.php` as `/hs-admin/`.
///
/// Those redirects are 307: browsers do not cache them, so a site switched
/// back to PHP WordPress gets its own admin back, and a POST stays a POST.
/// `/hs-login.php` gets a permanent 308; `/hs-admin/<screen>.php` a 301 when
/// `read` (GET or HEAD), else it is served [`in_place`].
fn admin_redirect(
    site: &Site,
    path: &str,
    raw_query: Option<&str>,
    read: bool,
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
                (None, Some(screen)) if read && screen.ends_with(".php") => {
                    (StatusCode::MOVED_PERMANENTLY, screen)
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

/// `uri` with `/hs-admin/<screen>.php` as `/hs-admin/<screen>`: a request
/// other than a GET or HEAD is served there, as a redirect would drop its body.
fn in_place(uri: Uri) -> Uri {
    let Some(screen) = uri
        .path()
        .strip_prefix("/hs-admin/")
        .and_then(|screen| screen.strip_suffix(".php"))
    else {
        return uri;
    };
    let query = uri.query().map(|q| format!("?{q}")).unwrap_or_default();
    format!("/hs-admin/{screen}{query}").parse().unwrap_or(uri)
}

/// Paths no canonical redirect touches: the static trees, the login screen
/// and hs-admin.
fn reserved(path: &str) -> bool {
    is_static(path) || path == "/hs-login" || path == "/hs-admin" || path.starts_with("/hs-admin/")
}

/// The post a request asks for, as `(query var, ID, post type)`: a non-zero
/// `page_id` takes the place of `p`'s ID, but `p` keeps the post type, and
/// an ID of 0 asks for no post.
fn single_post(query: &HashMap<String, String>) -> Option<(&'static str, u64, &'static str)> {
    let id = |name| query.get(name).map_or(0, |value| absint(value));
    match (id("p"), id("page_id")) {
        (0, 0) => None,
        (0, page_id) => Some(("page_id", page_id, "page")),
        (p, page_id) => Some(("p", if page_id > 0 { page_id } else { p }, "post")),
    }
}

/// Static file trees: never redirected or rewritten, so their public URLs
/// stay as they are. Of them, the default theme's and `wp-includes` files
/// are served.
fn is_static(path: &str) -> bool {
    path.starts_with("/wp-content/") || path.starts_with("/wp-includes/")
}

async fn route(
    site: &Arc<Site>,
    path: &str,
    raw_query: Option<&str>,
    query: &HashMap<String, String>,
) -> Result<Outcome, DbError> {
    if reserved(path) {
        return Ok(Outcome::Page(Box::new(not_found(site, false))));
    }
    if let Some(location) =
        canonical_path(site, path, raw_query).filter(|_| quiet(query, raw_query).is_none())
    {
        return Ok(Outcome::Redirect(location));
    }
    // `WP::query_vars`, which `handle_404()` reads, before the page on the
    // front takes its place.
    let asked = query;
    let front = front_query(site, query);
    let query = front.as_ref().unwrap_or(query);
    // The path has been parsed or is ignored. Author archives give away
    // login names, so they are off by default (docs/SECURITY_MODEL.md).
    // A structure with `%author%` names the author of a post's link, which
    // is no archive.
    let post_asked = ["p", "page_id", "name", "pagename"]
        .iter()
        .any(|name| query.get(*name).is_some_and(|v| !v.is_empty()));
    if query.contains_key("author") || (query.contains_key("author_name") && !post_asked) {
        return Ok(Outcome::Page(Box::new(not_found(site, false))));
    }
    // A negative or array `p` is `error=404` too: the 404 page with a 200
    // (where `WP::send_headers()` sends the var's own status), or where an
    // invalid date or the post of `p`, `page_id` or `attachment_id` sends it.
    let current = format!("{}{path}?{}", site.home, raw_query.unwrap_or_default());
    // `subpost_id`, when set, is `attachment_id`.
    let attachment = ["subpost_id", "attachment_id"]
        .iter()
        .find_map(|k| query.get(*k).filter(|v| !v.is_empty()))
        .map_or(0, |id| absint(id));
    // RSS2, Atom, RDF and RSS 0.92 feeds. `feed=rss` alone redirects to
    // RSS2, as `redirect_canonical()` rewrites only that pair.
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
    let bad = bad_p(query, raw_query);
    // A feed of the 404 page, with no var naming a post but a bad `p`, is
    // the posts feed of the query without that `p`, under the 404 page's
    // title; only `error=404` sends a 404.
    let named = ["page_id", "name", "pagename", "attachment_id", "subpost_id"]
        .iter()
        .chain((!bad).then_some(&"p"))
        .any(|name| query.get(*name).is_some_and(|v| !v.is_empty()));
    let feed_404 = (bad || asks_404(query)) && format.is_some() && !named;
    let without_p;
    let query = if feed_404 {
        let mut query = query.clone();
        query.remove("p");
        without_p = query;
        &without_p
    } else {
        query
    };
    let id = |name| query.get(name).map_or(0, |v| absint(v));
    let p = if bad { 0 } else { id("p") };
    // The post `p`, `page_id` or `attachment_id` names; ID 0 is no post's.
    let target = p.max(id("page_id")).max(attachment);
    if let Some(format) = format.filter(|_| asks_404(query) && target > 0) {
        let post = site.db.post(target).await?;
        return error_404_feed(site, post, raw_query, &current, format).await;
    }
    if (bad || asks_404(query)) && !query.contains_key("feed") {
        if let Some(location) = date_redirect(site, raw_query, query) {
            return Ok(Outcome::Redirect(location));
        }
        return Ok(
            match redirect_to(site, target, raw_query, error_404(site, false)).await? {
                Outcome::Redirect(location) if location == current => {
                    Outcome::Page(Box::new(error_404(site, false)))
                }
                outcome => outcome,
            },
        );
    }
    // `attachment_id`, or `subpost_id` over it, is get_posts()'s `p`: a post
    // or page redirects to its link; attachment pages are not served yet.
    let alone = ["p", "page_id", "name", "pagename", "feed"]
        .iter()
        .all(|k| !query.contains_key(*k));
    if alone && attachment > 0 {
        return redirect_to(site, attachment, raw_query, not_found(site, false)).await;
    }
    let single = match single_post(query) {
        Some(single) => Some(single),
        None => match by_slug(site, raw_query, query).await? {
            Some(Slug::Moved(location)) => return Ok(Outcome::Redirect(location)),
            Some(Slug::Post(single)) => Some(single),
            None => None,
        },
    };
    // A slug's `error=404` feed that the guess found nothing for.
    if let Some(format) = format.filter(|_| asks_404(query) && single.is_some()) {
        return error_404_feed(site, None, raw_query, &current, format).await;
    }
    // `is_posts_page`: the page for posts, by `page_id` or `pagename`, is
    // the posts listed under its title.
    let posts_page = single
        .filter(|&(name, id, _)| name != "p" && site.posts_page() == Some(id))
        .map(|(name, id, _)| (name, id));
    let listed;
    let (single, query) = match posts_page {
        Some(_) => {
            let mut query = query.clone();
            query.remove("page_id");
            query.remove("pagename");
            listed = query;
            (None, &listed)
        }
        None => (single, query),
    };
    let single_requested = single.is_some();
    let with_comments =
        query.get("feed").is_some_and(|f| f.contains("comments-")) || flag("withcomments");
    let comment_feed = with_comments || (single_requested && !flag("withoutcomments"));
    // A `cat` that only excludes makes no archive; one that includes is a
    // term archive.
    let archive = ["s", "m", "year", "monthnum", "day"]
        .iter()
        .any(|k| query.contains_key(*k));
    if let Some(format) = format.filter(|_| !single_requested) {
        // A search's or archive's comment feed lists the comments on the
        // posts it matches.
        let (post_query, terms) = main_query(site, query, true).await?;
        let comments = comment_feed.then(|| {
            if archive || terms.archive.is_some() {
                feed::CommentsOf::Posts(Box::new(post_query.clone()))
            } else {
                feed::CommentsOf::Site
            }
        });
        let ids = posts_page.map(|(_, id)| id);
        let (page, found) = list(site, query, true, (post_query, terms), ids).await?;
        let mut page = posts_page_titled(site, page, posts_page, found.as_ref(), Paging::of(query));
        if feed_404 {
            page.title = not_found(site, false).title;
        }
        // A paged query with nothing in it is a 404: a feed of no posts,
        // never of comments, titled as the 404 page.
        let missing = page.request.not_found && Paging::of(query).is_paged();
        let comments = comments.filter(|_| !missing && !feed_404);
        let missing = missing || feed_404 && asks_404(query);
        let page = std::future::ready(Ok(page));
        return feed::outcome(site, page, comments, format, redirect, missing).await;
    }
    match single {
        Some((name, id, post_type)) => {
            let post = site.db.post(id).await?;
            let hidden = post.as_ref().is_some_and(|p| hides(p, format.is_some()));
            let link = match &post {
                Some(post) => site.permalink(post).await,
                None => String::new(),
            };
            // `?p=` for a page or `?page_id=` for a post.
            let elsewhere = |post: &Post| {
                (post.post_type != post_type).then(|| post_location(&link, post, raw_query))
            };
            Ok(
                match (format, post.filter(|p| viewable(p, &["post", "page"]))) {
                    // A feed's redirect keeps the feed's headers.
                    (Some(format), found) => {
                        let past = found
                            .as_ref()
                            .filter(|p| past_pages(asked, p))
                            .map(|_| past_pages_location(query, &link, raw_query, name));
                        // A `page` past the last that is no number.
                        let garbled = matches!(past, Some(None));
                        let by_id = name == "p" || name == "page_id";
                        let redirect = found
                            .as_ref()
                            .and_then(elsewhere)
                            .or(past.flatten())
                            // Asked for by ID, a garbled `page` is a 404
                            // feed; by name, it goes to the feed by ID.
                            .or_else(|| {
                                found.as_ref().filter(|_| by_id != garbled).map(|post| {
                                    single_feed_location(site, raw_query, post, garbled)
                                })
                            })
                            .filter(|location| *location != current)
                            .or(redirect.filter(|_| !hidden));
                        // The 404 feed lists the post, never its comments.
                        let feed_404 = garbled && by_id;
                        let comments = (comment_feed && !feed_404)
                            .then_some(feed::CommentsOf::Post(id, post_type == "post"));
                        let page = async {
                            Ok(found.map_or_else(
                                || Page {
                                    hidden,
                                    ..missing_single(site)
                                },
                                |p| {
                                    let mut page = single_page(site, p, Paging::default());
                                    if feed_404 {
                                        page.title = not_found(site, false).title;
                                    }
                                    page
                                },
                            ))
                        };
                        return feed::outcome(site, page, comments, format, redirect, feed_404)
                            .await;
                    }
                    (None, Some(post)) => match elsewhere(&post) {
                        Some(location) => Outcome::Redirect(location),
                        None => paged_single(site, asked, query, raw_query, &link, name, post),
                    },
                    (None, None) => Outcome::Page(Box::new(Page {
                        hidden,
                        ..not_found(site, false)
                    })),
                },
            )
        }
        None => {
            let main = main_query(site, query, false).await?;
            let ids = posts_page.map(|(_, id)| id);
            let (page, found) = list(site, query, false, main, ids).await?;
            let redirect =
                posts_page_redirect(site, posts_page, &page, found.as_ref(), query, raw_query);
            if let Some(location) = redirect.await.filter(|location| *location != current) {
                return Ok(Outcome::Redirect(location));
            }
            let page = posts_page_titled(site, page, posts_page, found.as_ref(), Paging::of(query));
            // redirect_canonical() sends an invalid date on only from a 404.
            Ok(
                match date_redirect(site, raw_query, query).filter(|_| page.request.not_found) {
                    Some(location) => Outcome::Redirect(location),
                    None => Outcome::Page(Box::new(page)),
                },
            )
        }
    }
}

/// `redirect_canonical()` for the page for posts, `found` when it exists,
/// asked for by `page_id` or `pagename`. With posts listed, a `page` is a
/// 404 (it has no `<!--nextpage-->` pages) sent to the page found by
/// `pagename`, else to the first post. Another 404 by `page_id` goes to the
/// page; `page_id` stays when the page's link names it.
async fn posts_page_redirect(
    site: &Arc<Site>,
    asked: Option<(&str, u64)>,
    page: &Page,
    found: Option<&Post>,
    query: &HashMap<String, String>,
    raw_query: Option<&str>,
) -> Option<String> {
    let (by, _) = asked?;
    let paged = !page.request.posts.is_empty()
        && query
            .get("page")
            .is_some_and(|v| !is_empty_string(v.as_bytes()));
    let post = match by {
        "page_id" if paged => page.request.posts.first(),
        _ if paged || by == "page_id" && page.request.not_found => found,
        _ => None,
    }
    // Only to a post a visitor is shown.
    .filter(|post| viewable(post, &["post", "page"]))?;
    let link = site.permalink(post).await;
    let drop: Vec<&str> = [
        ("page", paged),
        ("page_id", by == "page_id" && site.permalinks().pretty()),
    ]
    .into_iter()
    .filter_map(|(arg, dropped)| dropped.then_some(arg))
    .collect();
    Some(tack(&link, raw_query, &drop))
}

/// `page` as the page for posts, `found` when it exists, lists the posts:
/// that page queried and its title first in the document's. A 404 keeps
/// its title, and the page only as `pagename` (`by`) found it.
fn posts_page_titled(
    site: &Site,
    mut page: Page,
    asked: Option<(&str, u64)>,
    found: Option<&Post>,
    paging: Paging,
) -> Page {
    let Some((by, id)) = asked else {
        return page;
    };
    if page.request.not_found {
        if by == "pagename" {
            page.request.queried_object_id = id;
        }
        return page;
    }
    if let Some(post) = found {
        page.request.queried_object_id = id;
        page.title = document_title([
            Some(single_post_title(post)),
            paging.title(),
            Some(site.name()),
        ]);
    }
    page
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

/// `single_post_title()`: the title through its filters.
fn single_post_title(post: &Post) -> String {
    lossy(&strip_tags(&wptexturize(&post.title, &CORE_SHORTCODE_TAGS)))
}

/// `wp_get_document_title()`: the parts that are not empty, joined by
/// dashes, through the `document_title` filters.
fn document_title(parts: [Option<String>; 3]) -> String {
    let title = parts
        .into_iter()
        .flatten()
        .filter(|part| !is_empty_string(part.as_bytes()))
        .collect::<Vec<_>>()
        .join(" - ");
    let title = wptexturize(title.as_bytes(), &CORE_SHORTCODE_TAGS);
    let title = esc_html(&convert_chars(&title));
    lossy(&capital_p_dangit(title.as_bytes()))
}

/// A singular page with the `page` var: the page if `(int)` of it is within
/// the post's pages, else a redirect dropping it, or a 404 for no number.
fn paged_single(
    site: &Site,
    asked: &HashMap<String, String>,
    query: &HashMap<String, String>,
    raw_query: Option<&str>,
    link: &str,
    name: &str,
    post: Post,
) -> Outcome {
    if !past_pages(asked, &post) {
        Outcome::Page(Box::new(single_page(site, post, Paging::of(query))))
    } else if let Some(location) = past_pages_location(query, link, raw_query, name) {
        Outcome::Redirect(location)
    } else {
        Outcome::Page(Box::new(not_found(site, false)))
    }
}

/// `handle_404()` for a singular post: a `page` asked for that is not one
/// of its `<!--nextpage-->` pages.
fn past_pages(asked: &HashMap<String, String>, post: &Post) -> bool {
    let Some(page) = asked.get("page").filter(|v| !v.is_empty() && *v != "0") else {
        return false;
    };
    let breaks = post
        .content
        .windows(15)
        .filter(|w| *w == b"<!--nextpage-->")
        .count();
    let number = hyscribe_phpser::Value::String(page.trim_matches('/').into()).to_int();
    !(breaks > 0 && number <= i64::try_from(breaks).unwrap_or(i64::MAX) + 1)
}

/// Where `redirect_canonical()` sends a post's 404 for a page past its
/// last, a number: the post's `link` without `page`. Asked for by `p` or
/// `page_id` (`name`), the singular vars `link` has no value for go too
/// (`_remove_qs_args_if_not_in_url()`).
fn past_pages_location(
    query: &HashMap<String, String>,
    link: &str,
    raw_query: Option<&str>,
    name: &str,
) -> Option<String> {
    if Paging::of(query).page == 0 {
        return None;
    }
    let in_link = parse_str(link.split_once('?').map_or("", |(_, q)| q).as_bytes());
    let absent = SINGULAR_ARGS
        .into_iter()
        .filter(|arg| matches!(name, "p" | "page_id") && in_link.get_str(arg).is_none());
    let drop: Vec<&str> = ["page"].into_iter().chain(absent).collect();
    Some(tack(link, raw_query, &drop))
}

/// A post or page on its own, with its type's template hierarchy.
fn single_page(site: &Site, post: Post, paging: Paging) -> Page {
    let name = lossy(&post.name);
    // `is_front_page()`: the page on the front, which `front-page` shows.
    let front = post.post_type == "page" && site.front_page() == Some(post.id);
    let mut candidates = if post.post_type == "page" {
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
    if front {
        candidates.insert(0, "front-page".into());
    }
    let request = Request {
        singular: true,
        front_page: front,
        queried_object_id: post.id,
        post: Some(post.id),
        paged: paging.paged,
        // A singular query has no limit: it found its one post.
        found_posts: 1,
        page: paging.page,
        now: now(),
        ..Request::default()
    };
    Page {
        status: StatusCode::OK,
        title: if front {
            let tagline = site.option("blogdescription");
            let tagline = Some(esc_html(tagline.as_bytes())).filter(|t| !t.is_empty());
            document_title([Some(site.name()), paging.title(), tagline])
        } else {
            document_title([
                Some(single_post_title(&post)),
                paging.title(),
                Some(site.name()),
            ])
        },
        candidates,
        request: Request {
            posts: vec![post],
            ..request
        },
        hidden: false,
    }
}

/// `WP_Query::parse_query()` with a page on the front: a query of nothing
/// but `preview`, `page`, `paged` and `cpage` (an `embed` and an empty
/// `pagename` aside) asks for that page, with `paged` as its `page`. The
/// request stays `is_paged()`.
fn front_query(site: &Site, query: &HashMap<String, String>) -> Option<HashMap<String, String>> {
    let front = site.front_page().filter(|&id| id != 0)?;
    let paging = ["preview", "page", "paged", "cpage", "embed"];
    if query.iter().any(|(name, value)| {
        rewrite::is_query_var(name)
            && !paging.contains(&name.as_str())
            && !(name == "pagename" && value.is_empty())
    }) {
        return None;
    }
    let mut query = query.clone();
    query.insert("page_id".into(), front.to_string());
    if let Some(paged) = query
        .get("paged")
        .filter(|p| !is_empty_string(p.as_bytes()))
        .cloned()
    {
        query.insert("page".into(), paged);
    }
    Some(query)
}

/// The query of a list page and the term archive its tax query makes.
async fn main_query(
    site: &Site,
    query: &HashMap<String, String>,
    feed: bool,
) -> Result<(PostQuery, Resolved), DbError> {
    let tax = tax_query(query);
    let (slugs, ids) = tax.lookups();
    let terms = if slugs.is_empty() && ids.is_empty() {
        Vec::new()
    } else {
        site.db.find_terms(&slugs, &ids).await?
    };
    let hierarchy = site.options.get("category_children");
    let resolved = tax.resolve(&terms, hierarchy.map_or(&[][..], Vec::as_slice));
    let term_archive = resolved.archive.is_some();
    let post_query = post_query(site, query, feed, resolved.filters.clone(), term_archive);
    Ok((post_query, resolved))
}

/// The search, as parse_search() leaves it: without line breaks.
fn shown_search(query: &HashMap<String, String>) -> Option<String> {
    // `parse_query()` empties a search over 1600 bytes.
    query.get("s").map(|s| {
        if s.len() > 1600 {
            String::new()
        } else {
            s.replace(['\r', '\n'], "")
        }
    })
}

/// The query of a list page with the tax query's `terms`; a feed lists
/// `posts_per_rss` posts.
fn post_query(
    site: &Site,
    query: &HashMap<String, String>,
    feed: bool,
    terms: Vec<TermFilter>,
    term_archive: bool,
) -> PostQuery {
    let search = shown_search(query);
    let flag = |name| {
        query
            .get(name)
            .is_some_and(|v| !is_empty_string(v.as_bytes()))
    };
    let exact = flag("exact");
    let date = date_query(query);
    let per_page = if feed {
        absint(&site.option("posts_per_rss"))
    } else {
        absint(&site.option("posts_per_page"))
    }
    .clamp(1, 100);
    // The home list's first page puts sticky posts first.
    let home = search.is_none() && !feed && !date.archive && !term_archive;
    let paged = Paging::of(query).paged;
    let sticky = if home && paged <= 1 {
        sticky_posts(
            site.options
                .get("sticky_posts")
                .map_or(&[][..], Vec::as_slice),
        )
    } else {
        vec![]
    };
    PostQuery {
        // _post_format_request() sets the post format's post types over any
        // other; `WP::parse_request()` keeps a publicly queryable one, which
        // lists only its published posts; a search covers posts and pages.
        post_types: match queryable_post_type(query) {
            _ if query.contains_key("post_format") => vec!["post".into()],
            Some(post_type) => vec![post_type.into()],
            None if search.is_some() => vec!["post".into(), "page".into()],
            None => vec!["post".into()],
        },
        // `sentence` makes the whole search one term.
        // `parse_search()` runs for a search not `empty()`.
        search: match &search {
            Some(s) if is_empty_string(s.as_bytes()) => Vec::new(),
            Some(s) if flag("sentence") => vec![s.clone()],
            Some(s) => search_terms(s),
            None => Vec::new(),
        },
        exact,
        sentence: search.clone().unwrap_or_default(),
        search_count: match &search {
            Some(_) if flag("sentence") => 1,
            Some(s) => search_count(s),
            None => 1,
        },
        // WordPress sorts a search by relevance, then date, except in feeds
        // and exact searches.
        order: if search.is_some() && !feed && !exact {
            vec![(OrderBy::Relevance, false), (OrderBy::Date, false)]
        } else {
            vec![]
        },
        terms,
        sticky,
        year: date.year,
        month: date.month,
        day: date.day,
        time: date
            .time
            .into_iter()
            .chain(date.week.map(|week| TimeCondition::Week {
                week,
                start_of_week: Value::String(site.option("start_of_week").into()).to_int(),
            }))
            .collect(),
        // Two dates for one part match nothing.
        include: date.conflicting.then(Vec::new),
        limit: u32::try_from(per_page).unwrap_or(10),
        offset: u32::try_from(offset(paged, per_page)).unwrap_or(u32::MAX),
        ..PostQuery::default()
    }
}

/// Where `redirect_canonical()` sends an invalid date: its month's or
/// year's `m`, with the request's own `m` in its place, then the other
/// arguments.
fn date_redirect(
    site: &Site,
    raw_query: Option<&str>,
    query: &HashMap<String, String>,
) -> Option<String> {
    let link = format!("{}/?{}", site.home, date_query(query).canonical?);
    Some(tack(&link, raw_query, &["year", "monthnum", "day"]))
}

/// `error=404`: the 404 page, which `handle_404()` sends with a 200.
fn error_404(site: &Site, feed: bool) -> Page {
    let mut page = not_found(site, feed);
    page.status = StatusCode::OK;
    page
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

/// A list page: the home page, a search, a term or a date archive; for
/// a feed, its posts, and no 404.
async fn list(
    site: &Site,
    query: &HashMap<String, String>,
    feed: bool,
    (post_query, terms): (PostQuery, Resolved),
    posts_page: Option<u64>,
) -> Result<(Page, Option<Post>), DbError> {
    let term_archive = terms.archive;
    // The page for posts is the blog: a search or the date vars only
    // filter its posts.
    let blog = posts_page.is_some();
    let search = shown_search(query).filter(|_| !blog);
    let search = search.as_ref();
    let date = date_query(query);
    // `error=404`: the 404 page, but `handle_404()` sends no 404 for it.
    if date.invalid {
        return Ok((error_404(site, feed), None));
    }
    let dated = date.archive && !blog;
    let posts_page = async {
        match posts_page {
            Some(id) => site.db.post(id).await,
            None => Ok(None),
        }
    };
    let (posts, found, posts_page) = tokio::join!(
        site.db.posts(&post_query),
        site.db.count_posts(&post_query),
        posts_page
    );
    let (posts, found, posts_page) = posts
        .and_then(|posts| found.and_then(|found| posts_page.map(|page| (posts, found, page))))?;
    let paging = Paging::of(query);
    let term = match &term_archive {
        Some(Archive::Term(_, term)) => term.as_ref(),
        _ => None,
    };
    // An archive with nothing in it is a 404 unless it is of a term that
    // exists, a search only when paged.
    let is_archive = dated || term_archive.is_some();
    let empty_404 = paging.is_paged() || is_archive && search.is_none() && term.is_none();
    if empty_404 && posts.is_empty() {
        let mut page = not_found(site, feed && !paging.is_paged());
        page.request.search_query = search.map_or_else(Vec::new, |s| s.as_bytes().to_vec());
        return Ok((page, posts_page));
    }
    let first_date = posts.first().map_or("", |post| post.date.as_str());
    let heading = if let Some(s) = search {
        Some(format!(
            "Search Results for &#8220;{}&#8221;",
            esc_attr(s.as_bytes())
        ))
    } else if term_archive.is_some() {
        Some(term.map_or_else(String::new, |term| esc_html(&term.name)))
    } else {
        // A week's or a time's archive has no title of its own.
        dated.then(|| {
            date.kind.as_ref().map_or_else(String::new, |kind| {
                let format = match kind {
                    Archive::Day => site.option("date_format"),
                    Archive::Month => "F Y".to_string(),
                    _ => "Y".to_string(),
                };
                date::format(&format, first_date, site.zone())
            })
        })
    };
    let tagline = site.option("blogdescription");
    let front = site.posts_on_front();
    let title = match heading {
        // A term or date archive without a title has the site's name alone.
        Some(heading) => document_title([
            Some(heading).filter(|h| !h.is_empty()),
            paging.title(),
            Some(site.name()),
        ]),
        // A feed is never `is_home()`, and posts away from the front have
        // no title of their own but the posts page's.
        None if feed || !front => document_title([None, paging.title(), Some(site.name())]),
        None => document_title([
            Some(site.name()),
            paging.title(),
            Some(esc_html(tagline.as_bytes())).filter(|t| !t.is_empty()),
        ]),
    };
    let mut candidates = list_templates(
        &terms.kinds,
        term_archive.as_ref(),
        search.is_some(),
        dated,
        query.contains_key("post_format"),
    );
    candidates.retain(|name| front || name != "front-page");
    let queried_object_id = term.map_or(0, |term| term.id);
    let archive = term_archive.or(dated.then(|| date.kind.clone().unwrap_or(Archive::Date)));
    // The home page lists the latest posts, the default install's setting;
    // a feed is never `is_home()`.
    let home = archive.is_none() && search.is_none() && !feed;
    let request = Request {
        front_page: home && front,
        home,
        post: posts.first().map(|post| post.id),
        queried_object_id,
        archive,
        date: dated,
        search: search.map(|s| s.as_bytes().to_vec()),
        search_query: search.map_or_else(Vec::new, |s| s.as_bytes().to_vec()),
        paged: paging.paged,
        page: paging.page,
        max_pages: found.div_ceil(u64::from(post_query.limit)),
        found_posts: found,
        now: now(),
        posts,
        ..Request::default()
    };
    let page = Page {
        status: StatusCode::OK,
        title,
        candidates,
        request,
        hidden: false,
    };
    Ok((page, posts_page))
}

/// The templates the template loader tries for a list page, in its order:
/// a search's; those of the taxonomies in `kinds`, each from the queried
/// term of `term_archive`; a date archive's; then the archive's, of posts
/// first where a post format sets the `post` post type, or the home page's.
fn list_templates(
    kinds: &[Taxonomy],
    term_archive: Option<&Archive>,
    search: bool,
    date: bool,
    post_format: bool,
) -> Vec<String> {
    let mut templates: Vec<String> = Vec::new();
    if search {
        templates.push("search".into());
    } else {
        let queried = term_archive.and_then(Archive::term);
        for kind in kinds {
            // get_taxonomy_template() names the queried term's taxonomy.
            let prefix = match (kind, queried) {
                (Taxonomy::Category, _) => "category".to_string(),
                (Taxonomy::Tag, _) => "tag".to_string(),
                (Taxonomy::Format, Some((taxonomy, _))) => format!("taxonomy-{}", taxonomy.name()),
                (Taxonomy::Format, None) => String::new(),
            };
            if let Some((_, term)) = queried {
                templates.push(format!("{prefix}-{}", lossy(&term.slug)));
                templates.push(format!("{prefix}-{}", term.id));
                if *kind == Taxonomy::Format {
                    templates.push(prefix.clone());
                }
            }
            if *kind == Taxonomy::Format {
                templates.push("taxonomy".into());
            } else {
                templates.push(prefix);
            }
        }
        if date {
            templates.push("date".into());
        }
        if term_archive.is_some() || date {
            if post_format {
                templates.push("archive-post".into());
            }
            templates.push("archive".into());
        } else {
            templates.extend(["front-page".into(), "home".into()]);
        }
    }
    templates.push("index".into());
    templates
}

/// `redirect_canonical()` for `feed=rss`: the same request with `feed=rss2`.
fn rss2_location(site: &Site, path: &str, raw_query: Option<&str>) -> String {
    format!("{}{path}?{}", site.home, rss2_query(raw_query))
}

/// `raw_query` with `feed=rss` as `feed=rss2`.
fn rss2_query(raw_query: Option<&str>) -> String {
    raw_query
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
        .join("&")
}

/// Whether a visitor sees `post`: published, and of one of `post_types`.
fn viewable(post: &Post, post_types: &[&str]) -> bool {
    post.status == "publish" && post_types.contains(&post.post_type.as_str())
}

/// Whether `redirect_canonical()` cancels its redirects for `post`, one
/// visitors cannot see: always for a feed's `p`, and for a 404's ID unless
/// its type is not public or it is an auto-draft.
fn hides(post: &Post, feed: bool) -> bool {
    !viewable(post, &["post", "page"])
        && (feed
            || matches!(post.post_type.as_str(), "post" | "page") && post.status != "auto-draft")
}

/// `error=404` on a feed of the post asked for by ID: a published post or
/// page goes to its link, the other arguments kept and `feed=rss` as
/// `feed=rss2`; at that link, or with no post, the 404 posts feed, which
/// lists the post if it is a post.
async fn error_404_feed(
    site: &Arc<Site>,
    found: Option<Post>,
    raw_query: Option<&str>,
    current: &str,
    format: feed::Format,
) -> Result<Outcome, DbError> {
    let found = found.filter(|post| viewable(post, &["post", "page"]));
    if let Some(post) = &found {
        let query = rss2_query(raw_query);
        let location = post_location(&site.permalink(post).await, post, Some(&query));
        if location != current {
            return Ok(Outcome::Redirect(location));
        }
    }
    let mut page = match found.filter(|post| post.post_type == "post") {
        Some(post) => single_page(site, post, Paging::default()),
        None => missing_single(site),
    };
    page.title = not_found(site, false).title;
    feed::outcome(site, std::future::ready(Ok(page)), None, format, None, true).await
}

/// A redirect to the published post or page with `id`'s link, else `missing`.
async fn redirect_to(
    site: &Arc<Site>,
    id: u64,
    raw_query: Option<&str>,
    missing: Page,
) -> Result<Outcome, DbError> {
    let post = site.db.post(id).await?;
    Ok(match post {
        Some(post) if viewable(&post, &["post", "page"]) => {
            let link = site.permalink(&post).await;
            Outcome::Redirect(post_location(&link, &post, raw_query))
        }
        post => Outcome::Page(Box::new(Page {
            hidden: post.is_some_and(|p| hides(&p, false)),
            ..missing
        })),
    })
}

/// The `post_type` `WP::parse_request()` keeps: a publicly queryable one.
fn queryable_post_type(query: &HashMap<String, String>) -> Option<&str> {
    query
        .get("post_type")
        .map(String::as_str)
        .filter(|t| matches!(*t, "post" | "attachment"))
}

/// What `name` or `pagename` asks for.
enum Slug {
    /// The post found, as `(query var, ID, post type)`, ID 0 when none is.
    Post((&'static str, u64, &'static str)),
    /// Where `redirect_guess_404_permalink()` sends a miss.
    Moved(String),
}

/// The post a `name` (a post) or a `pagename` (a page, by its path of
/// names) asks for, as `WP_Query` finds it, or for a miss the guess of
/// `redirect_guess_404_permalink()`: the first published post or page
/// whose name starts with the one asked for.
async fn by_slug(
    site: &Arc<Site>,
    raw_query: Option<&str>,
    query: &HashMap<String, String>,
) -> Result<Option<Slug>, DbError> {
    let var = |name| query.get(name).map(|v| v.trim()).filter(|v| !v.is_empty());
    let queryable = queryable_post_type(query);
    let (key, post_type, found, name) = if let Some(name) = var("name") {
        let name = sanitize_title_for_query(name.as_bytes());
        // Attachment pages are not served yet: none is found.
        let post = async {
            match queryable {
                Some("attachment") => Ok(None),
                _ => {
                    let name = String::from_utf8_lossy(&name);
                    narrowed(site, query, site.db.post_by_name(&name).await?).await
                }
            }
        };
        ("name", "post", post.await?, name)
    } else if let Some(path) = var("pagename") {
        let parts = page_path(path);
        let post = async {
            // A queryable post type is not hierarchical: no page is found.
            let pages = match queryable {
                Some(_) => Vec::new(),
                None => site.db.named_pages(&parts).await?,
            };
            // The page for posts lists the posts whatever its status.
            let post = page_by_path(&parts, &pages)
                .filter(|p| viewable(p, &["page"]) || site.posts_page() == Some(p.id))
                .cloned();
            narrowed(site, query, post).await
        };
        let post = post.await?;
        let name = sanitize_title_for_query(&hyscribe_phpser::wp_basename(path.as_bytes()));
        ("pagename", "page", post, name)
    } else if let Some(attachment) = ["subpost", "attachment"]
        .iter()
        .find_map(|k| query.get(*k).filter(|v| !v.is_empty()))
    {
        // get_posts() names the query after the attachment; none is served.
        let name = sanitize_title_for_query(&hyscribe_phpser::wp_basename(attachment.as_bytes()));
        ("attachment", "attachment", None, name)
    } else {
        return Ok(None);
    };
    // A feed is a 404 only by `error=404`, which guesses even for a post
    // found, and goes to the guess's feed.
    let feed = query.get("feed").is_some_and(|f| !f.is_empty());
    let feed_404 = feed && asks_404(query);
    if let Some(post) = found.filter(|_| !feed_404) {
        return Ok(Some(Slug::Post((key, post.id, post_type))));
    }
    let post_types = queryable.map_or(vec!["post", "page", "attachment"], |t| vec![t]);
    let date = |name| query.get(name).map(|v| absint(v)).filter(|&v| v > 0);
    // A guess past page 1 would only redirect again where a plain link
    // cannot name the page.
    let links = site.permalinks();
    let paged = date("page").filter(|&page| page > 1);
    let guess = if feed && !feed_404 || name.is_empty() || paged.is_some() && !links.pretty() {
        Ok(None)
    } else {
        let name = String::from_utf8_lossy(&name);
        let parts = (date("year"), date("monthnum"), date("day"));
        site.db.guess_post(&name, &post_types, parts).await
    };
    let link = match (&guess, paged) {
        // `redirect_guess_404_permalink()` keeps the page asked for.
        (Ok(Some(post)), Some(page)) => links.under(&site.permalink(post).await, &page.to_string()),
        (Ok(Some(post)), None) => site.permalink(post).await,
        _ => String::new(),
    };
    guess.map(|guess| {
        Some(match guess {
            Some(post) if feed_404 => {
                Slug::Moved(single_feed_location(site, raw_query, &post, true))
            }
            Some(post) => {
                // `page` and `feed` go first, as `redirect_canonical()` drops them.
                let mut drop = vec!["page", "feed"];
                drop.extend(singular_args_but(id_var(&post)));
                Slug::Moved(tack(&link, raw_query, &drop))
            }
            None => Slug::Post((key, 0, post_type)),
        })
    })
}

/// `post` if it is one the query's date vars and search also match, as
/// `WP_Query` adds them to a slug's query; its taxonomy vars are ignored.
async fn narrowed(
    site: &Site,
    query: &HashMap<String, String>,
    post: Option<Post>,
) -> Result<Option<Post>, DbError> {
    let date = date_query(query);
    // An invalid date still adds its conditions, which nothing meets.
    let dated = date.archive || date.invalid;
    match post {
        Some(post) if post.status == "publish" && (query.contains_key("s") || dated) => {
            let all = post_query(site, query, false, Vec::new(), false);
            let only = PostQuery {
                post_types: vec![post.post_type.clone()],
                // Conflicting dates already include nothing.
                include: all.include.or(Some(vec![post.id])),
                ..all
            };
            let count = site.db.count_posts(&only).await;
            count.map(|count| (count > 0).then_some(post))
        }
        post => Ok(post),
    }
}

/// The verbose page rules' check: `get_page_by_path()` finds a page or
/// attachment at `path` in a status other than the hidden internal ones.
async fn page_exists(site: &Site, path: &[u8]) -> Result<bool, DbError> {
    let parts = page_path(&String::from_utf8_lossy(path));
    let mut names = parts.clone();
    names.sort();
    names.dedup();
    let pages = site.db.named_pages(&names).await?;
    Ok(page_by_path(&parts, &pages).is_some_and(|page| matchable_status(&page.status)))
}

/// A status other than the internal ones hidden from searches, which
/// verbose page rules pass over.
fn matchable_status(status: &str) -> bool {
    !matches!(
        status,
        "trash"
            | "auto-draft"
            | "request-pending"
            | "request-confirmed"
            | "request-failed"
            | "request-completed"
    )
}

/// `get_page_by_path()`'s parts of `path`: its names between slashes, each
/// through `sanitize_title_for_query()`.
fn page_path(path: &str) -> Vec<Vec<u8>> {
    let path = rawurlencode(&urldecode(path.as_bytes()));
    let path = String::from_utf8_lossy(&path)
        .replace("%2F", "/")
        .replace("%20", " ");
    path.trim_matches('/')
        .split('/')
        .map(|part| sanitize_title_for_query(part.as_bytes()))
        .collect()
}

/// `get_page_by_path()`'s choice among `pages`: one named the last of
/// `parts` whose parents are named the parts before it, up to a top level
/// one, a page before an attachment.
fn page_by_path<'a>(parts: &[Vec<u8>], pages: &'a [Post]) -> Option<&'a Post> {
    let by_id = |id: u64| pages.iter().find(|page| page.id == id);
    let mut found = None;
    for page in pages.iter().filter(|page| Some(&page.name) == parts.last()) {
        let mut count = 0;
        let mut at = page;
        while let Some(parent) = (at.parent != 0).then(|| by_id(at.parent)).flatten() {
            count += 1;
            match parts
                .len()
                .checked_sub(count + 1)
                .and_then(|i| parts.get(i))
            {
                Some(name) if *name == parent.name => at = parent,
                _ => break,
            }
        }
        // Every name on the way up matched, the last name included.
        if at.parent == 0 && parts.len() == count + 1 {
            found = Some(page);
            if page.post_type == "page" {
                break;
            }
        }
    }
    found
}

/// The arguments `redirect_canonical()` drops when it sends a request for
/// one post to that post's link.
const SINGULAR_ARGS: [&str; 6] = [
    "p",
    "page_id",
    "attachment_id",
    "pagename",
    "name",
    "post_type",
];

/// The comments feed link a post's feed redirects to, as
/// `redirect_canonical()` builds it: `?feed=` and the var that asks for the
/// post by ID first, then the request's other arguments, encoded again,
/// `page` among them unless it is dropped.
fn single_feed_location(
    site: &Site,
    raw_query: Option<&str>,
    post: &Post,
    drop_page: bool,
) -> String {
    let name = id_var(post);
    // The request's own `feed` takes the place of the empty one.
    let link = format!("{}/?feed=&{name}={}", site.home, post.id);
    let mut drop = singular_args_but(name);
    drop.extend(drop_page.then_some("page"));
    tack(&link, raw_query, &drop)
}

/// A post's link, where `redirect_canonical()` sends a request for it.
fn post_location(link: &str, post: &Post, raw_query: Option<&str>) -> String {
    tack(link, raw_query, &singular_args_but(id_var(post)))
}

/// The query var that asks for `post` by ID.
fn id_var(post: &Post) -> &'static str {
    if post.post_type == "page" {
        "page_id"
    } else {
        "p"
    }
}

/// [`SINGULAR_ARGS`] but `name`, the one a post's link has.
fn singular_args_but(name: &str) -> Vec<&'static str> {
    SINGULAR_ARGS
        .into_iter()
        .filter(|arg| *arg != name)
        .collect()
}

/// `redirect_canonical()`'s tacking of the request's arguments onto `link`:
/// `drop` and `preview` removed, the rest parsed and encoded again, those
/// `link` has taking its values in place, the others after it.
fn tack(link: &str, raw_query: Option<&str>, drop: &[&str]) -> String {
    // A `name` goes unless empty or in the link, where a plain permalink
    // never has one.
    let named = parse_str(raw_query.unwrap_or_default().as_bytes())
        .get_str("name")
        .is_some_and(|name| !name.is_empty());
    let drop: Vec<&[u8]> = drop
        .iter()
        .chain(&["preview"])
        .chain(named.then_some(&"name"))
        .map(|arg| arg.as_bytes())
        .collect();
    let query = remove_query_arg(&drop, rss2_query(raw_query).as_bytes());
    let args: Vec<(Key, Value)> = rawurlencode_deep(parse_str(&query))
        .as_array()
        .unwrap_or_default()
        .iter()
        .map(|(key, value)| {
            (
                Key::from_php_string(rawurlencode(&key.to_php_string())),
                value.clone(),
            )
        })
        .collect();
    String::from_utf8_lossy(&add_query_values(&args, link.as_bytes())).into_owned()
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
        hidden: false,
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

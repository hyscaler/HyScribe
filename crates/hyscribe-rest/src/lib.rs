// SPDX-License-Identifier: AGPL-3.0-or-later

//! The REST API (ADR 0013): `WP_REST_Server::serve_request()` and the
//! routes ported so far, pure and synchronous. The site and the current
//! user are read through [`Site`]; `hyscribe-http` reads the request,
//! checks the logged in cookie and sends the answer. A request that
//! reaches a route, method or keyword not ported is answered `501`. A
//! ported write is answered in two steps: [`answer`] checks it and gives
//! the [`Plan`] to write, and [`written`] answers once it is written.

mod batch;
mod block_renderer;
mod comments;
mod editor;
mod embed;
mod fields;
mod hooks;
mod index;
mod media;
mod navigation;
mod oembed;
pub(crate) mod posts;
mod request;
mod response;
mod revisions;
mod routes;
mod schema;
mod search;
mod settings;
mod statuses;
mod taxonomies;
mod templates;
mod terms;
mod types;
mod url_details;
mod users;
mod widgets;

/// What the fuzz targets reach.
pub mod fuzzing {
    pub use crate::url_details::details;
}

use std::sync::OnceLock;

use hyscribe_fetch::{Args, Error as FetchError, Fetched};
use hyscribe_kses::esc_url_raw;
use hyscribe_model::{Comment, CommentQuery, CommentRow, Post, Term, TermQuery, User, UserQuery};
use hyscribe_phpser::{
    JsonError, Key, Value, is_empty_string, json_decode, json_encode, json_encode_pretty, lookup,
};
use hyscribe_render::site::Site as Reads;
use hyscribe_render::theme::Theme;

use request::{Arg, Params, Sanitize, defaults, has_valid_params, sanitize_params};
use response::{Response, RestError, object, text};

pub use comments::write::CommentSave;
pub use posts::write::{Outcome, Plan, Save, Upload, Write};
pub use settings::{Setting, SettingsSave};
pub use terms::write::TermSave;
pub use users::write::{Preferences, UserSave};

/// What the REST API reads from the site.
pub trait Site {
    /// `get_option( $name )`.
    fn option(&self, name: &str) -> Option<Vec<u8>>;
    /// `rest_url( $path )`, `path` starting with `/`.
    fn rest_url(&self, path: &str) -> String;
    /// `wp_verify_nonce( $nonce, 'wp_rest' )` for the logged in cookie's
    /// user, or a visitor without one.
    fn verify_nonce(&self, nonce: &[u8]) -> bool;
    /// `wp_create_nonce( 'wp_rest' )` for the same user.
    fn create_nonce(&self) -> String;
    /// `wp_create_nonce( $action )` for the same user.
    fn nonce(&self, action: &str) -> String;
    /// `user_can( $user, $cap, ...$args )`.
    fn can(&self, user: u64, cap: &str, args: &[Value]) -> bool;
    /// The terms `WP_Term_Query` selects with `query`, in its order.
    fn terms(&self, query: &TermQuery) -> Vec<Term>;
    /// How many terms `query` selects, without its limit and offset.
    fn count_terms(&self, query: &TermQuery) -> u64;
    /// `get_term_link( $term, $taxonomy )`.
    fn term_link(&self, taxonomy: &str, term: &Term) -> String;
    /// `get_post( $id )`.
    fn post(&self, id: u64) -> Option<Post> {
        self.reads().post(id)
    }
    /// The table prefix, which some meta keys and options begin with.
    fn table_prefix(&self) -> String;
    /// The comments `WP_Comment_Query` selects with `query`, in its order.
    fn comments(&self, query: &CommentQuery) -> Vec<CommentRow>;
    /// How many comments `query` selects, without its limit and offset.
    fn count_comments(&self, query: &CommentQuery) -> u64;
    /// `get_comment_link( $comment )`.
    fn comment_link(&self, comment: &Comment) -> String;
    /// The users `WP_User_Query` selects with `query`, in its order.
    fn users(&self, query: &UserQuery) -> Vec<User>;
    /// How many users `query` selects, without its limit and offset.
    fn count_users(&self, query: &UserQuery) -> u64;
    /// `get_userdata( $id )`.
    fn user(&self, id: u64) -> Option<User> {
        self.reads().user(id)
    }
    /// `get_user_meta( $id, $key, true )`, if stored.
    fn user_meta(&self, id: u64, key: &str) -> Option<Vec<u8>>;
    /// The user's `user_registered`.
    fn user_registered(&self, id: u64) -> String;
    /// The user's `allcaps`: its roles' capabilities and its own.
    fn allcaps(&self, id: u64) -> Vec<(Key, Value)>;
    /// `get_author_posts_url( $id, $nicename )`.
    fn author_link(&self, user: &User) -> String;
    /// Whether `author` has posts of `types`, published, or private too
    /// where a type's flag is set.
    fn has_posts(&self, author: u64, types: &[(String, bool)]) -> bool;
    /// Every post of `post_type` whatever its status, newest first.
    fn posts_of_type(&self, post_type: &str) -> Vec<Post>;
    /// `get_the_terms( $post, $taxonomy )`, by name.
    fn post_terms(&self, post: u64, taxonomy: &str) -> Vec<Term> {
        self.reads().post_terms(post, taxonomy)
    }
    /// `get_post_meta( $post, $key, true )`.
    fn post_meta(&self, post: u64, key: &str) -> Option<Vec<u8>> {
        self.reads().post_meta(post, key)
    }
    /// The IDs of the revisions of `post`, newest first.
    fn revisions(&self, post: u64) -> Vec<u64>;
    /// Whether the public users endpoint answers visitors as WordPress
    /// does; off, a visitor sees no user as having published posts.
    fn public_users(&self) -> bool;
    /// Whether `EMPTY_TRASH_DAYS` is not 0, WordPress's default of 30:
    /// without the Trash, a delete must be forced.
    fn uses_trash(&self) -> bool {
        true
    }
    /// Whether `MEDIA_TRASH` is true as well as `EMPTY_TRASH_DAYS` not 0:
    /// only then do media go to the trash before they are deleted.
    fn uses_media_trash(&self) -> bool {
        false
    }
    /// The site's content as `hyscribe-render` reads it, for the post
    /// routes.
    fn reads(&self) -> &dyn Reads;
    /// The active theme, which renders posts' content.
    fn theme(&self) -> &Theme;
    /// Whether outbound fetches are on (`HTTP_OUTBOUND`, ADR 0019).
    fn outbound(&self) -> bool {
        false
    }
    /// `wp_safe_remote_get( $url, $args )` through the outbound client,
    /// `None` while fetches are off.
    fn fetch(&self, _url: &[u8], _args: Args) -> Option<Result<Fetched, FetchError>> {
        None
    }
    /// `set_site_transient( $name, $value, $expiration )`, where the site
    /// writes.
    fn set_site_transient(&self, _name: &str, _value: &[u8], _expiration: i64) {}
    /// What `get_site_transient( $name )` writes for a transient past its
    /// timeout, where the site writes: both its options deleted.
    fn expire_site_transient(&self, _name: &str) {}
    /// `wp_getimagesize()` of the uploads file `file` (a path within the
    /// uploads folder): `Some(None)` for what is missing or not an image,
    /// `None` where the site does not read its uploads.
    fn image_size(&self, _file: &str) -> Option<Option<(u32, u32)>> {
        None
    }
    /// `wp_hash( $data )`, `None` without the salts it signs with.
    fn wp_hash(&self, data: &[u8]) -> Option<String>;
    /// `wp_generate_password( 10, false )`, an oEmbed answer's secret.
    fn embed_secret(&self) -> String;
}

/// A request for the API.
#[derive(Clone, Copy, Debug)]
pub struct Request<'a> {
    /// `$_SERVER['REQUEST_METHOD']`.
    pub method: &'a str,
    /// The `rest_route` query var.
    pub route: &'a [u8],
    /// `$_SERVER['REQUEST_URI']`, which rendered content links to.
    pub uri: &'a [u8],
    /// `$_GET`.
    pub get: &'a [(Key, Value)],
    /// `$_POST`.
    pub post: &'a [(Key, Value)],
    /// The user of a valid logged in cookie, 0 for none.
    pub cookie_user: u64,
    /// The `Origin` header.
    pub origin: Option<&'a [u8]>,
    /// The `X-WP-Nonce` header.
    pub nonce: Option<&'a [u8]>,
    /// `$_SERVER['REMOTE_ADDR']`, empty for none.
    pub remote_addr: &'a [u8],
    /// A `wp-postpass_` cookie was sent.
    pub postpass: bool,
    /// The `User-Agent` header.
    pub user_agent: Option<&'a [u8]>,
    /// The `X-HTTP-Method-Override` header.
    pub method_override: Option<&'a [u8]>,
    /// The `Content-Type` header.
    pub content_type: Option<&'a [u8]>,
    /// The body.
    pub body: &'a [u8],
    /// The `Host` header, `$_SERVER['HTTP_HOST']`.
    pub host: Option<&'a [u8]>,
    /// The request's time, `time()`.
    pub now: i64,
}

/// What a request comes to: its answer, or a write to make first.
pub enum Answer {
    /// The answer.
    Served(Served),
    /// The write the request asks for, its checks passed.
    Write(Box<Pending>),
}

impl Answer {
    /// The answer, if nothing is to be written first.
    pub fn served(self) -> Option<Served> {
        match self {
            Answer::Served(served) => Some(served),
            Answer::Write(_) => None,
        }
    }
}

/// A write whose checks passed, and what its answer is made from once
/// it is written.
pub struct Pending {
    /// What to write.
    pub plan: Plan,
    params: Params,
    finish: posts::write::Finish,
    /// The batch the write is one request of, and the request.
    batch: Option<Box<batch::Suspended>>,
}

/// The answer: its status, headers and body.
#[derive(Clone, Debug, PartialEq)]
pub struct Served {
    /// The status code.
    pub status: u16,
    /// The headers, each name once.
    pub headers: Vec<(String, String)>,
    /// The body.
    pub body: Vec<u8>,
}

impl Served {
    /// `501`, for what is not ported.
    pub fn not_ported() -> Served {
        Served {
            status: 501,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    /// `header()`: a header replacing any of its name.
    fn send(&mut self, name: &str, value: impl Into<String>) {
        self.headers
            .retain(|(known, _)| !known.eq_ignore_ascii_case(name));
        self.headers.push((name.into(), value.into()));
    }
}

/// The request reaches what is not ported.
struct NotPorted;

/// Why dispatching stops before an answer.
enum Halt {
    /// What is not ported, answered 501.
    NotPorted,
    /// A write to make first.
    Write(Box<Pending>),
}

/// An embedded request, a GET, plans no write.
impl From<Halt> for NotPorted {
    fn from(_: Halt) -> NotPorted {
        NotPorted
    }
}

/// A request on its way through a route: the site and the current user.
pub(crate) struct Context<'a> {
    site: &'a dyn Site,
    user: u64,
    /// The request is a HEAD request.
    pub(crate) head: bool,
    /// `$_SERVER['REQUEST_URI']`, which rendering reads.
    pub(crate) uri: &'a [u8],
    /// The request gives parameters a write's arguments would check.
    pub(crate) parameters: bool,
    /// `$_SERVER['REMOTE_ADDR']`, empty for none.
    pub(crate) remote_addr: &'a [u8],
    /// A `wp-postpass_` cookie was sent.
    pub(crate) postpass: bool,
    /// The `User-Agent` header.
    pub(crate) user_agent: Option<&'a [u8]>,
    /// `$_SERVER['HTTP_HOST']`.
    pub(crate) host: &'a [u8],
    /// `time()`.
    pub(crate) now: i64,
}

impl Context<'_> {
    /// `current_user_can( $cap )`.
    pub(crate) fn can(&self, cap: &str) -> bool {
        self.site.can(self.user, cap, &[])
    }

    /// `current_user_can( $cap, $id )`.
    pub(crate) fn can_for(&self, cap: &str, id: u64) -> bool {
        self.site.can(self.user, cap, &[response::int(id)])
    }

    /// An error sent with `rest_authorization_required_code()`.
    pub(crate) fn forbidden(&self, code: &'static str, message: &str) -> RestError {
        let status = if self.user == 0 { 401 } else { 403 };
        RestError::status(code, message, status)
    }

    /// `get_option( $name )`.
    pub(crate) fn option(&self, name: &str) -> Option<Vec<u8>> {
        self.site.option(name)
    }

    /// `rest_url( $path )`.
    pub(crate) fn rest_url(&self, path: &str) -> String {
        self.site.rest_url(path)
    }

    /// `wp_timezone()`.
    /// `get_option( 'gmt_offset' )`: `wp_timezone_override_offset()`'s
    /// offset now for a named zone, else the stored one.
    pub(crate) fn gmt_offset(&self) -> Value {
        let zone = self.option("timezone_string").unwrap_or_default();
        hyscribe_render::date::override_offset(&zone, self.now).map_or_else(
            || {
                let stored = self.option("gmt_offset");
                stored.map_or(Value::Bool(false), |stored| unserialized(&stored))
            },
            |hours| Value::Float(hyscribe_phpser::Float::from_f64(hours)),
        )
    }

    pub(crate) fn zone(&self) -> hyscribe_render::date::Zone {
        let option = |name| self.option(name).unwrap_or_default();
        hyscribe_render::date::site_zone(&option("timezone_string"), &option("gmt_offset"))
    }
}

pub(crate) type Permission = fn(&Context, &Params) -> Result<(), RestError>;
pub(crate) type Callback = fn(&Context, &Params) -> Result<Response, RestError>;

/// A ported endpoint: its arguments, permission check and callback, none
/// for a write, whose permission check alone is ported.
#[derive(Clone)]
pub(crate) struct Endpoint {
    args: Vec<Arg>,
    permission: Permission,
    callback: Option<Callback>,
}

/// The ported endpoints of the route `regex`.
fn endpoints(regex: &str) -> Option<Vec<Endpoint>> {
    types::endpoints(regex)
        .or_else(|| index::endpoints(regex))
        .or_else(|| editor::endpoints(regex))
        .or_else(|| taxonomies::endpoints(regex))
        .or_else(|| terms::endpoints(regex))
        .or_else(|| settings::endpoints(regex))
        .or_else(|| statuses::endpoints(regex))
        .or_else(|| posts::endpoints(regex))
        .or_else(|| comments::endpoints(regex))
        .or_else(|| users::endpoints(regex))
        .or_else(|| templates::endpoints(regex))
        .or_else(|| search::endpoints(regex))
        .or_else(|| navigation::endpoints(regex))
        .or_else(|| media::endpoints(regex))
        .or_else(|| revisions::endpoints(regex))
        .or_else(|| block_renderer::endpoints(regex))
        .or_else(|| url_details::endpoints(regex))
        .or_else(|| oembed::endpoints(regex))
        .or_else(|| widgets::endpoints(regex))
        .or_else(|| batch::endpoints(regex))
}

/// The ported write of the route `regex`'s endpoint `index`, if any.
fn writing(regex: &str, index: usize) -> Option<posts::write::Writing> {
    posts::write::writing(regex, index)
        .or_else(|| terms::write::writing(regex, index))
        .or_else(|| settings::writing(regex, index))
        .or_else(|| comments::write::writing(regex, index))
        .or_else(|| users::write::writing(regex, index))
        .or_else(|| templates::writing(regex, index))
        .or_else(|| revisions::writing(regex, index))
        .or_else(|| editor::global_styles::writing(regex, index))
        .or_else(|| editor::themes::writing(regex, index))
}

/// `maybe_unserialize()` of a stored option or meta value: `false` where
/// it is serialized data that does not unserialize.
pub(crate) fn unserialized(stored: &[u8]) -> Value {
    match hyscribe_phpser::maybe_unserialize(stored) {
        hyscribe_phpser::Maybe::Value(value) => value,
        hyscribe_phpser::Maybe::Plain(text) => Value::String(text.to_vec()),
        hyscribe_phpser::Maybe::Invalid(_) => Value::Bool(false),
    }
}

/// `wp_check_jsonp_callback()`: letters, digits, `_` and `.` only.
fn valid_callback(callback: &Value) -> bool {
    let Value::String(callback) = callback else {
        return false;
    };
    callback
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.'))
}

/// `wp_json_encode()` of `data`, pretty printed if asked for.
fn encode(data: &Value, pretty: bool) -> Vec<u8> {
    let json = if pretty {
        json_encode_pretty(data)
    } else {
        json_encode(data)
    };
    json.unwrap_or_default()
}

/// The parameters `serve_request()` reads itself, and the route's.
const GLOBAL_PARAMETERS: [&str; 8] = [
    "rest_route",
    "_wpnonce",
    "_method",
    "_envelope",
    "_jsonp",
    "_pretty",
    "_fields",
    "_embed",
];

/// `serve_request()`, from `rest_api_loaded()`, for a site without a
/// writer: a write whose checks pass is answered 501.
pub fn serve(site: &dyn Site, request: &Request) -> Served {
    answer(site, request)
        .served()
        .unwrap_or_else(Served::not_ported)
}

/// `serve_request()`, from `rest_api_loaded()`: the answer, or the write a
/// request asks for once its checks pass.
pub fn answer(site: &dyn Site, request: &Request) -> Answer {
    run(site, request, None)
}

/// The answer to `request` once `pending`, which [`answer`] or this gave
/// for it, is written, or a batch's next write: `site` is the site as it
/// is after the write.
pub fn written(site: &dyn Site, request: &Request, pending: Pending, outcome: Outcome) -> Answer {
    run(site, request, Some((pending, outcome)))
}

/// What `rest_preload_api_request()` keeps of one path.
#[derive(Clone, Debug, PartialEq)]
pub enum Preloaded {
    /// A 200 answer: its data and its response's headers.
    Kept(Value, Value),
    /// Another answer, which is left out.
    Left,
    /// What is not ported.
    NotPorted,
}

/// `rest_preload_api_request()`'s request of `path` (its query included)
/// with `method`, as `user` on `host` at `now`: the data
/// `response_to_data()` gives.
pub fn preload(
    site: &dyn Site,
    (user, host, now): (u64, &[u8], i64),
    method: &str,
    path: &[u8],
) -> Preloaded {
    let (route, query) = match path.iter().position(|&b| b == b'?') {
        Some(at) => (
            path.get(..at).unwrap_or_default(),
            path.get(at + 1..).unwrap_or_default(),
        ),
        None => (path, &b""[..]),
    };
    let get = hyscribe_phpser::parse_str(query);
    let get = get.as_array().unwrap_or_default().to_vec();
    let params = Params {
        get: get.clone(),
        ..Params::default()
    };
    let context = Context {
        site,
        user,
        head: false,
        uri: path,
        parameters: false,
        host,
        now,
        remote_addr: b"",
        postpass: false,
        user_agent: None,
    };
    let response = match dispatch(&context, (method, route), (params, None), None) {
        Ok(response) if response.status == 200 => response,
        Ok(_) => return Preloaded::Left,
        Err(_) => return Preloaded::NotPorted,
    };
    let headers = header_values(&response);
    let embed = lookup(&get, "_embed").map(embed::relations);
    let data = embed::to_data(&context, response, embed.as_ref());
    data.map_or(Preloaded::NotPorted, |data| Preloaded::Kept(data, headers))
}

/// A response's headers as its data: the collections set their totals as
/// integers.
fn header_values(response: &Response) -> Value {
    let header = |name: &String, value: &String| match name.as_str() {
        "X-WP-Total" | "X-WP-TotalPages" => Value::Int(text(value.clone()).to_int()),
        _ => text(value.clone()),
    };
    let headers = response.headers.iter();
    let headers =
        headers.map(|(name, value)| (Key::String(name.clone().into_bytes()), header(name, value)));
    Value::Array(headers.collect())
}

/// A PHP array's entries.
type Entries = Vec<(Key, Value)>;

/// The JSON body's parameters, or why there are none: JSON PHP refuses
/// with its error, `None` for a multipart form, which only PHP reads.
type Json = Result<Entries, Option<JsonError>>;

/// The request's body as `get_parameter_order()` reads it: the JSON body's
/// parameters, and a form body's, which a method other than POST reads itself.
fn body_params(request: &Request, method: &str) -> (Json, Entries) {
    let entries = |value: Value| match value {
        Value::Array(entries) => entries,
        _ => Vec::new(),
    };
    let kind = request::content_type(request.content_type);
    let empty = is_empty_string(request.body);
    // A multipart form is read by PHP alone.
    let multipart = request.method == "POST"
        && !request.body.is_empty()
        && kind.as_deref() == Some(b"multipart/form-data");
    let json = match &kind {
        _ if multipart => Err(None),
        Some(kind) if request::is_json(kind) && !empty => {
            hyscribe_phpser::json_decode_checked(request.body)
                .map(entries)
                .map_err(Some)
        }
        _ => Ok(Vec::new()),
    };
    let form = kind
        .as_deref()
        .is_none_or(|kind| kind == b"application/x-www-form-urlencoded");
    let mut post = request.post.to_vec();
    if method != "POST" && !empty && form {
        post = request::array_merge(entries(hyscribe_phpser::parse_str(request.body)), post);
    }
    (json, post)
}

/// [`answer`], or with `resumed` the answer once a write is made.
fn run(site: &dyn Site, request: &Request, resumed: Option<(Pending, Outcome)>) -> Answer {
    let get = |key| lookup(request.get, key);
    let jsonp = get("_jsonp").filter(|callback| !callback.is_empty());
    let charset =
        String::from_utf8_lossy(&site.option("blog_charset").unwrap_or_default()).into_owned();
    let kind = if jsonp.is_some() {
        "application/javascript"
    } else {
        "application/json"
    };
    let mut served = Served {
        status: 200,
        headers: Vec::new(),
        body: Vec::new(),
    };
    served.send("Content-Type", format!("{kind}; charset={charset}"));
    served.send("X-Robots-Tag", "noindex");
    let root = String::from_utf8_lossy(&esc_url_raw(site.rest_url("/").as_bytes())).into_owned();
    served.send("Link", format!("<{root}>; rel=\"https://api.w.org/\""));
    served.send("X-Content-Type-Options", "nosniff");
    if jsonp.is_some_and(|callback| !valid_callback(callback)) {
        let error = object([
            ("code", text("rest_callback_invalid")),
            ("message", text("Invalid JSONP callback function.")),
        ]);
        served.status = 400;
        served.body = encode(&error, false);
        return Answer::Served(served);
    }
    let end = request
        .route
        .iter()
        .rposition(|b| !matches!(b, b'/' | b'\\'));
    let route = request
        .route
        .get(..end.map_or(0, |end| end + 1))
        .unwrap_or_default();
    let route: &[u8] = if is_empty_string(route) { b"/" } else { route };
    let (method, overridden) = match (get("_method"), request.method_override) {
        (Some(Value::String(method)), _) => (method.to_ascii_uppercase(), false),
        (Some(_), _) => return Answer::Served(Served::not_ported()),
        (None, Some(method)) => (method.to_ascii_uppercase(), true),
        (None, None) => (request.method.as_bytes().to_vec(), false),
    };
    let method = String::from_utf8_lossy(&method).into_owned();
    served.send(
        "Access-Control-Expose-Headers",
        "X-WP-Total, X-WP-TotalPages, Link",
    );
    served.send(
        "Access-Control-Allow-Headers",
        "Authorization, X-WP-Nonce, Content-Disposition, Content-MD5, Content-Type",
    );
    // `rest_cookie_check_errors()`: without a nonce the request is a
    // visitor's, and a nonce must be the user's.
    let header = request.nonce.map(|nonce| Value::String(nonce.to_vec()));
    let nonce = lookup(request.post, "_wpnonce")
        .or(get("_wpnonce"))
        .or(header.as_ref());
    let user = nonce.map_or(0, |_| request.cookie_user);
    let verified = nonce.map(|nonce| {
        nonce
            .as_bytes()
            .is_some_and(|nonce| site.verify_nonce(nonce))
    });
    let (json, post) = body_params(request, &method);
    // `_oembed_rest_pre_serve_request()` serves an oEmbed GET for XML.
    let format = json.as_ref().ok().and_then(|json| lookup(json, "format"));
    let format = format.or_else(|| get("format"));
    let xml = method == "GET" && route == oembed::EMBED.as_bytes() && format == Some(&text("xml"));
    let params = Params {
        json: json.clone().unwrap_or_default(),
        post: if body_method(&method) {
            post
        } else {
            Vec::new()
        },
        get: request.get.to_vec(),
        ..Params::default()
    };
    let context = Context {
        site,
        user,
        head: method == "HEAD",
        uri: request.uri,
        parameters: [&params.json, &params.post, &params.get]
            .into_iter()
            .any(|source| gives_parameters(source)),
        remote_addr: request.remote_addr,
        postpass: request.postpass,
        user_agent: request.user_agent,
        host: request.host.unwrap_or_default(),
        now: request.now,
    };
    let response = match verified {
        Some(false) => RestError::status("rest_cookie_invalid_nonce", "Cookie check failed", 403)
            .into_response(),
        _ => {
            if verified.is_some() {
                served.send("X-WP-Nonce", site.create_nonce());
            }
            // `rest_pre_dispatch`, given the request's method and route: a
            // result it gives is the answer.
            let route_bytes = route.to_vec();
            let request_line: [&dyn std::any::Any; 2] = [&method, &route_bytes];
            let early: Option<Response> =
                hooks::apply_filters("rest_pre_dispatch", None, &request_line);
            let dispatched = early.map_or_else(
                || dispatch(&context, (&method, route), (params, json.err()), resumed),
                Ok,
            );
            match dispatched {
                Ok(response) => response,
                Err(Halt::NotPorted) => return Answer::Served(Served::not_ported()),
                Err(Halt::Write(pending)) => return Answer::Write(pending),
            }
        }
    };
    let embed = get("_embed").map(embed::relations);
    let response = match get("_envelope") {
        Some(_) => {
            let headers = header_values(&response);
            let status = Value::Int(i64::from(response.status));
            let Ok(body) = embed::to_data(&context, response, embed.as_ref()) else {
                return Answer::Served(Served::not_ported());
            };
            let envelope = object([("body", body), ("status", status), ("headers", headers)]);
            Response::new(envelope, 200)
        }
        None => response,
    };
    for (name, value) in &response.headers {
        served.send(name, value.clone());
    }
    served.status = response.status;
    let failed = overridden && (400..500).contains(&served.status);
    if user != 0 || verified == Some(false) || failed {
        served.send("Expires", "Wed, 11 Jan 1984 05:00:00 GMT");
        served.send(
            "Cache-Control",
            "no-cache, must-revalidate, max-age=0, no-store, private",
        );
    }
    // The XML is sent before the CORS headers, which come too late for it.
    if xml {
        let Some(body) = oembed::create_xml(&response.into_data()) else {
            return Answer::Served(Served::not_ported());
        };
        served.send("Content-Type", format!("text/xml; charset={charset}"));
        served.body = body;
        return Answer::Served(served);
    }
    // `rest_send_cors_headers()`.
    let origin = request
        .origin
        .filter(|origin| !Value::String(origin.to_vec()).is_empty());
    if let Some(origin) = origin {
        let origin = if origin == b"null" {
            origin.to_vec()
        } else {
            esc_url_raw(origin)
        };
        served.send(
            "Access-Control-Allow-Origin",
            String::from_utf8_lossy(&origin),
        );
        served.send(
            "Access-Control-Allow-Methods",
            "OPTIONS, GET, POST, PUT, PATCH, DELETE",
        );
        served.send("Access-Control-Allow-Credentials", "true");
        served.send("Vary", "Origin");
    } else if request.method == "GET" && user == 0 {
        served.send("Vary", "Origin");
    }
    if method == "HEAD" {
        return Answer::Served(served);
    }
    let Ok(data) = embed::to_data(&context, response, embed.as_ref()) else {
        return Answer::Served(Served::not_ported());
    };
    let json = encode(
        &data,
        request
            .get
            .iter()
            .any(|(key, _)| key.to_php_string() == b"_pretty"),
    );
    served.body = match jsonp {
        Some(callback) => [
            b"/**/",
            callback.as_bytes().unwrap_or_default(),
            b"(",
            &json,
            b")",
        ]
        .concat(),
        None => json,
    };
    Answer::Served(served)
}

/// The methods `get_parameter_order()` reads a body's parameters for.
fn body_method(method: &str) -> bool {
    matches!(method, "POST" | "PUT" | "PATCH" | "DELETE")
}

/// The permission checks of a matched route's endpoints and the endpoint
/// matched, if the route is ported.
fn ported(matched: &routes::Matched) -> Option<(Vec<Permission>, Endpoint)> {
    let mut endpoints = endpoints(matched.regex)?;
    let permissions = endpoints
        .iter()
        .map(|endpoint| endpoint.permission)
        .collect();
    // A route is ported with every endpoint the table lists.
    Some((permissions, endpoints.swap_remove(matched.endpoint)))
}

/// `rest_send_allow_header()`: a method is allowed as its last endpoint
/// permits, in the order methods first appear.
fn allowed(
    context: &Context,
    permissions: &[Permission],
    methods: &'static [Vec<String>],
    params: &Params,
) -> Vec<&'static str> {
    let methods: Vec<(&str, bool)> = permissions
        .iter()
        .zip(methods)
        .flat_map(|(permission, methods)| {
            let permitted = permission(context, params).is_ok();
            methods
                .iter()
                .map(move |method| (method.as_str(), permitted))
        })
        .collect();
    methods
        .iter()
        .enumerate()
        .filter(|(i, (method, _))| {
            methods.iter().position(|(known, _)| known == method) == Some(*i)
        })
        .filter(|(_, (method, _))| {
            methods
                .iter()
                .rev()
                .find(|(known, _)| known == method)
                .is_some_and(|(_, allow)| *allow)
        })
        .map(|(_, (method, _))| *method)
        .collect()
}

/// A `self` link to `route`, as `get_response_links()` gives it: with the
/// methods the route allows as its `targetHints`, when it allows any. A
/// link's parameters are its route's own, which its arguments accept, and
/// no permission check ported reads an argument's default.
pub(crate) fn self_link(context: &Context, response: &mut Response, route: &str) {
    self_link_with(context, response, route, false);
}

/// [`self_link`], marked `embeddable` when asked.
pub(crate) fn self_link_with(
    context: &Context,
    response: &mut Response,
    route: &str,
    embeddable: bool,
) {
    let href = (Key::String(b"href".to_vec()), text(context.rest_url(route)));
    let hints = routes::match_request("GET", route.as_bytes()).and_then(|matched| {
        let (permissions, endpoint) = ported(&matched)?;
        let mut params = Params {
            route: matched.regex,
            url: matched.params,
            ..Params::default()
        };
        let sanitized = sanitize_params(context, &mut params, &endpoint.args).ok();
        let allowed = sanitized.map(|()| allowed(context, &permissions, matched.methods, &params));
        let allowed = allowed.filter(|allowed| !allowed.is_empty())?;
        let allow = allowed.into_iter().map(text).collect();
        Some(object([("allow", response::list(allow))]))
    });
    let mut link = Vec::new();
    if embeddable {
        link.push((Key::String(b"embeddable".to_vec()), Value::Bool(true)));
    }
    link.push(href);
    link.extend(hints.map(|hints| (Key::String(b"targetHints".to_vec()), hints)));
    response.add_link_attributes("self", Value::Array(link));
}

/// `dispatch()` with the filters core adds: OPTIONS requests, the route's
/// arguments checked, its permission and callback, then the `Allow`
/// header and `_fields`; with `resumed`, the answer of a write once made.
fn dispatch(
    context: &Context,
    (method, route): (&str, &[u8]),
    (params, refused): (Params, Option<Option<JsonError>>),
    resumed: Option<(Pending, Outcome)>,
) -> Result<Response, Halt> {
    if method == "OPTIONS" {
        // `rest_handle_options_request()`: the description of the first
        // route matched, with its URL parameters unchecked.
        let Some(matched) = routes::described(route) else {
            let data = fields::filter_fields(Value::Array(Vec::new()), &params);
            return Ok(Response::new(data, 200));
        };
        let permissions: Vec<Permission> = endpoints(matched.regex)
            .ok_or(Halt::NotPorted)?
            .iter()
            .map(|endpoint| endpoint.permission)
            .collect();
        if widgets::would_write(context, matched.regex) {
            return Err(Halt::NotPorted);
        }
        let params = Params {
            route: matched.regex,
            url: matched.params,
            ..params
        };
        let response = Response::new(describe(context, matched.regex), 200);
        return Ok(finish(
            context,
            response,
            &permissions,
            matched.methods,
            &params,
        ));
    }
    let Some(matched) = routes::match_request(method, route) else {
        let message = "No route was found matching the URL and request method.";
        return Ok(RestError::status("rest_no_route", message, 404).into_response());
    };
    let (permissions, endpoint) = ported(&matched).ok_or(Halt::NotPorted)?;
    if widgets::would_write(context, matched.regex) {
        return Err(Halt::NotPorted);
    }
    // The link previews fetch, which waits on ADR 0019.
    if matched.regex == url_details::ROUTE && !context.site.outbound() {
        return Err(Halt::NotPorted);
    }
    let (answered, params) = match resumed {
        Some((pending, outcome)) => match pending.batch {
            Some(suspended) => {
                let written = (pending.plan, pending.finish, outcome);
                let answered = batch::resumed(context, *suspended, written)?;
                (Ok(answered), pending.params)
            }
            None => {
                let mut params = pending.params;
                let answered = (pending.finish)(context, &mut params, pending.plan, outcome);
                (answered, params)
            }
        },
        None => {
            let checked = checked(context, &matched, &endpoint, params, refused)?;
            if matched.regex == batch::ROUTE {
                let (params, _, passed) = checked;
                let answered = match passed {
                    Ok(()) => Ok(batch::serve(context, &params)?),
                    Err(error) => Err(error),
                };
                (answered, params)
            } else {
                responded(context, &endpoint, checked)?
            }
        }
    };
    if answered
        .as_ref()
        .is_err_and(|error| error.code == response::NOT_PORTED)
    {
        return Err(Halt::NotPorted);
    }
    let response = answered.unwrap_or_else(RestError::into_response);
    Ok(finish(
        context,
        response,
        &permissions,
        matched.methods,
        &params,
    ))
}

/// The filters after dispatching: the `Allow` header, then `_fields`.
fn finish(
    context: &Context,
    mut response: Response,
    permissions: &[Permission],
    methods: &'static [Vec<String>],
    params: &Params,
) -> Response {
    let allowed = allowed(context, permissions, methods, params);
    if !allowed.is_empty() {
        response.header("Allow", &allowed.join(", "));
    }
    if !response.is_error() {
        response.data = fields::filter_fields(response.data, params);
    }
    response
}

/// The description of the route `regex`, as `get_data_for_route()` gives
/// it in the `help` context, exported by `reference/wordpress/export.sh`.
pub(crate) fn describe(context: &Context, regex: &str) -> Value {
    let description = descriptions().get_str(regex).cloned().unwrap_or_default();
    placed(description, b"%rest_url%", &|path| context.rest_url(path))
}

/// Every route's description by its regex, with `%rest_url%` before its
/// links.
pub(crate) fn descriptions() -> &'static Value {
    const DESCRIPTIONS: &[u8] =
        include_bytes!("../../../wordpress/wp-includes/rest-descriptions.json");
    static DATA: OnceLock<Value> = OnceLock::new();
    DATA.get_or_init(|| json_decode(DESCRIPTIONS).unwrap_or_default())
}

/// Whether `source` gives a parameter other than those `serve_request()`
/// reads itself, which a write's arguments would check.
pub(crate) fn gives_parameters(source: &[(Key, Value)]) -> bool {
    source.iter().any(|(key, _)| {
        let name = key.to_php_string();
        !GLOBAL_PARAMETERS
            .iter()
            .any(|known| known.as_bytes() == name)
    })
}

/// `value` with each string that starts with `placeholder` given the URL
/// `url` makes of the rest, as exported data marks a site's URLs.
pub(crate) fn placed(value: Value, placeholder: &[u8], url: &dyn Fn(&str) -> String) -> Value {
    match value {
        Value::String(string) => match string.strip_prefix(placeholder) {
            Some(rest) => text(url(&String::from_utf8_lossy(rest))),
            None => Value::String(string),
        },
        Value::Array(entries) => Value::Array(
            entries
                .into_iter()
                .map(|(key, value)| (key, placed(value, placeholder, url)))
                .collect(),
        ),
        value => value,
    }
}

/// `parse_json_params()`'s `rest_invalid_json`, with PHP's JSON error.
fn invalid_json(error: JsonError) -> RestError {
    RestError {
        data: object([
            ("status", Value::Int(400)),
            ("json_error_code", Value::Int(error.0.into())),
            ("json_error_message", text(error.message())),
        ]),
        ..RestError::status("rest_invalid_json", "Invalid JSON body passed.", 400)
    }
}

/// The request checked by the matched endpoint's arguments and permission:
/// its parameters, the write a ported write endpoint makes, and whether
/// they pass. A body `refused`, JSON PHP refuses, is answered
/// `rest_invalid_json` before any check; a multipart form is not ported.
fn checked(
    context: &Context,
    matched: &routes::Matched,
    endpoint: &Endpoint,
    params: Params,
    refused: Option<Option<JsonError>>,
) -> Result<Checked, Halt> {
    let params = with_route(matched, endpoint, params);
    match refused {
        Some(Some(error)) => return Ok((params, None, Err(invalid_json(error)))),
        Some(None) => return Err(Halt::NotPorted),
        None => {}
    }
    let (params, writing, checked) = validated(context, matched, endpoint, params)?;
    let checked = checked.and_then(|()| (endpoint.permission)(context, &params));
    Ok((params, writing, checked))
}

/// `params` with the matched route's own: its URL parameters, and its
/// endpoint's arguments' defaults.
pub(crate) fn with_route(matched: &routes::Matched, endpoint: &Endpoint, params: Params) -> Params {
    Params {
        route: matched.regex,
        url: matched.params.clone(),
        defaults: defaults(&endpoint.args),
        ..params
    }
}

/// A request's parameters, the write its endpoint makes, and whether its
/// checks pass.
pub(crate) type Checked = (Params, Option<posts::write::Writing>, Result<(), RestError>);

/// `has_valid_params()` and `sanitize_params()` of `params`, which hold the
/// matched route's own, by the endpoint's arguments, those of a write
/// included; what they do not port is answered 501.
pub(crate) fn validated(
    context: &Context,
    matched: &routes::Matched,
    endpoint: &Endpoint,
    mut params: Params,
) -> Result<Checked, Halt> {
    let writing = match endpoint.callback {
        Some(_) => None,
        None => writing(matched.regex, matched.endpoint),
    };
    // The arguments of a write not ported are not ported either: one given
    // any is not answered, and one given none is checked, then answered 501.
    // A batch's requests are checked by their own routes.
    let batch = matched.regex == batch::ROUTE;
    if endpoint.callback.is_none() && writing.is_none() && context.parameters && !batch {
        return Err(Halt::NotPorted);
    }
    let mut args = endpoint.args.clone();
    let unported = |arg: &Arg| matches!(arg.sanitize, Sanitize::NotPorted) && params.has(arg.name);
    if args.iter().any(unported) {
        return Err(Halt::NotPorted);
    }
    if let Some(writing) = &writing {
        if (writing.unported)(&params) {
            return Err(Halt::NotPorted);
        }
        args = (writing.args)(context, &params, args);
    }
    let checked = has_valid_params(context, &params, &args)
        .and_then(|()| sanitize_params(context, &mut params, &args));
    Ok((params, writing, checked))
}

/// The answer of a request whose checks came to `checked`: its callback's,
/// the write a ported write plans, or 501 for a write not ported.
pub(crate) fn responded(
    context: &Context,
    endpoint: &Endpoint,
    (mut params, writing, checked): Checked,
) -> Result<(Result<Response, RestError>, Params), Halt> {
    let answered = match (endpoint.callback, writing) {
        (Some(callback), _) => checked.and_then(|()| callback(context, &params)),
        (None, Some(writing)) => match checked.and_then(|()| (writing.plan)(context, &params)) {
            Ok(plan) if plan.writes_nothing() => {
                (writing.finish)(context, &mut params, plan, Outcome::Written(0))
            }
            Ok(plan) => {
                let finish = writing.finish;
                let pending = Pending {
                    plan,
                    params,
                    finish,
                    batch: None,
                };
                return Err(Halt::Write(Box::new(pending)));
            }
            Err(error) => Err(error),
        },
        (None, None) => checked.and_then(|()| Err(RestError::not_ported())),
    };
    Ok((answered, params))
}

#[cfg(test)]
mod tests;

// SPDX-License-Identifier: AGPL-3.0-or-later

//! The REST API (ADR 0013): `WP_REST_Server::serve_request()` and the
//! routes ported so far, pure and synchronous. The site and the current
//! user are read through [`Site`]; `hyscribe-http` reads the request,
//! checks the logged in cookie and sends the answer. A request that
//! reaches a route, method or keyword not ported is answered `501`.

mod comments;
mod editor;
mod embed;
mod fields;
mod media;
mod navigation;
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
mod users;

use hyscribe_kses::esc_url_raw;
use hyscribe_model::{Comment, CommentQuery, CommentRow, Post, Term, TermQuery, User, UserQuery};
use hyscribe_phpser::{Key, Value, is_empty_string, json_encode, json_encode_pretty, lookup};
use hyscribe_render::site::Site as Reads;
use hyscribe_render::theme::Theme;

use request::{Arg, Params, defaults, has_valid_params, sanitize_params};
use response::{Response, RestError, object, text};

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
    /// The site's content as `hyscribe-render` reads it, for the post
    /// routes.
    fn reads(&self) -> &dyn Reads;
    /// The active theme, which renders posts' content.
    fn theme(&self) -> &Theme;
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
    /// The `X-HTTP-Method-Override` header.
    pub method_override: Option<&'a [u8]>,
    /// The request has a body that is not a form, which is not read.
    pub unread_body: bool,
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
    fn not_ported() -> Served {
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
}

pub(crate) type Permission = fn(&Context, &Params) -> Result<(), RestError>;
pub(crate) type Callback = fn(&Context, &Params) -> Result<Response, RestError>;

/// A ported endpoint: its arguments, permission check and callback, none
/// for a write, whose permission check alone is ported.
pub(crate) struct Endpoint {
    args: Vec<Arg>,
    permission: Permission,
    callback: Option<Callback>,
}

/// The ported endpoints of the route `regex`.
fn endpoints(regex: &str) -> Option<Vec<Endpoint>> {
    types::endpoints(regex)
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

/// `serve_request()`, from `rest_api_loaded()`: the route untrailingslashed.
pub fn serve(site: &dyn Site, request: &Request) -> Served {
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
        return served;
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
        (Some(_), _) => return Served::not_ported(),
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
    let given = |source: &[(Key, Value)]| {
        let names = source.iter().map(|(key, _)| key.to_php_string());
        names.into_iter().any(|name| {
            !GLOBAL_PARAMETERS
                .iter()
                .any(|known| known.as_bytes() == name)
        })
    };
    let context = Context {
        site,
        user,
        head: method == "HEAD",
        uri: request.uri,
        parameters: request.unread_body || given(request.get) || given(request.post),
    };
    let response = match verified {
        Some(false) => RestError::status("rest_cookie_invalid_nonce", "Cookie check failed", 403)
            .into_response(),
        _ => {
            if verified.is_some() {
                served.send("X-WP-Nonce", site.create_nonce());
            }
            match dispatch(&context, &method, route, request.get) {
                Ok(response) => response,
                Err(NotPorted) => return Served::not_ported(),
            }
        }
    };
    let embed = get("_embed").map(embed::relations);
    let response = match get("_envelope") {
        Some(_) => {
            // The collections set their totals as integers.
            let header = |name: &String, value: &String| match name.as_str() {
                "X-WP-Total" | "X-WP-TotalPages" => Value::Int(text(value.clone()).to_int()),
                _ => text(value.clone()),
            };
            let headers = response
                .headers
                .iter()
                .map(|(name, value)| (Key::String(name.clone().into_bytes()), header(name, value)))
                .collect();
            let status = Value::Int(i64::from(response.status));
            let Ok(body) = embed::to_data(&context, response, embed.as_ref()) else {
                return Served::not_ported();
            };
            let envelope = object([
                ("body", body),
                ("status", status),
                ("headers", Value::Array(headers)),
            ]);
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
        return served;
    }
    let Ok(data) = embed::to_data(&context, response, embed.as_ref()) else {
        return Served::not_ported();
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
    served
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
/// header and `_fields`.
fn dispatch(
    context: &Context,
    method: &str,
    route: &[u8],
    get: &[(Key, Value)],
) -> Result<Response, NotPorted> {
    let params = Params {
        get: get.to_vec(),
        ..Params::default()
    };
    if method == "OPTIONS" {
        // `rest_handle_options_request()`: a route's description is not
        // ported.
        if routes::any_matches(route) {
            return Err(NotPorted);
        }
        return Ok(Response::new(
            fields::filter_fields(Value::Array(Vec::new()), &params),
            200,
        ));
    }
    let Some(matched) = routes::match_request(method, route) else {
        let message = "No route was found matching the URL and request method.";
        return Ok(RestError::status("rest_no_route", message, 404).into_response());
    };
    let (permissions, endpoint) = ported(&matched).ok_or(NotPorted)?;
    // A write's arguments are not ported: one given any is not answered,
    // and one given none is checked, then answered 501.
    let callback = match endpoint.callback {
        None if context.parameters => return Err(NotPorted),
        None => |_: &Context, _: &Params| Err(RestError::not_ported()),
        Some(callback) => callback,
    };
    let mut params = Params {
        route: matched.regex,
        url: matched.params.clone(),
        defaults: defaults(&endpoint.args),
        ..params
    };
    let checked = has_valid_params(&params, &endpoint.args)
        .and_then(|()| sanitize_params(context, &mut params, &endpoint.args));
    let answered = checked.and_then(|()| {
        (endpoint.permission)(context, &params).and_then(|()| callback(context, &params))
    });
    if answered
        .as_ref()
        .is_err_and(|error| error.code == response::NOT_PORTED)
    {
        return Err(NotPorted);
    }
    let mut response = answered.unwrap_or_else(RestError::into_response);
    let allowed = allowed(context, &permissions, matched.methods, &params);
    if !allowed.is_empty() {
        response.header("Allow", &allowed.join(", "));
    }
    if !response.is_error() {
        response.data = fields::filter_fields(response.data, &params);
    }
    Ok(response)
}

#[cfg(test)]
mod tests;

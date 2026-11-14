// SPDX-License-Identifier: AGPL-3.0-or-later

//! The REST API (ADR 0013): `WP_REST_Server::serve_request()` and the
//! routes ported so far, pure and synchronous. The site and the current
//! user are read through [`Site`]; `hyscribe-http` reads the request,
//! checks the logged in cookie and sends the answer. A request that
//! reaches a route, method or keyword not ported is answered `501`.

mod fields;
pub(crate) mod links;
mod posts;
mod request;
mod response;
mod routes;
mod schema;
mod statuses;
mod types;

use hyscribe_kses::esc_url_raw;
use hyscribe_phpser::{Key, Value, is_empty_string, json_encode, json_encode_pretty, lookup};
use hyscribe_render::site::Site as Reads;
use hyscribe_render::theme::Theme;

pub use response::{Response, RestError, paginate};
pub use schema::{Checked, Refusal, sanitize_value_from_schema, validate_value_from_schema};

use request::{Arg, Params, Sanitize, defaults, has_valid_params, sanitize_params};
use response::{object, text};

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
    /// `user_can( $user, $cap, ...$args )`.
    fn can(&self, user: u64, cap: &str, args: &[Value]) -> bool;
    /// The site's content, as `hyscribe-render` reads it.
    fn reads(&self) -> &dyn Reads;
    /// The active theme.
    fn theme(&self) -> &Theme;
}

/// A request for the API.
#[derive(Clone, Copy, Debug)]
pub struct Request<'a> {
    /// `$_SERVER['REQUEST_METHOD']`.
    pub method: &'a str,
    /// The `rest_route` query var.
    pub route: &'a [u8],
    /// `$_SERVER['REQUEST_URI']`.
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
pub(crate) struct NotPorted;

/// A request on its way through a route: the site and the current user.
pub(crate) struct Context<'a> {
    site: &'a dyn Site,
    user: u64,
    /// The request is a HEAD request.
    pub(crate) head: bool,
    /// `$_SERVER['REQUEST_URI']` and `$_GET`, which embedded requests share.
    pub(crate) uri: &'a [u8],
    pub(crate) get: &'a [(Key, Value)],
}

impl Context<'_> {
    /// `current_user_can( $cap )`.
    pub(crate) fn can(&self, cap: &str) -> bool {
        self.site.can(self.user, cap, &[])
    }

    /// `current_user_can( $cap, $id )`.
    pub(crate) fn can_for(&self, cap: &str, id: u64) -> bool {
        self.site
            .can(self.user, cap, &[Value::Int(id.cast_signed())])
    }

    /// `rest_authorization_required_code()`.
    pub(crate) fn authorization_required(&self) -> u16 {
        if self.user == 0 { 401 } else { 403 }
    }

    /// `rest_url( $path )`.
    pub(crate) fn rest_url(&self, path: &str) -> String {
        self.site.rest_url(path)
    }
}

pub(crate) type Permission = fn(&Context, &Params) -> Result<(), RestError>;
pub(crate) type Callback = fn(&Context, &Params) -> Result<Response, Failure>;

/// Why an endpoint gives no response: WordPress's error, or a request
/// whose answer is not ported, such as one PHP would write on.
pub(crate) enum Failure {
    Error(RestError),
    NotPorted,
}

impl From<RestError> for Failure {
    fn from(error: RestError) -> Failure {
        Failure::Error(error)
    }
}

/// A ported endpoint: its arguments, permission check and callback, none
/// for a write, which answers 501 until phase 2 but takes part in `Allow`.
pub(crate) struct Endpoint {
    pub(crate) args: Vec<Arg>,
    pub(crate) permission: Permission,
    pub(crate) callback: Option<Callback>,
}

/// The endpoints of the route `regex`, if ported.
fn endpoints(regex: &str) -> Option<Vec<Endpoint>> {
    types::endpoints(regex)
        .or_else(|| statuses::endpoints(regex))
        .or_else(|| posts::endpoints(regex))
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
    let context = Context {
        site,
        user,
        head: method == "HEAD",
        uri: request.uri,
        get: request.get,
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
    let embed: links::Embed = get("_embed").map(links::embed_param);
    let response = match get("_envelope") {
        Some(_) => {
            let headers = response
                .headers
                .iter()
                .map(|(name, value)| (Key::String(name.clone().into_bytes()), text(value.clone())))
                .collect();
            let status = Value::Int(i64::from(response.status));
            let Ok(body) = links::response_to_data(&context, response, &embed) else {
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
    let Ok(data) = links::response_to_data(&context, response, &embed) else {
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

/// A matched request: its endpoint, every endpoint's permission check for
/// `Allow`, and its parameters, checked and sanitized by the endpoint's
/// arguments.
pub(crate) struct Prepared {
    methods: &'static [Vec<String>],
    permissions: Vec<Permission>,
    endpoint: Endpoint,
    params: Params,
    checked: Result<(), RestError>,
}

/// `$request` set up for the matched endpoint, as `dispatch()` does.
pub(crate) fn prepare(
    context: &Context,
    matched: routes::Matched,
    get: &[(Key, Value)],
) -> Result<Prepared, NotPorted> {
    let mut endpoints = endpoints(matched.regex).ok_or(NotPorted)?;
    let permissions = endpoints
        .iter()
        .map(|endpoint| endpoint.permission)
        .collect();
    // A route is ported with every endpoint the table lists.
    let endpoint = endpoints.swap_remove(matched.endpoint);
    let mut params = Params {
        get: get.to_vec(),
        url: matched.params,
        defaults: defaults(&endpoint.args),
    };
    let unported = |arg: &Arg| matches!(arg.sanitize, Sanitize::NotPorted) && params.has(arg.name);
    if endpoint.args.iter().any(unported) {
        return Err(NotPorted);
    }
    let checked = has_valid_params(&params, &endpoint.args)
        .and_then(|()| sanitize_params(context, &mut params, &endpoint.args));
    Ok(Prepared {
        methods: matched.methods,
        permissions,
        endpoint,
        params,
        checked,
    })
}

impl Prepared {
    /// `rest_send_allow_header()`: a method is allowed as its last
    /// endpoint permits, in the order methods first appear.
    pub(crate) fn allowed(&self, context: &Context) -> Vec<&'static str> {
        let methods: Vec<(&'static str, bool)> = self
            .permissions
            .iter()
            .zip(self.methods)
            .flat_map(|(permission, methods)| {
                let permitted = permission(context, &self.params).is_ok();
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
}

/// The `maximum` of the `per_page` argument of a route's endpoint, which
/// embedded collections are asked for.
fn per_page_maximum(regex: &str, endpoint: usize) -> Option<i64> {
    let args = endpoints(regex)?.into_iter().nth(endpoint).map(|e| e.args);
    let per_page = args
        .unwrap_or_default()
        .into_iter()
        .find(|arg| arg.name == "per_page")?;
    per_page.schema.get_str("maximum").map(Value::to_int)
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
    if method == "OPTIONS" {
        // `rest_handle_options_request()`: a route's description is not
        // ported.
        if routes::any_matches(route) {
            return Err(NotPorted);
        }
        // `_fields` keeps nothing of an empty answer.
        return Ok(Response::new(Value::Array(Vec::new()), 200));
    }
    let Some(matched) = routes::match_request(method, route) else {
        let message = "No route was found matching the URL and request method.";
        return Ok(RestError::status("rest_no_route", message, 404).into_response());
    };
    let prepared = prepare(context, matched, get)?;
    let callback = prepared.endpoint.callback.ok_or(NotPorted)?;
    let answered = prepared
        .checked
        .clone()
        .and_then(|()| (prepared.endpoint.permission)(context, &prepared.params))
        .map_err(Failure::Error)
        .and_then(|()| callback(context, &prepared.params));
    let mut response = match answered {
        Ok(response) => response,
        Err(Failure::Error(error)) => error.into_response(),
        Err(Failure::NotPorted) => return Err(NotPorted),
    };
    let allowed = prepared.allowed(context);
    if !allowed.is_empty() {
        response.header("Allow", &allowed.join(", "));
    }
    if !response.is_error() {
        response.data = fields::filter_fields(response.data, &prepared.params);
    }
    Ok(response)
}

#[cfg(test)]
mod tests;

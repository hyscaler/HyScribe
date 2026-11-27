// SPDX-License-Identifier: AGPL-3.0-or-later

//! The REST routes extensions register (ADR 0022's 2026-11-27 amendment),
//! served as `WP_REST_Server` serves a plugin's `register_rest_route()`:
//! matched after core's routes, their arguments checked and sanitized, the
//! permission callback before the callback, then the `Allow` header and
//! `_fields`; and their namespaces' indexes. An extension that does not
//! answer is a fatal error, as a PHP callback's would be.

use hyscribe_phpser::JsonError;
use hyscribe_phpser::{Key, Pattern, Value, json_encode};

use crate::request::{Invalid, Params, missing, refused};
use crate::response::{Response, RestError, list, object, text};
use crate::schema::{Refusal, sanitize_value_from_schema, validate_value_from_schema};
use crate::{Context, Halt, Plugins, fields, index, invalid_json, routes};

/// A REST route an extension registered: one endpoint of its route.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginRoute {
    /// Its extension's place and its own, as the site knows them.
    pub place: (usize, usize),
    /// `$route_namespace`.
    pub namespace: String,
    /// `/{namespace}/{route}`, a regex matched as `@^route$@i`.
    pub route: String,
    /// `methods`, upper case.
    pub methods: Vec<String>,
    /// `permission_callback`, by number.
    pub permission: u32,
    /// `callback`, by number.
    pub callback: u32,
    /// `args`: each argument's schema by its name, a `validate_callback`
    /// or `sanitize_callback` by number.
    pub args: Value,
}

/// A route as `WP_REST_Server` keeps it: the endpoints of every
/// registration of it, as `$override` false adds them, its namespace the
/// last one's.
struct Group<'a> {
    route: &'a str,
    namespace: &'a str,
    endpoints: Vec<&'a PluginRoute>,
}

/// What a path is matched against, in the order registered: a
/// namespace's index before its first route.
enum Entry<'a> {
    Index(&'a str),
    Route(Group<'a>),
}

impl Entry<'_> {
    fn route(&self) -> String {
        match self {
            Entry::Index(namespace) => format!("/{namespace}"),
            Entry::Route(group) => group.route.to_string(),
        }
    }

    /// Whether `namespace` registered it.
    fn of(&self, namespace: &str) -> bool {
        match self {
            Entry::Index(known) => *known == namespace,
            Entry::Route(group) => group.endpoints.iter().any(|e| e.namespace == namespace),
        }
    }

    /// Whether `get_routes( $namespace )` keeps it: a route's namespace is
    /// its last registration's.
    fn kept_in(&self, namespace: &str) -> bool {
        match self {
            Entry::Index(known) => *known == namespace,
            Entry::Route(group) => group.namespace == namespace,
        }
    }
}

fn entries(routes: &[PluginRoute]) -> Vec<Entry<'_>> {
    let mut entries: Vec<Entry> = Vec::new();
    for route in routes {
        if !entries
            .iter()
            .any(|entry| matches!(entry, Entry::Index(known) if *known == route.namespace))
        {
            entries.push(Entry::Index(&route.namespace));
        }
        let known = entries.iter_mut().find_map(|entry| match entry {
            Entry::Route(group) if group.route == route.route => Some(group),
            _ => None,
        });
        match known {
            Some(group) => {
                group.namespace = &route.namespace;
                group.endpoints.push(route);
            }
            None => entries.push(Entry::Route(Group {
                route: &route.route,
                namespace: &route.namespace,
                endpoints: vec![route],
            })),
        }
    }
    entries
}

/// The URL parameters `entry` takes from `path`, if it matches.
fn matching(entry: &Entry, path: &[u8]) -> Option<Vec<(Key, Value)>> {
    let pattern = Pattern::caseless(&format!("^{}$", entry.route()))?;
    routes::named(&pattern, path)
}

/// The entries `match_request_to_handler()` tries for `path`, in order:
/// those of the namespaces that start it, or every one when none does,
/// core's included.
fn candidates<'e>(entries: &'e [Entry<'e>], path: &[u8]) -> Vec<&'e Entry<'e>> {
    let start = routes::start(path);
    let namespaces = entries.iter().filter_map(|entry| match entry {
        Entry::Index(namespace) => Some(*namespace),
        Entry::Route(_) => None,
    });
    let ours: Vec<&str> = namespaces
        .filter(|namespace| start.starts_with(namespace.as_bytes()))
        .collect();
    if ours.is_empty() && !routes::namespaced(&start) {
        return entries.iter().collect();
    }
    // Each entry is kept in one namespace, so none comes twice.
    let kept = ours.into_iter().flat_map(|namespace| {
        let kept = entries.iter().filter(move |entry| entry.kept_in(namespace));
        kept.collect::<Vec<_>>()
    });
    kept.collect()
}

/// What answers an endpoint: a namespace's index, whose one endpoint is
/// `GET` with no permission callback, or an extension.
#[derive(Clone, Copy)]
enum Handler<'a> {
    Index(&'a str),
    Endpoint(&'a PluginRoute),
}

/// Each endpoint's handler and methods.
fn endpoints<'a>(entry: &'a Entry) -> Vec<(Handler<'a>, Vec<String>)> {
    match entry {
        Entry::Index(namespace) => vec![(Handler::Index(namespace), vec!["GET".into()])],
        Entry::Route(group) => {
            let endpoints = group.endpoints.iter();
            let each = endpoints.map(|e| (Handler::Endpoint(e), e.methods.clone()));
            each.collect()
        }
    }
}

/// An entry reached, its endpoint's handler and the URL parameters.
type Found<'e> = (&'e Entry<'e>, Handler<'e>, Vec<(Key, Value)>);

/// The entry of the extensions' routes `method` on `path` reaches, as
/// `match_request_to_handler()` finds it, its endpoint's handler and the
/// URL parameters.
fn found<'e>(entries: &'e [Entry<'e>], (method, path): (&str, &[u8])) -> Option<Found<'e>> {
    candidates(entries, path).into_iter().find_map(|entry| {
        let endpoints = endpoints(entry);
        let at = routes::serving(endpoints.iter().map(|(_, methods)| methods), method)?;
        let url = matching(entry, path)?;
        endpoints.get(at).map(|(handler, _)| (entry, *handler, url))
    })
}

/// Whether `method` on `path` reaches an extension's route.
pub(crate) fn matches(context: &Context, method: &str, path: &[u8]) -> bool {
    let routes = registered(context);
    found(&entries(&routes), (method, path)).is_some()
}

/// `dispatch()` of a request no core route matches, over the extensions'
/// routes; none when no route of theirs matches either.
pub(crate) fn dispatch(
    context: &Context,
    asked: (&str, &[u8]),
    (params, refused): (Params, Option<Option<JsonError>>),
) -> Option<Result<Response, Halt>> {
    let routes = registered(context);
    let entries = entries(&routes);
    let (entry, handler, url) = found(&entries, asked)?;
    let params = Params { url, ..params };
    Some(match handler {
        Handler::Endpoint(endpoint) => answer(context, asked, (entry, endpoint), (params, refused)),
        Handler::Index(namespace) => namespace_index(context, (entry, namespace), params, refused),
    })
}

/// `rest_handle_options_request()` of a path no core route matches: the
/// first route of the extensions' that matches it, whatever its
/// namespace or methods, described in the `help` context.
pub(crate) fn options(
    context: &Context,
    path: &[u8],
    params: &Params,
) -> Option<Result<Response, Halt>> {
    let routes = registered(context);
    let entries = entries(&routes);
    let (entry, url) = entries
        .iter()
        .find_map(|entry| matching(entry, path).map(|url| (entry, url)))?;
    let params = Params {
        url,
        ..params.clone()
    };
    let response = Response::new(describe(context, entry), 200);
    let request = request(context, ("OPTIONS", path), &params);
    Some(finished(
        context,
        entry,
        response,
        &params,
        request.as_ref(),
    ))
}

/// `get_namespace_index()` of an extension's namespace.
fn namespace_index(
    context: &Context,
    (entry, namespace): (&Entry, &str),
    params: Params,
    refused: Option<Option<JsonError>>,
) -> Result<Response, Halt> {
    let defaults = vec![
        (Key::String(b"namespace".to_vec()), text(namespace)),
        (Key::String(b"context".to_vec()), text("view")),
    ];
    let params = Params { defaults, ..params };
    let response = match refused {
        Some(Some(error)) => invalid_json(error).into_response(),
        Some(None) => return Err(Halt::NotPorted),
        None => match index::namespace_index(context, &params) {
            Ok(response) => response,
            Err(error) if error.code == crate::response::NOT_PORTED => return Err(Halt::NotPorted),
            Err(error) => error.into_response(),
        },
    };
    finished(context, entry, response, &params, None)
}

/// The descriptions of the routes of the extensions' namespace
/// `namespace`, by route, in their order, if it is one.
pub(crate) fn namespace_routes(context: &Context, namespace: &[u8]) -> Option<Vec<(Key, Value)>> {
    let routes = registered(context);
    let entries = entries(&routes);
    let namespace = std::str::from_utf8(namespace).ok()?;
    let ours = entries.iter().filter(|entry| entry.of(namespace));
    let described: Vec<(Key, Value)> = ours
        .map(|entry| {
            (
                Key::String(entry.route().into_bytes()),
                describe(context, entry),
            )
        })
        .collect();
    (!described.is_empty()).then_some(described)
}

/// The keywords `get_data_for_route()` shows of an argument's schema.
const KEYWORDS: [&str; 25] = [
    "title",
    "description",
    "default",
    "type",
    "format",
    "enum",
    "items",
    "properties",
    "additionalProperties",
    "patternProperties",
    "minProperties",
    "maxProperties",
    "minimum",
    "maximum",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "multipleOf",
    "minLength",
    "maxLength",
    "pattern",
    "minItems",
    "maxItems",
    "uniqueItems",
    "anyOf",
    "oneOf",
];

/// `get_data_for_route()`: an extension's route has no schema, so it is
/// described the same in every context.
fn describe(context: &Context, entry: &Entry) -> Value {
    let route = entry.route();
    let (namespace, endpoints) = match entry {
        Entry::Index(namespace) => {
            let default =
                |value: &str| object([("default", text(value)), ("required", Value::Bool(false))]);
            let args = object([
                ("namespace", default(namespace)),
                ("context", default("view")),
            ]);
            let get = vec![text("GET")];
            let endpoint = object([("methods", list(get)), ("args", args)]);
            (*namespace, vec![endpoint])
        }
        Entry::Route(group) => {
            let endpoints = group.endpoints.iter().map(|endpoint| {
                let methods = endpoint.methods.iter().map(|m| text(m.as_str())).collect();
                let args = arguments(&endpoint.args).map(|(name, opts)| {
                    let opts = opts.as_array().unwrap_or_default();
                    let mut shown: Vec<(Key, Value)> = opts
                        .iter()
                        .filter(|(key, _)| {
                            KEYWORDS.iter().any(|k| k.as_bytes() == key.to_php_string())
                        })
                        .cloned()
                        .collect();
                    let required = opts.iter().any(|(key, value)| {
                        key.to_php_string() == b"required" && !value.is_empty()
                    });
                    shown.push((Key::String(b"required".to_vec()), Value::Bool(required)));
                    (Key::String(name.as_bytes().to_vec()), Value::Array(shown))
                });
                object([
                    ("methods", list(methods)),
                    ("args", Value::Array(args.collect())),
                ])
            });
            (group.namespace, endpoints.collect())
        }
    };
    let methods = endpoints.iter().flat_map(|endpoint| {
        let methods = endpoint.get_str("methods").and_then(Value::as_array);
        methods
            .unwrap_or_default()
            .iter()
            .map(|(_, method)| method.clone())
    });
    let mut data = vec![
        (Key::String(b"namespace".to_vec()), text(namespace)),
        (Key::String(b"methods".to_vec()), list(methods.collect())),
        (Key::String(b"endpoints".to_vec()), list(endpoints)),
    ];
    if !variable(&route) {
        let href = object([("href", text(context.rest_url(&route)))]);
        let links = object([("self", list(vec![href]))]);
        data.push((Key::String(b"_links".to_vec()), links));
    }
    Value::Array(data)
}

/// Whether `route` is variable: a `{` once `get_data_for_route()` has made
/// each named group `(?P<name>...)` one.
pub(crate) fn variable(route: &str) -> bool {
    let bytes = route.as_bytes();
    let group = (0..bytes.len()).any(|at| {
        let Some(rest) = bytes.get(at..).and_then(|rest| rest.strip_prefix(b"(?P<")) else {
            return false;
        };
        let name = rest
            .iter()
            .take_while(|b| b.is_ascii_alphanumeric() || **b == b'_')
            .count();
        let after = rest.get(name..).unwrap_or_default();
        let closed = after.strip_prefix(b">").and_then(|after| {
            after
                .iter()
                .take_while(|b| **b != b'\n')
                .position(|b| *b == b')')
        });
        name > 0 && closed.is_some()
    });
    group || route.contains('{')
}

/// An endpoint's arguments: each name and its schema.
fn arguments(args: &Value) -> impl Iterator<Item = (String, &Value)> {
    let entries = args.as_array().unwrap_or_default().iter();
    entries.map(|(key, schema)| {
        (
            String::from_utf8_lossy(&key.to_php_string()).into_owned(),
            schema,
        )
    })
}

/// An argument's callback of `kind`, by number, if it names one.
fn callback(schema: &Value, kind: &str) -> Option<u32> {
    match schema.get_str(kind) {
        Some(Value::Int(n)) => u32::try_from(*n).ok(),
        _ => None,
    }
}

/// The request as an extension's callback is given it: its method, route,
/// parameters as `get_params()` gives them, content type and body; none
/// where any of them is not UTF-8, which cannot cross.
fn request(context: &Context, (method, path): (&str, &[u8]), params: &Params) -> Option<Value> {
    let (content_type, body) = context.given;
    let content_type = content_type.map_or(Value::Null, |kind| Value::String(kind.to_vec()));
    let mut merged = params.merged();
    // `rest_route` only under pretty permalinks.
    let plain = context.option("permalink_structure");
    if plain.is_none_or(|structure| crate::unserialized(&structure).is_empty()) {
        merged.retain(|(key, _)| key.to_php_string() != b"rest_route");
    }
    let request = object([
        ("method", text(method)),
        ("route", Value::String(path.to_vec())),
        ("params", Value::Array(merged)),
        ("content_type", content_type),
        ("body", Value::String(body.to_vec())),
    ]);
    let sources = [
        &params.json,
        &params.post,
        &params.get,
        &params.url,
        &params.defaults,
    ];
    let crosses = sources
        .iter()
        .all(|source| json_encode(&Value::Array(source.to_vec())).is_some());
    (crosses && json_encode(&request).is_some()).then_some(request)
}

/// The extension's answer of `callback` of `endpoint` over `args`; a fatal
/// error where it does not answer.
fn call(
    context: &Context,
    endpoint: &PluginRoute,
    callback: u32,
    args: &[Value],
) -> Result<Value, Halt> {
    let plugins = context.site.plugins();
    let answer = plugins.and_then(|plugins| plugins.call(endpoint, callback, args, context.user));
    answer.ok_or(Halt::Fatal)
}

/// The active extensions' routes.
fn registered(context: &Context) -> Vec<PluginRoute> {
    context
        .site
        .plugins()
        .map_or_else(Vec::new, Plugins::routes)
}

/// A `WP_Error` an extension answers: an object of a code, a message and
/// data, if any, and nothing else.
fn wp_error(answer: &Value) -> Option<(Value, Value, Value)> {
    let entries = answer.as_array()?;
    let known = |key: &Key| {
        matches!(
            key.to_php_string().as_slice(),
            b"code" | b"message" | b"data"
        )
    };
    let code = answer
        .get_str("code")
        .filter(|code| matches!(code, Value::String(c) if !c.is_empty()));
    let message = answer
        .get_str("message")
        .filter(|message| matches!(message, Value::String(_)));
    let data = answer.get_str("data").cloned().unwrap_or(Value::Null);
    let (code, message) = (code?.clone(), message?.clone());
    entries
        .iter()
        .all(|(key, _)| known(key))
        .then_some((code, message, data))
}

/// A status HTTP sends.
fn status(value: &Value) -> Option<u16> {
    match value {
        Value::Int(n) => u16::try_from(*n).ok().filter(|n| (100..=599).contains(n)),
        _ => None,
    }
}

/// `rest_convert_error_to_response()`: sent with its data's status, else 500.
fn error_response((code, message, data): (Value, Value, Value)) -> Response {
    let sent = data.get_str("status").and_then(status).unwrap_or(500);
    Response::new(
        object([("code", code), ("message", message), ("data", data)]),
        sent,
    )
}

/// The headers an extension may set on its answer: none that frames or
/// encodes it, sets a cookie, CORS, the nonce or a security policy, or
/// asks the server in front to serve a file.
fn settable(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    const NAMED: [&str; 11] = [
        "location",
        "link",
        "cache-control",
        "expires",
        "last-modified",
        "etag",
        "vary",
        "content-disposition",
        "content-language",
        "retry-after",
        "www-authenticate",
    ];
    const NOT: [&str; 4] = [
        "x-wp-nonce",
        "x-content-type-options",
        "x-frame-options",
        "x-xss-protection",
    ];
    const NOT_PREFIXED: [&str; 4] = ["x-accel-", "x-sendfile", "x-litespeed-", "x-forwarded-"];
    let own = name.starts_with("x-")
        && !NOT.contains(&name.as_str())
        && !NOT_PREFIXED.iter().any(|prefix| name.starts_with(prefix));
    own || NAMED.contains(&name.as_str())
}

/// `WP_REST_Response::header()`'s name: each word's first letter upper
/// case, `ucwords()` over the words `-` separates.
fn header_name(name: &[u8]) -> String {
    let mut named = String::new();
    let mut word = true;
    for &b in name {
        let b = if b == b' ' { b'-' } else { b };
        named.push(char::from(if word { b.to_ascii_uppercase() } else { b }));
        word = matches!(b, b'-' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c);
    }
    named
}

/// `rest_ensure_response()` of a callback's answer: a `WP_Error`, a
/// response of its `status`, `headers` and `data`, or else data sent 200.
/// A status HTTP cannot send, or headers not an object, is a fatal error.
fn response(answer: Value) -> Result<Response, Halt> {
    if let Some(error) = wp_error(&answer) {
        return Ok(error_response(error));
    }
    let envelope = answer.as_array().filter(|entries| {
        let known = |key: &Key| {
            matches!(
                key.to_php_string().as_slice(),
                b"status" | b"headers" | b"data"
            )
        };
        entries.iter().all(|(key, _)| known(key)) && answer.get_str("data").is_some()
    });
    if envelope.is_none() {
        return Ok(Response::new(answer, 200));
    }
    let sent = answer.get_str("status").map_or(Some(200), status);
    let mut response = Response::new(Value::Null, sent.ok_or(Halt::Fatal)?);
    let headers = answer
        .get_str("headers")
        .map_or(Some(&[][..]), Value::as_array);
    for (name, value) in headers.ok_or(Halt::Fatal)? {
        let name = header_name(&name.to_php_string());
        let value = match value {
            Value::String(_) | Value::Int(_) => {
                String::from_utf8_lossy(&value.to_php_string()).into_owned()
            }
            _ => continue,
        };
        if settable(&name) {
            response.header(&name, &value);
        }
    }
    response.data = answer.get_str("data").cloned().unwrap_or_default();
    Ok(response)
}

/// `has_valid_params()` of an extension's endpoint: its required
/// arguments, then each `validate_callback` over the parameter it checks.
fn has_valid(
    context: &Context,
    endpoint: &PluginRoute,
    params: &Params,
    request: &Value,
) -> Result<Result<(), RestError>, Halt> {
    let args: Vec<(String, &Value)> = arguments(&endpoint.args).collect();
    let names = args.iter().map(|(name, schema)| (name.as_str(), *schema));
    if let Err(error) = missing(params, names) {
        return Ok(Err(error));
    }
    let mut invalid = Invalid::default();
    for (name, schema) in &args {
        let (Some(value), Some(validate)) =
            (params.get(name), callback(schema, "validate_callback"))
        else {
            continue;
        };
        let given = [value.clone(), request.clone(), text(name.as_str())];
        let answer = call(context, endpoint, validate, &given)?;
        if answer == Value::Bool(false) {
            invalid.not_valid(name);
        } else if let Some((code, message, data)) = wp_error(&answer) {
            invalid.refuse(name, (code, message), data);
        }
    }
    Ok(invalid.check())
}

/// `sanitize_params()` of an extension's endpoint: each parameter of each
/// source by its argument's `sanitize_callback`, or by its schema as
/// `rest_parse_request_arg()` checks it when the argument has a type and
/// no callback; a keyword not ported is answered 501.
fn sanitized(
    context: &Context,
    endpoint: &PluginRoute,
    params: &mut Params,
    request: &Value,
) -> Result<Result<(), RestError>, Halt> {
    let mut invalid = Invalid::default();
    for source in params.sources_mut() {
        for (key, value) in source.iter_mut() {
            let name = String::from_utf8_lossy(&key.to_php_string()).into_owned();
            let Some((_, schema)) = arguments(&endpoint.args).find(|(known, _)| *known == name)
            else {
                continue;
            };
            let typed = schema.get_str("type").is_some_and(|kind| !kind.is_empty());
            let given = schema.get_str("sanitize_callback");
            let checked = match (given, callback(schema, "sanitize_callback")) {
                (_, Some(sanitize)) => {
                    let args = [value.clone(), request.clone(), text(name.as_str())];
                    let answer = call(context, endpoint, sanitize, &args)?;
                    match wp_error(&answer) {
                        Some((code, message, data)) => {
                            invalid.refuse(&name, (code, message), data);
                            None
                        }
                        None => Some(answer),
                    }
                }
                (None, None) if typed => {
                    let parsed = validate_value_from_schema(value, schema, &name)
                        .and_then(|()| sanitize_value_from_schema(value, schema, &name));
                    if parsed == Err(Refusal(None)) {
                        return Err(Halt::NotPorted);
                    }
                    refused(parsed, &name, &mut invalid)
                }
                _ => continue,
            };
            if let Some(checked) = checked {
                *value = checked;
            }
        }
    }
    Ok(invalid.check())
}

/// `dispatch()` and `respond_to_request()` of an extension's endpoint:
/// the body's JSON, its arguments checked and sanitized, its permission
/// callback, and its callback only when that answered `true`.
fn answer(
    context: &Context,
    (method, path): (&str, &[u8]),
    (entry, endpoint): (&Entry, &PluginRoute),
    (params, refused): (Params, Option<Option<JsonError>>),
) -> Result<Response, Halt> {
    let defaults = arguments(&endpoint.args).filter_map(|(name, schema)| {
        let default = schema
            .get_str("default")
            .filter(|value| **value != Value::Null)?;
        Some((Key::String(name.into_bytes()), default.clone()))
    });
    let mut params = Params {
        defaults: defaults.collect(),
        ..params
    };
    let asked = request(context, (method, path), &params).ok_or(Halt::NotPorted)?;
    let checked = match refused {
        Some(Some(error)) => Err(invalid_json(error)),
        Some(None) => return Err(Halt::NotPorted),
        None => match has_valid(context, endpoint, &params, &asked)? {
            Ok(()) => sanitized(context, endpoint, &mut params, &asked)?,
            Err(error) => Err(error),
        },
    };
    let asked = request(context, (method, path), &params).ok_or(Halt::NotPorted)?;
    let response = match checked {
        Err(error) => error.into_response(),
        Ok(()) => {
            let permission = call(
                context,
                endpoint,
                endpoint.permission,
                std::slice::from_ref(&asked),
            )?;
            if permission == Value::Bool(true) {
                response(call(
                    context,
                    endpoint,
                    endpoint.callback,
                    std::slice::from_ref(&asked),
                )?)?
            } else if let Some(error) = wp_error(&permission) {
                error_response(error)
            } else {
                let message = "Sorry, you are not allowed to do that.";
                context.forbidden("rest_forbidden", message).into_response()
            }
        }
    };
    finished(context, entry, response, &params, Some(&asked))
}

/// The filters after dispatching an extension's route: the `Allow` header,
/// each method allowed as its endpoint's permission callback answers
/// `true` over `request`, then `_fields`.
fn finished(
    context: &Context,
    entry: &Entry,
    mut response: Response,
    params: &Params,
    request: Option<&Value>,
) -> Result<Response, Halt> {
    let mut allowed: Vec<(String, bool)> = Vec::new();
    for (handler, methods) in endpoints(entry) {
        for method in methods {
            let allows = match (handler, request) {
                (Handler::Index(_), _) => true,
                (Handler::Endpoint(endpoint), Some(request)) => {
                    let given = [request.clone()];
                    call(context, endpoint, endpoint.permission, &given)? == Value::Bool(true)
                }
                (Handler::Endpoint(_), None) => return Err(Halt::NotPorted),
            };
            match allowed.iter_mut().find(|(known, _)| *known == method) {
                Some((_, held)) => *held = allows,
                None => allowed.push((method, allows)),
            }
        }
    }
    let allowed: Vec<String> = allowed
        .into_iter()
        .filter(|(_, allows)| *allows)
        .map(|(m, _)| m)
        .collect();
    if !allowed.is_empty() {
        response.header("Allow", &allowed.join(", "));
    }
    if !response.is_error() {
        response.data = fields::filter_fields(response.data, params);
    }
    Ok(response)
}

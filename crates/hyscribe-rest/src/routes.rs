// SPDX-License-Identifier: AGPL-3.0-or-later

//! The routes WordPress 7.1.3 registers, exported by
//! `reference/wordpress/export.sh`, matched to requests as
//! `WP_REST_Server::match_request_to_handler()` matches them.

use std::sync::OnceLock;

use hyscribe_phpser::{Key, Pattern, Value, json_decode};

const ROUTES: &[u8] = include_bytes!("../../../wordpress/wp-includes/rest-routes.json");

/// A route: its regex, compiled as `@^$route$@i`, its namespace, and each
/// endpoint's methods.
struct Route {
    regex: String,
    pattern: Option<Pattern>,
    namespace: String,
    endpoints: Vec<Vec<String>>,
}

struct Table {
    namespaces: Vec<String>,
    routes: Vec<Route>,
}

fn string(value: Option<&Value>) -> String {
    String::from_utf8_lossy(&value.map(Value::to_php_string).unwrap_or_default()).into_owned()
}

fn strings(value: Option<&Value>) -> Vec<String> {
    let items = value.and_then(Value::as_array).unwrap_or_default();
    items.iter().map(|(_, item)| string(Some(item))).collect()
}

fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| {
        let data = json_decode(ROUTES).unwrap_or(Value::Null);
        let route = |(_, route): &(Key, Value)| {
            let field = |i| route.get(&Key::Int(i));
            let regex = string(field(0));
            let endpoints = field(2).and_then(Value::as_array).unwrap_or_default();
            Route {
                pattern: Pattern::caseless(&format!("^{regex}$")),
                namespace: string(field(1)),
                endpoints: endpoints
                    .iter()
                    .map(|(_, methods)| strings(Some(methods)))
                    .collect(),
                regex,
            }
        };
        Table {
            namespaces: strings(data.get_str("namespaces")),
            routes: data
                .get_str("routes")
                .and_then(Value::as_array)
                .unwrap_or_default()
                .iter()
                .map(route)
                .collect(),
        }
    })
}

/// A request's route, its endpoints' methods, the endpoint matched by
/// index, and the URL parameters.
#[derive(Debug, PartialEq)]
pub(crate) struct Matched {
    pub(crate) regex: &'static str,
    pub(crate) methods: &'static [Vec<String>],
    pub(crate) endpoint: usize,
    pub(crate) params: Vec<(Key, Value)>,
}

/// The URL parameters `route` takes from `path`, if it matches.
fn captures(route: &Route, path: &[u8]) -> Option<Vec<(Key, Value)>> {
    route
        .pattern
        .as_ref()
        .and_then(|pattern| named(pattern, path))
}

/// The named groups `pattern` takes from `path`, if it matches.
pub(crate) fn named(pattern: &Pattern, path: &[u8]) -> Option<Vec<(Key, Value)>> {
    let captures = pattern.captures(path)?;
    let named = pattern.names().iter().filter_map(|(name, group)| {
        let found = captures.get(*group).copied().flatten();
        found.map(|(start, end)| {
            let value = path.get(start..end).unwrap_or_default().to_vec();
            (Key::String(name.clone().into_bytes()), Value::String(value))
        })
    });
    Some(named.collect())
}

/// `trailingslashit( ltrim( $path, '/' ) )`, which a request's namespaces
/// start.
pub(crate) fn start(path: &[u8]) -> Vec<u8> {
    let trimmed = path.strip_prefix(b"/").unwrap_or(path);
    [trimmed, b"/"].concat()
}

/// Whether a core namespace starts `start`.
pub(crate) fn namespaced(start: &[u8]) -> bool {
    let namespaces = table().namespaces.iter();
    namespaces
        .into_iter()
        .any(|namespace| start.starts_with(namespace.as_bytes()))
}

/// `match_request_to_handler()`: the first route of the request's
/// namespaces, or of all when it has none, that matches the path and has
/// an endpoint for the method, HEAD served by GET's.
pub(crate) fn match_request(method: &str, path: &[u8]) -> Option<Matched> {
    let table = table();
    let start = start(path);
    let namespaces: Vec<&str> = table
        .namespaces
        .iter()
        .filter(|namespace| start.starts_with(namespace.as_bytes()))
        .map(String::as_str)
        .collect();
    let candidates = table
        .routes
        .iter()
        .filter(|route| namespaces.is_empty() || namespaces.contains(&route.namespace.as_str()));
    for route in candidates {
        let Some(params) = captures(route, path) else {
            continue;
        };
        if let Some(endpoint) = serving(route.endpoints.iter(), method) {
            return Some(Matched {
                regex: &route.regex,
                methods: &route.endpoints,
                endpoint,
                params,
            });
        }
    }
    None
}

/// The first of the endpoints, each by its methods, that serves `method`,
/// HEAD served by GET's where it has none of its own.
pub(crate) fn serving<'a>(
    endpoints: impl Iterator<Item = &'a Vec<String>>,
    method: &str,
) -> Option<usize> {
    let serves = |methods: &Vec<String>, method: &str| methods.iter().any(|m| m == method);
    endpoints.into_iter().position(|methods| {
        let head = method == "HEAD" && !serves(methods, "HEAD");
        serves(methods, if head { "GET" } else { method })
    })
}

/// The regexes of the routes registered in `namespace`, in their order, if
/// it is one.
pub(crate) fn namespace_routes(namespace: &[u8]) -> Option<Vec<&'static str>> {
    let table = table();
    let known = table
        .namespaces
        .iter()
        .any(|known| known.as_bytes() == namespace);
    let routes = table.routes.iter();
    known.then(|| {
        routes
            .filter(|route| route.namespace.as_bytes() == namespace)
            .map(|route| route.regex.as_str())
            .collect()
    })
}

/// Every route's regex.
#[cfg(test)]
pub(crate) fn regexes() -> impl Iterator<Item = &'static str> {
    table().routes.iter().map(|route| route.regex.as_str())
}

/// The first route `path` matches, of every namespace and whatever its
/// methods, as `rest_handle_options_request()` looks for one; no endpoint
/// is dispatched.
pub(crate) fn described(path: &[u8]) -> Option<Matched> {
    table().routes.iter().find_map(|route| {
        Some(Matched {
            params: captures(route, path)?,
            regex: &route.regex,
            methods: &route.endpoints,
            endpoint: 0,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_route_compiles() {
        let table = table();
        assert!(table.routes.len() > 100);
        for route in &table.routes {
            assert!(route.pattern.is_some(), "{}", route.regex);
        }
    }
}

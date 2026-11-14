// SPDX-License-Identifier: AGPL-3.0-or-later

//! The PHP oracle's answers in `tests/golden/rest.tsv`, replayed for each
//! role over the default install's roles, with `%nonce%` as each user's
//! nonce.

use std::collections::HashMap;
use std::sync::OnceLock;

use hyscribe_auth::caps::{CapPost, CapabilityData, User};
use hyscribe_phpser::parse_str;
use hyscribe_testkit::{SqlValue, unhex};

use super::*;

const SITE: &str = "http://127.0.0.1:8081";

/// The default install's options.
fn options() -> &'static HashMap<String, Vec<u8>> {
    static OPTIONS: OnceLock<HashMap<String, Vec<u8>>> = OnceLock::new();
    OPTIONS.get_or_init(|| {
        let version = hyscribe_testkit::default_install_versions().pop().unwrap();
        let dump = hyscribe_testkit::default_install(&version).unwrap();
        dump.table("wp_options")
            .filter_map(|row| match (&row[1], &row[2]) {
                (SqlValue::Text(name), SqlValue::Text(value)) => {
                    Some((String::from_utf8_lossy(name).into_owned(), value.clone()))
                }
                _ => None,
            })
            .collect()
    })
}

/// The oracle with one user of `role`, under plain or pretty permalinks.
struct Oracle {
    role: &'static str,
    pretty: bool,
}

const ROLES: [(&str, u64); 5] = [
    ("administrator", 1),
    ("editor", 701),
    ("author", 702),
    ("contributor", 703),
    ("subscriber", 704),
];

impl CapabilityData for Oracle {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        options().get(name).cloned()
    }
    fn user_meta(&self, _: u64, key: &str) -> Option<Vec<u8>> {
        let role = self.role;
        (key == "wp_capabilities")
            .then(|| format!("a:1:{{s:{}:\"{role}\";b:1;}}", role.len()).into_bytes())
    }
    fn user_exists(&self, _: u64) -> bool {
        true
    }
    fn post(&self, _: u64) -> Option<CapPost> {
        None
    }
    fn post_meta(&self, _: u64, _: &str) -> Option<Vec<u8>> {
        None
    }
    fn comment_post(&self, _: u64) -> Option<u64> {
        None
    }
    fn term_taxonomy(&self, _: u64) -> Option<String> {
        None
    }
    fn constant(&self, _: &str) -> bool {
        false
    }
}

impl Site for Oracle {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        options().get(name).cloned()
    }
    fn rest_url(&self, path: &str) -> String {
        if self.pretty {
            format!("{SITE}/wp-json{path}")
        } else {
            format!("{SITE}/index.php?rest_route={path}")
        }
    }
    fn verify_nonce(&self, nonce: &[u8]) -> bool {
        nonce == b"%nonce%"
    }
    fn create_nonce(&self) -> String {
        "%nonce%".into()
    }
    fn can(&self, user: u64, cap: &str) -> bool {
        user != 0 && User::load(self, "wp_", user).has_cap(self, cap, &[])
    }
}

/// A recorded request and PHP's answer.
pub(crate) struct Case {
    pub(crate) who: String,
    pub(crate) method: String,
    pub(crate) target: String,
    pub(crate) flags: String,
    /// 0 for a request that writes, which the golden records as `writes`.
    pub(crate) status: u16,
    pub(crate) headers: Vec<String>,
    pub(crate) body: Vec<u8>,
}

fn cases() -> Vec<Case> {
    cases_of(include_str!("../tests/golden/rest.tsv"))
}

/// The cases of a golden recorded by `reference/rest/golden.sh`.
pub(crate) fn cases_of(golden: &str) -> Vec<Case> {
    golden
        .lines()
        .map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            let text = |i: usize| String::from_utf8(unhex(fields[i])).unwrap();
            Case {
                who: fields[0].into(),
                method: fields[1].into(),
                target: text(2),
                flags: fields[3].into(),
                status: fields[4].parse().unwrap_or(0),
                headers: text(5).lines().map(String::from).collect(),
                body: unhex(fields[6]),
            }
        })
        .collect()
}

/// The REST route of a request, as the rewrite rules and `$_GET` give it.
fn route(path: &str, get: &[(Key, Value)]) -> Vec<u8> {
    if let Some(route) = lookup(get, "rest_route") {
        return route.to_php_string();
    }
    let pretty = path.strip_prefix("/index.php").unwrap_or(path);
    let route = pretty.strip_prefix("/wp-json").unwrap();
    if route.is_empty() {
        b"/".to_vec()
    } else {
        route.as_bytes().to_vec()
    }
}

/// `case` served by HyScribe's API.
pub(crate) fn serve_case(case: &Case) -> Served {
    let (path, query) = case.target.split_once('?').unwrap_or((&case.target, ""));
    let get = match parse_str(query.as_bytes()) {
        Value::Array(get) => get,
        _ => Vec::new(),
    };
    let (role, user) = ROLES
        .iter()
        .find(|(role, _)| *role == case.who)
        .copied()
        .unwrap_or(("", 0));
    let flag = |name: &str| case.flags.split(',').any(|flag| flag == name);
    let oracle = Oracle {
        role,
        pretty: flag("pretty"),
    };
    let route = route(path, &get);
    let request = Request {
        method: &case.method,
        route: &route,
        get: &get,
        post: &[],
        cookie_user: user,
        origin: if flag("origin") {
            Some(b"http://example.test")
        } else if flag("originnull") {
            Some(b"null")
        } else {
            None
        },
        nonce: (user != 0 && !flag("nononce")).then_some(b"%nonce%"),
        method_override: flag("override").then_some(b"DELETE"),
    };
    serve(&oracle, &request)
}

/// The headers the golden keeps, as it writes them.
pub(crate) fn kept(headers: &[(String, String)]) -> Vec<String> {
    let kept = |name: &str| {
        [
            "content-type",
            "x-robots-tag",
            "link",
            "allow",
            "vary",
            "expires",
            "cache-control",
        ]
        .contains(&name)
            || name.starts_with("access-control-")
            || name.starts_with("x-wp-")
    };
    let mut headers: Vec<String> = headers
        .iter()
        .map(|(name, value)| (name.to_ascii_lowercase(), value))
        .filter(|(name, _)| kept(name))
        .map(|(name, value)| format!("{name}: {value}"))
        .collect();
    headers.sort();
    headers
}

/// Requests PHP answers on routes or with arguments not ported.
const UNPORTED: [(&str, &str); 10] = [
    ("GET", "/?rest_route=/"),
    ("GET", "/?rest_route=0/"),
    ("GET", "/?rest_route=/wp/v2"),
    ("GET", "/?rest_route=/wp/v2/posts"),
    (
        "GET",
        "/?rest_route=/wp/v2/templates/twentytwentyfive//index",
    ),
    ("GET", "/?rest_route=/wp-abilities/v1/categories/a-b"),
    ("GET", "/?rest_route=/wp/v2/search&per_page=1"),
    (
        "GET",
        "/?rest_route=/wp/v2/search&per_page=1&page=2&search=",
    ),
    ("GET", "/?rest_route=/wp/v2/types&_method[]=x"),
    ("OPTIONS", "/?rest_route=/wp/v2/types"),
];

/// Every recorded request is answered as PHP answers it, for every role,
/// but those not ported, which are answered 501.
#[test]
fn answers_match_php() {
    let cases = cases();
    assert_eq!(cases.len(), 6 * 73);
    let mut unported = 0;
    for case in &cases {
        let served = serve_case(case);
        let what = format!(
            "{} {} {} {}",
            case.who, case.method, case.target, case.flags
        );
        if served.status == 501 {
            assert!(
                UNPORTED.contains(&(case.method.as_str(), case.target.as_str())),
                "{what}"
            );
            assert!(
                !String::from_utf8_lossy(&case.body).contains("rest_no_route"),
                "{what}"
            );
            assert!(served.headers.is_empty() && served.body.is_empty());
            unported += 1;
            continue;
        }
        assert_eq!(served.status, case.status, "{what}");
        let mut php = case.headers.clone();
        php.sort();
        assert_eq!(kept(&served.headers), php, "{what}");
        assert_eq!(
            String::from_utf8_lossy(&served.body),
            String::from_utf8_lossy(&case.body),
            "{what}"
        );
    }
    assert_eq!(unported, 6 * UNPORTED.len());
}

/// The pagination headers of `/wp/v2/search`'s two pages of one result.
#[test]
fn pages_are_linked_as_php_links_them() {
    let oracle = Oracle {
        role: "",
        pretty: false,
    };
    let searches = cases()
        .into_iter()
        .filter(|case| case.who == "visitor" && case.target.contains("/wp/v2/search"));
    let mut seen = 0;
    for (page, case) in (1..).zip(searches) {
        let query = case.target.split_once('?').unwrap().1;
        let Value::Array(query) = parse_str(query.as_bytes()) else {
            panic!("no query");
        };
        let mut response = Response::new(Value::Null, 200);
        paginate(
            &mut response,
            (2, 2, page),
            &query,
            &oracle.rest_url("/wp/v2/search"),
        );
        let paging = |line: &&String| {
            line.starts_with("x-wp-total")
                || line.contains("rel=\"next\"")
                || line.contains("rel=\"prev\"")
        };
        let mut php: Vec<String> = case.headers.iter().filter(paging).cloned().collect();
        php.sort();
        assert_eq!(kept(&response.headers), php);
        seen += 1;
    }
    assert_eq!(seen, 2);
}

/// A single page links to no other: `paginate()` sends a `Link` only for a
/// page before or after, as the collections' `$page > 1` and
/// `$page < $max_pages` checks do.
#[test]
fn a_single_page_has_no_links() {
    let mut response = Response::new(Value::Null, 200);
    paginate(&mut response, (1, 1, 1), &[], "http://example.test/x");
    assert_eq!(
        kept(&response.headers),
        ["x-wp-total: 1", "x-wp-totalpages: 1"]
    );
}

/// A `self` link has `targetHints` only to a route ported, whose methods the
/// user may use.
#[test]
fn self_links_hint_what_their_route_allows() {
    let oracle = Oracle {
        role: "",
        pretty: false,
    };
    let context = Context {
        site: &oracle,
        user: 0,
        head: false,
    };
    for route in ["/nope", "/wp/v2/posts", "/wp/v2/block-types/core/paragraph"] {
        let mut response = Response::new(Value::Array(Vec::new()), 200);
        self_link(&context, &mut response, route);
        let href = oracle.rest_url(route);
        let link = object([("href", text(href))]);
        let links = object([("self", Value::Array(vec![(Key::Int(0), link)]))]);
        assert_eq!(
            response.into_data(),
            Value::Array(vec![(Key::String(b"_links".to_vec()), links)]),
            "{route}"
        );
    }
}

// SPDX-License-Identifier: AGPL-3.0-or-later

//! The PHP oracle's answers in `tests/golden/rest.tsv`, and for the content
//! routes in `content.tsv` and `content-ttd.tsv`, replayed for a visitor
//! and each role, with `%nonce%` as each user's nonce.

use hyscribe_auth::caps::{CapPost, CapabilityData, User};
use hyscribe_phpser::parse_str;
use hyscribe_testkit::unhex;

use super::*;

mod content;
mod posts;
mod site;

use content::Content;

const SITE: &str = "http://127.0.0.1:8081";

/// The oracle over `content`, under plain or pretty permalinks.
struct Oracle {
    content: &'static Content,
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
        self.content.options.get(name).cloned()
    }
    fn user_meta(&self, user: u64, key: &str) -> Option<Vec<u8>> {
        let meta = self.content.usermeta.iter();
        meta.rev()
            .find(|(id, name, _)| *id == user && name == key)
            .map(|(_, _, value)| value.clone())
    }
    fn user_exists(&self, _: u64) -> bool {
        true
    }
    fn post(&self, id: u64) -> Option<CapPost> {
        let post = self.content.posts.iter().find(|post| post.id == id)?;
        Some(CapPost {
            id,
            author: post.author,
            status: post.status.clone(),
            post_type: post.post_type.clone(),
            parent: post.parent,
        })
    }
    fn post_meta(&self, _: u64, _: &str) -> Option<Vec<u8>> {
        None
    }
    fn comment_post(&self, _: u64) -> Option<u64> {
        None
    }
    fn term_taxonomy(&self, id: u64) -> Option<String> {
        let terms = self.content.terms.iter();
        terms
            .map(|row| (row.term.id, &row.taxonomy))
            .find(|(term, _)| *term == id)
            .map(|(_, taxonomy)| taxonomy.clone())
    }
    fn constant(&self, _: &str) -> bool {
        false
    }
}

impl Site for Oracle {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        self.content.options.get(name).cloned()
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
    fn can(&self, user: u64, cap: &str, args: &[Value]) -> bool {
        user != 0 && User::load(self, "wp_", user).has_cap(self, cap, args)
    }
    fn terms(&self, query: &TermQuery) -> Vec<Term> {
        self.content.terms(query)
    }
    fn count_terms(&self, query: &TermQuery) -> u64 {
        self.content.count_terms(query)
    }
    fn term_link(&self, taxonomy: &str, term: &Term) -> String {
        self.content.term_link(taxonomy, term, self.pretty)
    }
    fn post(&self, id: u64) -> Option<Post> {
        self.content
            .posts
            .iter()
            .find(|post| post.id == id)
            .cloned()
    }
    fn reads(&self) -> &dyn Reads {
        static FIXTURES: [site::Fixture; 2] = [
            site::Fixture {
                ttd: false,
                pretty: false,
                extra: false,
            },
            site::Fixture {
                ttd: false,
                pretty: true,
                extra: false,
            },
        ];
        &FIXTURES[usize::from(self.pretty)]
    }
    fn theme(&self) -> &Theme {
        static THEME: std::sync::OnceLock<Theme> = std::sync::OnceLock::new();
        THEME.get_or_init(hyscribe_render::theme::twentytwentyfive)
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

/// `case` served by HyScribe's API over the default install.
pub(crate) fn serve_case(case: &Case) -> Served {
    serve_over(content::default_install(), case)
}

/// `case` served by HyScribe's API over `content`.
fn serve_over(content: &'static Content, case: &Case) -> Served {
    let (path, query) = case.target.split_once('?').unwrap_or((&case.target, ""));
    let get = match parse_str(query.as_bytes()) {
        Value::Array(get) => get,
        _ => Vec::new(),
    };
    let user = ROLES
        .iter()
        .find(|(role, _)| *role == case.who)
        .map_or(0, |(_, user)| *user);
    let flag = |name: &str| case.flags.split(',').any(|flag| flag == name);
    let oracle = Oracle {
        content,
        pretty: flag("pretty"),
    };
    let route = route(path, &get);
    let request = Request {
        method: &case.method,
        route: &route,
        uri: case.target.as_bytes(),
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
const UNPORTED: [(&str, &str); 9] = [
    ("GET", "/?rest_route=/"),
    ("GET", "/?rest_route=0/"),
    ("GET", "/?rest_route=/wp/v2"),
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
        content: content::default_install(),
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

/// Requests on the content routes PHP answers with what is not ported, or
/// where it writes.
const CONTENT_UNPORTED: [&str; 1] = ["/?rest_route=/wp/v2/categories/68&_embed=up"];

/// The content routes answer as PHP does over the default install and, by
/// hash, the Theme Test Data fixture, but those not ported.
#[test]
fn content_answers_match_php() {
    use sha2::Digest;
    let mut failures = Vec::new();
    let goldens = [
        (
            content::default_install(),
            include_str!("../tests/golden/content.tsv"),
            false,
        ),
        (
            content::theme_test_data(),
            include_str!("../tests/golden/content-ttd.tsv"),
            true,
        ),
    ];
    for (content, golden, hashed) in goldens {
        for case in cases_of(golden) {
            let served = serve_over(content, &case);
            let what = format!(
                "{} {} {} {} {hashed}",
                case.who, case.method, case.target, case.flags
            );
            if served.status == 501 {
                let unported = case.method != "GET" && case.method != "HEAD"
                    || CONTENT_UNPORTED.contains(&case.target.as_str());
                if !unported || !served.body.is_empty() {
                    failures.push(format!("{what}: 501"));
                }
                continue;
            }
            let body = if hashed {
                let digest = sha2::Sha256::digest(&served.body);
                digest
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
                    .into_bytes()
            } else {
                served.body.clone()
            };
            let mut php = case.headers.clone();
            php.sort();
            if served.status != case.status || kept(&served.headers) != php || body != case.body {
                failures.push(format!(
                    "{what}\n  php:  {} {php:?}\n       {}\n  ours: {} {:?}\n       {}",
                    case.status,
                    String::from_utf8_lossy(&case.body),
                    served.status,
                    kept(&served.headers),
                    String::from_utf8_lossy(&served.body)
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The arguments of every ported route use only keywords `hyscribe_rest`
/// checks.
#[test]
fn arguments_use_ported_keywords() {
    use hyscribe_phpser::json_encode;
    let mut seen = 0;
    for route in routes::regexes() {
        for endpoint in endpoints(route).unwrap_or_default() {
            for arg in endpoint.args {
                let json = String::from_utf8(json_encode(&arg.schema).unwrap()).unwrap();
                for schema in [Some(&arg.schema), arg.schema.get_str("items")]
                    .into_iter()
                    .flatten()
                {
                    let checked = schema::validate_value_from_schema(&Value::Null, schema, "p");
                    assert_ne!(checked, Err(schema::Refusal(None)), "{route} {json}");
                }
                seen += 1;
            }
        }
    }
    assert!(seen > 30);
}

/// The default install with `options` changed.
fn changed(options: &[(&str, &[u8])]) -> &'static Content {
    let default = content::default_install();
    let mut changed = default.options.clone();
    for (name, value) in options {
        changed.insert((*name).into(), value.to_vec());
    }
    Box::leak(Box::new(Content {
        options: changed,
        terms: Vec::new(),
        relationships: Vec::new(),
        posts: Vec::new(),
        usermeta: default.usermeta.clone(),
    }))
}

fn get(content: &'static Content, who: &str, target: &str) -> Served {
    let case = Case {
        who: who.into(),
        method: "GET".into(),
        target: target.into(),
        flags: String::new(),
        status: 0,
        headers: Vec::new(),
        body: Vec::new(),
    };
    serve_over(content, &case)
}

/// Where the category hierarchy is not stored, as on the default install,
/// `_get_term_hierarchy()` writes it for a parent's terms or the terms with
/// posts: those requests are answered 501.
#[test]
fn categories_that_would_write_are_not_served() {
    let requests = [
        "/?rest_route=/wp/v2/categories&hide_empty=1",
        "/?rest_route=/wp/v2/categories&parent=39",
    ];
    let stored = changed(&[("category_children", b"a:0:{}")]);
    let not_array = changed(&[("category_children", b"x")]);
    for target in requests {
        assert_eq!(
            get(content::default_install(), "visitor", target).status,
            501
        );
        assert_eq!(get(not_array, "visitor", target).status, 501);
        assert_eq!(get(stored, "visitor", target).status, 200);
    }
    let tags = "/?rest_route=/wp/v2/tags&hide_empty=1";
    assert_eq!(get(content::default_install(), "visitor", tags).status, 200);
}

/// Settings as `get_option()` reads them, serialized or not, and as their
/// schemas take them in `prepare_value()`: a value a schema does not take
/// is null.
#[test]
fn settings_are_read_as_get_option_reads_them() {
    let content = changed(&[
        ("use_smilies", b"b:0;"),
        ("posts_per_page", b"i:5;"),
        ("default_category", b"a:1:{x}"),
        ("start_of_week", b"a:1:{i:0;i:1;}"),
    ]);
    let served = get(content, "administrator", "/?rest_route=/wp/v2/settings");
    let data = hyscribe_phpser::json_decode(&served.body).unwrap();
    let field = |name| data.get_str(name).cloned();
    assert_eq!(field("use_smilies"), Some(Value::Bool(false)));
    assert_eq!(field("posts_per_page"), Some(Value::Int(5)));
    assert_eq!(field("default_category"), Some(Value::Null));
    assert_eq!(field("start_of_week"), Some(Value::Null));
}

/// `rest_parse_embed_param()`: an empty `_embed`, `1` or `true` embeds
/// every embeddable link, a list those of its relations.
#[test]
fn embedding_is_not_ported() {
    let mut response = Response::new(Value::Array(Vec::new()), 200);
    response.add_link_with("up", "u".into(), vec![("embeddable", Value::Bool(true))]);
    response.add_link("about", "a".into());
    for (embed, embeds) in [
        ("", true),
        ("1", true),
        ("true", true),
        ("up", true),
        ("about", false),
        ("x", false),
    ] {
        assert_eq!(
            super::embeds(&response, &Value::String(embed.into())),
            embeds,
            "{embed}"
        );
    }
}

/// Categories under a stored hierarchy: with `hide_empty`, an empty term
/// with a term under it that is not empty stays (`get_terms()`), and a
/// parent that is not there gives no `up` link.
#[test]
fn empty_categories_with_posts_under_them_stay() {
    let row = |id, parent, count| content::Row {
        taxonomy: "category".into(),
        term: Term {
            id,
            name: format!("c{id}").into_bytes(),
            slug: format!("c{id}").into_bytes(),
            parent,
            count,
            ..Term::default()
        },
        group: 0,
        taxonomy_id: id,
    };
    let default = content::default_install();
    let mut options = default.options.clone();
    options.insert(
        "category_children".into(),
        b"a:1:{i:1;a:1:{i:0;i:2;}}".to_vec(),
    );
    let content: &'static Content = Box::leak(Box::new(Content {
        options,
        terms: vec![row(1, 0, 0), row(2, 1, 1), row(3, 99, 0)],
        relationships: Vec::new(),
        posts: Vec::new(),
        usermeta: default.usermeta.clone(),
    }));
    let ids = |target| {
        let served = get(content, "visitor", target);
        let data = hyscribe_phpser::json_decode(&served.body).unwrap();
        let items = data.as_array().unwrap_or_default().to_vec();
        items
            .iter()
            .map(|(_, item)| {
                (
                    item.get_str("id").unwrap().to_int(),
                    item.get_path(&["_links", "up"]).is_some(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        ids("/?rest_route=/wp/v2/categories&hide_empty=1"),
        [(1, false), (2, true)]
    );
    assert_eq!(
        ids("/?rest_route=/wp/v2/categories"),
        [(1, false), (2, true), (3, false)]
    );
    assert_eq!(ids("/?rest_route=/wp/v2/categories&parent=1"), [(2, true)]);
    // A parent the stored hierarchy does not list has no terms.
    let mut options = content.options.clone();
    options.insert(
        "category_children".into(),
        b"a:1:{i:5;a:1:{i:0;i:6;}}".to_vec(),
    );
    let stale: &'static Content = Box::leak(Box::new(Content {
        options,
        terms: vec![row(1, 0, 0), row(2, 1, 1)],
        relationships: Vec::new(),
        posts: Vec::new(),
        usermeta: default.usermeta.clone(),
    }));
    let served = get(stale, "visitor", "/?rest_route=/wp/v2/categories&parent=1");
    assert_eq!(served.body, b"[]");
}

/// Only a flat taxonomy's collection registers `offset`, an integer; a
/// taxonomy's fields are kept as `in_array()` finds them in `_fields`, so
/// a field under one leaves it out; `parent=0` asks for no hierarchy.
#[test]
fn arguments_are_registered_as_php_registers_them() {
    let default = content::default_install();
    assert_eq!(
        get(default, "visitor", "/?rest_route=/wp/v2/tags&offset=x").status,
        400
    );
    assert_eq!(
        get(
            default,
            "visitor",
            "/?rest_route=/wp/v2/categories&offset=x"
        )
        .status,
        200
    );
    let target = "/?rest_route=/wp/v2/taxonomies/category&_fields=labels.name,slug&context=edit";
    let body = get(default, "administrator", target).body;
    assert_eq!(body, br#"{"slug":"category"}"#);
    let parent = get(
        default,
        "visitor",
        "/?rest_route=/wp/v2/categories&parent=0",
    );
    assert!(String::from_utf8_lossy(&parent.body).contains("uncategorized"));
}

/// A `self` link has `targetHints` only to a route ported, whose methods the
/// user may use.
#[test]
fn self_links_hint_what_their_route_allows() {
    let oracle = Oracle {
        content: content::default_install(),
        pretty: false,
    };
    let context = Context {
        site: &oracle,
        user: 0,
        head: false,
        uri: b"/",
        get: &[],
    };
    for route in ["/nope", "/wp/v2/media", "/wp/v2/block-types/core/paragraph"] {
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

// SPDX-License-Identifier: AGPL-3.0-or-later

//! The PHP oracle's answers in `tests/golden/rest.tsv`, replayed for each
//! role over the default install's roles, with `%nonce%` as each user's
//! nonce.

use std::sync::OnceLock;

use hyscribe_auth::caps::{CapPost, CapabilityData, User};
use hyscribe_phpser::{json_encode, parse_str};
use hyscribe_render::theme::twentytwentyfive;
use hyscribe_testkit::{RestCase, rest_cases, rest_headers};
use sha2::Digest;

use super::*;

mod site;

const SITE: &str = "http://127.0.0.1:8081";

/// The oracle with one user of `role`, over a fixture, without the
/// capabilities `deny` names.
struct Oracle {
    role: &'static str,
    fixture: site::Fixture,
    deny: &'static [&'static str],
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
        self.fixture.option(name)
    }
    fn user_meta(&self, _: u64, key: &str) -> Option<Vec<u8>> {
        let role = self.role;
        (key == "wp_capabilities")
            .then(|| format!("a:1:{{s:{}:\"{role}\";b:1;}}", role.len()).into_bytes())
    }
    fn user_exists(&self, _: u64) -> bool {
        true
    }
    fn post(&self, id: u64) -> Option<CapPost> {
        self.fixture.post(id).map(|post| CapPost {
            id: post.id,
            author: post.author,
            status: post.status,
            post_type: post.post_type,
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
        let mut taxonomies = ["category", "post_tag"].into_iter();
        let found = taxonomies.find(|taxonomy| self.fixture.term(id, taxonomy).is_some());
        found.map(String::from)
    }
    fn constant(&self, _: &str) -> bool {
        false
    }
}

impl Site for Oracle {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        self.fixture.option(name)
    }
    fn rest_url(&self, path: &str) -> String {
        if self.fixture.pretty {
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
        let denied = |cap: &str| self.deny.contains(&cap);
        user != 0 && !denied(cap) && User::load(self, "wp_", user).has_cap(self, cap, args)
    }
    fn reads(&self) -> &dyn Reads {
        &self.fixture
    }
    fn theme(&self) -> &Theme {
        static THEME: OnceLock<Theme> = OnceLock::new();
        THEME.get_or_init(twentytwentyfive)
    }
}

fn cases() -> Vec<RestCase> {
    rest_cases(include_str!("../tests/golden/rest.tsv"), false)
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

/// `case` served by HyScribe's API, over the Theme Test Data fixture when
/// `ttd`.
fn serve_case(case: &RestCase, ttd: bool) -> Served {
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
    let flag = |name: &str| case.flag(name);
    let oracle = Oracle {
        role,
        fixture: site::Fixture {
            ttd,
            pretty: flag("pretty"),
            extra: false,
        },
        deny: &[],
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
        let served = serve_case(case, false);
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
        assert_eq!(rest_headers(&served.headers), php, "{what}");
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
        fixture: site::Fixture {
            ttd: false,
            pretty: false,
            extra: false,
        },
        deny: &[],
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
        assert_eq!(rest_headers(&response.headers), php);
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
        rest_headers(&response.headers),
        ["x-wp-total: 1", "x-wp-totalpages: 1"]
    );
}

/// Cases of the Theme Test Data golden HyScribe answers 501: posts with
/// core's shortcodes, and for a logged in user the comment blocks.
const TTD_UNPORTED: [(&str, &str); 3] = [
    ("*", "/?rest_route=/wp/v2/posts/555"),
    ("*", "/?rest_route=/wp/v2/posts/555&excerpt_length=5"),
    ("user", "/?rest_route=/wp/v2/posts/51"),
];

/// Whether `case` is one of `unported`.
fn unported_case(unported: &[(&str, &str)], case: &RestCase) -> bool {
    unported.iter().any(|(who, target)| {
        *target == case.target && (*who == "*" || *who == "user" && case.who != "visitor")
    })
}

/// Every case of `golden` over a fixture, bodies whole or, for the Theme
/// Test Data fixture, by their sha256, but those `unported`, answered 501:
/// `(who, target)`, who `*` for anyone or `user` for any logged in user.
fn replay(golden: &str, ttd: bool, count: usize, unported: &[(&str, &str)]) {
    let cases = rest_cases(golden, ttd);
    assert_eq!(cases.len(), count);
    for case in &cases {
        let served = serve_case(case, ttd);
        let what = format!("{} {} {}", case.who, case.method, case.target);
        if unported_case(unported, case) {
            assert_eq!(served.status, 501, "{what}");
            continue;
        }
        assert_eq!(served.status, case.status, "{what}");
        let mut php = case.headers.clone();
        php.sort();
        assert_eq!(rest_headers(&served.headers), php, "{what}");
        let body = if ttd {
            let digest = sha2::Sha256::digest(&served.body);
            digest
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
                .into_bytes()
        } else {
            served.body
        };
        assert_eq!(
            String::from_utf8_lossy(&body),
            String::from_utf8_lossy(&case.body),
            "{what}"
        );
    }
}

/// The post routes over the default install, for every role, and the
/// statuses: `reference/rest/posts.tsv`, which `hyscribe-http` replays over
/// the database too.
#[test]
fn posts_match_php() {
    replay(
        include_str!("../tests/golden/posts.tsv"),
        false,
        6 * 89,
        &[],
    );
}

/// The post routes over the Theme Test Data fixture:
/// `reference/rest/posts-ttd.tsv`.
#[test]
fn theme_test_data_posts_match_php() {
    replay(
        include_str!("../tests/golden/posts-ttd.tsv"),
        true,
        6 * 39,
        &TTD_UNPORTED,
    );
}

/// An embedded collection is asked for the most its `per_page` allows, and
/// an `href` embedded twice is answered once.
#[test]
fn embedded_collections_are_asked_for_their_most() {
    assert_eq!(per_page_maximum("/wp/v2/posts", 0), Some(100));
    assert_eq!(per_page_maximum("/wp/v2/posts/(?P<id>[\\d]+)", 0), None);
    assert_eq!(per_page_maximum("/wp/v2/types", 0), None);
    let oracle = Oracle {
        role: "",
        fixture: site::Fixture {
            ttd: false,
            pretty: false,
            extra: false,
        },
        deny: &[],
    };
    let context = Context {
        site: &oracle,
        user: 0,
        head: false,
        uri: b"/",
        get: &[],
    };
    let mut cache = Vec::new();
    let href = format!("{SITE}/index.php?rest_route=/wp/v2/posts");
    let first = links::embedded_response(&context, href.as_bytes(), &mut cache);
    let posts = first.ok().flatten().unwrap();
    assert_eq!(posts.as_array().unwrap().len(), 1);
    cache[0].1 = Value::Null;
    let again = links::embedded_response(&context, href.as_bytes(), &mut cache);
    assert_eq!(again.ok().flatten(), Some(Value::Null));
    let elsewhere = links::embedded_response(&context, b"http://other.test/x", &mut cache);
    assert_eq!(elsewhere.ok().flatten(), None);
    let viewed = format!("{href}&context=view&per_page=5&_fields=id");
    let viewed = links::embedded_response(&context, viewed.as_bytes(), &mut cache);
    assert_eq!(
        json_encode(&viewed.ok().flatten().unwrap()).unwrap(),
        b"[{\"id\":1}]"
    );
}

/// Writes answer 501, as do arguments whose schemas use keywords not
/// ported, and content PHP would write the oEmbed or feed caches of or
/// that uses the duotone filter: `(fixture is TTD, who, method, target)`.
#[test]
fn writes_and_unported_content_answer_501() {
    let requests = [
        (false, "administrator", "POST", "/?rest_route=/wp/v2/posts"),
        (false, "administrator", "PUT", "/?rest_route=/wp/v2/posts/1"),
        (false, "visitor", "DELETE", "/?rest_route=/wp/v2/pages/2"),
        (
            false,
            "visitor",
            "GET",
            "/?rest_route=/wp/v2/posts&categories=1",
        ),
        (
            false,
            "editor",
            "GET",
            "/?rest_route=/wp/v2/posts&after=2026-01-01T00:00:00",
        ),
        (true, "visitor", "GET", "/?rest_route=/wp/v2/posts/582"),
        (
            true,
            "visitor",
            "GET",
            "/?rest_route=/wp/v2/posts/34&context=embed",
        ),
        (true, "visitor", "GET", "/?rest_route=/wp/v2/posts/21"),
        (true, "editor", "GET", "/?rest_route=/wp/v2/posts/51"),
        (
            true,
            "editor",
            "GET",
            "/?rest_route=/wp/v2/pages&include=1133",
        ),
    ];
    for (ttd, who, method, target) in requests {
        let case = RestCase {
            who: who.into(),
            method: method.into(),
            target: target.into(),
            flags: String::new(),
            status: 501,
            headers: Vec::new(),
            body: Vec::new(),
        };
        assert_eq!(
            serve_case(&case, ttd).status,
            501,
            "{who} {method} {target}"
        );
    }
}

/// A post with revisions links its history with their count and its
/// newest, as `prepare_links()` adds them.
#[test]
fn revisions_are_linked() {
    let oracle = Oracle {
        role: "",
        fixture: site::Fixture {
            ttd: false,
            pretty: false,
            extra: true,
        },
        deny: &[],
    };
    let get = [(
        Key::String(b"_fields".to_vec()),
        Value::String(b"_links".to_vec()),
    )];
    let request = Request {
        method: "GET",
        route: b"/wp/v2/posts/1",
        uri: b"/",
        get: &get,
        post: &[],
        cookie_user: 0,
        origin: None,
        nonce: None,
        method_override: None,
    };
    let body = String::from_utf8(serve(&oracle, &request).body).unwrap();
    let base =
        "http:\\/\\/127.0.0.1:8081\\/index.php?rest_route=\\/wp\\/v2\\/posts\\/1\\/revisions";
    assert!(
        body.contains(&format!(
            "\"version-history\":[{{\"count\":1,\"href\":\"{base}\"}}]"
        )),
        "{body}"
    );
    assert!(
        body.contains(&format!(
            "\"predecessor-version\":[{{\"id\":99,\"href\":\"{base}\\/99\"}}]"
        )),
        "{body}"
    );
}

/// `request` of `who` over the default install, with the extra posts, and
/// the capabilities `deny` names left out.
fn served(who: &'static str, target: &str, ttd: bool, deny: &'static [&'static str]) -> Served {
    let (route, query) = target.split_once('&').unwrap_or((target, ""));
    let get = parse_str(query.as_bytes())
        .as_array()
        .unwrap_or_default()
        .to_vec();
    let user = ROLES
        .iter()
        .find(|(role, _)| *role == who)
        .map_or(0, |(_, id)| *id);
    let oracle = Oracle {
        role: who,
        fixture: site::Fixture {
            ttd,
            pretty: false,
            extra: !ttd,
        },
        deny,
    };
    let request = Request {
        method: "GET",
        route: route.as_bytes(),
        uri: b"/",
        get: &get,
        post: &[],
        cookie_user: user,
        origin: None,
        nonce: (user != 0).then_some(b"%nonce%"),
        method_override: None,
    };
    serve(&oracle, &request)
}

fn allow(served: &Served) -> &str {
    let allow = served.headers.iter().find(|(name, _)| name == "Allow");
    allow.map_or("", |(_, value)| value.as_str())
}

/// The write permission checks behind `Allow` and `targetHints`, for an
/// administrator who may not edit others' posts nor assign terms: another
/// author, a sticky change or a term rules updates out.
#[test]
fn update_checks_rule_out_writes() {
    const DENY: &[&str] = &["edit_others_posts", "assign_term"];
    for query in ["&author=2", "&sticky=1", "&categories=1"] {
        let served = served(
            "administrator",
            &format!("/wp/v2/posts/1{query}"),
            false,
            DENY,
        );
        assert_eq!(allow(&served), "GET, DELETE", "{query}");
    }
    let served = served("administrator", "/wp/v2/posts/1&categories=1", false, &[]);
    assert_eq!(allow(&served), "GET, POST, PUT, PATCH, DELETE");
}

/// `sticky=1` keeps the sticky posts of `include`.
#[test]
fn sticky_posts_within_include() {
    let served = served(
        "visitor",
        "/wp/v2/posts&sticky=1&include=1241,1&_fields=id",
        true,
        &[],
    );
    assert_eq!(String::from_utf8(served.body).unwrap(), "[{\"id\":1241}]");
}

/// A draft's unset GMT dates come from its local ones; a post without an
/// author links none; a featured image shows as its parent's status, or
/// published without one, to those who may read it.
#[test]
fn drafts_show_their_gmt_dates() {
    let target = "/wp/v2/posts/98&context=edit&_fields=date_gmt,modified_gmt,_links";
    let body = String::from_utf8(served("administrator", target, false, &[]).body).unwrap();
    let dates = "{\"date_gmt\":\"2026-11-02T10:00:00\",\"modified_gmt\":\"2026-11-02T10:00:00\"";
    assert!(body.starts_with(dates), "{body}");
    assert!(!body.contains("\"author\":"), "{body}");
    let media = |id: u64| {
        let href =
            format!("http:\\/\\/127.0.0.1:8081\\/index.php?rest_route=\\/wp\\/v2\\/media\\/{id}");
        format!("\"wp:featuredmedia\":[{{\"embeddable\":true,\"href\":\"{href}\"}}]")
    };
    assert!(body.contains(&media(97)), "{body}");
    let other = served("editor", "/wp/v2/posts/96&_fields=_links", false, &[]);
    let body = String::from_utf8(other.body).unwrap();
    assert!(body.contains(&media(95)), "{body}");
}

/// Actions a user's capabilities rule out are not linked.
#[test]
fn actions_follow_capabilities() {
    const DENY: &[&str] = &[
        "publish_posts",
        "unfiltered_html",
        "edit_others_posts",
        "edit_categories",
        "assign_categories",
        "assign_post_tags",
    ];
    let target = "/wp/v2/posts/1&context=edit&_fields=_links";
    let body = String::from_utf8(served("administrator", target, false, DENY).body).unwrap();
    assert!(
        body.contains("\"wp:term\"") && !body.contains("action-"),
        "{body}"
    );
}

/// Embedding a route not ported answers 501, enveloped or not; an error
/// has no links to embed.
#[test]
fn embedding_what_is_not_ported() {
    let targets = [
        "/wp/v2/posts/1&_embed",
        "/wp/v2/posts&_embed",
        "/wp/v2/posts/1&_embed&_envelope",
    ];
    for target in targets {
        assert_eq!(
            served("visitor", target, false, &[]).status,
            501,
            "{target}"
        );
    }
    let missing = served("visitor", "/wp/v2/posts/999", false, &[]);
    let embedded = served("visitor", "/wp/v2/posts/999&_embed", false, &[]);
    assert_eq!(
        (embedded.status, embedded.body),
        (missing.status, missing.body)
    );
}

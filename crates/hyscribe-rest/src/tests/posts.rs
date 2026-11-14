// SPDX-License-Identifier: AGPL-3.0-or-later

//! The post routes' goldens, `posts.tsv` and `posts-ttd.tsv`, replayed for
//! a visitor and each role over the dumps' posts, and what the goldens
//! cannot show.

use std::sync::OnceLock;

use hyscribe_auth::caps::{CapPost, CapabilityData, User};
use hyscribe_phpser::parse_str;
use hyscribe_render::theme::twentytwentyfive;
use hyscribe_testkit::{RestCase, rest_cases, rest_headers};
use sha2::Digest;

use super::{SITE, site};
use crate::*;

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
    fn terms(&self, _: &TermQuery) -> Vec<Term> {
        Vec::new()
    }
    fn count_terms(&self, _: &TermQuery) -> u64 {
        0
    }
    fn term_link(&self, _: &str, _: &Term) -> String {
        String::new()
    }
    fn post(&self, id: u64) -> Option<Post> {
        self.fixture.post(id)
    }
    fn reads(&self) -> &dyn Reads {
        &self.fixture
    }
    fn theme(&self) -> &Theme {
        static THEME: OnceLock<Theme> = OnceLock::new();
        THEME.get_or_init(twentytwentyfive)
    }
}

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

const TTD_UNPORTED: [(&str, &str); 5] = [
    ("*", "/?rest_route=/wp/v2/pages/172&_embed=up"),
    (
        "*",
        "/?rest_route=/wp/v2/pages&parent=1725&_embed=up&exclude=1133",
    ),
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
        include_str!("../../tests/golden/posts.tsv"),
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
        include_str!("../../tests/golden/posts-ttd.tsv"),
        true,
        6 * 39,
        &TTD_UNPORTED,
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

/// A status asked for twice counts once, and one of the sample
/// permalink's fields comes alone.
#[test]
fn statuses_and_fields_once() {
    let twice = served(
        "editor",
        "/wp/v2/pages&status=draft,Draft&_fields=id",
        false,
        &[],
    );
    let once = served("editor", "/wp/v2/pages&status=draft&_fields=id", false, &[]);
    assert_eq!(twice.body, once.body);
    let target = "/wp/v2/posts/1&context=edit&_fields=generated_slug";
    let slug = served("administrator", target, false, &[]);
    assert_eq!(
        String::from_utf8(slug.body).unwrap(),
        "{\"generated_slug\":\"hello-world\"}"
    );
}

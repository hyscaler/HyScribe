// SPDX-License-Identifier: AGPL-3.0-or-later

//! The PHP oracle's answers to the post routes (`reference/rest/posts.tsv`
//! and `posts-ttd.tsv`), replayed over the fixture databases for a visitor
//! and each role, as `reference/rest/users.sql` gives the users their
//! roles; `%nonce%` is each user's nonce.

use hyscribe_db::Db;
use hyscribe_testkit::{RestCase, rest_cases, rest_headers};
use sha2::{Digest, Sha256};

use super::super::*;
use crate::tests::fixture_config;

/// A user of `role`, or the fixture's own user for the administrator.
struct AsRole<'a> {
    rest: Rest<'a>,
    role: &'static str,
}

const ROLES: [(&str, u64); 5] = [
    ("administrator", 1),
    ("editor", 701),
    ("author", 702),
    ("contributor", 703),
    ("subscriber", 704),
];

impl CapabilityData for AsRole<'_> {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        CapabilityData::option(&self.rest, name)
    }
    fn user_meta(&self, user_id: u64, key: &str) -> Option<Vec<u8>> {
        let role = self.role;
        match key {
            "wp_capabilities" if user_id > 1 => {
                Some(format!("a:1:{{s:{}:\"{role}\";b:1;}}", role.len()).into_bytes())
            }
            _ => self.rest.user_meta(user_id, key),
        }
    }
    fn user_exists(&self, user_id: u64) -> bool {
        user_id > 1 || self.rest.user_exists(user_id)
    }
    fn post(&self, id: u64) -> Option<CapPost> {
        self.rest.post(id)
    }
    fn post_meta(&self, id: u64, key: &str) -> Option<Vec<u8>> {
        self.rest.post_meta(id, key)
    }
    fn comment_post(&self, id: u64) -> Option<u64> {
        self.rest.comment_post(id)
    }
    fn term_taxonomy(&self, id: u64) -> Option<String> {
        self.rest.term_taxonomy(id)
    }
    /// The oracle leaves `DISALLOW_UNFILTERED_HTML` undefined, which
    /// HyScribe takes as defined until an open question is decided.
    fn constant(&self, name: &str) -> bool {
        name != "DISALLOW_UNFILTERED_HTML" && self.rest.constant(name)
    }
}

impl hyscribe_rest::Site for AsRole<'_> {
    fn terms(&self, query: &TermQuery) -> Vec<Term> {
        self.rest.terms(query)
    }
    fn count_terms(&self, query: &TermQuery) -> u64 {
        self.rest.count_terms(query)
    }
    fn term_link(&self, taxonomy: &str, term: &Term) -> String {
        self.rest.term_link(taxonomy, term)
    }
    fn post(&self, id: u64) -> Option<Post> {
        hyscribe_rest::Site::post(&self.rest, id)
    }
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        hyscribe_rest::Site::option(&self.rest, name)
    }
    fn rest_url(&self, path: &str) -> String {
        self.rest.rest_url(path)
    }
    fn verify_nonce(&self, nonce: &[u8]) -> bool {
        nonce == b"%nonce%"
    }
    fn create_nonce(&self) -> String {
        "%nonce%".into()
    }
    fn can(&self, user: u64, cap: &str, args: &[Value]) -> bool {
        user != 0 && Capabilities::load(self, "wp_", user).has_cap(self, cap, args)
    }
    fn reads(&self) -> &dyn RenderSite {
        self.rest.reads()
    }
    fn theme(&self) -> &hyscribe_render::theme::Theme {
        self.rest.theme()
    }
}

/// The site of the fixture `database`, as the oracle sees it, under
/// `/%postname%/` permalinks when `pretty`.
async fn site(database: &str, pretty: bool) -> Site {
    let mut config = fixture_config();
    config.name = database.into();
    let db = Db::connect(&config)
        .await
        .expect(hyscribe_testkit::NO_FIXTURE_DATABASE);
    let home = Some("http://127.0.0.1:8081".to_string());
    let mut site = Site::load(db, home.clone(), home).await.unwrap();
    if pretty {
        site.options
            .insert("permalink_structure".into(), b"/%postname%/".to_vec());
        site.build_rewriter();
    }
    site
}

/// `case` served by HyScribe's API over `site`.
fn serve_case(site: &Site, handle: &Handle, case: &RestCase) -> hyscribe_rest::Served {
    let (path, query) = case.target.split_once('?').unwrap_or((&case.target, ""));
    let get = parse_str(query.as_bytes())
        .as_array()
        .unwrap_or_default()
        .to_vec();
    let route = match hyscribe_phpser::lookup(&get, "rest_route") {
        Some(route) => route.to_php_string(),
        None => path.strip_prefix("/wp-json").unwrap().as_bytes().to_vec(),
    };
    let (role, user) = ROLES
        .iter()
        .find(|(role, _)| *role == case.who)
        .copied()
        .unwrap_or(("", 0));
    let rest = Rest {
        reads: LoginReads::new(site, handle.clone(), now()),
        site,
        user,
        caps: OnceCell::new(),
    };
    let as_role = AsRole { rest, role };
    let request = hyscribe_rest::Request {
        method: &case.method,
        route: &route,
        uri: case.target.as_bytes(),
        get: &get,
        post: &[],
        cookie_user: user,
        origin: None,
        nonce: (user != 0).then_some(b"%nonce%"),
        method_override: None,
    };
    hyscribe_rest::serve(&as_role, &request)
}

/// Every case of `golden` over `database`, its bodies compared whole or by
/// their sha256 when `hashed`, but those `unported`, answered 501.
async fn replay(
    database: &str,
    golden: &'static str,
    hashed: bool,
    count: usize,
    unported: &'static [(&'static str, &'static str)],
) {
    let plain = site(database, false).await;
    let pretty = site(database, true).await;
    let handle = Handle::current();
    tokio::task::spawn_blocking(move || {
        let cases = rest_cases(golden, hashed);
        assert_eq!(cases.len(), count);
        for case in &cases {
            let site = if case.flag("pretty") { &pretty } else { &plain };
            let served = serve_case(site, &handle, case);
            let what = format!("{} {} {}", case.who, case.method, case.target);
            if unported.iter().any(|(who, target)| {
                *target == case.target && (*who == "*" || *who == "user" && case.who != "visitor")
            }) {
                assert_eq!(served.status, 501, "{what}");
                assert!(served.headers.is_empty() && served.body.is_empty());
                continue;
            }
            assert_eq!(served.status, case.status, "{what}");
            let mut php = case.headers.clone();
            php.sort();
            assert_eq!(rest_headers(&served.headers), php, "{what}");
            let body = if hashed {
                let digest = Sha256::digest(&served.body);
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
    })
    .await
    .unwrap();
}

/// Requests HyScribe answers 501 that PHP would answer, not recorded as
/// PHP writes on them or they are writes: `(who, method, target)`.
async fn unported(database: &str, requests: &'static [(&str, &str, &str)]) {
    let site = site(database, false).await;
    let handle = Handle::current();
    tokio::task::spawn_blocking(move || {
        for (who, method, target) in requests {
            let case = RestCase {
                who: (*who).into(),
                method: (*method).into(),
                target: (*target).into(),
                flags: String::new(),
                status: 501,
                headers: Vec::new(),
                body: Vec::new(),
            };
            let served = serve_case(&site, &handle, &case);
            assert_eq!(served.status, 501, "{who} {method} {target}");
        }
    })
    .await
    .unwrap();
}

/// Writes answer 501, as do the arguments whose schemas use keywords not
/// ported (`format`, `oneOf`).
#[tokio::test]
async fn posts_match_php() {
    let golden = include_str!("../../../../hyscribe-rest/tests/golden/posts.tsv");
    replay("wordpress", golden, false, 6 * 89, &[]).await;
    unported(
        "wordpress",
        &[
            ("administrator", "POST", "/?rest_route=/wp/v2/posts"),
            ("administrator", "PUT", "/?rest_route=/wp/v2/posts/1"),
            ("visitor", "DELETE", "/?rest_route=/wp/v2/pages/2"),
            ("visitor", "GET", "/?rest_route=/wp/v2/posts&categories=1"),
            (
                "editor",
                "GET",
                "/?rest_route=/wp/v2/posts&after=2026-01-01T00:00:00",
            ),
        ],
    )
    .await;
}

/// Over the Theme Test Data fixture, posts with core's shortcodes or the
/// duotone filter are not ported, and those PHP writes the oEmbed or feed
/// caches of are answered 501 too.
#[tokio::test]
async fn theme_test_data_posts_match_php() {
    let golden = include_str!("../../../../hyscribe-rest/tests/golden/posts-ttd.tsv");
    let skipped = &[
        ("*", "/?rest_route=/wp/v2/pages/172&_embed=up"),
        (
            "*",
            "/?rest_route=/wp/v2/pages&parent=1725&_embed=up&exclude=1133",
        ),
        ("*", "/?rest_route=/wp/v2/posts/555"),
        ("*", "/?rest_route=/wp/v2/posts/555&excerpt_length=5"),
        ("user", "/?rest_route=/wp/v2/posts/51"),
    ];
    replay("wordpress_ttd", golden, true, 6 * 39, skipped).await;
    unported(
        "wordpress_ttd",
        &[
            ("visitor", "GET", "/?rest_route=/wp/v2/posts/582"),
            (
                "visitor",
                "GET",
                "/?rest_route=/wp/v2/posts/34&context=embed",
            ),
            ("visitor", "GET", "/?rest_route=/wp/v2/posts/21"),
            ("editor", "GET", "/?rest_route=/wp/v2/posts/51"),
            ("editor", "GET", "/?rest_route=/wp/v2/pages&include=1133"),
        ],
    )
    .await;
}

/// The routes ported check no comment's capabilities yet.
#[tokio::test]
async fn comments_are_not_read() {
    let site = site("wordpress", false).await;
    let handle = Handle::current();
    tokio::task::spawn_blocking(move || {
        let rest = Rest {
            reads: LoginReads::new(&site, handle, now()),
            site: &site,
            user: 1,
            caps: OnceCell::new(),
        };
        let as_role = AsRole {
            rest,
            role: "editor",
        };
        assert_eq!(as_role.comment_post(1), None);
    })
    .await
    .unwrap();
}

// SPDX-License-Identifier: AGPL-3.0-or-later

//! The PHP oracle's answers to the post routes (`reference/rest/posts.tsv`
//! and `posts-ttd.tsv`), replayed over the fixture databases for a visitor
//! and each role, as `reference/rest/users.sql` gives the users their
//! roles; `%nonce%` is each user's nonce.

use hyscribe_auth::caps::CapPost;
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
        CapabilityData::option(&self.rest.reads, name)
    }
    fn user_meta(&self, user_id: u64, key: &str) -> Option<Vec<u8>> {
        let role = self.role;
        match key {
            "wp_capabilities" if user_id > 1 => {
                Some(format!("a:1:{{s:{}:\"{role}\";b:1;}}", role.len()).into_bytes())
            }
            _ => CapabilityData::user_meta(&self.rest.reads, user_id, key),
        }
    }
    fn user_exists(&self, user_id: u64) -> bool {
        user_id > 1 || CapabilityData::user_exists(&self.rest.reads, user_id)
    }
    fn post(&self, id: u64) -> Option<CapPost> {
        CapabilityData::post(&self.rest.reads, id)
    }
    fn post_meta(&self, id: u64, key: &str) -> Option<Vec<u8>> {
        CapabilityData::post_meta(&self.rest.reads, id, key)
    }
    fn comment_post(&self, id: u64) -> Option<u64> {
        CapabilityData::comment_post(&self.rest.reads, id)
    }
    fn term_taxonomy(&self, id: u64) -> Option<String> {
        CapabilityData::term_taxonomy(&self.rest.reads, id)
    }
    fn constant(&self, name: &str) -> bool {
        CapabilityData::constant(&self.rest.reads, name)
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
    fn table_prefix(&self) -> String {
        self.rest.table_prefix()
    }
    fn comments(&self, query: &CommentQuery) -> Vec<CommentRow> {
        hyscribe_rest::Site::comments(&self.rest, query)
    }
    fn count_comments(&self, query: &CommentQuery) -> u64 {
        self.rest.count_comments(query)
    }
    fn comment_link(&self, comment: &Comment) -> String {
        self.rest.comment_link(comment)
    }
    fn users(&self, query: &UserQuery) -> Vec<User> {
        self.rest.users(query)
    }
    fn count_users(&self, query: &UserQuery) -> u64 {
        self.rest.count_users(query)
    }
    fn user_meta(&self, id: u64, key: &str) -> Option<Vec<u8>> {
        CapabilityData::user_meta(self, id, key)
    }
    fn user_registered(&self, id: u64) -> String {
        self.rest.user_registered(id)
    }
    fn allcaps(&self, id: u64) -> Vec<(Key, Value)> {
        self.rest.allcaps(id)
    }
    fn author_link(&self, user: &User) -> String {
        self.rest.author_link(user)
    }
    fn has_posts(&self, author: u64, types: &[(String, bool)]) -> bool {
        self.rest.has_posts(author, types)
    }
    fn wp_hash(&self, data: &[u8]) -> Option<String> {
        self.rest.wp_hash(data)
    }
    fn embed_secret(&self) -> String {
        self.rest.embed_secret()
    }
    fn posts_of_type(&self, post_type: &str) -> Vec<Post> {
        self.rest.posts_of_type(post_type)
    }
    fn revisions(&self, post: u64) -> Vec<u64> {
        self.rest.revisions(post)
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
    fn nonce(&self, _: &str) -> String {
        "%preview_nonce%".into()
    }
    fn can(&self, user: u64, cap: &str, args: &[Value]) -> bool {
        user != 0 && Capabilities::load(self, "wp_", user).has_cap(self, cap, args)
    }
    fn public_users(&self) -> bool {
        self.rest.public_users()
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
        origin: case.flag("origin").then_some(b"http://example.test"),
        nonce: (user != 0).then_some(b"%nonce%"),
        remote_addr: b"",
        user_agent: None,
        method_override: None,
        content_type: case.flag("json").then_some(b"application/json"),
        body: &case.sent,
        host: case.flag("localhost").then_some(b"localhost:8081"),
        now: now(),
    };
    hyscribe_rest::serve(&as_role, &request)
}

/// Every case of `golden` over `database`, its bodies compared whole or by
/// their sha256 when `hashed`, but those `unported` or flagged `refused`,
/// answered 501.
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
        let cases = rest_cases(golden);
        assert_eq!(cases.len(), count);
        for case in &cases {
            let site = if case.flag("pretty") { &pretty } else { &plain };
            let served = serve_case(site, &handle, case);
            let what = format!("{} {} {}", case.who, case.method, case.target);
            if case.flag("refused")
                || unported.iter().any(|(who, target)| {
                    *target == case.target
                        && (*who == "*" || *who == "user" && case.who != "visitor")
                })
            {
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
                sent: Vec::new(),
            };
            let served = serve_case(&site, &handle, &case);
            assert_eq!(served.status, 501, "{who} {method} {target}");
        }
    })
    .await
    .unwrap();
}

/// The index over the site's options and theme, as PHP gives it
/// (`reference/rest/index.list`), but where it keeps its routes.
#[tokio::test]
async fn index_matches_php() {
    let golden = include_str!("../../../../hyscribe-rest/tests/golden/index.tsv");
    replay("wordpress", golden, false, 6 * 41, &[]).await;
}

/// Writes answer 501, as do the arguments whose schemas use keywords not
/// ported (`format`), and term limits with children.
#[tokio::test]
async fn posts_match_php() {
    let golden = include_str!("../../../../hyscribe-rest/tests/golden/posts.tsv");
    replay("wordpress", golden, false, 6 * 103, &[]).await;
    unported(
        "wordpress",
        &[
            ("administrator", "POST", "/?rest_route=/wp/v2/posts"),
            ("administrator", "PUT", "/?rest_route=/wp/v2/posts/1"),
            (
                "visitor",
                "DELETE",
                "/?rest_route=/wp/v2/menu-items/1&force=1",
            ),
            (
                "visitor",
                "GET",
                "/?rest_route=/wp/v2/posts&categories[terms]=1&categories[include_children]=1",
            ),
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
        ("*", "/?rest_route=/wp/v2/posts/555"),
        ("*", "/?rest_route=/wp/v2/posts/555&excerpt_length=5"),
        ("user", "/?rest_route=/wp/v2/posts/51"),
    ];
    replay("wordpress_ttd", golden, true, 6 * 42, skipped).await;
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

/// The block renderer (MVP 4.4), but the blocks whose rendering here is
/// not PHP's, flagged `refused`.
#[tokio::test]
async fn block_renderer_matches_php() {
    let golden = include_str!("../../../../hyscribe-rest/tests/golden/renderer.tsv");
    replay("wordpress", golden, false, 6 * 45, &[]).await;
}

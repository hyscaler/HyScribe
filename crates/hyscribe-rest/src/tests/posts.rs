// SPDX-License-Identifier: AGPL-3.0-or-later

//! The post routes' goldens, `posts.tsv` and `posts-ttd.tsv`, replayed for
//! a visitor and each role over the dumps' posts, and what the goldens
//! cannot show.

use std::sync::OnceLock;

use hyscribe_auth::caps::{CapPost, CapabilityData, User};
use hyscribe_phpser::parse_str;
use hyscribe_render::theme::twentytwentyfive;
use hyscribe_testkit::{RestCase, rest_cases, rest_headers};

use super::{SITE, role, serve_with, sha256_hex, site};
use crate::posts::prepare::writes_or_unported;
use crate::*;
use hyscribe_model::{OrderBy, PostQuery, StatusFilter};

/// The oracle with one user of `role`, over a fixture, without the
/// capabilities `deny` names.
pub(super) struct Oracle {
    pub(super) role: &'static str,
    pub(super) fixture: site::Fixture,
    pub(super) deny: &'static [&'static str],
}

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
    fn post_meta(&self, id: u64, key: &str) -> Option<Vec<u8>> {
        self.fixture.post_meta(id, key)
    }
    fn comment_post(&self, _: u64) -> Option<u64> {
        None
    }
    fn term_taxonomy(&self, id: u64) -> Option<String> {
        let mut taxonomies = ["category", "post_tag"].into_iter();
        let found = taxonomies.find(|taxonomy| self.fixture.term(id, taxonomy).is_some());
        found.map(String::from)
    }
    /// HyScribe's `DISALLOW_UNFILTERED_HTML`, unless `deny` names it.
    fn constant(&self, name: &str) -> bool {
        name == "DISALLOW_UNFILTERED_HTML" && !self.deny.contains(&name)
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
    fn nonce(&self, _: &str) -> String {
        "%preview_nonce%".into()
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
    fn table_prefix(&self) -> String {
        "wp_".into()
    }
    fn comments(&self, _: &hyscribe_model::CommentQuery) -> Vec<hyscribe_model::CommentRow> {
        Vec::new()
    }
    fn count_comments(&self, _: &hyscribe_model::CommentQuery) -> u64 {
        0
    }
    fn comment_link(&self, _: &hyscribe_model::Comment) -> String {
        String::new()
    }
    fn users(&self, _: &hyscribe_model::UserQuery) -> Vec<hyscribe_model::User> {
        Vec::new()
    }
    fn count_users(&self, _: &hyscribe_model::UserQuery) -> u64 {
        0
    }
    fn user_meta(&self, id: u64, key: &str) -> Option<Vec<u8>> {
        CapabilityData::user_meta(self, id, key)
    }
    fn user_registered(&self, _: u64) -> String {
        String::new()
    }
    fn allcaps(&self, _: u64) -> Vec<(Key, Value)> {
        Vec::new()
    }
    fn author_link(&self, _: &hyscribe_model::User) -> String {
        String::new()
    }
    fn has_posts(&self, _: u64, _: &[(String, bool)]) -> bool {
        false
    }
    fn posts_of_type(&self, post_type: &str) -> Vec<Post> {
        let query = PostQuery {
            post_types: vec![post_type.into()],
            limit: u32::MAX,
            ..PostQuery::default()
        };
        self.fixture.query(&query)
    }
    fn revisions(&self, post: u64) -> Vec<u64> {
        let query = PostQuery {
            post_types: vec!["revision".into()],
            statuses: vec![StatusFilter {
                status: "inherit".into(),
                author: None,
            }],
            parents: vec![post],
            order: vec![(OrderBy::Date, false), (OrderBy::Id, false)],
            limit: u32::MAX,
            ..PostQuery::default()
        };
        self.fixture
            .query(&query)
            .iter()
            .map(|revision| revision.id)
            .collect()
    }
    fn public_users(&self) -> bool {
        true
    }
    fn reads(&self) -> &dyn Reads {
        &self.fixture
    }
    fn theme(&self) -> &Theme {
        static THEME: OnceLock<Theme> = OnceLock::new();
        THEME.get_or_init(twentytwentyfive)
    }
}

/// `case` served by HyScribe's API, over the Theme Test Data fixture when
/// `ttd`, with `reference/rest/media.sql` when `media`.
pub(super) fn serve_case(case: &RestCase, ttd: bool, media: bool) -> Served {
    let oracle = Oracle {
        role: role(&case.who).0,
        fixture: site::Fixture {
            ttd,
            pretty: case.flag("pretty"),
            extra: false,
            state: media.then(|| site::with_media(ttd)),
        },
        deny: &[],
    };
    serve_with(&oracle, case)
}

const TTD_UNPORTED: [(&str, &str); 3] = [
    ("*", "/?rest_route=/wp/v2/posts/555"),
    ("*", "/?rest_route=/wp/v2/posts/555&excerpt_length=5"),
    ("user", "/?rest_route=/wp/v2/posts/51"),
];

/// Whether `case` is one of `unported`.
fn unported_case(unported: &[(&str, &str)], case: &RestCase) -> bool {
    unported.iter().any(|(who, target)| {
        let user = *who == "user" && case.who != "visitor";
        *target == case.target && (*who == "*" || user || *who == case.who)
    })
}

/// Every case of `golden` over a fixture, bodies whole or, for the Theme
/// Test Data fixture, by their sha256, but those `unported`, answered 501:
/// `(who, target)`, who `*` for anyone, `user` for any logged in user or a
/// role.
pub(super) fn replay(
    golden: &str,
    (ttd, media): (bool, bool),
    count: usize,
    unported: &[(&str, &str)],
) {
    let cases = rest_cases(golden);
    assert_eq!(cases.len(), count);
    for case in &cases {
        let served = serve_case(case, ttd, media);
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
            sha256_hex(&served.body)
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
        (false, false),
        6 * 102,
        &[],
    );
}

/// The post routes over the Theme Test Data fixture:
/// `reference/rest/posts-ttd.tsv`.
#[test]
fn theme_test_data_posts_match_php() {
    replay(
        include_str!("../../tests/golden/posts-ttd.tsv"),
        (true, false),
        6 * 42,
        &TTD_UNPORTED,
    );
}

/// Writes answer 501 once allowed or given arguments, as do arguments of
/// keywords not ported and content PHP would write caches of or that uses
/// the duotone filter: `(fixture is TTD, who, method, target)`.
#[test]
fn writes_and_unported_content_answer_501() {
    let requests = [
        (false, "administrator", "POST", "/?rest_route=/wp/v2/posts"),
        (false, "administrator", "PUT", "/?rest_route=/wp/v2/posts/1"),
        (
            false,
            "visitor",
            "DELETE",
            "/?rest_route=/wp/v2/pages/2&force=1",
        ),
        (
            false,
            "visitor",
            "GET",
            "/?rest_route=/wp/v2/posts&categories[terms]=1&categories[include_children]=1",
        ),
        (
            false,
            "visitor",
            "GET",
            "/?rest_route=/wp/v2/posts&categories[terms]=1&categories[operator]=AND",
        ),
        (
            false,
            "visitor",
            "GET",
            "/?rest_route=/wp/v2/posts&categories=1&tags=2&tax_relation=OR",
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
            serve_case(&case, ttd, false).status,
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
            state: None,
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
        unread_body: false,
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
    let pretty = target.ends_with("#pretty");
    let target = target.trim_end_matches("#pretty");
    let (route, query) = target.split_once('&').unwrap_or((target, ""));
    let get = parse_str(query.as_bytes())
        .as_array()
        .unwrap_or_default()
        .to_vec();
    let (_, user) = role(who);
    let oracle = Oracle {
        role: who,
        fixture: site::Fixture {
            ttd,
            pretty,
            extra: !ttd,
            state: None,
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
        unread_body: false,
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
    let dates = "{\"date_gmt\":\"2026-11-02T08:00:00\",\"modified_gmt\":\"2026-11-02T08:00:00\"";
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
    let private = served("editor", "/wp/v2/posts/92&_fields=_links", false, &[]);
    assert!(body_of(private).contains(&media(93)));
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

/// Embedding a link answers; an error has no links to embed.
#[test]
fn embedding_links() {
    let target = "/wp/v2/posts/98&_embed=author";
    assert_eq!(served("administrator", target, false, &[]).status, 200);
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

fn body(served: Served) -> String {
    String::from_utf8(served.body).unwrap()
}

/// What only some data shows: sticky posts stay out of a list limited to
/// some posts, `_fields` asks for status fields by name, `private` is a
/// status for those who read private pages without editing them, and a
/// draft's sample permalink is a published post's.
#[test]
fn edges_of_the_lists() {
    let target = "/wp/v2/posts&ignore_sticky=0&include=1&_fields=id";
    assert_eq!(body(served("visitor", target, true, &[])), "[{\"id\":1}]");
    let statuses = body(served(
        "visitor",
        "/wp/v2/statuses/publish&_fields=name.x,slug",
        false,
        &[],
    ));
    assert!(
        statuses.contains("\"slug\"") && !statuses.contains("\"name\""),
        "{statuses}"
    );
    const NO_EDIT: &[&str] = &["edit_pages"];
    let private = served("editor", "/wp/v2/pages&status=private", false, NO_EDIT);
    assert_eq!(private.status, 200);
    assert_eq!(
        served("editor", "/wp/v2/pages&status=draft", false, NO_EDIT).status,
        400
    );
    let target = "/wp/v2/posts/98&context=edit&_fields=permalink_template#pretty";
    let template = body(served("administrator", target, false, &[]));
    assert_eq!(
        template,
        "{\"permalink_template\":\"http:\\/\\/127.0.0.1:8081\\/%postname%\\/\"}"
    );
}

/// A post's own excerpt shows without rendering its content, which a
/// shortcode leaves not ported.
#[test]
fn excerpts_need_no_content() {
    let embed = served(
        "visitor",
        "/wp/v2/posts/94&context=embed&_fields=excerpt",
        false,
        &[],
    );
    assert_eq!(embed.status, 200);
    assert_eq!(served("visitor", "/wp/v2/posts/94", false, &[]).status, 501);
}

fn body_of(served: Served) -> String {
    String::from_utf8(served.body).unwrap()
}

/// The categories' create action asks for `edit_categories`, the tags'
/// for `assign_post_tags`; without `DISALLOW_UNFILTERED_HTML`, unfiltered
/// HTML is an action of administrators.
#[test]
fn term_actions_ask_their_capabilities() {
    let target = "/wp/v2/posts/1&context=edit&_fields=_links";
    let allowed = served(
        "administrator",
        target,
        false,
        &["DISALLOW_UNFILTERED_HTML"],
    );
    assert!(body_of(allowed).contains("action-unfiltered-html"));
    let target = "/wp/v2/posts/1&context=edit&_fields=_links";
    let links = body(served("administrator", target, false, &["edit_categories"]));
    assert!(!links.contains("action-create-categories") && links.contains("action-create-tags"));
}

/// Content whose `the_content` is not ported or would write, as
/// `writes_or_unported()` tells it.
#[test]
fn content_not_ported() {
    let cases: [(&[u8], bool, bool); 22] = [
        (b"[gallery]", false, true),
        (b"a [video src=x]", false, true),
        (b"[galleryx]", false, false),
        (b"[gallery_x]", false, false),
        (b"[gallery-x]", false, false),
        (b"[gallery9]", false, false),
        (b"[caption", false, true),
        (b"https://example.test/a", false, true),
        (b"  http://example.test/a  ", false, true),
        (b"https://", false, false),
        (b"https://a b", false, false),
        (b"https://a<b", false, false),
        (b"x https://example.test/a", false, false),
        (b"<p>https://example.test/a</p>", false, true),
        (b"<p class=\"x\"> https://example.test/a </P>", false, true),
        (b"<pre>https://example.test/a</pre>", false, false),
        (b"<p>https://example.test/a", false, false),
        (b"<p>see https://example.test/a</p>", false, false),
        (b"<!-- wp:rss /-->", false, true),
        (b"{\"duotone\":1}", false, true),
        (b"<!-- wp:comments /-->", true, true),
        (b"<!-- wp:comments /-->", false, false),
    ];
    for (content, logged_in, expected) in cases {
        let what = String::from_utf8_lossy(content);
        assert_eq!(writes_or_unported(content, logged_in), expected, "{what}");
    }
    assert!(writes_or_unported(b"<!-- wp:loginout /-->", true));
    assert!(writes_or_unported(b"<!-- wp:post-comments-form /-->", true));
}

/// A site reads its users through `reads()` unless it says otherwise:
/// `/users/me` finds the fixture's administrator.
#[test]
fn users_are_read_through_the_site_reads() {
    let served = served(
        "administrator",
        "/wp/v2/users/me&_fields=id,slug",
        false,
        &[],
    );
    assert_eq!(
        String::from_utf8_lossy(&served.body),
        r#"{"id":1,"slug":"admin"}"#
    );
}

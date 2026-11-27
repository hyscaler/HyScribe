// SPDX-License-Identifier: AGPL-3.0-or-later

//! The PHP oracle's answers in `tests/golden/rest.tsv`, and for the content
//! routes in `content.tsv` and `content-ttd.tsv`, replayed for a visitor
//! and each role, with `%nonce%` as each user's nonce.

use hyscribe_auth::caps::{CapPost, CapabilityData, User as Capabilities};
use hyscribe_phpser::parse_str;
use hyscribe_testkit::{RestCase, rest_cases, rest_headers};

use crate::response::paginate;

use super::*;

mod batch;
pub(crate) mod content;
mod embed;
mod index;
mod media;
mod oembed;
mod pagination;
mod people;
mod plugins;
mod posts;
mod previews;
mod search;
pub(crate) mod site;
mod widgets;
mod writes;

use content::Content;

const SITE: &str = "http://127.0.0.1:8081";

/// The oracle over `content` and its fixture, under plain or pretty
/// permalinks, with the public users endpoint and the Trash on or off, and
/// with or without the salts `wp_hash()` signs with.
struct Oracle {
    content: &'static Content,
    fixture: site::Fixture,
    public_users: bool,
    trash: bool,
    salts: bool,
    /// The active extensions' routes, if any.
    plugins: Option<&'static dyn Plugins>,
}

impl Oracle {
    fn over(content: &'static Content, pretty: bool) -> Oracle {
        Oracle {
            content,
            fixture: content.reads.map_or(
                site::Fixture {
                    ttd: content.ttd,
                    pretty,
                    extra: false,
                    state: None,
                },
                |fixtures| fixtures[usize::from(pretty)],
            ),
            public_users: true,
            trash: true,
            salts: true,
            plugins: None,
        }
    }
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
    fn user_exists(&self, id: u64) -> bool {
        self.content.users.iter().any(|(user, _)| user.id == id)
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
    fn post_meta(&self, id: u64, key: &str) -> Option<Vec<u8>> {
        Site::post_meta(self, id, key)
    }
    fn comment_post(&self, id: u64) -> Option<u64> {
        let mut comments = self.content.comments.iter();
        let found = comments.find(|row| row.comment.id == id);
        found.map(|row| row.comment.post_id)
    }
    fn term_taxonomy(&self, id: u64) -> Option<String> {
        let terms = self.content.terms.iter();
        terms
            .map(|row| (row.term.id, &row.taxonomy))
            .find(|(term, _)| *term == id)
            .map(|(_, taxonomy)| taxonomy.clone())
    }
    /// HyScribe's constants: no one may post unfiltered HTML.
    fn constant(&self, name: &str) -> bool {
        name == "DISALLOW_UNFILTERED_HTML"
    }
}

impl Site for Oracle {
    fn plugins(&self) -> Option<&dyn Plugins> {
        self.plugins
    }
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        if self.fixture.pretty && name == "permalink_structure" {
            return Some(b"/%postname%/".to_vec());
        }
        self.content.options.get(name).cloned()
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
        user != 0 && Capabilities::load(self, "wp_", user).has_cap(self, cap, args)
    }
    fn terms(&self, query: &TermQuery) -> Vec<Term> {
        self.content.terms(query)
    }
    fn count_terms(&self, query: &TermQuery) -> u64 {
        self.content.count_terms(query)
    }
    fn term_link(&self, taxonomy: &str, term: &Term) -> String {
        self.content.term_link(taxonomy, term, self.fixture.pretty)
    }
    fn post(&self, id: u64) -> Option<Post> {
        self.content
            .posts
            .iter()
            .find(|post| post.id == id)
            .cloned()
    }
    fn reads(&self) -> &dyn Reads {
        &self.fixture
    }
    fn public_users(&self) -> bool {
        self.public_users
    }
    fn uses_trash(&self) -> bool {
        self.trash
    }
    fn theme(&self) -> &Theme {
        // The embedded theme the site's stylesheet names, else the default.
        static THEMES: std::sync::OnceLock<Vec<Theme>> = std::sync::OnceLock::new();
        let themes = THEMES.get_or_init(|| {
            hyscribe_render::theme::slugs()
                .map(|slug| hyscribe_render::theme::load(slug, None))
                .collect()
        });
        let stylesheet = self.content.options.get("stylesheet").cloned();
        let stylesheet = String::from_utf8_lossy(&stylesheet.unwrap_or_default()).into_owned();
        let slug = hyscribe_render::theme::resolve(&stylesheet);
        themes.iter().find(|theme| theme.slug() == slug).unwrap()
    }
    fn posts_of_type(&self, post_type: &str) -> Vec<Post> {
        let mut posts: Vec<Post> = self
            .content
            .posts
            .iter()
            .filter(|post| post.post_type == post_type)
            .cloned()
            .collect();
        posts.sort_by(|a, b| b.date.cmp(&a.date));
        posts
    }
    fn post_terms(&self, post: u64, taxonomy: &str) -> Vec<Term> {
        let related = |row: &&content::Row| {
            row.taxonomy == taxonomy
                && self
                    .content
                    .relationships
                    .contains(&(post, row.taxonomy_id))
        };
        let mut terms: Vec<Term> = self
            .content
            .terms
            .iter()
            .filter(related)
            .map(|row| row.term.clone())
            .collect();
        terms.sort_by(|a, b| a.name.cmp(&b.name));
        terms
    }
    fn post_meta(&self, post: u64, key: &str) -> Option<Vec<u8>> {
        let meta = self.content.postmeta.iter();
        let found = meta
            .clone()
            .find(|(id, name, _)| *id == post && name == key);
        found.map(|(_, _, value)| value.clone())
    }
    fn revisions(&self, post: u64) -> Vec<u64> {
        let mut revisions: Vec<&Post> = self
            .content
            .posts
            .iter()
            .filter(|revision| revision.parent == post && revision.post_type == "revision")
            .collect();
        revisions.sort_by(|a, b| (&b.date, b.id).cmp(&(&a.date, a.id)));
        revisions.iter().map(|revision| revision.id).collect()
    }
    fn table_prefix(&self) -> String {
        "wp_".into()
    }
    fn comments(&self, query: &CommentQuery) -> Vec<CommentRow> {
        self.content.comment_query(query)
    }
    fn count_comments(&self, query: &CommentQuery) -> u64 {
        self.content.count_comment_query(query)
    }
    /// `get_comment_link()` of a post, page or attachment: plain, or a post's name
    /// under `/%postname%/`.
    fn comment_link(&self, comment: &Comment) -> String {
        let post = Site::post(self, comment.post_id);
        let link = post.map_or(String::new(), |post| {
            match (self.fixture.pretty, post.post_type.as_str()) {
                (true, _) => format!("{SITE}/{}/", String::from_utf8_lossy(&post.name)),
                (false, "page") => format!("{SITE}/?page_id={}", post.id),
                (false, "attachment") => format!("{SITE}/?attachment_id={}", post.id),
                _ => format!("{SITE}/?p={}", post.id),
            }
        });
        let link = if self.fixture.pretty && link.is_empty() {
            "/".into()
        } else {
            link
        };
        format!("{link}#comment-{}", comment.id)
    }
    fn users(&self, query: &UserQuery) -> Vec<User> {
        self.content.user_query(query)
    }
    fn count_users(&self, query: &UserQuery) -> u64 {
        self.content.count_user_query(query)
    }
    fn user(&self, id: u64) -> Option<User> {
        let users = self.content.users.iter();
        users
            .map(|(user, _)| user)
            .find(|user| user.id == id)
            .cloned()
    }
    fn user_meta(&self, id: u64, key: &str) -> Option<Vec<u8>> {
        CapabilityData::user_meta(self, id, key)
    }
    fn user_registered(&self, id: u64) -> String {
        let mut users = self.content.users.iter();
        let found = users.find(|(user, _)| user.id == id);
        found
            .map(|(_, registered)| registered.clone())
            .unwrap_or_default()
    }
    fn allcaps(&self, id: u64) -> Vec<(Key, Value)> {
        Capabilities::load(self, "wp_", id).allcaps().to_vec()
    }
    fn author_link(&self, user: &User) -> String {
        if self.fixture.pretty {
            format!("{SITE}/author/{}/", String::from_utf8_lossy(&user.nicename))
        } else {
            format!("{SITE}/?author={}", user.id)
        }
    }
    fn has_posts(&self, author: u64, types: &[(String, bool)]) -> bool {
        self.content.has_posts(author, types)
    }
    /// The salt `reference/rest/widgets.php` gives the oracle.
    fn wp_hash(&self, data: &[u8]) -> Option<String> {
        let salt = self.salts.then_some(b"test-salt");
        salt.map(|salt| hyscribe_phpser::hash_hmac_md5(data, salt))
    }
    /// The secret `reference/rest/golden.sh` masks oEmbed answers with.
    fn embed_secret(&self) -> String {
        "0123456789".into()
    }
}

fn cases() -> Vec<RestCase> {
    rest_cases(include_str!("../tests/golden/rest.tsv"))
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
pub(crate) fn serve_case(case: &RestCase) -> Served {
    serve_over(content::default_install(), case)
}

/// `case` served by HyScribe's API over `content`.
pub(crate) fn serve_over(content: &'static Content, case: &RestCase) -> Served {
    serve_with(&Oracle::over(content, case.flag("pretty")), case)
}

/// `content` as rendering reads it.
pub(crate) fn render_site(content: &'static Content) -> Box<dyn hyscribe_render::site::Site> {
    Box::new(Oracle::over(content, false).fixture)
}

/// The role of `who` and its user, none for a visitor.
pub(super) fn role(who: &str) -> (&'static str, u64) {
    let found = ROLES.iter().find(|(role, _)| *role == who);
    found.copied().unwrap_or(("", 0))
}

/// `case` served by HyScribe's API over `site`.
pub(super) fn serve_with(site: &dyn Site, case: &RestCase) -> Served {
    let (path, query) = case.target.split_once('?').unwrap_or((&case.target, ""));
    let get = match parse_str(query.as_bytes()) {
        Value::Array(get) => get,
        _ => Vec::new(),
    };
    let (_, user) = role(&case.who);
    let flag = |name: &str| case.flag(name);
    let route = route(path, &get);
    // `$_POST`, which PHP reads from a form it is sent.
    let form = match parse_str(&case.sent) {
        Value::Array(form) if flag("form") => form,
        _ => Vec::new(),
    };
    let request = Request {
        method: &case.method,
        route: &route,
        uri: case.target.as_bytes(),
        get: &get,
        post: &form,
        cookie_user: user,
        origin: if flag("origin") {
            Some(b"http://example.test")
        } else if flag("originnull") {
            Some(b"null")
        } else {
            None
        },
        nonce: (user != 0 && !flag("nononce")).then_some(b"%nonce%"),
        remote_addr: b"",
        postpass: false,
        user_agent: None,
        method_override: flag("override").then_some(b"DELETE"),
        content_type: if flag("json") {
            Some(b"application/json")
        } else if flag("form") {
            Some(b"application/x-www-form-urlencoded")
        } else {
            None
        },
        body: &case.sent,
        host: Some(if flag("localhost") {
            b"localhost:8081"
        } else {
            b"127.0.0.1:8081"
        }),
        now: 0,
    };
    serve(site, &request)
}

/// Requests PHP answers on routes or with arguments not ported.
const UNPORTED: [(&str, &str); 4] = [
    ("GET", "/?rest_route=/"),
    ("GET", "/?rest_route=0/"),
    ("GET", "/?rest_route=/wp-abilities/v1/categories/a-b"),
    ("GET", "/?rest_route=/wp/v2/types&_method[]=x"),
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
        assert_eq!(rest_headers(&served.headers), php, "{what}");
        assert_eq!(
            String::from_utf8_lossy(&served.body),
            String::from_utf8_lossy(&case.body),
            "{what}"
        );
    }
    assert_eq!(unported, 6 * UNPORTED.len());
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

/// Requests on the content routes PHP answers with what is not ported, or
/// where it writes.
const CONTENT_UNPORTED: [&str; 1] = ["/?rest_route=/wp/v2/categories/68&_embed=up"];

/// The SHA-256 of `body` in hex, as the Theme Test Data goldens keep it.
pub(crate) fn sha256_hex(body: &[u8]) -> Vec<u8> {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(body);
    digest
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
        .into_bytes()
}

/// The answers of `goldens`, each over its content and compared by hash
/// when flagged, that differ from PHP's, with the public users endpoint on
/// or off, and how many are 501 as `unported` expects; a write is answered
/// 501 only where PHP goes on.
fn mismatches(
    goldens: &[(&'static Content, &str, bool)],
    public_users: bool,
    unported: &dyn Fn(&RestCase) -> bool,
) -> (Vec<String>, usize) {
    let mut failures = Vec::new();
    let mut not_served = 0;
    for &(content, golden, hashed) in goldens {
        for case in rest_cases(golden) {
            let oracle = Oracle {
                public_users,
                trash: !case.flag("notrash"),
                ..Oracle::over(content, case.flag("pretty"))
            };
            let served = serve_with(&oracle, &case);
            let what = format!(
                "{} {} {} {} {hashed}",
                case.who, case.method, case.target, case.flags
            );
            let write = case.method != "GET" && case.method != "HEAD";
            // PHP answers 501 too where a type does not support the Trash.
            if served.status == 501 && case.status != 501 {
                if !(write && case.status < 400 || unported(&case)) || !served.body.is_empty() {
                    failures.push(format!("{what}: 501"));
                }
                not_served += 1;
                continue;
            }
            // PHP stopped once its permission check passed; the callback's
            // own refusal is checked by the golden of its writes.
            let refused = served.status >= 400 && served.status != 401 && served.status != 403;
            if case.body == b"hyscribe-accepted" && refused {
                not_served += 1;
                continue;
            }
            let body = if hashed || case.flag("hash") {
                sha256_hex(&served.body)
            } else {
                served.body.clone()
            };
            let mut php = case.headers.clone();
            php.sort();
            if served.status != case.status
                || rest_headers(&served.headers) != php
                || body != case.body
            {
                failures.push(format!(
                    "{what}\n  php:  {} {php:?}\n       {}\n  ours: {} {:?}\n       {}",
                    case.status,
                    String::from_utf8_lossy(&case.body),
                    served.status,
                    rest_headers(&served.headers),
                    String::from_utf8_lossy(&served.body)
                ));
            }
        }
    }
    (failures, not_served)
}

/// The content routes answer as PHP does over the default install and, by
/// hash, the Theme Test Data fixture, but those not ported.
#[test]
fn content_answers_match_php() {
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
    let unported = |case: &RestCase| CONTENT_UNPORTED.contains(&case.target.as_str());
    let (failures, _) = mismatches(&goldens, true, &unported);
    assert!(
        failures.is_empty(),
        "{} failures\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Writes are refused as PHP refuses them, and answered 501 where PHP's
/// permission check lets them through (`reference/rest/writes.list`).
#[test]
fn writes_are_checked_as_php_checks_them() {
    // A settings update giving none writes nothing, so it is answered at
    // once, as `reference/rest/settings-writes.list` checks.
    let golden: String = include_str!("../tests/golden/writes.tsv")
        .lines()
        .filter(|line| {
            let case = &rest_cases(line)[0];
            case.target != "/?rest_route=/wp/v2/settings" || case.status >= 400
        })
        .map(|line| format!("{line}\n"))
        .collect();
    let golden = golden.as_str();
    let goldens = [(content::default_install(), golden, false)];
    let (failures, not_served) = mismatches(&goldens, true, &|_| false);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let accepted = rest_cases(golden)
        .iter()
        .filter(|case| case.body == b"hyscribe-accepted")
        .count();
    assert_eq!(not_served, accepted);
}

/// With `EMPTY_TRASH_DAYS` at 0, deletes without `force` are refused as PHP
/// refuses them (`reference/rest/trash-off.list`).
#[test]
fn deletes_without_the_trash_are_refused() {
    let golden = include_str!("../tests/golden/trash-off.tsv");
    let goldens = [(content::default_install(), golden, false)];
    let (failures, not_served) = mismatches(&goldens, true, &|_| false);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!((rest_cases(golden).len(), not_served), (6 * 7, 0));
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
pub(crate) fn changed(options: &[(&str, &[u8])]) -> &'static Content {
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
        postmeta: Vec::new(),
        users: default.users.clone(),
        ..Content::default()
    }))
}

pub(crate) fn get(content: &'static Content, who: &str, target: &str) -> Served {
    let case = RestCase {
        who: who.into(),
        method: "GET".into(),
        target: target.into(),
        flags: String::new(),
        status: 0,
        headers: Vec::new(),
        body: Vec::new(),
        sent: Vec::new(),
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

/// `WP_REST_Request::from_url()`: a URL not the API's, or with a route
/// that is empty or not a string, embeds nothing; a context or page size
/// left empty is set; a route not found embeds its error.
#[test]
fn links_are_served_as_from_url_reads_them() {
    let oracle = Oracle::over(content::default_install(), false);
    let context = Context {
        site: &oracle,
        user: 0,
        head: false,
        uri: b"/",
        parameters: false,
        remote_addr: b"",
        postpass: false,
        user_agent: None,
        host: b"",
        now: 0,
        given: (None, b""),
    };
    let served = |href: &str| crate::embed::serve(&context, href.as_bytes()).ok().unwrap();
    let rest = "http://127.0.0.1:8081/index.php?rest_route=";
    for href in [
        "http://example.test/x".to_string(),
        rest.to_string(),
        format!("{rest}/wp/v2/types&rest_route[]=x"),
    ] {
        assert_eq!(served(&href), Value::Array(Vec::new()), "{href}");
    }
    let comments = format!("{rest}/wp/v2/comments");
    assert_eq!(
        served(&format!("{comments}&context=&per_page=")),
        served(&comments)
    );
    let page = served(&format!("{comments}&per_page=1&context=view"));
    assert_eq!(page.as_array().map(<[_]>::len), Some(1));
    let viewed = page.get_path(&["0", "content"]);
    assert!(viewed.is_some(), "{page:?}");
    let missing = served(&format!("{rest}/nope"));
    assert_eq!(missing.get_str("code"), Some(&text("rest_no_route")));
}

/// `rest_parse_embed_param()`: an empty `_embed`, `0`, `1`, `true` or a
/// list of nothing embeds every relation, else those listed.
#[test]
fn embed_relations() {
    for embed in ["", "0", "1", "true", " ,"] {
        assert_eq!(crate::embed::relations(&text(embed)), None, "{embed}");
    }
    let listed = crate::embed::relations(&text("up, author")).unwrap();
    assert_eq!(
        listed,
        [(Key::Int(0), text("up")), (Key::Int(1), text("author"))]
    );
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
        postmeta: Vec::new(),
        users: default.users.clone(),
        ..Content::default()
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
        postmeta: Vec::new(),
        users: default.users.clone(),
        ..Content::default()
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
    let oracle = Oracle::over(content::default_install(), false);
    let context = Context {
        site: &oracle,
        user: 0,
        head: false,
        uri: b"/",
        parameters: false,
        remote_addr: b"",
        postpass: false,
        user_agent: None,
        host: b"",
        now: 0,
        given: (None, b""),
    };
    for route in [
        "/nope",
        "/wp/v2/plugins",
        "/wp/v2/block-types/core/paragraph",
    ] {
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

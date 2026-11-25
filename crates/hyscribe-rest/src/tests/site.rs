// SPDX-License-Identifier: AGPL-3.0-or-later

//! The default install and the Theme Test Data fixture as
//! `hyscribe-render` reads them, from their dumps, with the post queries
//! the goldens make answered as MariaDB answers them. `hyscribe-http`
//! replays the post routes over the databases too.

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use hyscribe_model::{Comment, OrderBy, Post, PostQuery, Term, User};
use hyscribe_phpser::Value;
use hyscribe_render::site::Site;

use hyscribe_testkit::{Dump, SqlValue};

pub(super) fn default_install() -> &'static Dump {
    static DUMP: OnceLock<Dump> = OnceLock::new();
    DUMP.get_or_init(|| hyscribe_testkit::default_install("7.1.3").unwrap())
}

fn theme_test_data() -> &'static Dump {
    static DUMP: OnceLock<Dump> = OnceLock::new();
    DUMP.get_or_init(|| {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../target/fixtures/theme-test-data/7.1.3/database.sql"
        );
        let sql = std::fs::read(path).unwrap_or_else(|e| {
            panic!("{path}: {e}; build it with reference/theme-test-data/build.sh")
        });
        Dump::parse(&sql).unwrap()
    })
}

/// A dump's options, `home` and `siteurl` as the oracle defines them.
fn options(dump: &'static Dump) -> HashMap<String, Vec<u8>> {
    let mut options: HashMap<String, Vec<u8>> = dump
        .table("wp_options")
        .map(|row| (string(&row[1]), text(&row[2])))
        .collect();
    for name in ["home", "siteurl"] {
        options.insert(name.into(), super::SITE.as_bytes().to_vec());
    }
    options
}

fn text(value: &SqlValue) -> Vec<u8> {
    match value {
        SqlValue::Text(bytes) => bytes.clone(),
        SqlValue::Number(n) => n.clone().into_bytes(),
        SqlValue::Null => Vec::new(),
    }
}

fn number(value: &SqlValue) -> u64 {
    Value::String(text(value)).to_int().unsigned_abs()
}

fn string(value: &SqlValue) -> String {
    String::from_utf8_lossy(&text(value)).into_owned()
}

/// A dump's posts, every column the routes read.
pub(super) fn posts(dump: &Dump) -> impl Iterator<Item = Post> + '_ {
    dump.table("wp_posts").map(|row| Post {
        id: number(&row[0]),
        author: number(&row[1]),
        date: string(&row[2]),
        date_gmt: string(&row[3]),
        content: text(&row[4]),
        title: text(&row[5]),
        excerpt: text(&row[6]),
        status: string(&row[7]),
        comment_status: string(&row[8]),
        ping_status: string(&row[9]),
        password: text(&row[10]),
        name: text(&row[11]),
        modified: string(&row[14]),
        modified_gmt: string(&row[15]),
        parent: number(&row[17]),
        guid: text(&row[18]),
        menu_order: Value::String(text(&row[19])).to_int(),
        post_type: string(&row[20]),
        mime_type: string(&row[21]),
        comment_count: Value::String(text(&row[22])).to_int(),
    })
}

/// The default install, or the Theme Test Data fixture when `ttd`, under
/// `/%postname%/` permalinks when `pretty`.
#[derive(Clone, Copy)]
pub(super) struct Fixture {
    pub(super) ttd: bool,
    pub(super) pretty: bool,
    /// With what neither fixture has: a revision of post 1 (99), drafts
    /// without an author whose GMT dates are unset (98, 96), and their
    /// featured images, attachments of the draft (97) and of none (95),
    /// and a draft (92) whose image (93) is private; a
    /// post with a gallery shortcode and its own excerpt (94); and the
    /// site two hours east of UTC.
    pub(super) extra: bool,
    /// A dump read in place of the fixture's: the default install with a
    /// state a golden adds.
    pub(super) state: Option<&'static Dump>,
}

/// A dump's options, posts, their meta and terms, read once.
struct Data {
    options: HashMap<String, Vec<u8>>,
    posts: Vec<Post>,
    meta: HashMap<(u64, Vec<u8>), Vec<u8>>,
    terms: HashMap<(u64, Vec<u8>), Vec<Term>>,
}

fn data(dump: &'static Dump) -> Data {
    let mut meta = HashMap::new();
    for row in dump.table("wp_postmeta") {
        meta.entry((number(&row[1]), text(&row[2])))
            .or_insert_with(|| text(&row[3]));
    }
    let names: HashMap<u64, (Vec<u8>, Vec<u8>)> = dump
        .table("wp_terms")
        .map(|t| (number(&t[0]), (text(&t[1]), text(&t[2]))))
        .collect();
    let taxonomies: HashMap<u64, (Vec<u8>, Term)> = dump
        .table("wp_term_taxonomy")
        .filter_map(|row| {
            let (name, slug) = names.get(&number(&row[1]))?.clone();
            let term = Term {
                id: number(&row[1]),
                name,
                slug,
                description: text(&row[3]),
                parent: number(&row[4]),
                count: number(&row[5]),
            };
            Some((number(&row[0]), (text(&row[2]), term)))
        })
        .collect();
    let mut terms: HashMap<(u64, Vec<u8>), Vec<Term>> = HashMap::new();
    for row in dump.table("wp_term_relationships") {
        if let Some((taxonomy, term)) = taxonomies.get(&number(&row[1])) {
            terms
                .entry((number(&row[0]), taxonomy.clone()))
                .or_default()
                .push(term.clone());
        }
    }
    // `ORDER BY t.name`, as MariaDB's case insensitive collation sorts.
    for list in terms.values_mut() {
        list.sort_by_key(|term| string_of(&term.name).to_lowercase());
    }
    Data {
        options: options(dump),
        posts: posts(dump).collect(),
        meta,
        terms,
    }
}

impl Fixture {
    fn data(self) -> &'static Data {
        static STATES: Mutex<Vec<(usize, &'static Data)>> = Mutex::new(Vec::new());
        if let Some(dump) = self.state {
            let key = std::ptr::from_ref(dump) as usize;
            let mut states = STATES.lock().unwrap();
            if let Some((_, data)) = states.iter().find(|(known, _)| *known == key) {
                return data;
            }
            let read: &'static Data = Box::leak(Box::new(data(dump)));
            states.push((key, read));
            return read;
        }
        static DEFAULT: OnceLock<Data> = OnceLock::new();
        static TTD: OnceLock<Data> = OnceLock::new();
        let memo = if self.ttd { &TTD } else { &DEFAULT };
        memo.get_or_init(|| data(self.dump()))
    }

    fn posts(self) -> impl Iterator<Item = Post> {
        let revision = Post {
            id: 99,
            author: 1,
            date: "2026-11-01 00:00:00".into(),
            status: "inherit".into(),
            name: b"1-revision-v1".to_vec(),
            parent: 1,
            post_type: "revision".into(),
            ..Post::default()
        };
        let draft = Post {
            id: 98,
            date: "2026-11-02 10:00:00".into(),
            date_gmt: "0000-00-00 00:00:00".into(),
            modified: "2026-11-02 10:00:00".into(),
            modified_gmt: "0000-00-00 00:00:00".into(),
            status: "draft".into(),
            post_type: "post".into(),
            ..Post::default()
        };
        let image = Post {
            id: 97,
            parent: 98,
            status: "inherit".into(),
            post_type: "attachment".into(),
            ..Post::default()
        };
        let other = Post {
            id: 96,
            ..draft.clone()
        };
        let shortcode = Post {
            id: 94,
            author: 1,
            content: b"[gallery]".to_vec(),
            excerpt: b"Short".to_vec(),
            status: "publish".into(),
            post_type: "post".into(),
            ..Post::default()
        };
        let orphan = Post {
            id: 95,
            parent: 0,
            ..image.clone()
        };
        let private = Post {
            id: 93,
            status: "private".into(),
            ..image.clone()
        };
        let third = Post {
            id: 92,
            ..draft.clone()
        };
        self.data().posts.iter().cloned().chain(
            self.extra
                .then_some([
                    revision, draft, image, other, orphan, shortcode, private, third,
                ])
                .into_iter()
                .flatten(),
        )
    }

    fn dump(self) -> &'static Dump {
        if let Some(dump) = self.state {
            dump
        } else if self.ttd {
            theme_test_data()
        } else {
            default_install()
        }
    }
}

thread_local! {
    /// Whether the test's thread has `wp_navigation_should_create_fallback`
    /// return false.
    pub(crate) static DECLINE_FALLBACK: Cell<bool> = const { Cell::new(false) };
}

/// A hook that declines to make the navigation fallback.
#[derive(Debug)]
struct Declined;

impl hyscribe_render::site::Hooks for Declined {
    fn wp_navigation_should_create_fallback(&self, _create: bool) -> bool {
        false
    }
}

impl Site for Fixture {
    fn hooks(&self) -> &dyn hyscribe_render::site::Hooks {
        match DECLINE_FALLBACK.get() {
            true => &Declined,
            false => &(),
        }
    }

    fn option(&self, name: &str) -> Option<Vec<u8>> {
        if self.pretty && name == "permalink_structure" {
            return Some(b"/%postname%/".to_vec());
        }
        if self.extra && name == "gmt_offset" {
            return Some(b"2".to_vec());
        }
        self.data().options.get(name).cloned()
    }

    fn home_url(&self) -> Vec<u8> {
        super::SITE.as_bytes().to_vec()
    }

    fn post(&self, id: u64) -> Option<Post> {
        self.posts().find(|post| post.id == id)
    }

    fn post_terms(&self, post_id: u64, taxonomy: &str) -> Vec<Term> {
        let key = (post_id, taxonomy.as_bytes().to_vec());
        self.data().terms.get(&key).cloned().unwrap_or_default()
    }

    fn term(&self, id: u64, taxonomy: &str) -> Option<Term> {
        let terms = self.data().terms.iter();
        let mut found = terms.filter(|((_, tax), _)| tax == taxonomy.as_bytes());
        found.find_map(|(_, list)| list.iter().find(|term| term.id == id).cloned())
    }

    /// The terms of `taxonomy` some post has, once each.
    fn taxonomy_terms(&self, taxonomy: &str) -> Vec<Term> {
        let terms = self.data().terms.iter();
        let mut found: Vec<Term> = terms
            .filter(|((_, tax), _)| tax == taxonomy.as_bytes())
            .flat_map(|(_, list)| list.iter().cloned())
            .collect();
        found.sort_by_key(|term| term.id);
        found.dedup_by_key(|term| term.id);
        found
    }

    fn user(&self, id: u64) -> Option<User> {
        let dump = self.dump();
        let meta = |key: &str| {
            let row = dump
                .table("wp_usermeta")
                .find(|row| number(&row[1]) == id && text(&row[2]) == key.as_bytes());
            row.map(|row| text(&row[3])).unwrap_or_default()
        };
        let row = dump.table("wp_users").find(|row| number(&row[0]) == id)?;
        Some(User {
            id,
            login: text(&row[1]),
            nicename: text(&row[3]),
            email: text(&row[4]),
            url: text(&row[5]),
            display_name: text(&row[9]),
            first_name: meta("first_name"),
            last_name: meta("last_name"),
            description: meta("description"),
        })
    }

    fn comments(&self, post_id: u64) -> Vec<Comment> {
        let mut comments: Vec<Comment> = self
            .dump()
            .table("wp_comments")
            .filter(|row| number(&row[1]) == post_id && text(&row[10]) == b"1")
            .map(|row| Comment {
                id: number(&row[0]),
                post_id,
                author: text(&row[2]),
                author_email: text(&row[3]),
                author_url: text(&row[4]),
                author_ip: text(&row[5]),
                date: string(&row[6]),
                date_gmt: string(&row[7]),
                content: text(&row[8]),
                comment_type: text(&row[12]),
                parent: number(&row[13]),
                user_id: number(&row[14]),
            })
            .collect();
        comments.sort_by(|a, b| (&a.date_gmt, a.id).cmp(&(&b.date_gmt, b.id)));
        comments
    }

    fn post_meta(&self, post_id: u64, key: &str) -> Option<Vec<u8>> {
        let featured = match post_id {
            98 => Some(b"97".to_vec()),
            96 => Some(b"95".to_vec()),
            92 => Some(b"93".to_vec()),
            _ => None,
        };
        if self.extra && key == "_thumbnail_id" && featured.is_some() {
            return featured;
        }
        let key = (post_id, key.as_bytes().to_vec());
        self.data().meta.get(&key).cloned()
    }

    fn query(&self, query: &PostQuery) -> Vec<Post> {
        let mut found: Vec<Post> = self
            .posts()
            .filter(|post| matches(self, query, post))
            .collect();
        found.sort_by(|a, b| order(query, a, b));
        let skip = usize::try_from(query.offset).unwrap();
        let take = usize::try_from(query.limit).unwrap();
        let page: Vec<Post> = found.into_iter().skip(skip).take(take).collect();
        let sticky = query
            .sticky
            .iter()
            .filter(|id| !query.exclude.contains(id))
            .filter_map(|id| self.post(*id).filter(|post| post.status == "publish"));
        let (first, rest): (Vec<Post>, Vec<Post>) = page
            .into_iter()
            .partition(|post| query.sticky.contains(&post.id));
        let missing: Vec<Post> = sticky
            .filter(|s| !first.iter().any(|p| p.id == s.id))
            .collect();
        first.into_iter().chain(missing).chain(rest).collect()
    }
}

/// `LIKE '%term%'` as MariaDB's case insensitive collation matches it.
fn like(haystack: &[u8], term: &str) -> bool {
    let haystack = String::from_utf8_lossy(haystack).to_lowercase();
    haystack.contains(&term.to_lowercase())
}

fn column<'a>(post: &'a Post, name: &str) -> &'a [u8] {
    match name {
        "post_title" => &post.title,
        "post_excerpt" => &post.excerpt,
        _ => &post.content,
    }
}

/// Whether `post` meets `query`'s `WHERE`.
fn matches(site: &Fixture, query: &PostQuery, post: &Post) -> bool {
    let types = query.post_types.is_empty() || query.post_types.contains(&post.post_type);
    let status = match query.statuses.as_slice() {
        [] => post.status == "publish",
        statuses => statuses.iter().any(|s| {
            s.status == post.status && s.author.is_none_or(|author| author == post.author)
        }),
    };
    let listed = |list: &[u64], id: u64| list.is_empty() || list.contains(&id);
    let columns: Vec<&str> = if query.search_columns.is_empty() {
        vec!["post_title", "post_excerpt", "post_content"]
    } else {
        query.search_columns.iter().map(String::as_str).collect()
    };
    let hit = |c: &&str, term: &str| {
        let text = column(post, c);
        if query.exact {
            string_of(text).to_lowercase() == term.to_lowercase()
        } else {
            like(text, term)
        }
    };
    let searched = query
        .search
        .iter()
        .all(|term| match term.strip_prefix('-') {
            Some(term) => columns.iter().all(|c| !hit(c, term)),
            None => {
                let file = || {
                    site.post_meta(post.id, "_wp_attached_file")
                        .unwrap_or_default()
                };
                columns.iter().any(|c| hit(c, term)) || query.filenames && like(&file(), term)
            }
        });
    let format = query.formats.as_ref().is_none_or(|formats| {
        let terms = site.post_terms(post.id, "post_format");
        terms.is_empty() && formats.standard
            || terms
                .iter()
                .any(|t| formats.slugs.contains(&string_of(&t.slug)))
    });
    let terms = query.terms.iter().all(|filter| {
        let terms = site.post_terms(post.id, &filter.taxonomy);
        terms.iter().any(|t| filter.terms.contains(&t.id)) != filter.exclude
    });
    types
        && status
        && terms
        && query
            .include
            .as_ref()
            .is_none_or(|ids| ids.contains(&post.id))
        && !query.exclude.contains(&post.id)
        && listed(&query.authors, post.author)
        && !query.excluded_authors.contains(&post.author)
        && listed(&query.parents, post.parent)
        && !query.excluded_parents.contains(&post.parent)
        && (query.names.is_empty() || query.names.contains(&string_of(&post.name)))
        && query
            .menu_order
            .is_none_or(|order| order == post.menu_order)
        && searched
        && (query.mime_types.is_empty() || query.mime_types.contains(&post.mime_type))
        && (query.search.is_empty() || query.logged_in || post.password.is_empty())
        && format
}

fn string_of(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// `parse_search_order()`'s rank of `post`, lower first.
fn relevance(query: &PostQuery, post: &Post) -> u8 {
    let terms: Vec<&String> = query
        .search
        .iter()
        .filter(|t| !t.starts_with('-'))
        .collect();
    if like(&post.title, &query.sentence) {
        1
    } else if terms.iter().all(|t| like(&post.title, t)) {
        2
    } else if terms.iter().any(|t| like(&post.title, t)) {
        3
    } else if like(&post.excerpt, &query.sentence) {
        4
    } else if like(&post.content, &query.sentence) {
        5
    } else {
        6
    }
}

/// `ORDER BY` of `query`, then the database's order, by ID.
fn order(query: &PostQuery, a: &Post, b: &Post) -> std::cmp::Ordering {
    let position =
        |list: &[String], post: &Post| list.iter().position(|n| *n == string_of(&post.name));
    for (column, ascending) in &query.order {
        let ordering = match column {
            OrderBy::Date => a.date.cmp(&b.date),
            OrderBy::Title => a
                .title
                .to_ascii_lowercase()
                .cmp(&b.title.to_ascii_lowercase()),
            OrderBy::Modified => a.modified.cmp(&b.modified),
            OrderBy::Name => a.name.cmp(&b.name),
            OrderBy::Author => a.author.cmp(&b.author),
            OrderBy::Parent => a.parent.cmp(&b.parent),
            OrderBy::Id => a.id.cmp(&b.id),
            OrderBy::MenuOrder => a.menu_order.cmp(&b.menu_order),
            OrderBy::Relevance => relevance(query, a).cmp(&relevance(query, b)),
            OrderBy::Included => {
                let included = query.include.clone().unwrap_or_default();
                let at = |p: &Post| included.iter().position(|id| *id == p.id);
                at(a).cmp(&at(b))
            }
            OrderBy::Names => position(&query.names, a).cmp(&position(&query.names, b)),
            _ => std::cmp::Ordering::Equal,
        };
        let ordering = if *ascending || *column == OrderBy::Relevance {
            ordering
        } else {
            ordering.reverse()
        };
        if ordering.is_ne() {
            return ordering;
        }
    }
    // MariaDB's sort of attachments by date leaves ties newest first.
    if query.post_types == ["attachment"] && query.order == [(OrderBy::Date, false)] {
        return b.id.cmp(&a.id);
    }
    a.id.cmp(&b.id)
}

/// The default install, or the Theme Test Data fixture when `ttd`, with
/// `reference/rest/media.sql`'s rows.
pub(super) fn with_media(ttd: bool) -> &'static Dump {
    static MEDIA: [OnceLock<Dump>; 2] = [const { OnceLock::new() }; 2];
    MEDIA[usize::from(ttd)].get_or_init(|| {
        let dump = if ttd {
            theme_test_data()
        } else {
            default_install()
        };
        let state = include_str!("../../../../reference/rest/media.sql");
        let mut rows = dump.rows.clone();
        rows.extend(Dump::parse(state.as_bytes()).unwrap().rows);
        Dump { rows }
    })
}

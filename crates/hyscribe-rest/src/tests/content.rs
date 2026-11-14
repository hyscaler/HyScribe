// SPDX-License-Identifier: AGPL-3.0-or-later

//! The oracle's databases as the tests read them: the default install or
//! the Theme Test Data fixture, with `reference/rest/users.sql`'s users,
//! and the queries the API makes over them as MySQL answers them.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::OnceLock;

use hyscribe_model::{Comment, CommentRow, Post, Term, TermOrder, TermQuery, User};
use hyscribe_phpser::Value;
use hyscribe_testkit::{Dump, SqlValue};

/// A term with what its queries read.
pub(super) struct Row {
    pub(super) taxonomy: String,
    pub(super) term: Term,
    pub(super) group: i64,
    pub(super) taxonomy_id: u64,
}

/// One database's content.
#[derive(Default)]
pub(crate) struct Content {
    pub(super) options: HashMap<String, Vec<u8>>,
    pub(super) terms: Vec<Row>,
    /// `(object_id, term_taxonomy_id)`.
    pub(super) relationships: Vec<(u64, u64)>,
    pub(super) posts: Vec<Post>,
    /// `(user_id, meta_key, meta_value)`.
    pub(super) usermeta: Vec<(u64, String, Vec<u8>)>,
    /// `(post_id, meta_key, meta_value)`.
    pub(super) postmeta: Vec<(u64, String, Vec<u8>)>,
    pub(super) comments: Vec<CommentRow>,
    /// Each user and its `user_registered`.
    pub(super) users: Vec<(User, String)>,
    /// The posts as rendering reads them, plain and pretty, of a state's
    /// dump; none for a fixture's own.
    pub(super) reads: Option<[super::site::Fixture; 2]>,
}

fn text(value: &SqlValue) -> Vec<u8> {
    match value {
        SqlValue::Number(n) => n.clone().into_bytes(),
        _ => value.as_text().unwrap_or_default().to_vec(),
    }
}

fn number(value: &SqlValue) -> u64 {
    Value::String(text(value)).to_int().unsigned_abs()
}

fn string(value: &SqlValue) -> String {
    String::from_utf8(text(value)).unwrap()
}

/// The roles and user levels of `reference/rest/users.sql`'s users.
const ADDED: [(u64, &str, u8); 4] = [
    (701, "editor", 7),
    (702, "author", 2),
    (703, "contributor", 1),
    (704, "subscriber", 0),
];

fn load(dump: &Dump) -> Content {
    let mut options: HashMap<String, Vec<u8>> = dump
        .table("wp_options")
        .map(|row| (string(&row[1]), text(&row[2])))
        .collect();
    // `WP_HOME` and `WP_SITEURL`, as the oracle defines them.
    for name in ["home", "siteurl"] {
        options.insert(name.into(), super::SITE.as_bytes().to_vec());
    }
    let names: HashMap<u64, (Vec<u8>, Vec<u8>, i64)> = dump
        .table("wp_terms")
        .map(|row| {
            let group = Value::String(text(&row[3])).to_int();
            (number(&row[0]), (text(&row[1]), text(&row[2]), group))
        })
        .collect();
    let terms = dump
        .table("wp_term_taxonomy")
        .map(|row| {
            let id = number(&row[1]);
            let (name, slug, group) = names[&id].clone();
            Row {
                taxonomy: string(&row[2]),
                term: Term {
                    id,
                    name,
                    slug,
                    description: text(&row[3]),
                    parent: number(&row[4]),
                    count: number(&row[5]),
                },
                group,
                taxonomy_id: number(&row[0]),
            }
        })
        .collect();
    let relationships = dump
        .table("wp_term_relationships")
        .map(|row| (number(&row[0]), number(&row[1])))
        .collect();
    let posts = dump
        .table("wp_posts")
        .map(|row| Post {
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
            ..Post::default()
        })
        .collect();
    let comments = dump
        .table("wp_comments")
        .map(|row| CommentRow {
            comment: Comment {
                id: number(&row[0]),
                post_id: number(&row[1]),
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
            },
            approved: string(&row[10]),
            agent: text(&row[11]),
            ..CommentRow::default()
        })
        .collect::<Vec<_>>();
    let comments = comments
        .into_iter()
        .map(|mut row| {
            let status = dump.table("wp_commentmeta").find(|meta| {
                number(&meta[1]) == row.comment.id && text(&meta[2]) == b"_wp_note_status"
            });
            row.note_status = status.map(|meta| text(&meta[3])).unwrap_or_default();
            row
        })
        .collect();
    let mut usermeta: Vec<(u64, String, Vec<u8>)> = dump
        .table("wp_usermeta")
        .map(|row| (number(&row[1]), string(&row[2]), text(&row[3])))
        .collect();
    for (id, role, level) in ADDED {
        let caps = format!("a:1:{{s:{}:\"{role}\";b:1;}}", role.len());
        usermeta.push((id, "wp_capabilities".into(), caps.into_bytes()));
        usermeta.push((id, "wp_user_level".into(), level.to_string().into_bytes()));
    }
    let postmeta = dump
        .table("wp_postmeta")
        .map(|row| (number(&row[1]), string(&row[2]), text(&row[3])))
        .collect();
    let meta = |id: u64, key: &str| {
        let found = usermeta
            .iter()
            .find(|(user, name, _)| *user == id && name == key);
        found.map(|(_, _, value)| value.clone()).unwrap_or_default()
    };
    let added = ADDED.iter().map(|(id, role, _)| {
        let user = User {
            id: *id,
            display_name: role.as_bytes().to_vec(),
            nicename: role.as_bytes().to_vec(),
            login: role.as_bytes().to_vec(),
            email: format!("{role}@example.test").into_bytes(),
            ..User::default()
        };
        (user, "2026-11-01 00:00:00".to_string())
    });
    let users = dump
        .table("wp_users")
        .map(|row| {
            let id = number(&row[0]);
            let user = User {
                id,
                login: text(&row[1]),
                nicename: text(&row[3]),
                email: text(&row[4]),
                url: text(&row[5]),
                display_name: text(&row[9]),
                first_name: meta(id, "first_name"),
                last_name: meta(id, "last_name"),
                description: meta(id, "description"),
            };
            (user, string(&row[6]))
        })
        .chain(added)
        .collect();
    Content {
        options,
        terms,
        relationships,
        posts,
        usermeta,
        comments,
        postmeta,
        users,
        reads: None,
    }
}

/// The default install.
pub(crate) fn default_install() -> &'static Content {
    static CONTENT: OnceLock<Content> = OnceLock::new();
    CONTENT.get_or_init(|| {
        let version = hyscribe_testkit::default_install_versions().pop().unwrap();
        load(&hyscribe_testkit::default_install(&version).unwrap())
    })
}

/// The default install with the rows of `state`, a dump's inserts.
pub(crate) fn default_install_with(state: &str) -> Content {
    let version = hyscribe_testkit::default_install_versions().pop().unwrap();
    let mut dump = hyscribe_testkit::default_install(&version).unwrap();
    dump.rows
        .extend(Dump::parse(state.as_bytes()).unwrap().rows);
    let dump: &'static Dump = Box::leak(Box::new(dump));
    let fixture = |pretty| super::site::Fixture {
        ttd: false,
        pretty,
        extra: false,
        state: Some(dump),
    };
    Content {
        reads: Some([fixture(false), fixture(true)]),
        ..load(dump)
    }
}

/// The default install with `reference/rest/people-more.sql`'s posts and
/// comments.
pub(super) fn default_install_more() -> &'static Content {
    static CONTENT: OnceLock<Content> = OnceLock::new();
    CONTENT.get_or_init(|| {
        default_install_with(include_str!("../../../../reference/rest/people-more.sql"))
    })
}

/// The Theme Test Data fixture, built by `reference/theme-test-data/build.sh`.
pub(super) fn theme_test_data() -> &'static Content {
    static CONTENT: OnceLock<Content> = OnceLock::new();
    CONTENT.get_or_init(|| {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../target/fixtures/theme-test-data/7.1.3/database.sql"
        );
        let sql = std::fs::read(path).expect("build it with reference/theme-test-data/build.sh");
        load(&Dump::parse(&sql).unwrap())
    })
}

/// MySQL's `utf8mb4_unicode_520_ci` order of plain text: case ignored.
pub(super) fn collate(a: &[u8], b: &[u8]) -> Ordering {
    let key = |text: &[u8]| String::from_utf8_lossy(text).to_lowercase();
    key(a).cmp(&key(b))
}

impl Content {
    /// The rows `query` selects, in its order, ties as the fixtures' query
    /// plans break them: the later term first.
    fn selected(&self, query: &TermQuery) -> Vec<&Row> {
        let lower = |bytes: &[u8]| String::from_utf8_lossy(bytes).to_lowercase();
        let mut found: Vec<&Row> = self
            .terms
            .iter()
            .filter(|row| {
                let term = &row.term;
                row.taxonomy == query.taxonomy
                    && (query.include.is_empty() || query.include.contains(&term.id))
                    && !query.exclude.contains(&term.id)
                    && (query.slugs.is_empty()
                        || query.slugs.iter().any(|s| lower(s) == lower(&term.slug)))
                    && query.parent.is_none_or(|parent| term.parent == parent)
                    && (!query.nonempty || term.count > 0)
                    && query.search.as_deref().is_none_or(|search| {
                        let search = search.to_lowercase();
                        lower(&term.name).contains(&search) || lower(&term.slug).contains(&search)
                    })
                    && (query.objects.is_empty()
                        || self.relationships.iter().any(|(object, tt)| {
                            *tt == row.taxonomy_id && query.objects.contains(object)
                        }))
            })
            .collect();
        found.sort_by_key(|row| std::cmp::Reverse(row.term.id));
        if let Some((order, descending)) = query.order {
            let position = |list: &[u64], id: u64| list.iter().position(|x| *x == id);
            found.sort_by(|a, b| {
                let (x, y) = (&a.term, &b.term);
                let ordering = match order {
                    TermOrder::Name => collate(&x.name, &y.name),
                    TermOrder::Slug => collate(&x.slug, &y.slug),
                    TermOrder::TermGroup => a.group.cmp(&b.group),
                    TermOrder::Count => x.count.cmp(&y.count),
                    TermOrder::Description => collate(&x.description, &y.description),
                    TermOrder::Include => {
                        position(&query.include, x.id).cmp(&position(&query.include, y.id))
                    }
                    TermOrder::Slugs => {
                        let at =
                            |slug: &[u8]| query.slugs.iter().position(|s| lower(s) == lower(slug));
                        at(&x.slug).cmp(&at(&y.slug))
                    }
                    _ => x.id.cmp(&y.id),
                };
                if descending {
                    ordering.reverse()
                } else {
                    ordering
                }
            });
        }
        found
    }

    pub(super) fn terms(&self, query: &TermQuery) -> Vec<Term> {
        let skip = usize::try_from(query.offset).unwrap();
        let take = query
            .limit
            .map_or(usize::MAX, |n| usize::try_from(n).unwrap());
        self.selected(query)
            .into_iter()
            .skip(skip)
            .take(take)
            .map(|row| row.term.clone())
            .collect()
    }

    pub(super) fn count_terms(&self, query: &TermQuery) -> u64 {
        self.selected(query).len() as u64
    }

    /// `get_term_link()`, plain or under `/%postname%/`.
    pub(super) fn term_link(&self, taxonomy: &str, term: &Term, pretty: bool) -> String {
        let site = super::SITE;
        match (taxonomy, pretty) {
            ("category", false) => format!("{site}/?cat={}", term.id),
            ("post_tag", false) => format!("{site}/?tag={}", String::from_utf8_lossy(&term.slug)),
            ("category" | "post_tag", true) => {
                let mut slugs = vec![String::from_utf8_lossy(&term.slug).into_owned()];
                let mut parent = term.parent;
                while let Some(row) = self
                    .terms
                    .iter()
                    .find(|row| row.taxonomy == taxonomy && row.term.id == parent && parent > 0)
                {
                    slugs.insert(0, String::from_utf8_lossy(&row.term.slug).into_owned());
                    parent = row.term.parent;
                }
                let base = if taxonomy == "category" {
                    "category"
                } else {
                    "tag"
                };
                format!("{site}/{base}/{}/", slugs.join("/"))
            }
            // A taxonomy without rewrite rules.
            _ => format!(
                "{site}/?taxonomy={taxonomy}&term={}",
                String::from_utf8_lossy(&term.slug)
            ),
        }
    }
}

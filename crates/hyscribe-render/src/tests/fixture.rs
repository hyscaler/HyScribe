// SPDX-License-Identifier: AGPL-3.0-or-later

//! A [`Site`] over the default install fixture dump, for block goldens
//! rendered by PHP WordPress on the same database (ADR 0009).

use std::cmp::Ordering;
use std::sync::OnceLock;

use hyscribe_model::{
    Comment, DateCount, FormatFilter, OrderBy, Period, Post, PostQuery, Term, TermFilter, User,
};
use hyscribe_phpser::Value;
use hyscribe_testkit::{Dump, SqlValue, default_install};

use crate::site::{Archive, Request, Site, Taxonomy};

fn dump() -> &'static Dump {
    static DUMP: OnceLock<Dump> = OnceLock::new();
    DUMP.get_or_init(|| default_install("7.1.3").unwrap())
}

fn text(value: &SqlValue) -> Vec<u8> {
    match value {
        SqlValue::Text(bytes) => bytes.clone(),
        SqlValue::Number(n) => n.clone().into_bytes(),
        SqlValue::Null => Vec::new(),
    }
}

fn number(value: &SqlValue) -> u64 {
    Value::String(text(value)).to_int() as u64
}

fn string(value: &SqlValue) -> String {
    String::from_utf8(text(value)).unwrap()
}

/// A `wp_posts` row, columns in table order.
fn post(row: &[SqlValue]) -> Post {
    Post {
        id: number(&row[0]),
        author: number(&row[1]),
        date: string(&row[2]),
        date_gmt: string(&row[3]),
        content: text(&row[4]),
        title: text(&row[5]),
        excerpt: text(&row[6]),
        status: string(&row[7]),
        password: text(&row[10]),
        name: text(&row[11]),
        modified: string(&row[14]),
        modified_gmt: string(&row[15]),
        parent: number(&row[17]),
        guid: text(&row[18]),
        menu_order: Value::String(text(&row[19])).to_int(),
        post_type: string(&row[20]),
        comment_count: Value::String(text(&row[22])).to_int(),
        comment_status: string(&row[8]),
        ping_status: string(&row[9]),
    }
}

/// The default install, seen from `home`.
pub(crate) struct DefaultInstall {
    pub(crate) home: Vec<u8>,
}

/// The dump's approved comments, of every type.
fn approved_comments() -> impl Iterator<Item = Comment> {
    dump()
        .table("wp_comments")
        .filter(|row| text(&row[10]) == b"1")
        .map(|row| Comment {
            id: number(&row[0]),
            post_id: number(&row[1]),
            author: text(&row[2]),
            author_email: text(&row[3]),
            author_url: text(&row[4]),
            date: string(&row[6]),
            date_gmt: string(&row[7]),
            content: text(&row[8]),
            comment_type: text(&row[12]),
            parent: number(&row[13]),
            user_id: number(&row[14]),
        })
}

impl DefaultInstall {
    /// The request state PHP gives the query strings the golden scripts use.
    pub(crate) fn request(&self, query: &str) -> Request {
        // `&forced=3` marks page 3 and `&child` a parent, both set by the
        // golden script in memory.
        if let Some(base) = query.strip_suffix("&forced=3") {
            return Request {
                paged: 3,
                ..self.request(base)
            };
        }
        if let Some(base) = query.strip_suffix("&child") {
            let mut request = self.request(base);
            request.posts.iter_mut().for_each(|post| post.parent = 7);
            return request;
        }
        if let Some(base) = query.strip_suffix("&book") {
            let mut request = self.request(base);
            request
                .posts
                .iter_mut()
                .for_each(|post| post.post_type = "book".into());
            return request;
        }
        if let Some(base) = query.strip_suffix("&slug") {
            let mut request = self.request(base);
            if let Some(Archive::Term(_, Some(term))) = &mut request.archive {
                term.slug = b"1".to_vec();
            }
            return request;
        }
        if let Some((base, _)) = query.split_once("&cpage=") {
            return Request {
                get: hyscribe_phpser::parse_str(query.as_bytes()),
                vars: hyscribe_phpser::parse_str(query.as_bytes()),
                ..self.request(base)
            };
        }
        // The `page` var only numbers a list page, which shows page 1.
        let (base, page) = query.rsplit_once("page=").unwrap_or((query, ""));
        if let (Ok(page), true) = (page.parse(), base.is_empty() || base.ends_with('&')) {
            return Request {
                page,
                ..self.request(base.trim_end_matches('&'))
            };
        }
        if query == "paged=1" {
            return Request {
                paged: 1,
                ..self.request("")
            };
        }
        let posts = self.main_query(query);
        Request {
            front_page: query.is_empty(),
            home: query.is_empty(),
            paged: if query == "p=1&paged=2" { 2 } else { 0 },
            singular: matches!(query, "p=1" | "page_id=2" | "p=1&paged=2"),
            not_found: matches!(query, "p=999" | "cat=999" | "paged=2" | "cat=1&paged=3"),
            post: posts.first().map(|post| post.id),
            posts,
            archive: match query {
                "cat=1" => Some(Archive::Term(
                    Taxonomy::Category,
                    self.post_terms(1, "category").into_iter().next(),
                )),
                "m=2026" => Some(Archive::Year),
                "m=202610" => Some(Archive::Month),
                "m=20261027" => Some(Archive::Day),
                _ => None,
            },
            date: query.starts_with("m="),
            search: query.strip_prefix("s=").map(|s| s.as_bytes().to_vec()),
            search_query: query
                .strip_prefix("s=")
                .unwrap_or_default()
                .as_bytes()
                .to_vec(),
            ..Request::default()
        }
    }

    /// The posts of the main query for a request the golden scripts use.
    pub(crate) fn main_query(&self, request: &str) -> Vec<Post> {
        match request {
            "" => self.query(&PostQuery {
                post_types: vec!["post".into()],
                limit: 10,
                ..PostQuery::default()
            }),
            "p=1" | "p=1&paged=2" | "cat=1" | "m=2026" | "m=202610" | "m=20261027" | "s=hello" => {
                self.post(1).into_iter().collect()
            }
            "page_id=2" => self.post(2).into_iter().collect(),
            _ => Vec::new(),
        }
    }
}

impl Site for DefaultInstall {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        dump()
            .table("wp_options")
            .find(|row| text(&row[1]) == name.as_bytes())
            .map(|row| text(&row[2]))
    }

    fn home_url(&self) -> Vec<u8> {
        self.home.clone()
    }

    fn post(&self, id: u64) -> Option<Post> {
        dump()
            .table("wp_posts")
            .map(post)
            .find(|post| post.id == id)
    }

    fn post_terms(&self, post_id: u64, taxonomy: &str) -> Vec<Term> {
        let dump = dump();
        dump.table("wp_term_relationships")
            .filter(|row| number(&row[0]) == post_id)
            .filter_map(|row| {
                let taxonomy_row = dump.table("wp_term_taxonomy").find(|t| {
                    number(&t[0]) == number(&row[1]) && text(&t[2]) == taxonomy.as_bytes()
                })?;
                let term = dump
                    .table("wp_terms")
                    .find(|t| number(&t[0]) == number(&taxonomy_row[1]))?;
                Some(Term {
                    id: number(&term[0]),
                    name: text(&term[1]),
                    slug: text(&term[2]),
                    description: Vec::new(),
                    parent: 0,
                })
            })
            .collect()
    }

    fn post_meta(&self, post_id: u64, key: &str) -> Option<Vec<u8>> {
        dump()
            .table("wp_postmeta")
            .find(|row| number(&row[1]) == post_id && text(&row[2]) == key.as_bytes())
            .map(|row| text(&row[3]))
    }

    /// The dump's published posts grouped as `wp_get_archives()` groups
    /// them. Its one post makes a week the same group as its day.
    fn date_counts(&self, period: Period) -> Vec<DateCount> {
        let mut counts: Vec<DateCount> = Vec::new();
        let posts = dump().table("wp_posts").map(post);
        for p in posts.filter(|p| p.status == "publish" && p.post_type == "post") {
            let part = |at: std::ops::Range<usize>| p.date.get(at).unwrap().parse().unwrap();
            let (year, month, day) = (part(0..4), part(5..7), part(8..10));
            let (month, day) = match period {
                Period::Year => (0, 0),
                Period::Month => (month, 0),
                Period::Day | Period::Week(_) => (month, day),
            };
            let week = match period {
                Period::Week(start) => mysql_week(year, month, day, start),
                _ => 0,
            };
            let key = DateCount {
                year,
                month,
                day,
                week,
                posts: 1,
            };
            let same = |c: &&mut DateCount| {
                (c.year, c.month, c.day, c.week) == (key.year, key.month, key.day, key.week)
            };
            match counts.iter_mut().find(same) {
                Some(count) => count.posts += 1,
                None => counts.push(key),
            }
        }
        counts.sort_by_key(|c| std::cmp::Reverse((c.year, c.month, c.day)));
        counts
    }

    /// Ties keep table order, as the fixture's posts share one date.
    /// Over the dump's posts, as `get_adjacent_post()` orders them.
    fn adjacent_post(
        &self,
        post: &Post,
        previous: bool,
        terms: Option<(&str, &[u64])>,
    ) -> Option<Post> {
        let key = |p: &Post| (p.date.clone(), p.id);
        let mut found: Vec<Post> = dump()
            .table("wp_posts")
            .map(crate::fixture::post)
            .filter(|p| p.status == "publish" && p.post_type == post.post_type)
            .filter(|p| {
                if previous {
                    key(p) < key(post)
                } else {
                    key(p) > key(post)
                }
            })
            .filter(|p| {
                terms.is_none_or(|(taxonomy, ids)| {
                    self.post_terms(p.id, taxonomy)
                        .iter()
                        .any(|term| ids.contains(&term.id))
                })
            })
            .collect();
        found.sort_by_key(key);
        if previous {
            found.pop()
        } else {
            found.into_iter().next()
        }
    }

    fn comments(&self, post_id: u64) -> Vec<Comment> {
        let mut comments: Vec<Comment> = approved_comments()
            .filter(|c| c.post_id == post_id && c.comment_type != b"note")
            .collect();
        comments.sort_by(|a, b| (&a.date_gmt, a.id).cmp(&(&b.date_gmt, b.id)));
        comments
    }

    fn comment(&self, id: u64) -> Option<Comment> {
        approved_comments().find(|comment| comment.id == id)
    }

    fn user(&self, id: u64) -> Option<User> {
        dump()
            .table("wp_users")
            .find(|row| number(&row[0]) == id)
            .map(|row| {
                let meta = |key: &[u8]| {
                    dump()
                        .table("wp_usermeta")
                        .find(|meta| number(&meta[1]) == id && text(&meta[2]) == key)
                        .map(|meta| text(&meta[3]))
                        .unwrap_or_default()
                };
                User {
                    id,
                    display_name: text(&row[9]),
                    nicename: text(&row[3]),
                    login: text(&row[1]),
                    email: text(&row[4]),
                    url: text(&row[5]),
                    first_name: meta(b"first_name"),
                    last_name: meta(b"last_name"),
                    description: meta(b"description"),
                }
            })
    }

    fn query(&self, query: &PostQuery) -> Vec<Post> {
        select(
            dump().table("wp_posts").map(post).collect(),
            query,
            &|id, taxonomy| self.post_terms(id, taxonomy),
        )
    }
}

/// What `WP_Query` finds of `posts` for `query`, with `terms_of` giving a
/// post's terms in a taxonomy. Searches ignore ASCII case; sorts compare
/// bytes, not in the database's case-insensitive collation.
pub fn select(
    posts: Vec<Post>,
    query: &PostQuery,
    terms_of: &dyn Fn(u64, &str) -> Vec<Term>,
) -> Vec<Post> {
    let has_term = |post: &Post, filter: &TermFilter| {
        terms_of(post.id, &filter.taxonomy)
            .iter()
            .any(|term| filter.terms.contains(&term.id))
    };
    let has_format = |post: &Post, formats: &FormatFilter| {
        let terms = terms_of(post.id, "post_format");
        (formats.standard && terms.is_empty())
            || terms.iter().any(|term| {
                formats
                    .slugs
                    .iter()
                    .any(|slug| slug.as_bytes() == term.slug)
            })
    };
    let mut found: Vec<Post> = posts
        .iter()
        .filter(|post| post.status == "publish" && query.post_types.contains(&post.post_type))
        .filter(|post| {
            query
                .include
                .as_ref()
                .is_none_or(|ids| ids.contains(&post.id))
        })
        .filter(|post| !query.exclude.contains(&post.id))
        .filter(|post| query.authors.is_empty() || query.authors.contains(&post.author))
        .filter(|post| query.parents.is_empty() || query.parents.contains(&post.parent))
        .filter(|post| query.terms.iter().all(|f| has_term(post, f) != f.exclude))
        .filter(|post| query.formats.as_ref().is_none_or(|f| has_format(post, f)))
        .filter(|post| {
            query.search.iter().all(|term| {
                [&post.title, &post.excerpt, &post.content]
                    .iter()
                    .any(|text| contains(text, term))
            })
        })
        .cloned()
        .collect();
    let order = if query.order.is_empty() {
        &[(OrderBy::Date, false)][..]
    } else {
        &query.order[..]
    };
    let phrase = query.search.join(" ");
    found.sort_by(|a, b| {
        order
            .iter()
            .fold(Ordering::Equal, |done, &(column, ascending)| {
                done.then_with(|| {
                    let by = column_order(a, b, column, &phrase);
                    if ascending { by } else { by.reverse() }
                })
            })
    });
    let page: Vec<Post> = found
        .into_iter()
        .skip(query.offset as usize)
        .take(query.limit as usize)
        .collect();
    // Sticky posts first: those found, then the others not excluded.
    let (mut stuck, rest): (Vec<Post>, Vec<Post>) = page
        .into_iter()
        .partition(|post| query.sticky.contains(&post.id));
    let missing: Vec<u64> = query
        .sticky
        .iter()
        .copied()
        .filter(|id| !stuck.iter().any(|post| post.id == *id) && !query.exclude.contains(id))
        .collect();
    if !missing.is_empty() {
        let stickies = PostQuery {
            post_types: query.post_types.clone(),
            limit: u32::MAX,
            include: Some(missing),
            ..PostQuery::default()
        };
        stuck.extend(select(posts, &stickies, terms_of));
    }
    stuck.extend(rest);
    stuck
}

/// Whether `text` contains `term`, in any ASCII case.
fn contains(text: &[u8], term: &str) -> bool {
    let term = term.to_ascii_lowercase();
    text.to_ascii_lowercase()
        .windows(term.len().max(1))
        .any(|w| w == term.as_bytes())
}

/// `a` against `b` by `column`, relevance to the search `phrase` included.
fn column_order(a: &Post, b: &Post, column: OrderBy, phrase: &str) -> Ordering {
    match column {
        OrderBy::Date => a.date.cmp(&b.date),
        OrderBy::Title => a.title.cmp(&b.title),
        OrderBy::Modified => a.modified.cmp(&b.modified),
        OrderBy::Name => a.name.cmp(&b.name),
        OrderBy::Author => a.author.cmp(&b.author),
        OrderBy::Parent => a.parent.cmp(&b.parent),
        OrderBy::Type => a.post_type.cmp(&b.post_type),
        OrderBy::Id => a.id.cmp(&b.id),
        OrderBy::MenuOrder => a.menu_order.cmp(&b.menu_order),
        OrderBy::CommentCount => a.comment_count.cmp(&b.comment_count),
        OrderBy::Relevance => contains(&a.title, phrase).cmp(&contains(&b.title, phrase)),
    }
}

/// `_wp_mysql_week()`'s week of a day as MySQL numbers it: `WEEK( d, 1 )`
/// from Monday, `WEEK( d - start days, 0 )` from Tuesday to Saturday,
/// else `WEEK( d, 0 )`.
fn mysql_week(year: u64, month: u64, day: u64, start: i64) -> u64 {
    use crate::date::{days_from_civil, weekday};
    let year = i64::try_from(year).unwrap();
    let mut date = days_from_civil(year, month as i64, day as i64);
    if (2..=6).contains(&start) {
        date -= start;
    }
    let mut y = year;
    while days_from_civil(y, 1, 1) > date {
        y -= 1;
    }
    let first = days_from_civil(y, 1, 1);
    let monday = start == 1;
    // 1 January's weekday from Monday (mode 1) or Sunday (mode 0).
    let weekday = if monday {
        (weekday(first) + 6) % 7
    } else {
        weekday(first)
    };
    let week_one = if monday && weekday <= 3 {
        first - weekday
    } else {
        first + (7 - weekday) % 7
    };
    if date < week_one {
        0
    } else {
        u64::try_from((date - week_one) / 7 + 1).unwrap()
    }
}

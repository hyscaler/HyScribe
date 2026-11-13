// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Theme Test Data fixture, built by `reference/theme-test-data/build.sh`
//! under `target/`, for the media goldens: its posts, their meta and the
//! users' names, and the attachment queries over them.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::OnceLock;

use hyscribe_model::{OrderBy, Post, PostQuery};
use hyscribe_phpser::Value;
use hyscribe_testkit::{Dump, SqlValue};

struct Fixture {
    posts: Vec<Post>,
    meta: HashMap<(u64, Vec<u8>), Vec<u8>>,
    names: HashMap<u64, Vec<u8>>,
}

/// A value's text, or a number's digits: the posts' columns hold no NULL.
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

fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../target/fixtures/theme-test-data/7.1.3/database.sql"
        );
        let sql = std::fs::read(path).expect("build it with reference/theme-test-data/build.sh");
        let dump = Dump::parse(&sql).unwrap();
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
                password: text(&row[10]),
                name: text(&row[11]),
                parent: number(&row[17]),
                guid: text(&row[18]),
                post_type: string(&row[20]),
                mime_type: string(&row[21]),
                comment_count: Value::String(text(&row[22])).to_int(),
                ..Post::default()
            })
            .collect();
        let meta = dump
            .table("wp_postmeta")
            .map(|row| ((number(&row[1]), text(&row[2])), text(&row[3])))
            .collect();
        let names = dump
            .table("wp_users")
            .map(|row| (number(&row[0]), text(&row[9])))
            .collect();
        Fixture { posts, meta, names }
    })
}

/// Every post of the fixture.
pub(crate) fn posts() -> Vec<Post> {
    fixture().posts.clone()
}

/// A post's meta `key`.
pub(crate) fn meta(id: u64, key: &str) -> Option<Vec<u8>> {
    fixture().meta.get(&(id, key.as_bytes().to_vec())).cloned()
}

/// A user's display name.
pub(crate) fn display_name(id: u64) -> Option<Vec<u8>> {
    fixture().names.get(&id).cloned()
}

/// `LIKE` as MySQL compares text without case, for the patterns the
/// goldens use: a type's group, `image/%`, or a search's `%term%`.
fn like(value: &[u8], pattern: &str) -> bool {
    let value = String::from_utf8_lossy(value).to_lowercase();
    let inner = pattern.trim_matches('%').to_lowercase();
    if pattern.starts_with('%') {
        value.contains(&inner)
    } else {
        value.starts_with(&inner)
    }
}

/// Whether `post` holds the search `term`, in its file's name too when
/// `filenames`.
fn searched(post: &Post, term: &str, filenames: bool) -> bool {
    let pattern = format!("%{term}%");
    let file = meta(post.id, "_wp_attached_file").filter(|_| filenames);
    [
        Some(&post.title),
        Some(&post.excerpt),
        Some(&post.content),
        file.as_ref(),
    ]
    .iter()
    .flatten()
    .any(|field| like(field, &pattern))
}

/// What a query of attachments finds: the page asked and how many in all,
/// ties in the order of IDs the way the first column sorts.
pub(crate) fn list(posts: Vec<Post>, query: &PostQuery) -> (Vec<Post>, u64) {
    let mut found: Vec<Post> = posts
        .into_iter()
        .filter(|post| {
            let date = |from: usize, len: usize| {
                post.date
                    .get(from..from + len)
                    .and_then(|part| part.parse::<u64>().ok())
            };
            query.post_types.contains(&post.post_type)
                && query.statuses.iter().any(|f| f.status == post.status)
                && (query.mime_types.is_empty()
                    || query
                        .mime_types
                        .iter()
                        .any(|pattern| like(post.mime_type.as_bytes(), pattern)))
                && (query.parents.is_empty() || query.parents.contains(&post.parent))
                && (query.authors.is_empty() || query.authors.contains(&post.author))
                && !query.excluded_authors.contains(&post.author)
                && !query.exclude.contains(&post.id)
                && query.year.is_none_or(|year| date(0, 4) == Some(year))
                && query.month.is_none_or(|month| date(5, 2) == Some(month))
                && query
                    .search
                    .iter()
                    .all(|term| searched(post, term, query.filenames))
        })
        .collect();
    let first_ascending = query.order.first().is_some_and(|(_, asc)| *asc);
    found.sort_by(|a, b| {
        let by = |column: OrderBy| -> Ordering {
            match column {
                OrderBy::Title => a
                    .title
                    .to_ascii_lowercase()
                    .cmp(&b.title.to_ascii_lowercase()),
                OrderBy::Author => a.author.cmp(&b.author),
                OrderBy::Parent => a.parent.cmp(&b.parent),
                // Titles holding the first term come first.
                OrderBy::Relevance => {
                    let term = format!("%{}%", query.search[0]);
                    like(&b.title, &term).cmp(&like(&a.title, &term))
                }
                _ => a.date.cmp(&b.date),
            }
        };
        let id = if first_ascending {
            a.id.cmp(&b.id)
        } else {
            b.id.cmp(&a.id)
        };
        query
            .order
            .iter()
            .map(|&(column, ascending)| {
                let order = by(column);
                if ascending || column == OrderBy::Relevance {
                    order
                } else {
                    order.reverse()
                }
            })
            .chain([id])
            .find(|order| order.is_ne())
            .unwrap_or(Ordering::Equal)
    });
    let total = found.len() as u64;
    let page = found
        .into_iter()
        .skip(query.offset as usize)
        .take(query.limit as usize)
        .collect();
    (page, total)
}

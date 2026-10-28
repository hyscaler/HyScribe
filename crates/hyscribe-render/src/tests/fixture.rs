// SPDX-License-Identifier: AGPL-3.0-or-later

//! A [`Site`] over the default install fixture dump, for block goldens
//! rendered by PHP WordPress on the same database (ADR 0009).

use std::sync::OnceLock;

use hyscribe_model::{Comment, Post, PostQuery, Term, User};
use hyscribe_phpser::Value;
use hyscribe_testkit::{Dump, SqlValue, default_install};

use crate::site::{Archive, Request, Site};

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
                paged: true,
                page: 3,
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
            if let Some(Archive::Category(term)) = &mut request.archive {
                term.slug = b"1".to_vec();
            }
            return request;
        }
        if query == "paged=1" {
            return Request {
                page: 1,
                ..self.request("")
            };
        }
        let posts = self.main_query(query);
        Request {
            front_page: query.is_empty(),
            home: query.is_empty(),
            paged: query == "p=1&paged=2",
            singular: matches!(query, "p=1" | "page_id=2" | "p=1&paged=2"),
            not_found: matches!(query, "p=999" | "cat=999" | "paged=2" | "cat=1&paged=3"),
            page: if query == "p=1&paged=2" { 2 } else { 0 },
            post: posts.first().map(|post| post.id),
            posts,
            archive: match query {
                "cat=1" => Some(Archive::Category(self.post_terms(1, "category").remove(0))),
                "m=2026" => Some(Archive::Year),
                "m=202610" => Some(Archive::Month),
                "m=20261027" => Some(Archive::Day),
                _ => None,
            },
            date: query.starts_with("m="),
            search: query.strip_prefix("s=").map(|s| s.as_bytes().to_vec()),
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
                    first_name: meta(b"first_name"),
                    last_name: meta(b"last_name"),
                }
            })
    }

    fn query(&self, query: &PostQuery) -> Vec<Post> {
        let mut posts: Vec<Post> = dump()
            .table("wp_posts")
            .map(post)
            .filter(|post| post.status == "publish" && query.post_types.contains(&post.post_type))
            .filter(|post| {
                query
                    .include
                    .as_ref()
                    .is_none_or(|ids| ids.contains(&post.id))
            })
            .filter(|post| !query.exclude.contains(&post.id))
            .collect();
        posts.sort_by(|a, b| {
            if query.by_menu_order {
                (a.menu_order, &a.title).cmp(&(b.menu_order, &b.title))
            } else if query.ascending {
                a.date.cmp(&b.date)
            } else {
                b.date.cmp(&a.date)
            }
        });
        posts
            .into_iter()
            .skip(query.offset as usize)
            .take(query.limit as usize)
            .collect()
    }
}

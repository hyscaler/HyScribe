// SPDX-License-Identifier: AGPL-3.0-or-later

//! A [`Site`] over the default install fixture dump, for block goldens
//! rendered by PHP WordPress on the same database (ADR 0009).

use std::sync::OnceLock;

use hyscribe_model::{Post, PostQuery, Term};
use hyscribe_phpser::Value;
use hyscribe_testkit::{Dump, SqlValue, default_install};

use crate::site::Site;

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
    }
}

/// The default install, seen from `home`.
pub(crate) struct DefaultInstall {
    pub(crate) home: Vec<u8>,
}

impl DefaultInstall {
    /// The posts of the main query for a request the golden scripts use.
    pub(crate) fn main_query(&self, request: &str) -> Vec<Post> {
        match request {
            "" => self.query(&PostQuery {
                post_types: vec!["post".into()],
                limit: 10,
                ..PostQuery::default()
            }),
            "p=1" => self.post(1).into_iter().collect(),
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
            if query.ascending {
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

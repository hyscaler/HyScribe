// SPDX-License-Identifier: AGPL-3.0-or-later

//! What one request's rendering reads from the database: render's `Site`
//! over `hyscribe-db` (ADR 0009). It runs on a blocking thread and waits on
//! each query; reads are memoized for the request, as WordPress's object
//! cache keeps them, and the first database error is kept so the page can
//! fail as a whole.

use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;

use hyscribe_db::DbError;
use hyscribe_model::{Comment, Post, PostQuery, Term, User};
use hyscribe_render::site::Site as RenderSite;
use tokio::runtime::Handle;

use crate::Site;

/// A memo of reads keyed by post and name.
type ByPost<V> = RefCell<HashMap<(u64, String), V>>;

pub(crate) struct Reads<'a> {
    site: &'a Site,
    handle: Handle,
    error: RefCell<Option<DbError>>,
    options: RefCell<HashMap<String, Option<Vec<u8>>>>,
    posts: RefCell<HashMap<u64, Option<Post>>>,
    users: RefCell<HashMap<u64, Option<User>>>,
    comments: RefCell<HashMap<u64, Vec<Comment>>>,
    terms: ByPost<Vec<Term>>,
    meta: ByPost<Option<Vec<u8>>>,
}

impl<'a> Reads<'a> {
    pub(crate) fn new(site: &'a Site, handle: Handle) -> Reads<'a> {
        Reads {
            site,
            handle,
            error: RefCell::new(None),
            options: RefCell::default(),
            posts: RefCell::default(),
            users: RefCell::default(),
            comments: RefCell::default(),
            terms: RefCell::default(),
            meta: RefCell::default(),
        }
    }

    /// The first database error a read met, if any.
    pub(crate) fn finish(self) -> Result<(), DbError> {
        self.error.into_inner().map_or(Ok(()), Err)
    }

    /// The query's result, or its type's default after an error.
    fn wait<T: Default>(&self, query: impl Future<Output = Result<T, DbError>>) -> T {
        self.handle.block_on(query).unwrap_or_else(|err| {
            self.error.borrow_mut().get_or_insert(err);
            T::default()
        })
    }

    /// `key`'s entry in `cache`, read with `read` the first time.
    fn memo<K: std::hash::Hash + Eq, V: Clone>(
        &self,
        cache: &RefCell<HashMap<K, V>>,
        key: K,
        read: impl FnOnce() -> V,
    ) -> V {
        if let Some(value) = cache.borrow().get(&key) {
            return value.clone();
        }
        let value = read();
        cache.borrow_mut().insert(key, value.clone());
        value
    }
}

impl RenderSite for Reads<'_> {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        match self.site.options.get(name) {
            Some(value) => Some(value.clone()),
            None => self.memo(&self.options, name.to_string(), || {
                self.wait(self.site.db.option(name))
            }),
        }
    }

    fn home_url(&self) -> Vec<u8> {
        self.site.home.clone().into_bytes()
    }

    fn post(&self, id: u64) -> Option<Post> {
        self.memo(&self.posts, id, || self.wait(self.site.db.post(id)))
    }

    fn post_terms(&self, post_id: u64, taxonomy: &str) -> Vec<Term> {
        self.memo(&self.terms, (post_id, taxonomy.to_string()), || {
            self.wait(self.site.db.post_terms(post_id, taxonomy))
        })
    }

    fn post_meta(&self, post_id: u64, key: &str) -> Option<Vec<u8>> {
        self.memo(&self.meta, (post_id, key.to_string()), || {
            self.wait(self.site.db.post_meta(post_id, key))
        })
    }

    fn query(&self, query: &PostQuery) -> Vec<Post> {
        self.wait(self.site.db.posts(query))
    }

    fn adjacent_post(
        &self,
        post: &Post,
        previous: bool,
        terms: Option<(&str, &[u64])>,
    ) -> Option<Post> {
        self.wait(self.site.db.adjacent_post(post, previous, terms))
    }

    fn comments(&self, post_id: u64) -> Vec<Comment> {
        self.memo(&self.comments, post_id, || {
            self.wait(self.site.db.comments(post_id))
        })
    }

    fn user(&self, id: u64) -> Option<User> {
        self.memo(&self.users, id, || self.wait(self.site.db.user(id)))
    }
}

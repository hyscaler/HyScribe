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
use hyscribe_model::{
    CalendarMonth, Comment, DateCount, Period, Post, PostQuery, Term, TermQuery, User,
};
use hyscribe_render::outbound::{Get, Response};
use hyscribe_render::site::Site as RenderSite;
use tokio::runtime::Handle;

use crate::Site;
use crate::outbound::{Write, remote_get, write};

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
    comment: RefCell<HashMap<u64, Option<Comment>>>,
    terms: ByPost<Vec<Term>>,
    term: ByPost<Option<Term>>,
    meta: ByPost<Option<Vec<u8>>>,
    date_counts: RefCell<HashMap<Period, Vec<DateCount>>>,
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
            comment: RefCell::default(),
            terms: RefCell::default(),
            term: RefCell::default(),
            meta: RefCell::default(),
            date_counts: RefCell::default(),
        }
    }

    /// The first database error a read met, if any.
    pub(crate) fn finish(self) -> Result<(), DbError> {
        self.error.into_inner().map_or(Ok(()), Err)
    }

    /// The query's result, or its type's default after an error.
    /// `future` run to its end on the runtime.
    pub(crate) fn block_on<T>(&self, future: impl Future<Output = T>) -> T {
        self.handle.block_on(future)
    }

    pub(crate) fn wait<T: Default>(&self, query: impl Future<Output = Result<T, DbError>>) -> T {
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

impl Reads<'_> {
    /// Drops the memo of a transient just written.
    fn forget_transient(&self, name: &str) {
        let mut options = self.options.borrow_mut();
        options.remove(&format!("_site_transient_{name}"));
        options.remove(&format!("_site_transient_timeout_{name}"));
    }
}

impl RenderSite for Reads<'_> {
    fn remote_get(&self, url: &[u8], get: &Get) -> Option<Result<Response, Vec<u8>>> {
        remote_get(self.site, self, url, get)
    }

    fn feed_allowed(&self, host: &[u8]) -> bool {
        self.site.feed_hosts.iter().any(|allowed| allowed == host)
    }

    fn update_post_meta(&self, post_id: u64, key: &str, value: &[u8]) {
        write(self.site, self, Write::Meta(post_id, key, value));
        if self.site.writer.is_some() {
            let written = Some(value.to_vec());
            self.meta
                .borrow_mut()
                .insert((post_id, key.to_string()), written);
        }
    }

    fn set_site_transient(&self, name: &str, value: &[u8], expiration: i64) {
        write(self.site, self, Write::Transient(name, value, expiration));
        self.forget_transient(name);
    }

    fn delete_site_transient(&self, name: &str) {
        write(self.site, self, Write::Expire(name));
        self.forget_transient(name);
    }

    fn wp_hash(&self, data: &[u8]) -> Option<String> {
        let salt = self.site.salt("auth", |name| self.option(name))?;
        Some(hyscribe_phpser::hash_hmac_md5(data, &salt))
    }

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

    fn post_meta_all(&self, post_id: u64, key: &str) -> Vec<Vec<u8>> {
        self.wait(self.site.db.post_meta_all(post_id, key))
    }

    fn query(&self, query: &PostQuery) -> Vec<Post> {
        self.wait(self.site.db.posts(query))
    }

    fn count_posts(&self, query: &PostQuery) -> u64 {
        self.wait(self.site.db.count_posts(query))
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

    fn older_comments(&self, post_id: u64, date_gmt: &str) -> u64 {
        self.wait(self.site.db.older_comments(post_id, date_gmt))
    }

    fn cloud_terms(&self, taxonomy: &str, limit: u64) -> Vec<Term> {
        self.wait(self.site.db.cloud_terms(taxonomy, limit))
    }

    fn term_query(&self, query: &TermQuery) -> Vec<Term> {
        self.wait(self.site.db.term_query(query))
    }

    fn taxonomy_terms(&self, taxonomy: &str) -> Vec<Term> {
        self.wait(self.site.db.taxonomy_terms(taxonomy))
    }

    fn term_posts(&self, taxonomy: &str) -> Vec<(u64, u64)> {
        self.wait(self.site.db.term_posts(taxonomy))
    }

    fn has_published_posts(&self) -> bool {
        self.wait(self.site.db.has_published_posts("post"))
    }

    fn calendar_month(&self, year: i64, month: u64, last_day: u64) -> CalendarMonth {
        self.wait(self.site.db.calendar_month(year, month, last_day))
    }

    fn recent_comments(&self, limit: u64) -> Vec<Comment> {
        self.wait(self.site.db.recent_comments(limit))
    }

    fn comment(&self, id: u64) -> Option<Comment> {
        self.memo(&self.comment, id, || self.wait(self.site.db.comment(id)))
    }

    fn held_comment(&self, id: u64) -> Option<Comment> {
        self.wait(self.site.db.held_comment(id))
    }

    fn any_comment(&self, id: u64) -> Option<Comment> {
        self.wait(self.site.db.any_comment(id))
    }

    fn held_comments(&self, post_id: u64, email: &[u8], only: Option<i64>) -> Vec<Comment> {
        self.wait(self.site.db.held_comments(post_id, email, only))
    }

    fn user(&self, id: u64) -> Option<User> {
        self.memo(&self.users, id, || self.wait(self.site.db.user(id)))
    }

    fn date_counts(&self, period: Period) -> Vec<DateCount> {
        self.memo(&self.date_counts, period, || {
            self.wait(self.site.db.date_counts(period))
        })
    }

    fn term(&self, id: u64, taxonomy: &str) -> Option<Term> {
        self.memo(&self.term, (id, taxonomy.to_string()), || {
            self.wait(self.site.db.term(id, taxonomy))
        })
    }
}

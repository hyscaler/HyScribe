// SPDX-License-Identifier: AGPL-3.0-or-later

//! What dynamic blocks read from the site and the request (ADR 0009).

use hyscribe_model::{
    CalendarMonth, Comment, DateCount, Period, Post, PostQuery, Term, TermQuery, User,
};
use hyscribe_phpser::Value;

pub use crate::outbound::{Get, Hooks, Response};

/// The site's data, as blocks read it while rendering.
pub trait Site {
    /// `get_option( $name )` as stored, `None` when the option is missing.
    fn option(&self, name: &str) -> Option<Vec<u8>>;

    /// `home_url()`: the `home` option, or what `WP_HOME` sets.
    fn home_url(&self) -> Vec<u8>;

    /// `get_post( $id )`, whatever its type and status.
    fn post(&self, id: u64) -> Option<Post>;

    /// `get_the_terms( $post_id, $taxonomy )`, in WordPress's order.
    fn post_terms(&self, post_id: u64, taxonomy: &str) -> Vec<Term>;

    /// `get_post_meta( $post_id, $key, true )`, `None` when unset.
    fn post_meta(&self, post_id: u64, key: &str) -> Option<Vec<u8>>;

    /// `get_post_meta( $post_id, $key )`: every value, in the order stored.
    fn post_meta_all(&self, post_id: u64, key: &str) -> Vec<Vec<u8>> {
        self.post_meta(post_id, key).into_iter().collect()
    }

    /// The published posts a `WP_Query` with `query`'s arguments finds.
    fn query(&self, query: &PostQuery) -> Vec<Post>;

    /// `found_posts` of that query: its posts on every page.
    fn count_posts(&self, query: &PostQuery) -> u64 {
        let all = PostQuery {
            limit: u32::MAX,
            offset: 0,
            ..query.clone()
        };
        self.query(&all).len() as u64
    }

    /// A post's approved comments, oldest first, then by ID. Sites without
    /// comment data have none.
    fn comments(&self, _post_id: u64) -> Vec<Comment> {
        vec![]
    }

    /// How many approved top level comments of every type, notes too,
    /// `post_id` has from before `date_gmt`: `get_page_of_comment()`'s
    /// count. Sites without notes count their comments.
    fn older_comments(&self, post_id: u64, date_gmt: &str) -> u64 {
        let comments = self.comments(post_id);
        let older = comments
            .iter()
            .filter(|c| c.parent == 0 && c.date_gmt.as_str() < date_gmt);
        older.count() as u64
    }

    /// The approved comments on published posts, newest first, `limit` of
    /// them. Sites without comment data have none.
    fn recent_comments(&self, _limit: u64) -> Vec<Comment> {
        vec![]
    }

    /// `get_comment( $id )` when that comment is approved, of any type.
    /// Sites without comment data have none.
    fn comment(&self, _id: u64) -> Option<Comment> {
        None
    }

    /// `get_comment( $id )` when that comment is held for moderation.
    /// Sites without comment data have none.
    fn held_comment(&self, _id: u64) -> Option<Comment> {
        None
    }

    /// `get_comment( $id )` in any state: approved, held, spam or trashed.
    /// Sites without comment data have none; by default a comment this
    /// site shows, approved or held.
    fn any_comment(&self, id: u64) -> Option<Comment> {
        self.comment(id).or_else(|| self.held_comment(id))
    }

    /// The comments of `post_id` held for moderation, notes left out, whose
    /// author's email is `email`, oldest first then by ID; only comment
    /// `only` when set: `include_unapproved` of a comment query. Sites
    /// without comment data have none.
    fn held_comments(&self, _post_id: u64, _email: &[u8], _only: Option<i64>) -> Vec<Comment> {
        vec![]
    }

    /// `get_userdata( $id )`. Sites without user data have none.
    fn user(&self, _id: u64) -> Option<User> {
        None
    }

    /// `wp_hash( $data )`, `None` without the salts it signs with.
    fn wp_hash(&self, _data: &[u8]) -> Option<String> {
        None
    }

    /// `get_term( $id, $taxonomy )`. Sites without term data have none.
    fn term(&self, _id: u64, _taxonomy: &str) -> Option<Term> {
        None
    }

    /// `get_terms()` for a tag cloud: the terms of `taxonomy` with posts,
    /// the most used first, `limit` of them, with their post counts.
    /// Sites without term data have none.
    fn cloud_terms(&self, _taxonomy: &str, _limit: u64) -> Vec<Term> {
        vec![]
    }

    /// The terms `WP_Term_Query`'s SQL selects, in its order. Sites
    /// without term data have none.
    fn term_query(&self, _query: &TermQuery) -> Vec<Term> {
        vec![]
    }

    /// Every term of `taxonomy` with its post count, by name. Sites
    /// without term data have none.
    fn taxonomy_terms(&self, _taxonomy: &str) -> Vec<Term> {
        vec![]
    }

    /// The published posts in each term of `taxonomy`, as `(term ID, post
    /// ID)`. Sites without term data have none.
    fn term_posts(&self, _taxonomy: &str) -> Vec<(u64, u64)> {
        vec![]
    }

    /// Whether a post is published. Sites without post data have none.
    fn has_published_posts(&self) -> bool {
        false
    }

    /// `get_calendar()`'s reads of a month with `last_day` days. Sites
    /// without post data have no posts in it or around it.
    fn calendar_month(&self, _year: i64, _month: u64, _last_day: u64) -> CalendarMonth {
        Default::default()
    }

    /// `wp_get_archives()`'s periods of published posts, newest first.
    /// Sites without post data have none.
    fn date_counts(&self, _period: Period) -> Vec<DateCount> {
        vec![]
    }

    /// `get_adjacent_post()` for a visitor: the published post of `post`'s
    /// type just before (`previous`) or after it by date, then ID, sharing
    /// one of `terms` in their taxonomy when given. Sites without other
    /// posts have none.
    fn adjacent_post(
        &self,
        _post: &Post,
        _previous: bool,
        _terms: Option<(&str, &[u64])>,
    ) -> Option<Post> {
        None
    }

    /// `wp_safe_remote_get( $url, $args )` through the outbound client
    /// (ADR 0019): the response, or `WP_Error`'s message. `None` while
    /// `HTTP_OUTBOUND` is off, as sites without a client fetch nothing.
    fn remote_get(&self, _url: &[u8], _args: &Get) -> Option<Result<Response, Vec<u8>>> {
        None
    }

    /// Whether the RSS block may fetch from `host`, in lower case: whether
    /// `RSS_ALLOWED_HOSTS` names it. None may by default.
    fn feed_allowed(&self, _host: &[u8]) -> bool {
        false
    }

    /// `update_post_meta( $post_id, $key, $value )`, which a page view
    /// writes in primary mode only: shadow mode and tests write nothing.
    fn update_post_meta(&self, _post_id: u64, _key: &str, _value: &[u8]) {}

    /// `set_site_transient( $name, $value, $expiration )`, the value
    /// serialized; written in primary mode only.
    fn set_site_transient(&self, _name: &str, _value: &[u8], _expiration: i64) {}

    /// `delete_site_transient( $name )`; written in primary mode only.
    fn delete_site_transient(&self, _name: &str) {}

    /// The filters the embeds and the RSS block run, under WordPress's
    /// names; none attached by default.
    fn hooks(&self) -> &dyn Hooks {
        &()
    }
}

/// The main query's conditionals that blocks check.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Request {
    /// `is_front_page()`
    pub front_page: bool,
    /// `is_home()`
    pub home: bool,
    /// The `paged` query var, as `absint()` casts it.
    pub paged: u64,
    /// `$wp_query->max_num_pages`.
    pub max_pages: u64,
    /// `$wp_query->found_posts`.
    pub found_posts: u64,
    /// `get_queried_object_id()`
    pub queried_object_id: u64,
    /// The global `$post`: the main query's first post, which top-level
    /// blocks get as their `postId` context.
    pub post: Option<u64>,
    /// `time()`, which relative dates are measured from.
    pub now: i64,
    /// The main query's posts, which query blocks that inherit it list.
    pub posts: Vec<Post>,
    /// `is_archive()`: which archive, if any.
    pub archive: Option<Archive>,
    /// `is_date()`, which a category archive with a date can be as well.
    pub date: bool,
    /// `is_search()`: the raw search query, `get_query_var( 's' )`.
    pub search: Option<Vec<u8>>,
    /// `get_query_var( 's' )`, which a 404 keeps where `search` is gone.
    pub search_query: Vec<u8>,
    /// `is_singular()`: the main query's one post is the queried object.
    pub singular: bool,
    /// `is_attachment()`: an attachment asked for as one, or one without a
    /// parent; an attached one found by `name` is only a single post.
    pub attachment: bool,
    /// `is_404()`
    pub not_found: bool,
    /// `is_preview()`
    pub preview: bool,
    /// `is_embed()`
    pub embed: bool,
    /// `is_feed()`: posts show in full, as a singular post does.
    pub feed: bool,
    /// The `page` query var, 0 or 1 on the first page.
    pub page: u64,
    /// `$_SERVER['REQUEST_URI']`: the path and query as requested.
    pub uri: Vec<u8>,
    /// `$_GET`: the query as `parse_str()` reads it.
    pub get: Value,
    /// `wp_get_current_commenter()`: the commenter's cookies.
    pub commenter: Commenter,
    /// The query vars: `$_GET` with what the path gives under pretty
    /// permalinks.
    pub vars: Value,
}

/// `wp_get_current_commenter()`: the name, email and URL in the commenter
/// cookies, as `sanitize_comment_cookies()` leaves them, empty for a
/// cookie that is not sent.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Commenter {
    /// `comment_author`, escaped for attributes.
    pub author: Vec<u8>,
    /// `comment_author_email`, escaped for attributes.
    pub email: Vec<u8>,
    /// `comment_author_url`.
    pub url: Vec<u8>,
}

impl Request {
    /// `is_paged()`
    pub fn is_paged(&self) -> bool {
        self.paged > 1
    }

    /// The site's home: the front page, or a home that is not the page for
    /// posts, as the site title and home link blocks check it.
    pub(crate) fn is_site_home(&self, site: &dyn Site) -> bool {
        let page_for_posts = site
            .option("page_for_posts")
            .map_or(0, |v| Value::String(v).to_int());
        self.front_page
            || self.home && u64::try_from(page_for_posts).ok() != Some(self.queried_object_id)
    }
}

/// The taxonomies a visitor can query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Taxonomy {
    /// `category`.
    Category,
    /// `post_tag`.
    Tag,
    /// `post_format`.
    Format,
}

impl Taxonomy {
    /// The taxonomy's name, such as `post_tag`.
    pub fn name(self) -> &'static str {
        match self {
            Taxonomy::Category => "category",
            Taxonomy::Tag => "post_tag",
            Taxonomy::Format => "post_format",
        }
    }
}

/// The archives the default install has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Archive {
    /// A category, tag or post format archive, and its term, which a query
    /// naming no term that exists lacks.
    Term(Taxonomy, Option<Term>),
    /// `?m=YYYY`.
    Year,
    /// `?m=YYYYMM`.
    Month,
    /// `?m=YYYYMMDD`.
    Day,
    /// A week's or a time's: no year, month or day.
    Date,
}

impl Archive {
    /// The queried term of a term archive, with its taxonomy.
    pub fn term(&self) -> Option<(Taxonomy, &Term)> {
        match self {
            Archive::Term(taxonomy, Some(term)) => Some((*taxonomy, term)),
            _ => None,
        }
    }
}

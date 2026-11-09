// SPDX-License-Identifier: AGPL-3.0-or-later

//! WordPress rows as plain data: what `hyscribe-db` reads and
//! `hyscribe-render` renders (ADR 0009).

/// A row of `{prefix}posts`, with the columns rendering needs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Post {
    /// `ID`.
    pub id: u64,
    /// `post_author`.
    pub author: u64,
    /// `post_date`, as stored.
    pub date: String,
    /// `post_date_gmt`, as stored.
    pub date_gmt: String,
    /// `post_content`.
    pub content: Vec<u8>,
    /// `post_title`.
    pub title: Vec<u8>,
    /// `post_excerpt`.
    pub excerpt: Vec<u8>,
    /// `post_status`.
    pub status: String,
    /// `post_password`.
    pub password: Vec<u8>,
    /// `post_name`.
    pub name: Vec<u8>,
    /// `post_modified`, as stored.
    pub modified: String,
    /// `post_modified_gmt`, as stored.
    pub modified_gmt: String,
    /// `post_parent`.
    pub parent: u64,
    /// `guid`.
    pub guid: Vec<u8>,
    /// `menu_order`.
    pub menu_order: i64,
    /// `post_type`.
    pub post_type: String,
    /// `comment_count`.
    pub comment_count: i64,
    /// `comment_status`: `open` or `closed`.
    pub comment_status: String,
    /// `ping_status`: `open` or `closed`.
    pub ping_status: String,
}

/// A term of a taxonomy, as `get_the_terms()` returns it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Term {
    /// `term_id`.
    pub id: u64,
    /// `name`.
    pub name: Vec<u8>,
    /// `slug`.
    pub slug: Vec<u8>,
    /// `description`, of the term in its taxonomy.
    pub description: Vec<u8>,
    /// `parent`: the term above it in its taxonomy, 0 for none.
    pub parent: u64,
}

/// A row of `{prefix}users`, with the columns rendering needs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct User {
    /// `ID`.
    pub id: u64,
    /// `display_name`.
    pub display_name: Vec<u8>,
    /// `user_nicename`.
    pub nicename: Vec<u8>,
    /// `user_login`.
    pub login: Vec<u8>,
    /// `user_email`, read only to hash for the avatar.
    pub email: Vec<u8>,
    /// `user_url`.
    pub url: Vec<u8>,
    /// The `first_name` user meta, or empty.
    pub first_name: Vec<u8>,
    /// The `last_name` user meta, or empty.
    pub last_name: Vec<u8>,
}

/// A row of `{prefix}comments`, with the columns rendering needs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Comment {
    /// `comment_ID`.
    pub id: u64,
    /// `comment_post_ID`.
    pub post_id: u64,
    /// `comment_author`.
    pub author: Vec<u8>,
    /// `comment_author_email`.
    pub author_email: Vec<u8>,
    /// `comment_author_url`.
    pub author_url: Vec<u8>,
    /// `comment_date`, local time.
    pub date: String,
    /// `comment_date_gmt`.
    pub date_gmt: String,
    /// `comment_content`.
    pub content: Vec<u8>,
    /// `comment_type`: empty or `comment` for a comment, or `pingback`,
    /// `trackback` and the like.
    pub comment_type: Vec<u8>,
    /// `comment_parent`.
    pub parent: u64,
    /// `user_id`.
    pub user_id: u64,
}

/// What a list of posts is filtered by.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PostQuery {
    /// Post types to include, such as `post`, or `post` and `page` for a
    /// search; none means any.
    pub post_types: Vec<String>,
    /// Search terms; every term must appear in the title, excerpt or content.
    pub search: Vec<String>,
    /// `exact`: a term must be a whole title, excerpt or content.
    pub exact: bool,
    /// `YEAR(post_date)`.
    pub year: Option<u64>,
    /// `MONTH(post_date)`.
    pub month: Option<u64>,
    /// `DAYOFMONTH(post_date)`.
    pub day: Option<u64>,
    /// Conditions on the time and week of `post_date`.
    pub time: Vec<TimeCondition>,
    /// `post__in`: only these posts, when set.
    pub include: Option<Vec<u64>>,
    /// `post__not_in`.
    pub exclude: Vec<u64>,
    /// `author__in`: posts by any of these users; none means any author.
    pub authors: Vec<u64>,
    /// `post_parent__in`: children of any of these posts; none means any
    /// parent.
    pub parents: Vec<u64>,
    /// `tax_query` clauses, all of which a post must meet.
    pub terms: Vec<TermFilter>,
    /// The post format filter, when set.
    pub formats: Option<FormatFilter>,
    /// The sort, each column ascending when its flag is set; none means
    /// newest first.
    pub order: Vec<(OrderBy, bool)>,
    /// Sticky posts to put first, as `WP_Query` does on the first page of a
    /// home list: those the query found, in its order, then the others not
    /// excluded, newest first.
    pub sticky: Vec<u64>,
    /// How many posts.
    pub limit: u32,
    /// How many to skip.
    pub offset: u32,
}

/// A `tax_query` clause on term IDs, without children.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TermFilter {
    /// The taxonomy, such as `category`.
    pub taxonomy: String,
    /// `term_id`s; IDs not in the taxonomy are ignored.
    pub terms: Vec<u64>,
    /// `NOT IN`: posts with none of the terms, where `IN` wants one.
    pub exclude: bool,
}

/// A condition on `post_date` as `WP_Query` and `WP_Date_Query` write it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TimeCondition {
    /// `HOUR(post_date) = n`.
    Hour(u64),
    /// `MINUTE(post_date) = n`.
    Minute(u64),
    /// `SECOND(post_date) = n`.
    Second(u64),
    /// `DATE_FORMAT(post_date, format) = time`, compared as numbers, which
    /// `build_time_query()` writes for a minute with an hour or a second.
    Formatted {
        /// Such as `%H.%i`.
        format: String,
        /// Such as `8.080000`.
        time: String,
    },
    /// `_wp_mysql_week( post_date ) = week`, its mode from `start_of_week`.
    Week {
        /// The week number.
        week: u64,
        /// `(int) get_option( 'start_of_week' )`.
        start_of_week: i64,
    },
}

/// Posts with any of these post formats.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FormatFilter {
    /// `standard`: posts without a format.
    pub standard: bool,
    /// Format term slugs, such as `post-format-aside`.
    pub slugs: Vec<String>,
}

/// A column `WP_Query` sorts by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderBy {
    /// `post_date`.
    Date,
    /// `post_title`.
    Title,
    /// `post_modified`.
    Modified,
    /// `post_name`.
    Name,
    /// `post_author`.
    Author,
    /// `post_parent`.
    Parent,
    /// `post_type`.
    Type,
    /// `ID`.
    Id,
    /// `menu_order`.
    MenuOrder,
    /// `comment_count`.
    CommentCount,
    /// A search's relevance: titles containing the whole search first.
    Relevance,
}

/// A period `wp_get_archives()` groups published posts by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Period {
    /// `yearly`.
    Year,
    /// `monthly`.
    Month,
    /// `daily`.
    Day,
}

/// A period with published posts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DateCount {
    /// `YEAR(post_date)`.
    pub year: u64,
    /// `MONTH(post_date)`, 0 for a year.
    pub month: u64,
    /// `DAYOFMONTH(post_date)`, 0 for a year or month.
    pub day: u64,
    /// How many posts.
    pub posts: u64,
}

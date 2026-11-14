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
    /// `post_mime_type`.
    pub mime_type: String,
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
    /// `count`: its published posts.
    pub count: u64,
}

/// What a login checks of a user: `ID`, `user_login` and `user_pass`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Credentials {
    /// `ID`.
    pub id: u64,
    /// `user_login`.
    pub login: Vec<u8>,
    /// `user_pass`, the password hash.
    pub pass: Vec<u8>,
}

/// The column `get_user_by()` looks a user up by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserField {
    /// `user_login`.
    Login,
    /// `user_email`.
    Email,
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
    /// The `description` user meta, the biography, or empty.
    pub description: Vec<u8>,
}

/// The columns `WP_User_Query::parse_orderby()` sorts by, `post_count`
/// being each user's posts.
pub const USER_ORDERBY: [&str; 8] = [
    "user_login",
    "user_nicename",
    "user_email",
    "user_url",
    "user_registered",
    "display_name",
    "ID",
    "post_count",
];

/// The columns `WP_User_Query` searches.
pub const USER_SEARCH_COLUMNS: [&str; 6] = [
    "ID",
    "user_login",
    "user_email",
    "user_url",
    "user_nicename",
    "display_name",
];

/// The columns `WP_Comment_Query::parse_orderby()` sorts by.
pub const COMMENT_ORDERBY: [&str; 15] = [
    "comment_agent",
    "comment_approved",
    "comment_author",
    "comment_author_email",
    "comment_author_IP",
    "comment_author_url",
    "comment_content",
    "comment_date",
    "comment_date_gmt",
    "comment_ID",
    "comment_karma",
    "comment_parent",
    "comment_post_ID",
    "comment_type",
    "user_id",
];

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
    /// `comment_author_IP`.
    pub author_ip: Vec<u8>,
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
    /// The statuses to include; none means `publish`.
    pub statuses: Vec<StatusFilter>,
    /// Search terms; every term must appear in the title, excerpt or content.
    pub search: Vec<String>,
    /// `exact`: a term must be a whole title, excerpt or content.
    pub exact: bool,
    /// `s` as searched, which the relevance order matches as a phrase.
    pub sentence: String,
    /// `search_terms_count`: the words of `s` before any were dropped.
    pub search_count: usize,
    /// `wp_allow_query_attachment_by_filename`: a search term may match the
    /// `_wp_attached_file` meta too.
    pub filenames: bool,
    /// `is_user_logged_in()`: a search finds password protected posts too.
    pub logged_in: bool,
    /// `wp_post_mime_type_where()`: `post_mime_type` is one of these, each
    /// a `LIKE` pattern when it holds a `%`; none means any.
    pub mime_types: Vec<String>,
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
    /// `author__not_in`: posts by none of these users.
    pub excluded_authors: Vec<u64>,
    /// `post_parent__in`: children of any of these posts; none means any
    /// parent.
    pub parents: Vec<u64>,
    /// `post_parent__not_in`.
    pub excluded_parents: Vec<u64>,
    /// `post_name__in`: posts with any of these slugs; none means any.
    pub names: Vec<String>,
    /// `menu_order`, when set.
    pub menu_order: Option<i64>,
    /// `search_columns`: the columns a search term must appear in one of;
    /// none means the title, excerpt and content.
    pub search_columns: Vec<String>,
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

/// A status a query selects, and the only author whose posts of it it
/// selects when given, as `WP_Query` limits private posts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StatusFilter {
    /// `post_status`.
    pub status: String,
    /// `post_author`, when only theirs.
    pub author: Option<u64>,
}

/// The SQL part of a `WP_Term_Query` for one taxonomy: what its `WHERE`,
/// `ORDER BY` and `LIMIT` clauses say.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TermQuery {
    /// `tt.taxonomy`.
    pub taxonomy: String,
    /// `include`: `t.term_id IN (...)` when not empty.
    pub include: Vec<u64>,
    /// `tt.parent = n`.
    pub parent: Option<u64>,
    /// `tt.count > 0`.
    pub nonempty: bool,
    /// The order and whether it descends; none for `orderby=none`.
    pub order: Option<(TermOrder, bool)>,
    /// `LIMIT n`.
    pub limit: Option<u64>,
    /// `OFFSET n` of a limited query.
    pub offset: u64,
    /// `search`: the terms whose name or slug contains it.
    pub search: Option<String>,
}

/// What `WP_Term_Query::parse_orderby()` orders terms by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TermOrder {
    /// `t.term_id`.
    TermId,
    /// `t.name`.
    Name,
    /// `t.slug`.
    Slug,
    /// `t.term_group`.
    TermGroup,
    /// `tt.count`.
    Count,
    /// `tt.parent`.
    Parent,
    /// `tt.taxonomy`.
    Taxonomy,
    /// `tt.term_taxonomy_id`.
    TermTaxonomyId,
    /// `tt.description`.
    Description,
    /// `FIELD( t.term_id, include )`.
    Include,
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
    /// `RAND()`.
    Random,
    /// `RAND(seed)`, an order the seed repeats.
    Seeded(u64),
    /// `post__in`: the order of the query's `include`.
    Included,
    /// `post_parent__in`: the order of the query's `parents`.
    Parents,
    /// `post_name__in`: the order of the query's `names`.
    Names,
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
    /// `weekly`, with weeks from the `start_of_week` day.
    Week(i64),
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
    /// The week of the year, as `_wp_mysql_week()` numbers it; 0 but for
    /// a week, whose month and day are those of its latest post.
    pub week: u64,
    /// How many posts.
    pub posts: u64,
}

/// What `get_calendar()` reads of a month: the months with posts before
/// and after it, as `(year, month)`, and its days with posts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CalendarMonth {
    /// The month of the latest post before the month.
    pub previous: Option<(u64, u64)>,
    /// The month of the earliest post after the month.
    pub next: Option<(u64, u64)>,
    /// The days of the month with posts.
    pub days: Vec<u64>,
}

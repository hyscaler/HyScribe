// SPDX-License-Identifier: AGPL-3.0-or-later

//! Read access to a WordPress database (M1.3, ADR 0003).
//!
//! - **Read only.** Every connection is put in a read only transaction mode,
//!   and this crate has no write methods: shadow mode cannot write.
//! - **No string-built SQL.** Queries are fixed text; only the table prefix
//!   is inserted, and `Prefix` admits nothing but `[A-Za-z0-9_]`. Values
//!   are always bound parameters.
//! - **Bytes, not text.** Text columns are read as bytes, so content is
//!   never re-encoded whatever the site's charset.

use std::fmt;

use hyscribe_config::{Database, Prefix};
use hyscribe_model::{
    COMMENT_ORDERBY, CalendarMonth, Comment, Credentials, DateCount, OrderBy, Period, Post,
    PostQuery, Term, TermOrder, TermQuery, TimeCondition, USER_ORDERBY, USER_SEARCH_COLUMNS, User,
    UserField,
};
use sqlx::mysql::{MySqlConnectOptions, MySqlPool, MySqlPoolOptions, MySqlRow};
use sqlx::{Executor, Row};

/// Puts the table prefix into SQL: the only SQL this crate builds.
trait PrefixSql {
    /// `query` with every `{prefix}` replaced. `query` is a literal and the
    /// prefix cannot hold SQL syntax, so the result is safe.
    fn sql(&self, query: &'static str) -> sqlx::AssertSqlSafe<String>;

    /// The same for SQL that [`Sql`] joined from fixed fragments only.
    fn sql_from_fragments(&self, query: String) -> sqlx::AssertSqlSafe<String>;
}

impl PrefixSql for Prefix {
    fn sql(&self, query: &'static str) -> sqlx::AssertSqlSafe<String> {
        self.sql_from_fragments(query.to_string())
    }

    fn sql_from_fragments(&self, query: String) -> sqlx::AssertSqlSafe<String> {
        sqlx::AssertSqlSafe(query.replace("{prefix}", self.as_str()))
    }
}

/// Why a database call failed. Messages never contain the password.
#[derive(Debug)]
pub struct DbError(String);

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DbError {}

fn db_error(err: sqlx::Error) -> DbError {
    DbError(err.to_string())
}

/// The `{prefix}comments` columns [`Comment`] reads, as a literal for
/// `concat!`.
macro_rules! comment_columns {
    () => {
        "comment_ID, comment_post_ID, comment_author, comment_author_email, comment_author_url, \
         comment_author_IP, \
         CAST(comment_date AS CHAR) AS comment_date, \
         CAST(comment_date_gmt AS CHAR) AS comment_date_gmt, comment_content, comment_type, \
         comment_parent, user_id"
    };
}

/// The `{prefix}posts` columns [`Post`] reads, as a literal for `concat!`.
macro_rules! post_columns {
    () => {
        "ID, post_author, CAST(post_date AS CHAR) AS post_date, \
         CAST(post_date_gmt AS CHAR) AS post_date_gmt, post_content, post_title, post_excerpt, \
         post_status, post_password, post_name, CAST(post_modified AS CHAR) AS post_modified, \
         CAST(post_modified_gmt AS CHAR) AS post_modified_gmt, post_parent, guid, CAST(menu_order AS SIGNED) AS menu_order, post_type, post_mime_type, comment_count, \
         comment_status, ping_status"
    };
}

/// The columns of a term in its taxonomy, with its post count, and the
/// tables they come from, `t` and `tt`.
macro_rules! terms_from {
    () => {
        "t.term_id, t.name, t.slug, tt.description, tt.parent, tt.count FROM {prefix}terms t \
         JOIN {prefix}term_taxonomy tt ON tt.term_id = t.term_id "
    };
}

/// Reads columns from a row, keeping the first error, so a row decodes in
/// one step.
struct Columns<'r> {
    row: &'r MySqlRow,
    error: Option<sqlx::Error>,
}

impl<'r> Columns<'r> {
    fn new(row: &'r MySqlRow) -> Columns<'r> {
        Columns { row, error: None }
    }

    fn keep<T: Default>(&mut self, value: Result<T, sqlx::Error>) -> T {
        value.unwrap_or_else(|err| {
            self.error.get_or_insert(err);
            T::default()
        })
    }

    /// A text or blob column's bytes, whatever its charset.
    fn bytes(&mut self, name: &str) -> Vec<u8> {
        let value = self.row.try_get_unchecked::<Vec<u8>, _>(name);
        self.keep(value)
    }

    fn text(&mut self, name: &str) -> String {
        String::from_utf8_lossy(&self.bytes(name)).into_owned()
    }

    fn unsigned(&mut self, name: &str) -> u64 {
        let value = self.row.try_get::<u64, _>(name);
        self.keep(value)
    }

    fn signed(&mut self, name: &str) -> i64 {
        let value = self.row.try_get::<i64, _>(name);
        self.keep(value)
    }

    fn finish<T>(self, value: T) -> Result<T, DbError> {
        self.error.map_or(Ok(value), |err| Err(db_error(err)))
    }
}

fn term(row: &MySqlRow) -> Result<Term, DbError> {
    let mut c = Columns::new(row);
    let term = Term {
        id: c.unsigned("term_id"),
        name: c.bytes("name"),
        slug: c.bytes("slug"),
        description: c.bytes("description"),
        parent: c.unsigned("parent"),
        count: c.signed("count").unsigned_abs(),
    };
    c.finish(term)
}

fn term_id(row: &MySqlRow) -> Result<u64, DbError> {
    let mut c = Columns::new(row);
    let id = c.unsigned("term_id");
    c.finish(id)
}

fn date_count(row: &MySqlRow) -> Result<DateCount, DbError> {
    let mut c = Columns::new(row);
    let count = DateCount {
        year: c.signed("y").unsigned_abs(),
        month: c.signed("m").unsigned_abs(),
        day: c.signed("d").unsigned_abs(),
        week: c.signed("w").unsigned_abs(),
        posts: c.signed("posts").unsigned_abs(),
    };
    c.finish(count)
}

/// The columns of a user, with the first value of each name meta.
macro_rules! user_columns {
    () => {
        "SELECT ID, display_name, user_nicename, user_login, user_email, user_url, \
         COALESCE((SELECT meta_value FROM {prefix}usermeta WHERE user_id = u.ID \
         AND meta_key = 'first_name' ORDER BY umeta_id LIMIT 1), '') AS first_name, \
         COALESCE((SELECT meta_value FROM {prefix}usermeta WHERE user_id = u.ID \
         AND meta_key = 'last_name' ORDER BY umeta_id LIMIT 1), '') AS last_name, \
         COALESCE((SELECT meta_value FROM {prefix}usermeta WHERE user_id = u.ID \
         AND meta_key = 'description' ORDER BY umeta_id LIMIT 1), '') AS description"
    };
}

fn user(row: &MySqlRow) -> Result<User, DbError> {
    let mut c = Columns::new(row);
    let user = User {
        id: c.unsigned("ID"),
        display_name: c.bytes("display_name"),
        nicename: c.bytes("user_nicename"),
        login: c.bytes("user_login"),
        email: c.bytes("user_email"),
        url: c.bytes("user_url"),
        first_name: c.bytes("first_name"),
        last_name: c.bytes("last_name"),
        description: c.bytes("description"),
    };
    c.finish(user)
}

fn comment(row: &MySqlRow) -> Result<Comment, DbError> {
    let mut c = Columns::new(row);
    let comment = Comment {
        id: c.unsigned("comment_ID"),
        post_id: c.unsigned("comment_post_ID"),
        author: c.bytes("comment_author"),
        author_email: c.bytes("comment_author_email"),
        author_url: c.bytes("comment_author_url"),
        author_ip: c.bytes("comment_author_IP"),
        date: c.text("comment_date"),
        date_gmt: c.text("comment_date_gmt"),
        content: c.bytes("comment_content"),
        comment_type: c.bytes("comment_type"),
        parent: c.unsigned("comment_parent"),
        user_id: c.unsigned("user_id"),
    };
    c.finish(comment)
}

fn post(row: &MySqlRow) -> Result<Post, DbError> {
    let mut c = Columns::new(row);
    let post = Post {
        id: c.unsigned("ID"),
        author: c.unsigned("post_author"),
        date: c.text("post_date"),
        date_gmt: c.text("post_date_gmt"),
        content: c.bytes("post_content"),
        title: c.bytes("post_title"),
        excerpt: c.bytes("post_excerpt"),
        status: c.text("post_status"),
        password: c.bytes("post_password"),
        name: c.bytes("post_name"),
        modified: c.text("post_modified"),
        modified_gmt: c.text("post_modified_gmt"),
        parent: c.unsigned("post_parent"),
        guid: c.bytes("guid"),
        menu_order: c.signed("menu_order"),
        post_type: c.text("post_type"),
        mime_type: c.text("post_mime_type"),
        comment_count: c.signed("comment_count"),
        comment_status: c.text("comment_status"),
        ping_status: c.text("ping_status"),
    };
    c.finish(post)
}

/// A read only connection pool for one site.
#[derive(Clone, Debug)]
pub struct Db {
    pool: MySqlPool,
    prefix: Prefix,
}

impl Db {
    /// Connects with `config`, read only.
    pub async fn connect(config: &Database) -> Result<Db, DbError> {
        let prefix = config.prefix.clone();
        let options = MySqlConnectOptions::new()
            .host(&config.host)
            .port(config.port)
            .database(&config.name)
            .username(&config.user)
            .password(config.password.expose())
            .charset(&config.charset);
        let pool = MySqlPoolOptions::new()
            .max_connections(8)
            // Report an unreachable database soon instead of retrying for 30s.
            .acquire_timeout(std::time::Duration::from_secs(5))
            .after_connect(|conn, _| {
                Box::pin(async move {
                    conn.execute("SET SESSION TRANSACTION READ ONLY")
                        .await
                        .map(|_| ())
                })
            })
            .connect_with(options)
            .await
            .map_err(db_error)?;
        Ok(Db { pool, prefix })
    }

    /// Every option WordPress loads on each request, in table order.
    pub async fn autoload_options(&self) -> Result<Vec<(String, Vec<u8>)>, DbError> {
        // WordPress 6.6 and later write on/auto/auto-on; older rows say yes.
        let sql = self.prefix.sql(
            "SELECT option_name, option_value FROM {prefix}options \
             WHERE autoload IN ('yes', 'on', 'auto-on', 'auto') ORDER BY option_id",
        );
        let rows = sqlx::query(sql)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter()
            .map(|row| {
                let mut c = Columns::new(row);
                let option = (c.text("option_name"), c.bytes("option_value"));
                c.finish(option)
            })
            .collect()
    }

    /// One post by ID, whatever its type and status.
    pub async fn post(&self, id: u64) -> Result<Option<Post>, DbError> {
        let sql = self.prefix.sql(concat!(
            "SELECT ",
            post_columns!(),
            " FROM {prefix}posts WHERE ID = ?"
        ));
        let row = sqlx::query(sql)
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;
        row.as_ref().map(post).transpose()
    }

    /// The published post (of type `post`) named `name`.
    pub async fn post_by_name(&self, name: &str) -> Result<Option<Post>, DbError> {
        let mut sql = Sql::new(concat!(
            "SELECT ",
            post_columns!(),
            " FROM {prefix}posts WHERE "
        ));
        sql.push("post_name = ? AND post_type = 'post' AND post_status = 'publish' LIMIT 1");
        sql.bind(Bind::Text(name.to_string()));
        self.select(sql).await.map(|posts| posts.into_iter().next())
    }

    /// `get_page_by_path()`'s candidates: the pages and attachments of any
    /// status named one of `names`.
    pub async fn named_pages(&self, names: &[Vec<u8>]) -> Result<Vec<Post>, DbError> {
        let mut sql = Sql::new(concat!(
            "SELECT ",
            post_columns!(),
            " FROM {prefix}posts WHERE "
        ));
        sql.push("post_type IN ('page', 'attachment') AND post_name IN (");
        list(
            &mut sql,
            std::iter::once(Bind::Text(String::new())).chain(
                names
                    .iter()
                    .map(|n| Bind::Text(String::from_utf8_lossy(n).into_owned())),
            ),
        );
        self.select(sql).await
    }

    /// `redirect_guess_404_permalink()`'s guess: the first published post
    /// of `post_types` whose name starts with `name`, on the date parts
    /// given.
    pub async fn guess_post(
        &self,
        name: &str,
        post_types: &[&str],
        (year, month, day): (Option<u64>, Option<u64>, Option<u64>),
    ) -> Result<Option<Post>, DbError> {
        let mut sql = Sql::new(concat!(
            "SELECT ",
            post_columns!(),
            " FROM {prefix}posts WHERE "
        ));
        sql.push("post_name LIKE ? AND post_status = 'publish' AND post_type IN (");
        sql.bind(Bind::Text(format!("{}%", esc_like(name))));
        list(
            &mut sql,
            post_types.iter().map(|t| Bind::Text((*t).to_string())),
        );
        for (fragment, value) in [
            (" AND YEAR(post_date) = ?", year),
            (" AND MONTH(post_date) = ?", month),
            (" AND DAYOFMONTH(post_date) = ?", day),
        ] {
            if let Some(value) = value {
                sql.push(fragment);
                sql.bind(Bind::Int(value));
            }
        }
        sql.push(" LIMIT 1");
        self.select(sql).await.map(|posts| posts.into_iter().next())
    }

    /// Published posts matching `query`, as the main query of a list page
    /// (home, search, category or date archive) selects them, sticky posts
    /// first.
    pub async fn posts(&self, query: &PostQuery) -> Result<Vec<Post>, DbError> {
        let found = self.select(posts_sql(query)).await?;
        let (posts, rest): (Vec<Post>, Vec<Post>) = found
            .into_iter()
            .partition(|post| query.sticky.contains(&post.id));
        let missing: Vec<u64> = query
            .sticky
            .iter()
            .copied()
            .filter(|id| !posts.iter().any(|post| post.id == *id) && !query.exclude.contains(id))
            .collect();
        let stickies = if missing.is_empty() {
            Ok(Vec::new())
        } else {
            let stickies = PostQuery {
                post_types: query.post_types.clone(),
                limit: u32::MAX,
                include: Some(missing),
                ..PostQuery::default()
            };
            self.select(posts_sql(&stickies)).await
        };
        stickies.map(|stickies| posts.into_iter().chain(stickies).chain(rest).collect())
    }

    /// The posts `sql` selects.
    async fn select(&self, sql: Sql) -> Result<Vec<Post>, DbError> {
        let rows = sql
            .query(&self.prefix)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter().map(post).collect()
    }

    /// `found_posts`: how many posts `query` matches on every page.
    pub async fn count_posts(&self, query: &PostQuery) -> Result<u64, DbError> {
        let mut sql = Sql::new("SELECT COUNT(*) AS found FROM {prefix}posts WHERE ");
        post_criteria(&mut sql, query);
        let row = sql
            .query(&self.prefix)
            .fetch_one(&self.pool)
            .await
            .map_err(db_error)?;
        row.try_get::<i64, _>("found")
            .map(i64::unsigned_abs)
            .map_err(db_error)
    }

    /// One option by name, autoloaded or not.
    pub async fn option(&self, name: &str) -> Result<Option<Vec<u8>>, DbError> {
        let sql = self
            .prefix
            .sql("SELECT option_value FROM {prefix}options WHERE option_name = ? LIMIT 1");
        let row = sqlx::query(sql)
            .bind(name)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;
        row.map(|row| {
            let mut c = Columns::new(&row);
            let value = c.bytes("option_value");
            c.finish(value)
        })
        .transpose()
    }

    /// `get_post_meta( $post_id, $key, true )`: the first value stored.
    pub async fn post_meta(&self, post_id: u64, key: &str) -> Result<Option<Vec<u8>>, DbError> {
        self.first_meta(
            "SELECT meta_value FROM {prefix}postmeta WHERE post_id = ? AND meta_key = ? ORDER BY meta_id LIMIT 1",
            post_id,
            key,
        )
        .await
    }

    /// `get_term( $id, $taxonomy )`.
    pub async fn term(&self, id: u64, taxonomy: &str) -> Result<Option<Term>, DbError> {
        let sql = self.prefix.sql(concat!(
            "SELECT ",
            terms_from!(),
            "WHERE t.term_id = ? AND tt.taxonomy = ?"
        ));
        let row = sqlx::query(sql)
            .bind(id)
            .bind(taxonomy)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;
        row.as_ref().map(term).transpose()
    }

    /// The taxonomy of term `id`, as `get_term( $id )` finds it: none when
    /// no taxonomy or more than one has it.
    pub async fn term_taxonomy(&self, id: u64) -> Result<Option<String>, DbError> {
        let sql = self.prefix.sql(concat!(
            "SELECT tt.taxonomy FROM {prefix}term_taxonomy AS tt ",
            "INNER JOIN {prefix}terms AS t ON t.term_id = tt.term_id ",
            "WHERE tt.term_id = ? LIMIT 2"
        ));
        let rows = sqlx::query(sql)
            .bind(id)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        let [row] = rows.as_slice() else {
            return Ok(None);
        };
        let mut c = Columns::new(row);
        let taxonomy = c.text("taxonomy");
        c.finish(taxonomy).map(Some)
    }

    /// `get_the_terms( $post_id, $taxonomy )`: ordered by name, as
    /// `wp_get_object_terms()` orders them.
    pub async fn post_terms(&self, post_id: u64, taxonomy: &str) -> Result<Vec<Term>, DbError> {
        let sql = self.prefix.sql(concat!(
            "SELECT ",
            terms_from!(),
            "JOIN {prefix}term_relationships tr ON tr.term_taxonomy_id = tt.term_taxonomy_id \
             WHERE tr.object_id = ? AND tt.taxonomy = ? ORDER BY t.name ASC"
        ));
        let rows = sqlx::query(sql)
            .bind(post_id)
            .bind(taxonomy)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter().map(term).collect()
    }

    /// `get_adjacent_post()` for a visitor; see [`adjacent_sql`].
    pub async fn adjacent_post(
        &self,
        post: &Post,
        previous: bool,
        terms: Option<(&str, &[u64])>,
    ) -> Result<Option<Post>, DbError> {
        let row = adjacent_sql(post, previous, terms)
            .query(&self.prefix)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;
        row.as_ref().map(crate::post).transpose()
    }

    /// A post's approved comments, as `WP_Comment_Query` orders them for
    /// the comment template: oldest first, then by ID.
    pub async fn comments(&self, post_id: u64) -> Result<Vec<Comment>, DbError> {
        let sql = self.prefix.sql(concat!(
            "SELECT ",
            comment_columns!(),
            " FROM {prefix}comments \
             WHERE comment_post_ID = ? AND comment_approved = '1' AND comment_type != 'note' \
             ORDER BY comment_date_gmt ASC, comment_ID ASC"
        ));
        let rows = sqlx::query(sql)
            .bind(post_id)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter().map(comment).collect()
    }

    /// The comments a comment feed lists, newest first: `post`'s, or
    /// those on published posts and attachments.
    pub async fn feed_comments(
        &self,
        post: Option<u64>,
        limit: u64,
    ) -> Result<Vec<Comment>, DbError> {
        let sql = self.prefix.sql(concat!(
            "SELECT ",
            comment_columns!(),
            " FROM {prefix}comments JOIN {prefix}posts ON comment_post_ID = ID \
             WHERE (? IS NULL AND (post_status = 'publish' \
             OR (post_status = 'inherit' AND post_type = 'attachment')) OR comment_post_ID = ?) \
             AND comment_approved = '1' AND comment_type != 'note' \
             ORDER BY comment_date_gmt DESC LIMIT ?"
        ));
        let rows = sqlx::query(sql)
            .bind(post)
            .bind(post)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter().map(comment).collect()
    }

    /// `get_comments()` of the latest comments block: the approved
    /// comments on published posts, newest first, `limit` of them.
    pub async fn recent_comments(&self, limit: u64) -> Result<Vec<Comment>, DbError> {
        let sql = self.prefix.sql(concat!(
            "SELECT ",
            comment_columns!(),
            " FROM {prefix}comments JOIN {prefix}posts ON comment_post_ID = ID \
             WHERE post_status = 'publish' AND comment_approved = '1' \
             AND comment_type != 'note' \
             ORDER BY comment_date_gmt DESC, comment_ID DESC LIMIT ?"
        ));
        let rows = sqlx::query(sql)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter().map(comment).collect()
    }

    /// The comments an archive's or search's comment feed lists, newest
    /// first: those on the posts `query` matches, however many.
    pub async fn archive_feed_comments(
        &self,
        query: &PostQuery,
        limit: u64,
    ) -> Result<Vec<Comment>, DbError> {
        let mut sql = Sql::new(concat!(
            "SELECT ",
            comment_columns!(),
            " FROM {prefix}comments WHERE comment_approved = '1' AND comment_type != 'note' \
             AND comment_post_ID IN (SELECT ID FROM {prefix}posts WHERE "
        ));
        post_criteria(&mut sql, query);
        sql.push(") ORDER BY comment_date_gmt DESC LIMIT ?");
        sql.bind(Bind::Int(limit));
        let rows = sql
            .query(&self.prefix)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter().map(comment).collect()
    }

    /// `get_lastcommentmodified( 'GMT' )`: the latest approved comment's
    /// time, `None` without one.
    pub async fn last_comment_modified(&self) -> Result<Option<String>, DbError> {
        let sql = self.prefix.sql(
            "SELECT CAST(MAX(comment_date_gmt) AS CHAR) AS latest FROM {prefix}comments \
             WHERE comment_approved = '1'",
        );
        let row = sqlx::query(sql)
            .fetch_one(&self.pool)
            .await
            .map_err(db_error)?;
        row.try_get("latest").map_err(db_error)
    }

    /// Posts of type `post` in one status, as the dashboard lists them.
    pub async fn posts_by_status(&self, query: &StatusQuery<'_>) -> Result<Vec<Post>, DbError> {
        let mut sql = Sql::new(concat!(
            "SELECT ",
            post_columns!(),
            " FROM {prefix}posts WHERE post_type = 'post' AND post_status = ?"
        ));
        sql.bind(Bind::Text(query.status.to_string()));
        if let Some(author) = query.author {
            sql.push(" AND post_author = ?");
            sql.bind(Bind::Int(author));
        }
        sql.push(match query.order {
            StatusOrder::DateAsc => " ORDER BY post_date ASC LIMIT ?",
            StatusOrder::DateDesc => " ORDER BY post_date DESC LIMIT ?",
            StatusOrder::ModifiedDesc => " ORDER BY post_modified DESC LIMIT ?",
        });
        sql.bind(Bind::Int(u64::from(query.limit)));
        let rows = sql
            .query(&self.prefix)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter().map(post).collect()
    }

    /// `wp_count_posts( $post_type, 'readable' )`: the posts of each status,
    /// private ones only by `private_author` when given.
    pub async fn status_counts(
        &self,
        post_type: &str,
        private_author: Option<u64>,
    ) -> Result<Vec<(String, u64)>, DbError> {
        let mut sql = Sql::new(
            "SELECT post_status, COUNT(*) AS num_posts FROM {prefix}posts WHERE post_type = ?",
        );
        sql.bind(Bind::Text(post_type.into()));
        if let Some(author) = private_author {
            sql.push(
                " AND (post_status != 'private' OR (post_author = ? AND post_status = 'private'))",
            );
            sql.bind(Bind::Int(author));
        }
        sql.push(" GROUP BY post_status");
        let rows = sql
            .query(&self.prefix)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter()
            .map(|row| {
                let mut c = Columns::new(row);
                let count = (c.text("post_status"), c.signed("num_posts").unsigned_abs());
                c.finish(count)
            })
            .collect()
    }

    /// The posts of `post_type` by `author` the admin's All list shows.
    pub async fn own_posts(&self, post_type: &str, author: u64) -> Result<u64, DbError> {
        let sql = self.prefix.sql(
            "SELECT COUNT(1) AS found FROM {prefix}posts WHERE post_type = ? \
             AND post_status NOT IN ('trash', 'auto-draft', 'inherit', 'request-pending', \
             'request-confirmed', 'request-failed', 'request-completed') AND post_author = ?",
        );
        let row = sqlx::query(sql)
            .bind(post_type)
            .bind(author)
            .fetch_one(&self.pool)
            .await
            .map_err(db_error)?;
        row.try_get::<i64, _>("found")
            .map(i64::unsigned_abs)
            .map_err(db_error)
    }

    /// The posts of `post_type` among `ids` neither trashed nor auto-drafts.
    pub async fn sticky_count(&self, post_type: &str, ids: &[u64]) -> Result<u64, DbError> {
        let mut sql = Sql::new(
            "SELECT COUNT(1) AS found FROM {prefix}posts WHERE post_type = ? \
             AND post_status NOT IN ('trash', 'auto-draft') AND ID IN (",
        );
        sql.bind(Bind::Text(post_type.into()));
        list(&mut sql, ids_or_none(ids));
        let row = sql
            .query(&self.prefix)
            .fetch_one(&self.pool)
            .await
            .map_err(db_error)?;
        row.try_get::<i64, _>("found")
            .map(i64::unsigned_abs)
            .map_err(db_error)
    }

    /// `months_dropdown()`'s months of the posts of `post_type`, newest first:
    /// of the trashed ones when `trash`, else of the others.
    pub async fn post_months(
        &self,
        post_type: &str,
        trash: bool,
    ) -> Result<Vec<(u32, u32)>, DbError> {
        let sql = self.prefix.sql(if trash {
            "SELECT DISTINCT YEAR(post_date) AS year, MONTH(post_date) AS month FROM {prefix}posts \
             WHERE post_type = ? AND post_status = 'trash' ORDER BY post_date DESC"
        } else {
            "SELECT DISTINCT YEAR(post_date) AS year, MONTH(post_date) AS month FROM {prefix}posts \
             WHERE post_type = ? AND post_status != 'auto-draft' AND post_status != 'trash' \
             ORDER BY post_date DESC"
        });
        let rows = sqlx::query(sql)
            .bind(post_type)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter()
            .map(|row| {
                let mut c = Columns::new(row);
                let year = u32::try_from(c.signed("year")).unwrap_or_default();
                let month = u32::try_from(c.signed("month")).unwrap_or_default();
                c.finish((year, month))
            })
            .collect()
    }

    /// `get_pending_comments_num( $post_id )`.
    pub async fn pending_comments(&self, post_id: u64) -> Result<u64, DbError> {
        let sql = self.prefix.sql(
            "SELECT COUNT(comment_ID) AS found FROM {prefix}comments \
             WHERE comment_post_ID = ? AND comment_approved = '0' AND comment_type != 'note'",
        );
        let row = sqlx::query(sql)
            .bind(post_id)
            .fetch_one(&self.pool)
            .await
            .map_err(db_error)?;
        row.try_get::<i64, _>("found")
            .map(i64::unsigned_abs)
            .map_err(db_error)
    }

    /// `WP_User_Query`'s users for a `capability`: those whose capabilities
    /// meta, `key`, names any of `names` (roles or capabilities), by name.
    pub async fn users_with(&self, key: &str, names: &[String]) -> Result<Vec<User>, DbError> {
        let mut sql = Sql::new(
            "SELECT DISTINCT ID, display_name, user_nicename, user_login, user_email, user_url, \
             '' AS first_name, '' AS last_name, '' AS description \
             FROM {prefix}users u INNER JOIN {prefix}usermeta m ON u.ID = m.user_id \
             WHERE m.meta_key = ? AND (",
        );
        sql.bind(Bind::Text(key.into()));
        for (i, name) in names.iter().enumerate() {
            sql.push(if i > 0 {
                " OR m.meta_value LIKE ?"
            } else {
                "m.meta_value LIKE ?"
            });
            sql.bind(Bind::Text(format!("%\"{}\"%", esc_like(name))));
        }
        sql.push(") ORDER BY display_name ASC");
        let rows = sql
            .query(&self.prefix)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter().map(user).collect()
    }

    /// `wp_count_comments( $post )`'s approved, moderated, spam and trashed
    /// comments, and `user`'s approved and pending ones; notes left out, and
    /// those on other posts when `post` is not 0.
    pub async fn comment_counts(&self, user: u64, post: u64) -> Result<[u64; 5], DbError> {
        let mut sql = Sql::new(
            "SELECT CAST(COALESCE(SUM(comment_approved = '1'), 0) AS SIGNED) AS approved, \
             CAST(COALESCE(SUM(comment_approved = '0'), 0) AS SIGNED) AS moderated, \
             CAST(COALESCE(SUM(comment_approved = 'spam'), 0) AS SIGNED) AS spam, \
             CAST(COALESCE(SUM(comment_approved = 'trash'), 0) AS SIGNED) AS trash, \
             CAST(COALESCE(SUM(user_id = ? AND comment_approved IN ('0', '1')), 0) AS SIGNED) AS mine \
             FROM {prefix}comments WHERE comment_type != 'note'",
        );
        sql.bind(Bind::Int(user));
        post_filter(&mut sql, post);
        let row = sql
            .query(&self.prefix)
            .fetch_one(&self.pool)
            .await
            .map_err(db_error)?;
        let mut c = Columns::new(&row);
        let counts = ["approved", "moderated", "spam", "trash", "mine"]
            .map(|name| c.signed(name).unsigned_abs());
        c.finish(counts)
    }

    /// Whether any approved or pending comment is of one of `types`.
    pub async fn has_comment_types(&self, types: &[&str]) -> Result<bool, DbError> {
        let mut sql = Sql::new(
            "SELECT comment_ID FROM {prefix}comments \
             WHERE comment_approved IN ('0', '1') AND comment_type IN (",
        );
        list(
            &mut sql,
            types.iter().map(|kind| Bind::Text((*kind).into())),
        );
        sql.push(" LIMIT 1");
        let rows = sql
            .query(&self.prefix)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        Ok(!rows.is_empty())
    }

    /// `WP_Comment_Query`'s comments in `filter`, sorted by `order`'s
    /// columns of `COMMENT_ORDERBY`, descending where true, with their
    /// `comment_approved`.
    pub async fn list_comments(
        &self,
        filter: &CommentFilter<'_>,
        order: &[(&str, bool)],
        limit: u32,
        offset: u32,
    ) -> Result<Vec<(Comment, String)>, DbError> {
        let mut sql = Sql::new(concat!(
            "SELECT ",
            comment_columns!(),
            ", comment_approved FROM {prefix}comments WHERE "
        ));
        comment_filter(&mut sql, filter);
        let columns = order.iter().filter_map(|(name, desc)| {
            let column = COMMENT_ORDERBY.iter().find(|column| *column == name)?;
            Some((*column, *desc))
        });
        for (i, (column, desc)) in columns.enumerate() {
            sql.push(if i == 0 { " ORDER BY " } else { ", " });
            sql.push(column);
            sql.push(if desc { " DESC" } else { " ASC" });
        }
        sql.push(" LIMIT ? OFFSET ?");
        sql.bind(Bind::Int(limit.into()));
        sql.bind(Bind::Int(offset.into()));
        let rows = sql
            .query(&self.prefix)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter()
            .map(|row| {
                comment(row).and_then(|comment| {
                    let mut c = Columns::new(row);
                    let approved = c.text("comment_approved");
                    c.finish((comment, approved))
                })
            })
            .collect()
    }

    /// How many comments `filter` finds.
    pub async fn count_comments(&self, filter: &CommentFilter<'_>) -> Result<u64, DbError> {
        let mut sql = Sql::new("SELECT COUNT(*) AS found FROM {prefix}comments WHERE ");
        comment_filter(&mut sql, filter);
        let row = sql
            .query(&self.prefix)
            .fetch_one(&self.pool)
            .await
            .map_err(db_error)?;
        row.try_get::<i64, _>("found")
            .map(i64::unsigned_abs)
            .map_err(db_error)
    }

    /// `get_comment_meta( $id, $key, true )`: the first value stored.
    pub async fn comment_meta(&self, id: u64, key: &str) -> Result<Option<Vec<u8>>, DbError> {
        self.first_meta(
            "SELECT meta_value FROM {prefix}commentmeta WHERE comment_id = ? AND meta_key = ? ORDER BY meta_id LIMIT 1",
            id,
            key,
        )
        .await
    }

    /// `get_comment( $id )` in any state.
    pub async fn any_comment(&self, id: u64) -> Result<Option<Comment>, DbError> {
        let sql = self.prefix.sql(concat!(
            "SELECT ",
            comment_columns!(),
            " FROM {prefix}comments WHERE comment_ID = ?"
        ));
        let row = sqlx::query(sql)
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;
        row.as_ref().map(comment).transpose()
    }

    /// One approved comment by ID, of any type.
    pub async fn comment(&self, id: u64) -> Result<Option<Comment>, DbError> {
        let sql = self.prefix.sql(concat!(
            "SELECT ",
            comment_columns!(),
            " FROM {prefix}comments WHERE comment_ID = ? AND comment_approved = '1'"
        ));
        let row = sqlx::query(sql)
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;
        row.as_ref().map(comment).transpose()
    }

    /// One user by ID, with the first value of each name meta as
    /// `get_user_meta( $id, $key, true )` reads it.
    pub async fn user(&self, id: u64) -> Result<Option<User>, DbError> {
        let sql = self.prefix.sql(concat!(
            user_columns!(),
            " FROM {prefix}users u WHERE ID = ?"
        ));
        let row = sqlx::query(sql)
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;
        row.as_ref().map(user).transpose()
    }

    /// `WP_User_Query`'s users in `filter`, sorted by `order`'s columns of
    /// `USER_ORDERBY`, descending where true, `limit` from `offset`.
    pub async fn list_users(
        &self,
        filter: &UserFilter<'_>,
        order: &[(&str, bool)],
        limit: u32,
        offset: u32,
    ) -> Result<Vec<User>, DbError> {
        let columns: Vec<(&'static str, bool)> = order
            .iter()
            .filter_map(|(name, desc)| {
                let column = USER_ORDERBY.iter().find(|column| *column == name)?;
                Some((*column, *desc))
            })
            .collect();
        let mut sql = Sql::new(concat!(user_columns!(), " FROM {prefix}users u"));
        // `get_posts_by_author_sql( 'post' )`'s counts, for their order.
        if columns.iter().any(|(column, _)| *column == "post_count") {
            sql.push(
                " LEFT OUTER JOIN (SELECT post_author, COUNT(*) AS post_count FROM {prefix}posts \
                 WHERE post_type = 'post' AND (post_status = 'publish' \
                 OR (post_status = 'private' AND (? OR post_author = ?))) \
                 GROUP BY post_author) p ON (u.ID = p.post_author)",
            );
            sql.bind(Bind::Int(u64::from(filter.all_private)));
            sql.bind(Bind::Int(filter.me));
        }
        user_filter(&mut sql, filter);
        for (i, (column, desc)) in columns.into_iter().enumerate() {
            sql.push(if i == 0 { " ORDER BY " } else { ", " });
            sql.push(column);
            sql.push(if desc { " DESC" } else { " ASC" });
        }
        sql.push(" LIMIT ? OFFSET ?");
        sql.bind(Bind::Int(limit.into()));
        sql.bind(Bind::Int(offset.into()));
        let rows = sql
            .query(&self.prefix)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter().map(user).collect()
    }

    /// How many users `filter` finds.
    pub async fn count_users(&self, filter: &UserFilter<'_>) -> Result<u64, DbError> {
        let mut sql = Sql::new("SELECT COUNT(*) AS found FROM {prefix}users u");
        user_filter(&mut sql, filter);
        let row = sql
            .query(&self.prefix)
            .fetch_one(&self.pool)
            .await
            .map_err(db_error)?;
        row.try_get::<i64, _>("found")
            .map(i64::unsigned_abs)
            .map_err(db_error)
    }

    /// `wp_get_users_with_no_role()`: those whose `key` meta does not match
    /// `regex`, the roles' names.
    pub async fn users_without_role(&self, key: &str, regex: &str) -> Result<Vec<u64>, DbError> {
        let sql = self.prefix.sql(
            "SELECT user_id FROM {prefix}usermeta WHERE meta_key = ? AND meta_value NOT REGEXP ?",
        );
        let rows = sqlx::query(sql)
            .bind(key)
            .bind(regex)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter()
            .map(|row| {
                let mut c = Columns::new(row);
                let id = c.unsigned("user_id");
                c.finish(id)
            })
            .collect()
    }

    /// The `key` meta of every user, by user ID, as `count_users()` reads
    /// them.
    pub async fn users_meta(&self, key: &str) -> Result<Vec<(u64, Vec<u8>)>, DbError> {
        let sql = self.prefix.sql(
            "SELECT user_id, meta_value FROM {prefix}usermeta \
             INNER JOIN {prefix}users ON user_id = ID WHERE meta_key = ?",
        );
        let rows = sqlx::query(sql)
            .bind(key)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter()
            .map(|row| {
                let mut c = Columns::new(row);
                let value = (c.unsigned("user_id"), c.bytes("meta_value"));
                c.finish(value)
            })
            .collect()
    }

    /// `count_many_users_posts( $ids )`: each author's published posts, and
    /// private ones too when `all_private`, or else `me`'s own.
    pub async fn user_post_counts(
        &self,
        ids: &[u64],
        all_private: bool,
        me: u64,
    ) -> Result<Vec<(u64, u64)>, DbError> {
        let mut sql = Sql::new(
            "SELECT post_author, COUNT(*) AS found FROM {prefix}posts WHERE post_type = 'post' \
             AND (post_status = 'publish' OR (post_status = 'private' AND (? OR post_author = ?))) \
             AND post_author IN (",
        );
        sql.bind(Bind::Int(u64::from(all_private)));
        sql.bind(Bind::Int(me));
        list(&mut sql, field_ids(ids));
        sql.push(" GROUP BY post_author");
        let rows = sql
            .query(&self.prefix)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter()
            .map(|row| {
                let mut c = Columns::new(row);
                let counted = (c.unsigned("post_author"), c.signed("found").unsigned_abs());
                c.finish(counted)
            })
            .collect()
    }

    /// `get_user_by( 'login' | 'email', $value )`: the user whose column
    /// matches as the database's collation compares, the first by ID.
    pub async fn credentials(
        &self,
        field: UserField,
        value: &[u8],
    ) -> Result<Option<Credentials>, DbError> {
        let sql = self.prefix.sql(match field {
            UserField::Login => {
                "SELECT ID, user_login, user_pass FROM {prefix}users WHERE user_login = ? ORDER BY ID LIMIT 1"
            }
            UserField::Email => {
                "SELECT ID, user_login, user_pass FROM {prefix}users WHERE user_email = ? ORDER BY ID LIMIT 1"
            }
        });
        let row = sqlx::query(sql)
            .bind(value)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;
        row.map(|row| {
            let mut c = Columns::new(&row);
            let credentials = Credentials {
                id: c.unsigned("ID"),
                login: c.bytes("user_login"),
                pass: c.bytes("user_pass"),
            };
            c.finish(credentials)
        })
        .transpose()
    }

    /// `get_user_meta( $user_id, $key, true )`: the first value stored.
    pub async fn user_meta(&self, user_id: u64, key: &str) -> Result<Option<Vec<u8>>, DbError> {
        self.first_meta(
            "SELECT meta_value FROM {prefix}usermeta WHERE user_id = ? AND meta_key = ? ORDER BY umeta_id LIMIT 1",
            user_id,
            key,
        )
        .await
    }

    /// The first `meta_value` of `sql`'s object `id` and `key`.
    async fn first_meta(
        &self,
        sql: &'static str,
        id: u64,
        key: &str,
    ) -> Result<Option<Vec<u8>>, DbError> {
        let row = sqlx::query(self.prefix.sql(sql))
            .bind(id)
            .bind(key)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;
        row.map(|row| {
            let mut c = Columns::new(&row);
            let value = c.bytes("meta_value");
            c.finish(value)
        })
        .transpose()
    }

    /// The table prefix, which some meta keys begin with.
    pub fn prefix(&self) -> &str {
        self.prefix.as_str()
    }

    /// The terms of `slugs`, `(taxonomy, slug)` pairs, and the categories
    /// with `category_ids`, each with its taxonomy.
    pub async fn find_terms(
        &self,
        slugs: &[(String, Vec<u8>)],
        category_ids: &[u64],
    ) -> Result<Vec<(String, Term)>, DbError> {
        let mut sql = Sql::new(concat!(
            "SELECT tt.taxonomy, ",
            terms_from!(),
            "WHERE (tt.taxonomy = 'category' AND t.term_id IN ("
        ));
        list(&mut sql, ids_or_none(category_ids));
        sql.push(")");
        for (taxonomy, slug) in slugs {
            sql.push(" OR (tt.taxonomy = ? AND t.slug = ?)");
            sql.bind(Bind::Text(taxonomy.clone()));
            sql.bind(Bind::Text(String::from_utf8_lossy(slug).into_owned()));
        }
        let rows = sql
            .query(&self.prefix)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter()
            .map(|row| {
                let mut c = Columns::new(row);
                let taxonomy = c.text("taxonomy");
                c.finish(taxonomy)
                    .and_then(|taxonomy| term(row).map(|term| (taxonomy, term)))
            })
            .collect()
    }

    /// A page of the terms of `taxonomy` with posts, by ID, as the sitemap
    /// of a taxonomy lists them.
    pub async fn terms_with_posts(
        &self,
        taxonomy: &str,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<Term>, DbError> {
        let sql = self.prefix.sql(concat!(
            "SELECT ",
            terms_from!(),
            "WHERE tt.taxonomy = ? AND tt.count > 0 ORDER BY t.term_id LIMIT ? OFFSET ?"
        ));
        let rows = sqlx::query(sql)
            .bind(taxonomy)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter().map(term).collect()
    }

    /// `get_terms()` for a tag cloud: the terms of `taxonomy` with posts,
    /// the most used first, `limit` of them, each with its post count.
    pub async fn cloud_terms(&self, taxonomy: &str, limit: u64) -> Result<Vec<Term>, DbError> {
        let sql = self.prefix.sql(concat!(
            "SELECT ",
            terms_from!(),
            "WHERE tt.taxonomy = ? AND tt.count > 0 ORDER BY tt.count DESC LIMIT ?"
        ));
        let rows = sqlx::query(sql)
            .bind(taxonomy)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter().map(term).collect()
    }

    /// Every term of `taxonomy` with its post count, by name, as
    /// `get_terms()` orders them for `wp_list_categories()`.
    pub async fn taxonomy_terms(&self, taxonomy: &str) -> Result<Vec<Term>, DbError> {
        let sql = self.prefix.sql(concat!(
            "SELECT ",
            terms_from!(),
            "WHERE tt.taxonomy = ? ORDER BY t.name ASC"
        ));
        let rows = sqlx::query(sql)
            .bind(taxonomy)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter().map(term).collect()
    }

    /// The terms `WP_Term_Query` selects with `query`, in its order: its
    /// query for IDs as written, whose plan orders ties, then those terms.
    pub async fn term_query(&self, query: &TermQuery) -> Result<Vec<Term>, DbError> {
        let mut sql = Sql::new(
            "SELECT t.term_id FROM {prefix}terms AS t \
             INNER JOIN {prefix}term_taxonomy AS tt ON t.term_id = tt.term_id \
             WHERE tt.taxonomy IN (?)",
        );
        sql.bind(Bind::Text(query.taxonomy.clone()));
        if !query.include.is_empty() {
            sql.push(" AND t.term_id IN (");
            list(&mut sql, query.include.iter().copied().map(Bind::Int));
        }
        if let Some(parent) = query.parent {
            sql.push(" AND tt.parent = ?");
            sql.bind(Bind::Int(parent));
        }
        if query.nonempty {
            sql.push(" AND tt.count > 0");
        }
        if let Some(search) = &query.search {
            search_terms(&mut sql, search);
        }
        if let Some((order, descending)) = query.order {
            sql.push(" ORDER BY ");
            match term_order_column(order) {
                Some(column) => sql.push(column),
                None => {
                    sql.push("FIELD( t.term_id, ");
                    list(&mut sql, query.include.iter().copied().map(Bind::Int));
                }
            }
            sql.push(if descending { " DESC" } else { " ASC" });
        }
        if let Some(limit) = query.limit {
            sql.push(" LIMIT ?, ?");
            sql.bind(Bind::Int(query.offset));
            sql.bind(Bind::Int(limit));
        }
        let rows = sql
            .query(&self.prefix)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        let ids: Result<Vec<u64>, DbError> = rows.iter().map(term_id).collect();
        let mut sql = Sql::new(concat!(
            "SELECT ",
            terms_from!(),
            "WHERE tt.taxonomy = ? AND t.term_id IN ("
        ));
        sql.bind(Bind::Text(query.taxonomy.clone()));
        list(&mut sql, ids_or_none(ids.as_deref().unwrap_or_default()));
        let rows = sql
            .query(&self.prefix)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error);
        ids.and_then(|ids| {
            let terms: Result<Vec<Term>, DbError> =
                rows.and_then(|rows| rows.iter().map(term).collect());
            terms.map(|mut terms| {
                terms.sort_by_key(|term| ids.iter().position(|id| *id == term.id));
                terms
            })
        })
    }

    /// The published posts in each term of `taxonomy`, as `(term ID, post
    /// ID)`: what `_pad_term_counts()` reads.
    pub async fn term_posts(&self, taxonomy: &str) -> Result<Vec<(u64, u64)>, DbError> {
        let sql = self.prefix.sql(
            "SELECT tt.term_id, tr.object_id FROM {prefix}term_relationships tr \
             JOIN {prefix}term_taxonomy tt ON tt.term_taxonomy_id = tr.term_taxonomy_id \
             JOIN {prefix}posts p ON tr.object_id = p.ID \
             WHERE tt.taxonomy = ? AND p.post_type = 'post' AND p.post_status = 'publish'",
        );
        let rows = sqlx::query(sql)
            .bind(taxonomy)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter()
            .map(|row| {
                let mut c = Columns::new(row);
                let pair = (c.unsigned("term_id"), c.unsigned("object_id"));
                c.finish(pair)
            })
            .collect()
    }

    /// `wp_count_terms()` of `taxonomy`: its terms, those `search` finds
    /// when given.
    pub async fn count_terms(&self, taxonomy: &str, search: Option<&str>) -> Result<u64, DbError> {
        let mut sql = Sql::new(
            "SELECT COUNT(*) AS found FROM {prefix}terms AS t \
             INNER JOIN {prefix}term_taxonomy AS tt ON t.term_id = tt.term_id \
             WHERE tt.taxonomy IN (?)",
        );
        sql.bind(Bind::Text(taxonomy.into()));
        if let Some(search) = search {
            search_terms(&mut sql, search);
        }
        let row = sql
            .query(&self.prefix)
            .fetch_one(&self.pool)
            .await
            .map_err(db_error)?;
        row.try_get::<i64, _>("found")
            .map(i64::unsigned_abs)
            .map_err(db_error)
    }

    /// How many terms of `taxonomy` have posts.
    pub async fn count_terms_with_posts(&self, taxonomy: &str) -> Result<u64, DbError> {
        let sql = self.prefix.sql(
            "SELECT COUNT(*) AS found FROM {prefix}term_taxonomy \
             WHERE taxonomy = ? AND count > 0",
        );
        let row = sqlx::query(sql)
            .bind(taxonomy)
            .fetch_one(&self.pool)
            .await
            .map_err(db_error)?;
        row.try_get::<i64, _>("found")
            .map(i64::unsigned_abs)
            .map_err(db_error)
    }

    /// `wp_get_archives()`'s periods of published posts, newest first.
    pub async fn date_counts(&self, period: Period) -> Result<Vec<DateCount>, DbError> {
        let mut sql = Sql::new("SELECT CAST(YEAR(post_date) AS SIGNED) AS y, ");
        match period {
            Period::Year => sql.push("0 AS m, 0 AS d, 0 AS w"),
            Period::Month => sql.push("CAST(MONTH(post_date) AS SIGNED) AS m, 0 AS d, 0 AS w"),
            Period::Day => sql.push(
                "CAST(MONTH(post_date) AS SIGNED) AS m, \
                 CAST(DAYOFMONTH(post_date) AS SIGNED) AS d, 0 AS w",
            ),
            // `_wp_mysql_week()`, the month and day of the week's latest post.
            Period::Week(start) => {
                sql.push(
                    "CAST(MONTH(MAX(post_date)) AS SIGNED) AS m, \
                     CAST(DAYOFMONTH(MAX(post_date)) AS SIGNED) AS d, ",
                );
                match start {
                    1 => sql.push("CAST(WEEK(post_date, 1) AS SIGNED) AS w"),
                    2..=6 => {
                        sql.push(
                            "CAST(WEEK(DATE_SUB(post_date, INTERVAL ? DAY), 0) AS SIGNED) AS w",
                        );
                        sql.bind(Bind::Int(start.unsigned_abs()));
                    }
                    _ => sql.push("CAST(WEEK(post_date, 0) AS SIGNED) AS w"),
                }
            }
        }
        sql.push(
            ", COUNT(ID) AS posts FROM {prefix}posts \
             WHERE post_type = 'post' AND post_status = 'publish' GROUP BY ",
        );
        sql.push(match period {
            Period::Year => "y ORDER BY y DESC",
            Period::Month => "y, m ORDER BY y DESC, m DESC",
            Period::Day => "y, m, d ORDER BY y DESC, m DESC, d DESC",
            Period::Week(_) => "w, y ORDER BY MAX(post_date) DESC",
        });
        let rows = sql
            .query(&self.prefix)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter().map(date_count).collect()
    }

    /// Whether a post of `post_type` is published, as `wp_count_posts()`
    /// tells the calendar block.
    pub async fn has_published_posts(&self, post_type: &str) -> Result<bool, DbError> {
        let sql = self.prefix.sql(
            "SELECT COUNT(*) AS found FROM {prefix}posts \
             WHERE post_type = ? AND post_status = 'publish' LIMIT 1",
        );
        let row = sqlx::query(sql)
            .bind(post_type)
            .fetch_one(&self.pool)
            .await
            .map_err(db_error)?;
        let mut c = Columns::new(&row);
        let found = c.signed("found") > 0;
        c.finish(found)
    }

    /// `get_calendar()`'s reads of the month `month` of `year`, which has
    /// `last_day` days: the published posts' months around it and its days.
    pub async fn calendar_month(
        &self,
        year: i64,
        month: u64,
        last_day: u64,
    ) -> Result<CalendarMonth, DbError> {
        // The dates as get_calendar() writes them into its SQL.
        let first = format!("{year}-{month:02}-01");
        let last = format!("{year}-{month:02}-{last_day} 23:59:59");
        let previous = self
            .month_with_posts(
                "SELECT CAST(YEAR(post_date) AS SIGNED) AS y, CAST(MONTH(post_date) AS SIGNED) AS m \
                 FROM {prefix}posts WHERE post_date < ? AND post_type = 'post' \
                 AND post_status = 'publish' ORDER BY post_date DESC LIMIT 1",
                &first,
            )
            .await;
        let next = self
            .month_with_posts(
                "SELECT CAST(YEAR(post_date) AS SIGNED) AS y, CAST(MONTH(post_date) AS SIGNED) AS m \
                 FROM {prefix}posts WHERE post_date > ? AND post_type = 'post' \
                 AND post_status = 'publish' ORDER BY post_date ASC LIMIT 1",
                &last,
            )
            .await;
        let days = self
            .days_with_posts(&format!("{first} 00:00:00"), &last)
            .await;
        previous.and_then(|previous| {
            next.and_then(|next| {
                days.map(|days| CalendarMonth {
                    previous,
                    next,
                    days,
                })
            })
        })
    }

    /// The `(year, month)` of the post `sql` finds by `date`, if any.
    async fn month_with_posts(
        &self,
        sql: &'static str,
        date: &str,
    ) -> Result<Option<(u64, u64)>, DbError> {
        let row = sqlx::query(self.prefix.sql(sql))
            .bind(date)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;
        row.map(|row| {
            let mut c = Columns::new(&row);
            let month = (c.signed("y").unsigned_abs(), c.signed("m").unsigned_abs());
            c.finish(month)
        })
        .transpose()
    }

    /// The days of published posts from `from` to `to`.
    async fn days_with_posts(&self, from: &str, to: &str) -> Result<Vec<u64>, DbError> {
        let sql = self.prefix.sql(
            "SELECT DISTINCT CAST(DAYOFMONTH(post_date) AS SIGNED) AS d FROM {prefix}posts \
             WHERE post_date >= ? AND post_type = 'post' AND post_status = 'publish' \
             AND post_date <= ?",
        );
        let rows = sqlx::query(sql)
            .bind(from)
            .bind(to)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter()
            .map(|row| {
                let mut c = Columns::new(row);
                let day = c.signed("d").unsigned_abs();
                c.finish(day)
            })
            .collect()
    }

    /// `get_lastpostmodified( 'GMT' )`: the latest modified or published
    /// time of a published post of a public type, `None` without one.
    pub async fn last_post_modified(&self) -> Result<Option<String>, DbError> {
        let sql = self.prefix.sql(
            "SELECT CAST(GREATEST(MAX(post_modified_gmt), MAX(post_date_gmt)) AS CHAR) AS latest \
             FROM {prefix}posts WHERE post_status = 'publish' \
             AND post_type IN ('post', 'page', 'attachment')",
        );
        let row = sqlx::query(sql)
            .fetch_one(&self.pool)
            .await
            .map_err(db_error)?;
        row.try_get("latest").map_err(db_error)
    }
}

/// The statement [`Db::adjacent_post`] runs: `get_adjacent_post()`'s, for
/// a visitor, selecting the whole row.
fn adjacent_sql(post: &Post, previous: bool, terms: Option<(&str, &[u64])>) -> Sql {
    let mut sql = Sql::new(concat!(
        "SELECT ",
        post_columns!(),
        " FROM {prefix}posts AS p"
    ));
    if terms.is_some() {
        sql.push(
            " INNER JOIN {prefix}term_relationships AS tr ON p.ID = tr.object_id \
             INNER JOIN {prefix}term_taxonomy AS tt ON tr.term_taxonomy_id = tt.term_taxonomy_id",
        );
    }
    sql.push(if previous {
        " WHERE (p.post_date < ? OR (p.post_date = ? AND p.ID < ?)) AND p.post_type = ?"
    } else {
        " WHERE (p.post_date > ? OR (p.post_date = ? AND p.ID > ?)) AND p.post_type = ?"
    });
    sql.bind(Bind::Text(post.date.clone()));
    sql.bind(Bind::Text(post.date.clone()));
    sql.bind(Bind::Int(post.id));
    sql.bind(Bind::Text(post.post_type.clone()));
    if let Some((taxonomy, ids)) = terms {
        sql.push(" AND tt.taxonomy = ? AND tt.term_id IN (");
        sql.bind(Bind::Text(taxonomy.to_string()));
        list(&mut sql, ids_or_none(ids));
    }
    sql.push(if previous {
        " AND p.post_status = 'publish' ORDER BY p.post_date DESC, p.ID DESC LIMIT 1"
    } else {
        " AND p.post_status = 'publish' ORDER BY p.post_date ASC, p.ID ASC LIMIT 1"
    });
    sql
}

/// The statement [`Db::posts`] runs for `query`.
fn posts_sql(query: &PostQuery) -> Sql {
    let mut sql = Sql::new(concat!(
        "SELECT ",
        post_columns!(),
        " FROM {prefix}posts WHERE "
    ));
    post_criteria(&mut sql, query);
    sql.push(" ORDER BY ");
    let ranked = ranked(query);
    let order: Vec<(OrderBy, bool)> = query
        .order
        .iter()
        .copied()
        .filter(|(column, _)| ranked || *column != OrderBy::Relevance)
        .collect();
    if order.is_empty() {
        sql.push("post_date DESC");
    }
    for (i, &(column, ascending)) in order.iter().enumerate() {
        if i > 0 {
            sql.push(", ");
        }
        match column {
            OrderBy::Relevance => {
                relevance(&mut sql, query);
                continue;
            }
            OrderBy::Seeded(seed) => {
                sql.push("RAND(?)");
                sql.bind(Bind::Int(seed));
            }
            OrderBy::Included => {
                sql.push("FIELD(ID, ");
                list(
                    &mut sql,
                    field_ids(query.include.as_deref().unwrap_or_default()),
                );
            }
            OrderBy::Parents => {
                sql.push("FIELD(post_parent, ");
                list(&mut sql, field_ids(&query.parents));
            }
            column => sql.push(order_column(column)),
        }
        sql.push(if ascending { " ASC" } else { " DESC" });
    }
    sql.push(" LIMIT ? OFFSET ?");
    sql.bind(Bind::Int(query.limit.into()));
    sql.bind(Bind::Int(query.offset.into()));
    sql
}

/// The terms of `query`'s search not left out.
fn titled(query: &PostQuery) -> Vec<&String> {
    query
        .search
        .iter()
        .filter(|term| !term.starts_with('-'))
        .collect()
}

/// The phrase match of `parse_search_order()`: none when a word of the
/// search, split on ASCII whitespace as PCRE's `\s`, is left out.
fn phrase(query: &PostQuery) -> Option<String> {
    let left_out = query
        .sentence
        .split([' ', '\t', '\n', '\x0B', '\x0C', '\r'])
        .any(|word| word.starts_with('-'));
    (!left_out).then(|| like(&query.sentence))
}

/// `parse_search_order()` gives an order: a term in the title, and a
/// phrase or fewer than seven terms to rank by.
fn ranked(query: &PostQuery) -> bool {
    let titled = titled(query).len();
    titled > 0 && (titled < 7 || phrase(query).is_some())
}

/// `WP_Query::parse_search_order()`: the first term in the title for one
/// word; with more, the CASE of the phrase and terms in each field.
fn relevance(sql: &mut Sql, query: &PostQuery) {
    let titled = titled(query);
    let title = |term: &String, sql: &mut Sql| {
        sql.push("post_title LIKE ?");
        sql.bind(Bind::Text(like(term)));
    };
    if query.search_count < 2 {
        for term in titled.iter().take(1) {
            title(term, sql);
        }
        sql.push(" DESC");
        return;
    }
    let phrase = phrase(query);
    sql.push("(CASE ");
    if let Some(phrase) = &phrase {
        sql.push("WHEN post_title LIKE ? THEN 1 ");
        sql.bind(Bind::Text(phrase.clone()));
    }
    // WordPress leaves out THEN 3 for one term, which repeats THEN 2.
    if titled.len() < 7 {
        for (joiner, then) in [(" AND ", " THEN 2 "), (" OR ", " THEN 3 ")] {
            sql.push("WHEN ");
            for (i, term) in titled.iter().enumerate() {
                if i > 0 {
                    sql.push(joiner);
                }
                title(term, sql);
            }
            sql.push(then);
        }
    }
    if let Some(phrase) = phrase {
        for when in [
            "WHEN post_excerpt LIKE ? THEN 4 ",
            "WHEN post_content LIKE ? THEN 5 ",
        ] {
            sql.push(when);
            sql.bind(Bind::Text(phrase.clone()));
        }
    }
    sql.push("ELSE 6 END)");
}

/// The `WHERE` criteria of [`posts_sql`]: posts of the query's statuses and
/// types (any, if none), authors in or out, parents, terms, formats, inclusions,
/// search and date.
fn post_criteria(sql: &mut Sql, query: &PostQuery) {
    if query.statuses.is_empty() {
        sql.push("post_status = 'publish'");
    } else {
        sql.push("(");
        for (i, filter) in query.statuses.iter().enumerate() {
            if i > 0 {
                sql.push(" OR ");
            }
            if let Some(author) = filter.author {
                sql.push("post_author = ? AND ");
                sql.bind(Bind::Int(author));
            }
            sql.push("post_status = ?");
            sql.bind(Bind::Text(filter.status.clone()));
        }
        sql.push(")");
    }
    if !query.post_types.is_empty() {
        sql.push(" AND post_type IN (");
        list(sql, query.post_types.iter().map(|t| Bind::Text(t.clone())));
    }
    if !query.authors.is_empty() {
        sql.push(" AND post_author IN (");
        list(sql, query.authors.iter().map(|&id| Bind::Int(id)));
    }
    if !query.excluded_authors.is_empty() {
        sql.push(" AND post_author NOT IN (");
        list(sql, query.excluded_authors.iter().map(|&id| Bind::Int(id)));
    }
    if !query.parents.is_empty() {
        sql.push(" AND post_parent IN (");
        list(sql, query.parents.iter().map(|&id| Bind::Int(id)));
    }
    for filter in &query.terms {
        sql.push(if filter.exclude {
            " AND ID NOT IN ("
        } else {
            " AND ID IN ("
        });
        sql.push(
            "SELECT tr.object_id FROM {prefix}term_relationships tr \
             JOIN {prefix}term_taxonomy tt ON tt.term_taxonomy_id = tr.term_taxonomy_id \
             WHERE tt.taxonomy = ? AND tt.term_id IN (",
        );
        sql.bind(Bind::Text(filter.taxonomy.clone()));
        list(sql, ids_or_none(&filter.terms));
        sql.push(")");
    }
    if let Some(formats) = &query.formats {
        sql.push(" AND (");
        if formats.standard {
            sql.push(
                "NOT EXISTS (SELECT 1 FROM {prefix}term_relationships tr \
                 JOIN {prefix}term_taxonomy tt ON tt.term_taxonomy_id = tr.term_taxonomy_id \
                 WHERE tt.taxonomy = 'post_format' AND tr.object_id = {prefix}posts.ID)",
            );
        }
        if !formats.slugs.is_empty() {
            sql.push(if formats.standard {
                " OR ID IN ("
            } else {
                "ID IN ("
            });
            sql.push(
                "SELECT tr.object_id FROM {prefix}term_relationships tr \
                 JOIN {prefix}term_taxonomy tt ON tt.term_taxonomy_id = tr.term_taxonomy_id \
                 JOIN {prefix}terms t ON t.term_id = tt.term_id \
                 WHERE tt.taxonomy = 'post_format' AND t.slug IN (",
            );
            list(
                sql,
                formats.slugs.iter().map(|slug| Bind::Text(slug.clone())),
            );
            sql.push(")");
        }
        sql.push(")");
    }
    if let Some(include) = &query.include {
        sql.push(" AND ID IN (");
        list(sql, ids_or_none(include));
    }
    for id in &query.exclude {
        sql.push(" AND ID <> ?");
        sql.bind(Bind::Int(*id));
    }
    // A term after `-` excludes the posts with it in any column.
    for term in &query.search {
        let (term, columns) = match term.strip_prefix('-') {
            Some(rest) => (
                rest,
                " AND (post_title NOT LIKE ? AND post_excerpt NOT LIKE ? AND post_content NOT LIKE ?)",
            ),
            None => (
                term.as_str(),
                " AND (post_title LIKE ? OR post_excerpt LIKE ? OR post_content LIKE ?)",
            ),
        };
        sql.push(columns);
        let pattern = if query.exact {
            esc_like(term)
        } else {
            like(term)
        };
        for _ in 0..3 {
            sql.bind(Bind::Text(pattern.clone()));
        }
    }
    // A visitor's search leaves out protected posts.
    if !query.search.is_empty() {
        sql.push(" AND post_password = ''");
    }
    let date = [
        (" AND YEAR(post_date) = ?", query.year),
        (" AND MONTH(post_date) = ?", query.month),
        (" AND DAYOFMONTH(post_date) = ?", query.day),
    ];
    for (fragment, value) in date {
        if let Some(value) = value {
            sql.push(fragment);
            sql.bind(Bind::Int(value));
        }
    }
    for condition in &query.time {
        let value = match condition {
            TimeCondition::Hour(n) => {
                sql.push(" AND HOUR(post_date) = ?");
                *n
            }
            TimeCondition::Minute(n) => {
                sql.push(" AND MINUTE(post_date) = ?");
                *n
            }
            TimeCondition::Second(n) => {
                sql.push(" AND SECOND(post_date) = ?");
                *n
            }
            TimeCondition::Formatted { format, time } => {
                sql.push(" AND DATE_FORMAT(post_date, ?) = CAST(? AS DECIMAL(65, 6))");
                sql.bind(Bind::Text(format.clone()));
                sql.bind(Bind::Text(time.clone()));
                continue;
            }
            TimeCondition::Week {
                week,
                start_of_week,
            } => {
                match start_of_week {
                    1 => sql.push(" AND WEEK(post_date, 1) = ?"),
                    2..=6 => {
                        sql.push(" AND WEEK(DATE_SUB(post_date, INTERVAL ? DAY), 0) = ?");
                        sql.bind(Bind::Int(start_of_week.unsigned_abs()));
                    }
                    _ => sql.push(" AND WEEK(post_date, 0) = ?"),
                }
                *week
            }
        };
        sql.bind(Bind::Int(value));
    }
}

/// `ids` led by 0, which no row has, so an empty list matches nothing, as
/// WordPress's `array( 0 )`.
fn ids_or_none(ids: &[u64]) -> impl Iterator<Item = Bind> + '_ {
    std::iter::once(0).chain(ids.iter().copied()).map(Bind::Int)
}

/// `values` bound as a comma separated list, closing the parenthesis.
fn list(sql: &mut Sql, values: impl Iterator<Item = Bind>) {
    for (i, value) in values.enumerate() {
        sql.push(if i == 0 { "?" } else { ", ?" });
        sql.bind(value);
    }
    sql.push(")");
}

/// The list of a `FIELD()` order: `ids` and a 0 after them, which keeps the
/// list valid when empty and, matching nothing earlier, changes no order.
fn field_ids(ids: &[u64]) -> impl Iterator<Item = Bind> + '_ {
    ids.iter().copied().chain(std::iter::once(0)).map(Bind::Int)
}

/// The column a term order sorts by; none for the `FIELD()` of `include`.
fn term_order_column(order: TermOrder) -> Option<&'static str> {
    Some(match order {
        TermOrder::TermId => "t.term_id",
        TermOrder::Name => "t.name",
        TermOrder::Slug => "t.slug",
        TermOrder::TermGroup => "t.term_group",
        TermOrder::Count => "tt.count",
        TermOrder::Parent => "tt.parent",
        TermOrder::Taxonomy => "tt.taxonomy",
        TermOrder::TermTaxonomyId => "tt.term_taxonomy_id",
        TermOrder::Description => "tt.description",
        TermOrder::Include => return None,
    })
}

/// The `ORDER BY` term of an order without binds: a column or `RAND()`.
fn order_column(column: OrderBy) -> &'static str {
    match column {
        OrderBy::Date => "post_date",
        OrderBy::Title => "post_title",
        OrderBy::Modified => "post_modified",
        OrderBy::Name => "post_name",
        OrderBy::Author => "post_author",
        OrderBy::Parent => "post_parent",
        OrderBy::Type => "post_type",
        OrderBy::Id => "ID",
        OrderBy::MenuOrder => "menu_order",
        OrderBy::CommentCount => "comment_count",
        _ => "RAND()",
    }
}

/// `WP_Term_Query::get_search_sql()`: a name or slug containing `search`.
fn search_terms(sql: &mut Sql, search: &str) {
    sql.push(" AND ((t.name LIKE ?) OR (t.slug LIKE ?))");
    sql.bind(Bind::Text(like(search)));
    sql.bind(Bind::Text(like(search)));
}

/// A `LIKE` pattern matching `text` anywhere.
fn like(text: &str) -> String {
    format!("%{}%", esc_like(text))
}

/// `$wpdb->esc_like()`: `text` with `%`, `_` and backslashes escaped.
fn esc_like(text: &str) -> String {
    let mut pattern = String::new();
    for c in text.chars() {
        if matches!(c, '%' | '_' | '\\') {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    pattern
}

enum Bind {
    Text(String),
    Int(u64),
    Signed(i64),
}

/// The users `Db::list_users()` lists.
pub struct UserFilter<'a> {
    /// The `{prefix}capabilities` meta key.
    pub caps_key: &'a str,
    /// Roles each listed user's capabilities name.
    pub roles: &'a [String],
    /// Only these users, unless none are given.
    pub include: &'a [u64],
    /// `get_search_sql()` of a text in columns of `USER_SEARCH_COLUMNS`, the
    /// ID compared whole and the others containing it.
    pub search: Option<(&'a [&'a str], &'a str)>,
    /// Private posts count for everyone's post counts, else only `me`'s.
    pub all_private: bool,
    /// The current user.
    pub me: u64,
}

/// The `WHERE` of `filter`.
fn user_filter(sql: &mut Sql, filter: &UserFilter<'_>) {
    sql.push(" WHERE 1=1");
    for role in filter.roles {
        sql.push(
            " AND EXISTS (SELECT 1 FROM {prefix}usermeta r WHERE r.user_id = u.ID \
             AND r.meta_key = ? AND r.meta_value LIKE ?)",
        );
        sql.bind(Bind::Text(filter.caps_key.into()));
        sql.bind(Bind::Text(like(&format!("\"{role}\""))));
    }
    if !filter.include.is_empty() {
        sql.push(" AND u.ID IN (");
        list(sql, filter.include.iter().copied().map(Bind::Int));
    }
    if let Some((columns, text)) = filter.search {
        let columns: Vec<&'static str> = columns
            .iter()
            .filter_map(|name| USER_SEARCH_COLUMNS.iter().find(|column| *column == name))
            .copied()
            .collect();
        // A caller's columns are all known; none would make the query invalid.
        if columns.is_empty() {
            return;
        }
        for (i, column) in columns.into_iter().enumerate() {
            sql.push(if i == 0 { " AND (" } else { " OR " });
            sql.push(column);
            if column == "ID" {
                sql.push(" = ?");
                sql.bind(Bind::Text(text.into()));
            } else {
                sql.push(" LIKE ?");
                sql.bind(Bind::Text(like(text)));
            }
        }
        sql.push(")");
    }
}

/// The comments `Db::list_comments()` lists, notes aside.
pub struct CommentFilter<'a> {
    /// The `comment_approved` values.
    pub approved: &'a [&'a str],
    /// Only those of this user ID, `user_id = %d`.
    pub user: Option<i64>,
    /// Only those of these `comment_type`s, when any are given.
    pub types: &'a [&'a str],
    /// `get_search_sql()`: only those with this in their author, email,
    /// URL, IP or content.
    pub search: Option<&'a str>,
    /// Only those on this post, unless 0.
    pub post: u64,
}

/// Only the comments on `post`, unless 0, in a condition the index serves.
fn post_filter(sql: &mut Sql, post: u64) {
    if post > 0 {
        sql.push(" AND comment_post_ID = ?");
        sql.bind(Bind::Int(post));
    }
}

/// The `WHERE` of `filter`.
fn comment_filter(sql: &mut Sql, filter: &CommentFilter<'_>) {
    sql.push("comment_type != 'note'");
    post_filter(sql, filter.post);
    sql.push(" AND comment_approved IN (");
    list(sql, filter.approved.iter().map(|v| Bind::Text((*v).into())));
    if let Some(user) = filter.user {
        sql.push(" AND user_id = ?");
        sql.bind(Bind::Signed(user));
    }
    if !filter.types.is_empty() {
        sql.push(" AND comment_type IN (");
        list(sql, filter.types.iter().map(|v| Bind::Text((*v).into())));
    }
    if let Some(search) = filter.search {
        sql.push(
            " AND (comment_author LIKE ? OR comment_author_email LIKE ? OR comment_author_url LIKE ? \
             OR comment_author_IP LIKE ? OR comment_content LIKE ?)",
        );
        for _ in 0..5 {
            sql.bind(Bind::Text(like(search)));
        }
    }
}

/// The posts `Db::posts_by_status()` lists.
pub struct StatusQuery<'a> {
    /// `post_status`.
    pub status: &'a str,
    /// Only this author's.
    pub author: Option<u64>,
    /// Their order.
    pub order: StatusOrder,
    /// How many.
    pub limit: u32,
}

/// The orders of `StatusQuery`.
#[derive(Clone, Copy)]
pub enum StatusOrder {
    /// Oldest first.
    DateAsc,
    /// Newest first.
    DateDesc,
    /// Last modified first.
    ModifiedDesc,
}

/// SQL assembled only from fixed fragments, with bound values.
struct Sql {
    text: String,
    binds: Vec<Bind>,
}

impl Sql {
    fn new(fragment: &'static str) -> Sql {
        Sql {
            text: fragment.to_string(),
            binds: Vec::new(),
        }
    }

    fn push(&mut self, fragment: &'static str) {
        self.text.push_str(fragment);
    }

    fn bind(&mut self, value: Bind) {
        self.binds.push(value);
    }

    fn query(
        self,
        prefix: &Prefix,
    ) -> sqlx::query::Query<'static, sqlx::MySql, sqlx::mysql::MySqlArguments> {
        let mut query = sqlx::query(prefix.sql_from_fragments(self.text));
        for value in self.binds {
            query = match value {
                Bind::Text(text) => query.bind(text),
                Bind::Int(n) => query.bind(n),
                Bind::Signed(n) => query.bind(n),
            };
        }
        query
    }
}

#[cfg(test)]
mod tests;

// SPDX-License-Identifier: AGPL-3.0-or-later

//! Access to a WordPress database (M1.3, ADR 0003).
//!
//! - **Read only reads.** [`Db`] puts every connection in a read only
//!   transaction mode, as the read only user when `.env` names one.
//! - **Gated writes.** Only a [`Writer`], which exists in primary mode
//!   only, writes, one transaction per action (ADRs 0010 and 0012).
//! - **No string-built SQL.** Queries are fixed text; only the table prefix
//!   is inserted, and `Prefix` admits nothing but `[A-Za-z0-9_]`. Values
//!   are always bound parameters.
//! - **Bytes, not text.** Text columns are read as bytes, so content is
//!   never re-encoded whatever the site's charset.
//! - **Portable SQL.** Queries are standard SQL; what is not portable is
//!   written by the engine's [`Dialect`] (ADR 0014).

use std::fmt;

use hyscribe_config::{Database, Prefix};
use hyscribe_model::{
    COMMENT_ORDERBY, CalendarMonth, Comment, Credentials, DateCount, OrderBy, Period, Post,
    PostQuery, Term, TermOrder, TermQuery, TimeCondition, USER_ORDERBY, USER_SEARCH_COLUMNS, User,
    UserField,
};
use sqlx::Row as _;

pub use dialect::Connection;
use dialect::{Access, DatePart, Dialect};

/// The engine's dialect: MySQL and MariaDB, the only one for now.
type Engine = dialect::MySql;
type Backend = <Engine as Dialect>::Backend;
type Row = <Backend as sqlx::Database>::Row;

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
        #[cfg(test)]
        tests::emitted(query);
        self.sql_from_fragments(query.to_string())
    }

    fn sql_from_fragments(&self, query: String) -> sqlx::AssertSqlSafe<String> {
        sqlx::AssertSqlSafe(query.replace("{prefix}", self.as_str()))
    }
}

/// Why a database call failed. Messages never contain the password.
#[derive(Clone, Debug)]
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

/// `date` as text under its own name.
fn date_column(sql: &mut Sql, date: &'static str) {
    sql.push(", ");
    Engine::date_text(sql, date);
    sql.push(" AS ");
    sql.push(date);
}

/// `SELECT` the `{prefix}comments` columns [`Comment`] reads, then `rest`.
fn select_comments(rest: &'static str) -> Sql {
    let mut sql = Sql::new(
        "SELECT comment_ID, comment_post_ID, comment_author, comment_author_email, \
         comment_author_url, comment_author_IP, comment_content, comment_type, comment_parent, \
         user_id",
    );
    date_column(&mut sql, "comment_date");
    date_column(&mut sql, "comment_date_gmt");
    sql.push(rest);
    sql
}

/// `SELECT` the `{prefix}posts` columns [`Post`] reads, then `rest`.
fn select_posts(rest: &'static str) -> Sql {
    let mut sql = Sql::new(
        "SELECT ID, post_author, post_content, post_title, post_excerpt, post_status, \
         post_password, post_name, post_parent, guid, post_type, post_mime_type, comment_count, \
         comment_status, ping_status",
    );
    for date in [
        "post_date",
        "post_date_gmt",
        "post_modified",
        "post_modified_gmt",
    ] {
        date_column(&mut sql, date);
    }
    sql.push(", ");
    Engine::integer(&mut sql, |sql| sql.push("menu_order"));
    sql.push(" AS menu_order");
    sql.push(rest);
    sql
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
    row: &'r Row,
    error: Option<sqlx::Error>,
}

impl<'r> Columns<'r> {
    fn new(row: &'r Row) -> Columns<'r> {
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

fn term(row: &Row) -> Result<Term, DbError> {
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

fn term_id(row: &Row) -> Result<u64, DbError> {
    let mut c = Columns::new(row);
    let id = c.unsigned("term_id");
    c.finish(id)
}

fn date_count(row: &Row) -> Result<DateCount, DbError> {
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

fn user(row: &Row) -> Result<User, DbError> {
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

fn comment(row: &Row) -> Result<Comment, DbError> {
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

fn post(row: &Row) -> Result<Post, DbError> {
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
    pool: sqlx::Pool<Backend>,
    prefix: Prefix,
}

impl Db {
    /// Connects with `config`, read only, as the read user if there is one.
    pub async fn connect(config: &Database) -> Result<Db, DbError> {
        let pool = Engine::connect(config, Access::Read)
            .await
            .map_err(db_error)?;
        Ok(Db {
            pool,
            prefix: config.prefix.clone(),
        })
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
        let mut sql = select_posts(" FROM {prefix}posts WHERE ID = ?");
        sql.bind(Bind::Int(id));
        let row = sql
            .query(&self.prefix)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;
        row.as_ref().map(post).transpose()
    }

    /// The published post (of type `post`) named `name`.
    pub async fn post_by_name(&self, name: &str) -> Result<Option<Post>, DbError> {
        let mut sql = select_posts(" FROM {prefix}posts WHERE ");
        sql.push("post_name = ? AND post_type = 'post' AND post_status = 'publish' LIMIT 1");
        sql.bind(Bind::Text(name.to_string()));
        self.select(sql).await.map(|posts| posts.into_iter().next())
    }

    /// `get_page_by_path()`'s candidates: the pages and attachments of any
    /// status named one of `names`.
    pub async fn named_pages(&self, names: &[Vec<u8>]) -> Result<Vec<Post>, DbError> {
        let mut sql = select_posts(" FROM {prefix}posts WHERE ");
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
        let mut sql = select_posts(" FROM {prefix}posts WHERE ");
        sql.push("post_name LIKE ? AND post_status = 'publish' AND post_type IN (");
        sql.bind(Bind::Text(format!("{}%", esc_like(name))));
        list(
            &mut sql,
            post_types.iter().map(|t| Bind::Text((*t).to_string())),
        );
        date_parts(
            &mut sql,
            [
                (DatePart::Year, year),
                (DatePart::Month, month),
                (DatePart::Day, day),
            ],
        );
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

    /// The privileges of the connected user (the read user when there is
    /// one) and its roles as `role <name>`, which the setup wizard checks.
    pub async fn privileges(&self) -> Result<Vec<String>, DbError> {
        let mut sql = Sql::new("");
        Engine::privileges(&mut sql);
        let rows = sql.query(&self.prefix).fetch_all(&self.pool).await;
        rows.map_err(db_error).and_then(|rows| {
            rows.iter()
                .map(|row| {
                    let mut c = Columns::new(row);
                    let privilege = c.text("privilege");
                    c.finish(privilege)
                })
                .collect()
        })
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
        let mut sql = select_comments(
            " FROM {prefix}comments \
             WHERE comment_post_ID = ? AND comment_approved = '1' AND comment_type != 'note' \
             ORDER BY comment_date_gmt ASC, comment_ID ASC",
        );
        sql.bind(Bind::Int(post_id));
        let rows = sql
            .query(&self.prefix)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter().map(comment).collect()
    }

    /// How many approved top level comments of every type, notes too,
    /// `post_id` has from before `date_gmt`: `get_page_of_comment()`'s count.
    pub async fn older_comments(&self, post_id: u64, date_gmt: &str) -> Result<u64, DbError> {
        let mut sql = Sql::new(
            "SELECT COUNT(*) AS found FROM {prefix}comments WHERE comment_post_ID = ? \
             AND comment_approved = '1' AND comment_parent = 0 AND comment_date_gmt < ?",
        );
        sql.bind(Bind::Int(post_id));
        sql.bind(Bind::Text(date_gmt.into()));
        let row = sql
            .query(&self.prefix)
            .fetch_one(&self.pool)
            .await
            .map_err(db_error)?;
        row.try_get::<i64, _>("found")
            .map(i64::unsigned_abs)
            .map_err(db_error)
    }

    /// The comments a comment feed lists, newest first: `post`'s, or
    /// those on published posts and attachments.
    pub async fn feed_comments(
        &self,
        post: Option<u64>,
        limit: u64,
    ) -> Result<Vec<Comment>, DbError> {
        let mut sql = select_comments(
            " FROM {prefix}comments JOIN {prefix}posts ON comment_post_ID = ID \
             WHERE (? IS NULL AND (post_status = 'publish' \
             OR (post_status = 'inherit' AND post_type = 'attachment')) OR comment_post_ID = ?) \
             AND comment_approved = '1' AND comment_type != 'note' \
             ORDER BY comment_date_gmt DESC LIMIT ?",
        );
        sql.bind(Bind::Maybe(post));
        sql.bind(Bind::Maybe(post));
        sql.bind(Bind::Int(limit));
        let rows = sql
            .query(&self.prefix)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter().map(comment).collect()
    }

    /// `get_comments()` of the latest comments block: the approved
    /// comments on published posts, newest first, `limit` of them.
    pub async fn recent_comments(&self, limit: u64) -> Result<Vec<Comment>, DbError> {
        let mut sql = select_comments(
            " FROM {prefix}comments JOIN {prefix}posts ON comment_post_ID = ID \
             WHERE post_status = 'publish' AND comment_approved = '1' \
             AND comment_type != 'note' \
             ORDER BY comment_date_gmt DESC, comment_ID DESC LIMIT ?",
        );
        sql.bind(Bind::Int(limit));
        let rows = sql
            .query(&self.prefix)
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
        let mut sql = select_comments(
            " FROM {prefix}comments WHERE comment_approved = '1' AND comment_type != 'note' \
             AND comment_post_ID IN (SELECT ID FROM {prefix}posts WHERE ",
        );
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
        let mut sql = Sql::new("SELECT ");
        Engine::date_text(&mut sql, "MAX(comment_date_gmt)");
        sql.push(" AS latest FROM {prefix}comments WHERE comment_approved = '1'");
        self.latest(sql).await
    }

    /// Posts of type `post` in one status, as the dashboard lists them.
    pub async fn posts_by_status(&self, query: &StatusQuery<'_>) -> Result<Vec<Post>, DbError> {
        let mut sql =
            select_posts(" FROM {prefix}posts WHERE post_type = 'post' AND post_status = ?");
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
        self.counts_by_status(sql).await
    }

    /// The posts of `post_type` named `name` of each status, as
    /// `WP_Privacy_Requests_Table::get_request_counts()` counts requests.
    pub async fn named_status_counts(
        &self,
        post_type: &str,
        name: &str,
    ) -> Result<Vec<(String, u64)>, DbError> {
        let mut sql = Sql::new(
            "SELECT post_status, COUNT(*) AS num_posts FROM {prefix}posts \
             WHERE post_type = ? AND post_name = ?",
        );
        sql.bind(Bind::Text(post_type.into()));
        sql.bind(Bind::Text(name.into()));
        self.counts_by_status(sql).await
    }

    /// The statuses and counts `sql` selects, grouped by status.
    async fn counts_by_status(&self, mut sql: Sql) -> Result<Vec<(String, u64)>, DbError> {
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
        let mut sql = Sql::new("SELECT DISTINCT ");
        Engine::date_part(&mut sql, DatePart::Year, "post_date");
        sql.push(" AS year, ");
        Engine::date_part(&mut sql, DatePart::Month, "post_date");
        sql.push(" AS month FROM {prefix}posts WHERE post_type = ? AND ");
        sql.push(if trash {
            "post_status = 'trash'"
        } else {
            "post_status != 'auto-draft' AND post_status != 'trash'"
        });
        // PHP orders by `post_date`, which falls in the month: the same order.
        sql.push(" ORDER BY year DESC, month DESC");
        sql.bind(Bind::Text(post_type.into()));
        let rows = sql
            .query(&self.prefix)
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

    /// `get_available_post_mime_types( $post_type )`: the MIME types its
    /// posts have, each once.
    pub async fn mime_types(&self, post_type: &str) -> Result<Vec<String>, DbError> {
        let sql = self.prefix.sql(
            "SELECT DISTINCT post_mime_type FROM {prefix}posts \
             WHERE post_type = ? AND post_mime_type != ''",
        );
        let rows = sqlx::query(sql)
            .bind(post_type)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter()
            .map(|row| {
                let mut c = Columns::new(row);
                let mime = c.text("post_mime_type");
                c.finish(mime)
            })
            .collect()
    }

    /// Whether a post other than `id` is named `name`, as
    /// `wp_unique_post_slug()` checks an attachment's slug.
    pub async fn post_name_taken(&self, name: &str, id: u64) -> Result<bool, DbError> {
        let sql = self
            .prefix
            .sql("SELECT ID FROM {prefix}posts WHERE post_name = ? AND ID != ? LIMIT 1");
        let row = sqlx::query(sql)
            .bind(name)
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;
        Ok(row.is_some())
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
            "SELECT COUNT(CASE WHEN comment_approved = '1' THEN 1 END) AS approved, \
             COUNT(CASE WHEN comment_approved = '0' THEN 1 END) AS moderated, \
             COUNT(CASE WHEN comment_approved = 'spam' THEN 1 END) AS spam, \
             COUNT(CASE WHEN comment_approved = 'trash' THEN 1 END) AS trash, \
             COUNT(CASE WHEN user_id = ? AND comment_approved IN ('0', '1') THEN 1 END) AS mine \
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
        let mut sql = select_comments(", comment_approved FROM {prefix}comments WHERE ");
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

    /// `get_comment( $id )->comment_approved`, `None` for no such comment.
    pub async fn comment_approved(&self, id: u64) -> Result<Option<String>, DbError> {
        let sql = self
            .prefix
            .sql("SELECT comment_approved FROM {prefix}comments WHERE comment_ID = ?");
        let row = sqlx::query(sql)
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let mut c = Columns::new(&row);
        let approved = c.text("comment_approved");
        c.finish(approved).map(Some)
    }

    /// `get_comment( $id )` in any state.
    pub async fn any_comment(&self, id: u64) -> Result<Option<Comment>, DbError> {
        let mut sql = select_comments(" FROM {prefix}comments WHERE comment_ID = ?");
        sql.bind(Bind::Int(id));
        let row = sql
            .query(&self.prefix)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;
        row.as_ref().map(comment).transpose()
    }

    /// One comment by ID that is held for moderation, of any type.
    pub async fn held_comment(&self, id: u64) -> Result<Option<Comment>, DbError> {
        let mut sql = select_comments(
            " FROM {prefix}comments WHERE comment_ID = ? AND comment_approved = '0'",
        );
        sql.bind(Bind::Int(id));
        let row = sql
            .query(&self.prefix)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;
        row.as_ref().map(comment).transpose()
    }

    /// One approved comment by ID, of any type.
    pub async fn comment(&self, id: u64) -> Result<Option<Comment>, DbError> {
        let mut sql = select_comments(
            " FROM {prefix}comments WHERE comment_ID = ? AND comment_approved = '1'",
        );
        sql.bind(Bind::Int(id));
        let row = sql
            .query(&self.prefix)
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
                 OR (post_status = 'private' AND (? = 1 OR post_author = ?))) \
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

    /// The authors of posts of `post_type`, of any status, as the Export
    /// screen reads them.
    pub async fn post_authors(&self, post_type: &str) -> Result<Vec<u64>, DbError> {
        let sql = self
            .prefix
            .sql("SELECT DISTINCT post_author FROM {prefix}posts WHERE post_type = ?");
        let rows = sqlx::query(sql)
            .bind(post_type)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter()
            .map(|row| {
                let mut c = Columns::new(row);
                let id = c.unsigned("post_author");
                c.finish(id)
            })
            .collect()
    }

    /// Every post of `post_type` whatever its status, newest first, as
    /// `WP_Query` orders posts by date.
    pub async fn posts_of_type(&self, post_type: &str) -> Result<Vec<Post>, DbError> {
        let mut sql = select_posts(" FROM {prefix}posts WHERE ");
        sql.push("post_type = ? ORDER BY post_date DESC");
        sql.bind(Bind::Text(post_type.to_string()));
        self.select(sql).await
    }

    /// The published posts of `post_type` with the `wp_theme` term named
    /// `theme`, newest first, as `WP_Query` finds them by a taxonomy query.
    pub async fn theme_posts(&self, post_type: &str, theme: &str) -> Result<Vec<Post>, DbError> {
        let mut sql = select_posts(" FROM {prefix}posts WHERE ");
        sql.push(
            "post_type = ? AND post_status = 'publish' AND ID IN (SELECT tr.object_id \
             FROM {prefix}term_relationships tr JOIN {prefix}term_taxonomy tt \
             ON tt.term_taxonomy_id = tr.term_taxonomy_id JOIN {prefix}terms t \
             ON t.term_id = tt.term_id WHERE tt.taxonomy = 'wp_theme' AND t.name = ?) \
             ORDER BY post_date DESC",
        );
        sql.bind(Bind::Text(post_type.to_string()));
        sql.bind(Bind::Text(theme.to_string()));
        self.select(sql).await
    }

    /// The IDs of `post`'s revisions, newest first, as
    /// `wp_get_latest_revision_id_and_total_count()` reads them.
    pub async fn revisions(&self, post: u64) -> Result<Vec<u64>, DbError> {
        let sql = self.prefix.sql(
            "SELECT ID FROM {prefix}posts WHERE post_parent = ? AND post_type = 'revision' \
             AND post_status = 'inherit' ORDER BY post_date DESC, ID DESC",
        );
        let rows = sqlx::query(sql)
            .bind(post)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter()
            .map(|row| {
                let mut c = Columns::new(row);
                let id = c.unsigned("ID");
                c.finish(id)
            })
            .collect()
    }

    /// `wp_get_users_with_no_role()`: those whose `key` meta does not match
    /// `regex`, the roles' names.
    pub async fn users_without_role(&self, key: &str, regex: &str) -> Result<Vec<u64>, DbError> {
        let mut sql = Sql::new("SELECT user_id FROM {prefix}usermeta WHERE meta_key = ? AND ");
        sql.bind(Bind::Text(key.into()));
        Engine::not_matching(&mut sql, "meta_value", regex.into());
        let rows = sql
            .query(&self.prefix)
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
             AND (post_status = 'publish' OR (post_status = 'private' AND (? = 1 OR post_author = ?))) \
             AND post_author IN (",
        );
        sql.bind(Bind::Int(u64::from(all_private)));
        sql.bind(Bind::Int(me));
        list(&mut sql, ids_or_none(ids));
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
                "SELECT ID, user_login, user_pass, user_activation_key FROM {prefix}users WHERE user_login = ? ORDER BY ID LIMIT 1"
            }
            UserField::Email => {
                "SELECT ID, user_login, user_pass, user_activation_key FROM {prefix}users WHERE user_email = ? ORDER BY ID LIMIT 1"
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
                activation_key: c.bytes("user_activation_key"),
            };
            c.finish(credentials)
        })
        .transpose()
    }

    /// Whether the user is the author of a post of any type or status, or
    /// the owner of a link, as `users.php` asks before a deletion.
    pub async fn owns_content(&self, user: u64) -> Result<bool, DbError> {
        for sql in [
            "SELECT ID FROM {prefix}posts WHERE post_author = ? LIMIT 1",
            "SELECT link_id FROM {prefix}links WHERE link_owner = ? LIMIT 1",
        ] {
            let query = sqlx::query(self.prefix.sql(sql)).bind(user);
            if query
                .fetch_optional(&self.pool)
                .await
                .map_err(db_error)?
                .is_some()
            {
                return Ok(true);
            }
        }
        Ok(false)
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

    /// The server's `VERSION()` and the connection's collation, as Site
    /// Health's Info tab shows them.
    pub async fn server(&self) -> Result<(String, String), DbError> {
        let row = sqlx::query("SELECT VERSION() AS version, @@collation_connection AS collation")
            .fetch_one(&self.pool)
            .await
            .map_err(db_error)?;
        let mut c = Columns::new(&row);
        let server = (c.text("version"), c.text("collation"));
        c.finish(server)
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
        let mut sql = term_where(Sql::new("SELECT t.term_id"), query);
        if let Some((order, descending)) = query.order {
            sql.push(" ORDER BY ");
            // `parse_orderby()` takes the order of an empty list as the name.
            let order = match order {
                TermOrder::Include if query.include.is_empty() => TermOrder::Name,
                TermOrder::Slugs if query.slugs.is_empty() => TermOrder::Name,
                order => order,
            };
            match (term_order_column(order), order) {
                (Some(column), _) => sql.push(column),
                (None, TermOrder::Slugs) => Engine::list_order(
                    &mut sql,
                    "t.slug",
                    query
                        .slugs
                        .iter()
                        .map(|slug| Bind::Text(String::from_utf8_lossy(slug).into_owned())),
                ),
                (None, _) => Engine::list_order(
                    &mut sql,
                    "t.term_id",
                    query.include.iter().copied().map(Bind::Int),
                ),
            }
            sql.push(if descending { " DESC" } else { " ASC" });
        }
        if let Some(limit) = query.limit {
            sql.push(" LIMIT ? OFFSET ?");
            sql.bind(Bind::Int(limit));
            sql.bind(Bind::Int(query.offset));
        }
        let rows = sql
            .query(&self.prefix)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        let ids: Result<Vec<u64>, DbError> = rows.iter().map(term_id).collect();
        let mut sql = Sql::new(concat!("SELECT ", terms_from!(), "WHERE tt.taxonomy IN ("));
        taxonomies(&mut sql, query);
        sql.push(" AND t.term_id IN (");
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

    /// `wp_count_terms()` of `query`: the terms it selects, without its
    /// order, limit and offset.
    pub async fn term_query_count(&self, query: &TermQuery) -> Result<u64, DbError> {
        let sql = term_where(Sql::new("SELECT COUNT(*) AS found"), query);
        let row = sql.query(&self.prefix).fetch_one(&self.pool).await;
        row.and_then(|row| row.try_get::<i64, _>("found"))
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
        let mut sql = Sql::new("SELECT ");
        let number = |sql: &mut Sql, part, date| {
            Engine::integer(sql, |sql| Engine::date_part(sql, part, date));
        };
        number(&mut sql, DatePart::Year, "post_date");
        sql.push(" AS y, ");
        match period {
            Period::Year => sql.push("0 AS m, 0 AS d, 0 AS w"),
            Period::Month => {
                number(&mut sql, DatePart::Month, "post_date");
                sql.push(" AS m, 0 AS d, 0 AS w");
            }
            Period::Day => {
                number(&mut sql, DatePart::Month, "post_date");
                sql.push(" AS m, ");
                number(&mut sql, DatePart::Day, "post_date");
                sql.push(" AS d, 0 AS w");
            }
            // `_wp_mysql_week()`, the month and day of the week's latest post.
            Period::Week(start) => {
                number(&mut sql, DatePart::Month, "MAX(post_date)");
                sql.push(" AS m, ");
                number(&mut sql, DatePart::Day, "MAX(post_date)");
                sql.push(" AS d, ");
                Engine::integer(&mut sql, |sql| Engine::week(sql, "post_date", start));
                sql.push(" AS w");
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
                " FROM {prefix}posts WHERE post_date < ? AND post_type = 'post' \
                 AND post_status = 'publish' ORDER BY post_date DESC LIMIT 1",
                &first,
            )
            .await;
        let next = self
            .month_with_posts(
                " FROM {prefix}posts WHERE post_date > ? AND post_type = 'post' \
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

    /// The `(year, month)` of the post `from` finds by `date`, if any.
    async fn month_with_posts(
        &self,
        from: &'static str,
        date: &str,
    ) -> Result<Option<(u64, u64)>, DbError> {
        let mut sql = Sql::new("SELECT ");
        Engine::integer(&mut sql, |sql| {
            Engine::date_part(sql, DatePart::Year, "post_date");
        });
        sql.push(" AS y, ");
        Engine::integer(&mut sql, |sql| {
            Engine::date_part(sql, DatePart::Month, "post_date");
        });
        sql.push(" AS m");
        sql.push(from);
        sql.bind(Bind::Text(date.into()));
        let row = sql
            .query(&self.prefix)
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
        let mut sql = Sql::new("SELECT DISTINCT ");
        Engine::integer(&mut sql, |sql| {
            Engine::date_part(sql, DatePart::Day, "post_date");
        });
        sql.push(
            " AS d FROM {prefix}posts \
             WHERE post_date >= ? AND post_type = 'post' AND post_status = 'publish' \
             AND post_date <= ?",
        );
        sql.bind(Bind::Text(from.into()));
        sql.bind(Bind::Text(to.into()));
        let rows = sql
            .query(&self.prefix)
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
        let mut sql = Sql::new("SELECT ");
        Engine::date_text(
            &mut sql,
            "CASE WHEN MAX(post_modified_gmt) > MAX(post_date_gmt) \
             THEN MAX(post_modified_gmt) ELSE MAX(post_date_gmt) END",
        );
        sql.push(
            " AS latest FROM {prefix}posts WHERE post_status = 'publish' \
             AND post_type IN ('post', 'page', 'attachment')",
        );
        self.latest(sql).await
    }

    /// The `latest` time `sql` selects, `None` for none.
    async fn latest(&self, sql: Sql) -> Result<Option<String>, DbError> {
        let row = sql.query(&self.prefix).fetch_one(&self.pool).await;
        row.and_then(|row| row.try_get("latest")).map_err(db_error)
    }
}

/// The statement [`Db::adjacent_post`] runs: `get_adjacent_post()`'s, for
/// a visitor, selecting the whole row.
fn adjacent_sql(post: &Post, previous: bool, terms: Option<(&str, &[u64])>) -> Sql {
    let mut sql = select_posts(" FROM {prefix}posts AS p");
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
    let mut sql = select_posts(" FROM {prefix}posts WHERE ");
    post_criteria(&mut sql, query);
    sql.push(" ORDER BY ");
    let ranked = ranked(query);
    // `parse_orderby()` leaves out the order of an empty list.
    let kept = |column: OrderBy| match column {
        OrderBy::Relevance => ranked,
        OrderBy::Included => query.include.as_ref().is_some_and(|ids| !ids.is_empty()),
        OrderBy::Parents => !query.parents.is_empty(),
        OrderBy::Names => !query.names.is_empty(),
        _ => true,
    };
    let order: Vec<(OrderBy, bool)> = query
        .order
        .iter()
        .copied()
        .filter(|(column, _)| kept(*column))
        .collect();
    if order.is_empty() {
        // With no key left, the date in the order the first was asked.
        let ascending = query.order.first().is_some_and(|(_, ascending)| *ascending);
        sql.push(if ascending {
            "post_date ASC"
        } else {
            "post_date DESC"
        });
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
            OrderBy::Seeded(seed) => Engine::seeded_random(&mut sql, seed),
            OrderBy::Included => Engine::list_order(
                &mut sql,
                "ID",
                people::ints(query.include.as_deref().unwrap_or_default()),
            ),
            OrderBy::Parents => {
                Engine::list_order(&mut sql, "post_parent", people::ints(&query.parents));
            }
            OrderBy::Names => Engine::list_order(
                &mut sql,
                "post_name",
                query.names.iter().map(|name| Bind::Text(name.clone())),
            ),
            column => match order_column(column) {
                Some(column) => sql.push(column),
                None => Engine::random(&mut sql),
            },
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
    for (i, pattern) in query.mime_types.iter().enumerate() {
        sql.push(if i == 0 { " AND (" } else { " OR " });
        sql.push(if pattern.contains('%') {
            "post_mime_type LIKE ?"
        } else {
            "post_mime_type = ?"
        });
        sql.bind(Bind::Text(pattern.clone()));
    }
    if !query.mime_types.is_empty() {
        sql.push(")");
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
    if !query.excluded_parents.is_empty() {
        sql.push(" AND post_parent NOT IN (");
        list(sql, query.excluded_parents.iter().map(|&id| Bind::Int(id)));
    }
    if !query.names.is_empty() {
        sql.push(" AND post_name IN (");
        list(sql, query.names.iter().map(|name| Bind::Text(name.clone())));
    }
    if let Some(order) = query.menu_order {
        sql.push(" AND menu_order = ?");
        sql.bind(Bind::Signed(order));
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
    let columns: Vec<&str> = if query.search_columns.is_empty() {
        vec!["post_title", "post_excerpt", "post_content"]
    } else {
        query.search_columns.iter().map(String::as_str).collect()
    };
    // A term after `-` excludes the posts with it in any column.
    for term in &query.search {
        let (term, like_op, joiner, filename) = match term.strip_prefix('-') {
            Some(rest) => (
                rest,
                " NOT LIKE ?",
                " AND ",
                " AND EXISTS (SELECT 1 FROM {prefix}postmeta sq1 WHERE sq1.post_id = {prefix}posts.ID \
                 AND sq1.meta_key = '_wp_attached_file' AND sq1.meta_value NOT LIKE ?)",
            ),
            None => (
                term.as_str(),
                " LIKE ?",
                " OR ",
                " OR EXISTS (SELECT 1 FROM {prefix}postmeta sq1 WHERE sq1.post_id = {prefix}posts.ID \
                 AND sq1.meta_key = '_wp_attached_file' AND sq1.meta_value LIKE ?)",
            ),
        };
        sql.push(" AND (");
        for (i, column) in columns.iter().enumerate() {
            if i > 0 {
                sql.push(joiner);
            }
            // Columns are ported names only, never the request's text.
            sql.push(column_name(column));
            sql.push(like_op);
        }
        // `WP_Query` joins the meta as `sq1` and matches its value alike.
        if query.filenames {
            sql.push(filename);
        }
        sql.push(")");
        let pattern = if query.exact {
            esc_like(term)
        } else {
            like(term)
        };
        for _ in 0..columns.len() + usize::from(query.filenames) {
            sql.bind(Bind::Text(pattern.clone()));
        }
    }
    // A visitor's search leaves out protected posts.
    if !query.search.is_empty() && !query.logged_in {
        sql.push(" AND post_password = ''");
    }
    date_parts(
        sql,
        [
            (DatePart::Year, query.year),
            (DatePart::Month, query.month),
            (DatePart::Day, query.day),
        ],
    );
    for condition in &query.time {
        sql.push(" AND ");
        let value = match condition {
            TimeCondition::Hour(n) => {
                Engine::date_part(sql, DatePart::Hour, "post_date");
                *n
            }
            TimeCondition::Minute(n) => {
                Engine::date_part(sql, DatePart::Minute, "post_date");
                *n
            }
            TimeCondition::Second(n) => {
                Engine::date_part(sql, DatePart::Second, "post_date");
                *n
            }
            TimeCondition::Formatted { format, time } => {
                Engine::formatted_time(sql, "post_date", format.clone(), time.clone());
                continue;
            }
            TimeCondition::Week {
                week,
                start_of_week,
            } => {
                Engine::week(sql, "post_date", *start_of_week);
                *week
            }
        };
        sql.push(" = ?");
        sql.bind(Bind::Int(value));
    }
}

/// ` AND` each date part given equals its value.
fn date_parts(sql: &mut Sql, parts: [(DatePart, Option<u64>); 3]) {
    for (part, value) in parts {
        if let Some(value) = value {
            sql.push(" AND ");
            Engine::date_part(sql, part, "post_date");
            sql.push(" = ?");
            sql.bind(Bind::Int(value));
        }
    }
}

/// A searchable column's name: the content's unless the title or excerpt.
fn column_name(column: &str) -> &'static str {
    match column {
        "post_title" => "post_title",
        "post_excerpt" => "post_excerpt",
        _ => "post_content",
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

/// The column a term order sorts by; none for the order of `include`.
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
        TermOrder::Include | TermOrder::Slugs => return None,
    })
}

/// The column of an order without binds; none for a random order.
fn order_column(column: OrderBy) -> Option<&'static str> {
    Some(match column {
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
        _ => return None,
    })
}

/// `WP_Term_Query::get_search_sql()`: a name or slug containing `search`.
fn search_terms(sql: &mut Sql, search: &str) {
    sql.push(" AND ((t.name LIKE ?) OR (t.slug LIKE ?))");
    sql.bind(Bind::Text(like(search)));
    sql.bind(Bind::Text(like(search)));
}

/// The `FROM` and `WHERE` of a `WP_Term_Query` after `select`.
/// The taxonomies of `query`, closing the parenthesis.
fn taxonomies(sql: &mut Sql, query: &TermQuery) {
    let names = std::iter::once(&query.taxonomy).chain(&query.taxonomies);
    list(sql, names.map(|name| Bind::Text(name.clone())));
}

fn term_where(mut sql: Sql, query: &TermQuery) -> Sql {
    sql.push(
        " FROM {prefix}terms AS t \
         INNER JOIN {prefix}term_taxonomy AS tt ON t.term_id = tt.term_id",
    );
    if !query.objects.is_empty() {
        sql.push(
            " INNER JOIN {prefix}term_relationships AS tr \
             ON tr.term_taxonomy_id = tt.term_taxonomy_id",
        );
    }
    sql.push(" WHERE tt.taxonomy IN (");
    taxonomies(&mut sql, query);
    if !query.include.is_empty() {
        sql.push(" AND t.term_id IN (");
        list(&mut sql, query.include.iter().copied().map(Bind::Int));
    }
    if !query.exclude.is_empty() {
        sql.push(" AND t.term_id NOT IN (");
        list(&mut sql, query.exclude.iter().copied().map(Bind::Int));
    }
    if !query.slugs.is_empty() {
        sql.push(" AND t.slug IN (");
        let slugs = query.slugs.iter();
        list(
            &mut sql,
            slugs.map(|slug| Bind::Text(String::from_utf8_lossy(slug).into_owned())),
        );
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
    if !query.objects.is_empty() {
        sql.push(" AND tr.object_id IN (");
        list(&mut sql, query.objects.iter().copied().map(Bind::Int));
    }
    sql
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
    Maybe(Option<u64>),
    Signed(i64),
    Bytes(Vec<u8>),
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
    /// The text with a marker for each [`Dialect`] fragment, which the
    /// portability test reads.
    #[cfg(test)]
    portable: String,
}

impl Sql {
    fn new(fragment: &'static str) -> Sql {
        Sql {
            text: fragment.to_string(),
            binds: Vec::new(),
            #[cfg(test)]
            portable: fragment.to_string(),
        }
    }

    /// Appends a portable fragment.
    fn push(&mut self, fragment: &'static str) {
        self.text.push_str(fragment);
        #[cfg(test)]
        self.portable.push_str(fragment);
    }

    fn bind(&mut self, value: Bind) {
        self.binds.push(value);
    }

    fn query(
        self,
        prefix: &Prefix,
    ) -> sqlx::query::Query<'static, Backend, <Backend as sqlx::Database>::Arguments> {
        #[cfg(test)]
        tests::emitted(&self.portable);
        let mut query = sqlx::query(prefix.sql_from_fragments(self.text));
        for value in self.binds {
            query = match value {
                Bind::Text(text) => query.bind(text),
                Bind::Int(n) => query.bind(n),
                Bind::Maybe(n) => query.bind(n),
                Bind::Signed(n) => query.bind(n),
                Bind::Bytes(bytes) => query.bind(bytes),
            };
        }
        query
    }
}

#[cfg(test)]
mod tests;

mod dialect;
mod people;
mod write;

pub use write::{CommentFields, DB_VERSIONS, Meta, PostFields, TermRow, UserFields, Write, Writer};

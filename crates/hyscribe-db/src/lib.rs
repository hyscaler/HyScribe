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
use hyscribe_model::{Comment, OrderBy, Post, PostQuery, Term, TimeCondition, User};
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
         CAST(post_modified_gmt AS CHAR) AS post_modified_gmt, post_parent, guid, CAST(menu_order AS SIGNED) AS menu_order, post_type, comment_count, \
         comment_status, ping_status"
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
    };
    c.finish(term)
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
        let sql = self.prefix.sql(
            "SELECT meta_value FROM {prefix}postmeta WHERE post_id = ? AND meta_key = ? ORDER BY meta_id LIMIT 1",
        );
        let row = sqlx::query(sql)
            .bind(post_id)
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

    /// `get_the_terms( $post_id, $taxonomy )`: ordered by name, as
    /// `wp_get_object_terms()` orders them.
    pub async fn post_terms(&self, post_id: u64, taxonomy: &str) -> Result<Vec<Term>, DbError> {
        let sql = self.prefix.sql(
            "SELECT t.term_id, t.name, t.slug, tt.description FROM {prefix}terms t \
             JOIN {prefix}term_taxonomy tt ON tt.term_id = t.term_id \
             JOIN {prefix}term_relationships tr ON tr.term_taxonomy_id = tt.term_taxonomy_id \
             WHERE tr.object_id = ? AND tt.taxonomy = ? ORDER BY t.name ASC",
        );
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
        let sql = self.prefix.sql(
            "SELECT ID, display_name, user_nicename, user_login, user_email, user_url, \
             COALESCE((SELECT meta_value FROM {prefix}usermeta WHERE user_id = u.ID \
             AND meta_key = 'first_name' ORDER BY umeta_id LIMIT 1), '') AS first_name, \
             COALESCE((SELECT meta_value FROM {prefix}usermeta WHERE user_id = u.ID \
             AND meta_key = 'last_name' ORDER BY umeta_id LIMIT 1), '') AS last_name \
             FROM {prefix}users u WHERE ID = ?",
        );
        let row = sqlx::query(sql)
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;
        row.as_ref().map(user).transpose()
    }

    /// The terms of `slugs`, `(taxonomy, slug)` pairs, and the categories
    /// with `category_ids`, each with its taxonomy.
    pub async fn find_terms(
        &self,
        slugs: &[(String, Vec<u8>)],
        category_ids: &[u64],
    ) -> Result<Vec<(String, Term)>, DbError> {
        let mut sql = Sql::new(
            "SELECT tt.taxonomy, t.term_id, t.name, t.slug, tt.description FROM {prefix}terms t \
             JOIN {prefix}term_taxonomy tt ON tt.term_id = t.term_id \
             WHERE (tt.taxonomy = 'category' AND t.term_id IN (",
        );
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
        let sql = self.prefix.sql(
            "SELECT t.term_id, t.name, t.slug, tt.description FROM {prefix}terms t \
             JOIN {prefix}term_taxonomy tt ON tt.term_id = t.term_id \
             WHERE tt.taxonomy = ? AND tt.count > 0 ORDER BY t.term_id LIMIT ? OFFSET ?",
        );
        let rows = sqlx::query(sql)
            .bind(taxonomy)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter().map(term).collect()
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
    if query.order.is_empty() {
        sql.push("post_date DESC");
    }
    for (i, &(column, ascending)) in query.order.iter().enumerate() {
        if i > 0 {
            sql.push(", ");
        }
        sql.push(order_column(column));
        if column == OrderBy::Relevance {
            sql.bind(Bind::Text(like(&query.search.join(" "))));
        }
        sql.push(if ascending { " ASC" } else { " DESC" });
    }
    sql.push(" LIMIT ? OFFSET ?");
    sql.bind(Bind::Int(query.limit.into()));
    sql.bind(Bind::Int(query.offset.into()));
    sql
}

/// The `WHERE` criteria of [`posts_sql`]: published posts of the query's
/// types (any, if none), authors, parents, terms, formats, inclusions,
/// search and date.
fn post_criteria(sql: &mut Sql, query: &PostQuery) {
    sql.push("post_status = 'publish'");
    if !query.post_types.is_empty() {
        sql.push(" AND post_type IN (");
        list(sql, query.post_types.iter().map(|t| Bind::Text(t.clone())));
    }
    if !query.authors.is_empty() {
        sql.push(" AND post_author IN (");
        list(sql, query.authors.iter().map(|&id| Bind::Int(id)));
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
    for term in &query.search {
        sql.push(" AND (post_title LIKE ? OR post_excerpt LIKE ? OR post_content LIKE ?)");
        let pattern = if query.exact {
            esc_like(term)
        } else {
            like(term)
        };
        for _ in 0..3 {
            sql.bind(Bind::Text(pattern.clone()));
        }
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

/// The `ORDER BY` term of `column`; relevance binds the search phrase.
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
        OrderBy::Relevance => "post_title LIKE ?",
    }
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
            };
        }
        query
    }
}

#[cfg(test)]
mod tests;

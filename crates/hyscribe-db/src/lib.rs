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
use hyscribe_model::Post;
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

/// The `{prefix}posts` columns [`Post`] reads, as a literal for `concat!`.
macro_rules! post_columns {
    () => {
        "ID, post_author, CAST(post_date AS CHAR) AS post_date, \
         CAST(post_date_gmt AS CHAR) AS post_date_gmt, post_content, post_title, post_excerpt, \
         post_status, post_password, post_name, CAST(post_modified AS CHAR) AS post_modified, \
         post_parent, guid, CAST(menu_order AS SIGNED) AS menu_order, post_type, comment_count"
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
        parent: c.unsigned("post_parent"),
        guid: c.bytes("guid"),
        menu_order: c.signed("menu_order"),
        post_type: c.text("post_type"),
        comment_count: c.signed("comment_count"),
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

    /// Published posts matching `query`, as the main query of a list page
    /// (home, search, category or date archive) selects them.
    pub async fn posts(&self, query: &PostQuery) -> Result<Vec<Post>, DbError> {
        let mut sql = Sql::new(concat!(
            "SELECT ",
            post_columns!(),
            " FROM {prefix}posts WHERE post_status = 'publish' AND post_type IN ("
        ));
        for (i, post_type) in query.post_types.iter().enumerate() {
            sql.push(if i == 0 { "?" } else { ", ?" });
            sql.bind(Bind::Text(post_type.to_string()));
        }
        sql.push(")");
        for term in &query.search {
            sql.push(" AND (post_title LIKE ? OR post_excerpt LIKE ? OR post_content LIKE ?)");
            for _ in 0..3 {
                sql.bind(Bind::Text(like(term)));
            }
        }
        if let Some(category) = query.category {
            sql.push(
                " AND ID IN (SELECT tr.object_id FROM {prefix}term_relationships tr \
                 JOIN {prefix}term_taxonomy tt ON tt.term_taxonomy_id = tr.term_taxonomy_id \
                 WHERE tt.taxonomy = 'category' AND tt.term_id = ?)",
            );
            sql.bind(Bind::Int(category));
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
        // WordPress's own ordering, ties included: title matches of the
        // whole search phrase first, then newest first.
        if query.search.is_empty() {
            sql.push(" ORDER BY post_date DESC");
        } else {
            sql.push(" ORDER BY post_title LIKE ? DESC, post_date DESC");
            sql.bind(Bind::Text(like(&query.search.join(" "))));
        }
        sql.push(" LIMIT ? OFFSET ?");
        sql.bind(Bind::Int(query.limit.into()));
        sql.bind(Bind::Int(query.offset.into()));
        let rows = sql
            .query(&self.prefix)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter().map(post).collect()
    }

    /// A category's name, if the category exists.
    pub async fn category_name(&self, term_id: u64) -> Result<Option<Vec<u8>>, DbError> {
        let sql = self.prefix.sql(
            "SELECT t.name FROM {prefix}terms t JOIN {prefix}term_taxonomy tt ON tt.term_id = t.term_id \
             WHERE tt.taxonomy = 'category' AND t.term_id = ? LIMIT 1",
        );
        let row = sqlx::query(sql)
            .bind(term_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;
        row.map(|row| {
            let mut c = Columns::new(&row);
            let name = c.bytes("name");
            c.finish(name)
        })
        .transpose()
    }
}

/// What a list of posts is filtered by.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PostQuery {
    /// Post types to include, such as `post`, or `post` and `page` for a
    /// search.
    pub post_types: Vec<&'static str>,
    /// Search terms; every term must appear in the title, excerpt or content.
    pub search: Vec<String>,
    /// A category's `term_id`.
    pub category: Option<u64>,
    /// `YEAR(post_date)`.
    pub year: Option<u64>,
    /// `MONTH(post_date)`.
    pub month: Option<u64>,
    /// `DAYOFMONTH(post_date)`.
    pub day: Option<u64>,
    /// How many posts.
    pub limit: u32,
    /// How many to skip.
    pub offset: u32,
}

/// A `LIKE` pattern matching `text` anywhere, with `%`, `_` and `\\`
/// escaped as `$wpdb->esc_like()` does.
fn like(text: &str) -> String {
    let mut pattern = String::from("%");
    for c in text.chars() {
        if matches!(c, '%' | '_' | '\\') {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    pattern.push('%');
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

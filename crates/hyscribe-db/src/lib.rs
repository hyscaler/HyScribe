// SPDX-License-Identifier: AGPL-3.0-or-later

//! Read access to a WordPress database (M1.3, ADR 0003).
//!
//! - **Read only.** Every connection is put in a read only transaction mode,
//!   and this crate has no write methods: shadow mode cannot write.
//! - **No string-built SQL.** Queries are fixed text; only the table prefix
//!   is inserted, and [`Prefix`] admits nothing but `[A-Za-z0-9_]`. Values
//!   are always bound parameters.
//! - **Bytes, not text.** Text columns are read as bytes, so content is
//!   never re-encoded whatever the site's charset.

use std::fmt;

use hyscribe_config::Database;
use sqlx::mysql::{MySqlConnectOptions, MySqlPool, MySqlPoolOptions, MySqlRow};
use sqlx::{Executor, Row};

/// A validated `$table_prefix`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prefix(String);

impl Prefix {
    /// The same rule `wp-settings.php` applies to `$table_prefix`.
    pub fn new(prefix: &str) -> Option<Prefix> {
        prefix
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            .then(|| Prefix(prefix.to_string()))
    }

    /// `query` with every `{prefix}` replaced by the prefix: the only SQL
    /// this crate builds. `query` is a literal and the prefix cannot hold
    /// SQL syntax, so the result is safe.
    fn sql(&self, query: &'static str) -> sqlx::AssertSqlSafe<String> {
        sqlx::AssertSqlSafe(query.replace("{prefix}", &self.0))
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

/// A row of `{prefix}posts`, with the columns rendering needs.
#[derive(Clone, Debug, PartialEq, Eq)]
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
    /// `post_name`.
    pub name: Vec<u8>,
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
}

/// The `{prefix}posts` columns [`Post`] reads, as a literal for `concat!`.
macro_rules! post_columns {
    () => {
        "ID, post_author, CAST(post_date AS CHAR) AS post_date, \
         CAST(post_date_gmt AS CHAR) AS post_date_gmt, post_content, post_title, post_excerpt, \
         post_status, post_name, post_parent, guid, CAST(menu_order AS SIGNED) AS menu_order, \
         post_type, comment_count"
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
        name: c.bytes("post_name"),
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
        let prefix = Prefix::new(&config.prefix).ok_or_else(|| {
            DbError("the table prefix may only hold letters, digits and underscores".into())
        })?;
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

    /// The table prefix.
    pub fn prefix(&self) -> &Prefix {
        &self.prefix
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

    /// One option's raw value.
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

    /// Published posts of `post_type`, newest first, as the main query of a
    /// plain blog home page selects them.
    pub async fn published(
        &self,
        post_type: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<Post>, DbError> {
        let sql = self.prefix.sql(concat!(
            "SELECT ",
            post_columns!(),
            " FROM {prefix}posts WHERE post_type = ? AND post_status = 'publish' \
             ORDER BY post_date DESC, ID DESC LIMIT ? OFFSET ?"
        ));
        let rows = sqlx::query(sql)
            .bind(post_type)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter().map(post).collect()
    }
}

#[cfg(test)]
mod tests;

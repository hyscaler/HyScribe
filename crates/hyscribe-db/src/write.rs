// SPDX-License-Identifier: AGPL-3.0-or-later

//! The gated writer (ADRs 0010 and 0012): a second pool, as the main
//! database user, that only primary mode opens. Each action is one
//! [`Write`], a transaction committed as a whole, or a savepoint in the
//! caller's transaction; each method is one fixed statement with bound
//! values, named after what it does. Writes are refused when `db_version`
//! is newer than supported (ADR 0003) and on tables whose engine cannot
//! roll back.

use hyscribe_config::{Database, Mode, Prefix};
use sqlx::Row as _;

use crate::dialect::{Access, Dialect};
use crate::{Backend, Bind, Columns, DbError, Engine, PrefixSql, Sql, db_error, ids_or_none, list};

mod cron;
mod meta;
mod posts;
mod terms;

pub use meta::Meta;
pub use terms::TermRow;

/// The `db_version`s this release supports and writes to: WordPress 7.0
/// and 7.1.
pub const DB_VERSIONS: std::ops::RangeInclusive<u64> = 61833..=61833;

type Query = sqlx::query::Query<'static, Backend, <Backend as sqlx::Database>::Arguments>;

/// A table the writer writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Table {
    Posts,
    Postmeta,
    Comments,
    Commentmeta,
    Terms,
    TermTaxonomy,
    TermRelationships,
    Termmeta,
    Users,
    Usermeta,
    Options,
    Links,
}

impl Table {
    const ALL: [Table; 12] = [
        Table::Posts,
        Table::Postmeta,
        Table::Comments,
        Table::Commentmeta,
        Table::Terms,
        Table::TermTaxonomy,
        Table::TermRelationships,
        Table::Termmeta,
        Table::Users,
        Table::Usermeta,
        Table::Options,
        Table::Links,
    ];

    fn name(self) -> &'static str {
        match self {
            Table::Posts => "posts",
            Table::Postmeta => "postmeta",
            Table::Comments => "comments",
            Table::Commentmeta => "commentmeta",
            Table::Terms => "terms",
            Table::TermTaxonomy => "term_taxonomy",
            Table::TermRelationships => "term_relationships",
            Table::Termmeta => "termmeta",
            Table::Users => "users",
            Table::Usermeta => "usermeta",
            Table::Options => "options",
            Table::Links => "links",
        }
    }
}

/// The writable pool. Only [`Writer::open`] makes one, and only in primary
/// mode, so shadow mode has no writer at all.
#[derive(Debug)]
pub struct Writer {
    pool: sqlx::Pool<Backend>,
    prefix: Prefix,
    /// The tables written whose engine cannot roll back, with the engine.
    refused: Vec<(Table, String)>,
    /// The `home` and `siteurl` the configuration sets (`WP_HOME`,
    /// `WP_SITEURL`), which `get_option()` gives instead of the stored ones.
    urls: Vec<(&'static str, String)>,
}

impl Writer {
    /// The writer for `mode`: none in shadow mode. In primary mode it
    /// connects as the main user and notes the tables whose engine cannot
    /// roll back, so writes to them are refused.
    pub async fn open(config: &Database, mode: Mode) -> Result<Option<Writer>, DbError> {
        if mode == Mode::Shadow {
            return Ok(None);
        }
        let prefix = config.prefix.clone();
        let mut sql = Sql::new("");
        Engine::rollback_less(
            &mut sql,
            Table::ALL
                .iter()
                .map(|table| Bind::Text(format!("{}{}", prefix.as_str(), table.name()))),
        );
        let (pool, rows) = match Engine::connect(config, Access::Write).await {
            Ok(pool) => {
                let rows = sql.query(&prefix).fetch_all(&pool).await;
                rows.map(|rows| (pool, rows))
            }
            Err(err) => Err(err),
        }
        .map_err(db_error)?;
        let refused = rows
            .iter()
            .filter_map(|row| {
                let name = text(row, "name");
                Table::ALL
                    .into_iter()
                    .find(|table| name == format!("{}{}", prefix.as_str(), table.name()))
                    .map(|table| (table, text(row, "engine")))
            })
            .collect();
        Ok(Some(Writer {
            pool,
            prefix,
            refused,
            urls: Vec::new(),
        }))
    }

    /// The writer with the configuration's `home` and `siteurl`, by option name.
    pub fn with_urls(self, urls: Vec<(&'static str, String)>) -> Writer {
        Writer { urls, ..self }
    }

    /// What the writer refuses, as the service logs it at start: each table
    /// whose engine cannot roll back, and a `db_version` it does not write to.
    pub async fn refusals(&self) -> Vec<String> {
        let tables = self.refused.iter();
        let mut refusals: Vec<String> = tables.map(|(t, e)| self.refusal(*t, e)).collect();
        refusals.extend(self.begin().await.err().map(|err| err.0));
        refusals
    }

    /// Why a write to `table`, whose engine is `engine`, is refused.
    fn refusal(&self, table: Table, engine: &str) -> String {
        let name = format!("{}{}", self.prefix.as_str(), table.name());
        format!(
            "{name} uses the {engine} engine, which cannot roll back, so HyScribe does not \
             write to it; convert it with ALTER TABLE {name} ENGINE=InnoDB"
        )
    }

    /// Starts an action. Refused when the database's `db_version` is not
    /// one this release supports, newer in particular.
    pub async fn begin(&self) -> Result<Write<'_>, DbError> {
        Write::start(self, self.pool.begin().await).await
    }

    /// Starts an action on `conn`, as a savepoint when `conn` is in a
    /// transaction already, so the caller's rollback undoes it too.
    pub async fn begin_on<'w>(
        &'w self,
        conn: &'w mut crate::Connection,
    ) -> Result<Write<'w>, DbError> {
        Write::start(self, sqlx::Connection::begin(conn).await).await
    }
}

/// A text column of an information schema row, whatever its type.
fn text(row: &crate::Row, name: &str) -> String {
    String::from_utf8_lossy(&bytes(row, name)).into_owned()
}

/// A text column's bytes.
fn bytes(row: &crate::Row, name: &str) -> Vec<u8> {
    row.try_get_unchecked::<Vec<u8>, _>(name)
        .unwrap_or_default()
}

/// One action's writes: committed together by [`Write::commit`], rolled
/// back when dropped. Once a statement fails, none runs after it and the
/// commit fails: a deadlock has rolled the whole transaction back, and
/// what ran next would commit on its own.
pub struct Write<'w> {
    writer: &'w Writer,
    tx: sqlx::Transaction<'w, Backend>,
    /// The statements left before each next one fails, if limited.
    budget: Option<usize>,
    /// The first statement's error, which every later one returns.
    failed: Option<DbError>,
}

impl<'w> Write<'w> {
    async fn start(
        writer: &'w Writer,
        tx: Result<sqlx::Transaction<'w, Backend>, sqlx::Error>,
    ) -> Result<Write<'w>, DbError> {
        let mut tx = tx.map_err(db_error)?;
        let sql = writer
            .prefix
            .sql("SELECT option_value FROM {prefix}options WHERE option_name = 'db_version'");
        let row = sqlx::query(sql)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_error)?;
        let version = row.map(|row| text(&row, "option_value"));
        match version.as_deref().map(str::parse::<u64>) {
            Some(Ok(number)) if DB_VERSIONS.contains(&number) => Ok(Write {
                writer,
                tx,
                budget: None,
                failed: None,
            }),
            _ => Err(DbError(format!(
                "the database's db_version ({}) is not one this release writes to ({} to {}), \
                 so HyScribe writes nothing to it",
                version.as_deref().unwrap_or("missing"),
                DB_VERSIONS.start(),
                DB_VERSIONS.end()
            ))),
        }
    }

    /// Lets `statements` more statements run and fails each one after, so
    /// tests reach every error path of an action.
    pub fn fail_after(&mut self, statements: usize) {
        self.budget = Some(statements);
    }

    /// Counts a statement against the budget [`Write::fail_after`] set.
    fn spend(&mut self) -> Result<(), DbError> {
        match &mut self.budget {
            Some(0) => Err(DbError("the test's statement budget is spent".into())),
            Some(left) => {
                *left -= 1;
                Ok(())
            }
            None => Ok(()),
        }
    }

    /// Commits every write of the action, unless a statement failed.
    pub async fn commit(self) -> Result<(), DbError> {
        match self.failed {
            Some(err) => Err(err),
            None => self.tx.commit().await.map_err(db_error),
        }
    }

    /// The table prefix, which names the user meta keys of the site, as
    /// `{prefix}capabilities` and `{prefix}user_level`.
    pub fn prefix(&self) -> &str {
        self.writer.prefix.as_str()
    }

    /// The configuration's value of the option `name` (`home` or `siteurl`),
    /// if it sets one.
    pub fn configured(&self, name: &str) -> Option<&str> {
        let url = self.writer.urls.iter().find(|(option, _)| *option == name);
        url.map(|(_, url)| url.as_str())
    }

    fn sql(&self, query: &'static str) -> Query {
        sqlx::query(self.writer.prefix.sql(query))
    }

    /// Runs one statement of the action, unless one failed before: the
    /// first error is kept and returned for every later statement.
    async fn statement<T>(
        &mut self,
        run: impl AsyncFnOnce(&mut crate::Connection) -> Result<T, DbError>,
    ) -> Result<T, DbError> {
        if let Some(err) = &self.failed {
            return Err(err.clone());
        }
        let result = match self.spend() {
            Ok(()) => run(&mut self.tx).await,
            Err(err) => Err(err),
        };
        if let Err(err) = &result {
            self.failed = Some(err.clone());
        }
        result
    }

    async fn run(
        &mut self,
        table: Table,
        query: Query,
    ) -> Result<<Backend as sqlx::Database>::QueryResult, DbError> {
        let writer = self.writer;
        let refused = writer.refused.iter().find(|(t, _)| *t == table);
        self.statement(async |conn| match refused {
            Some((_, engine)) => Err(DbError(writer.refusal(table, engine))),
            None => query.execute(conn).await.map_err(db_error),
        })
        .await
    }

    /// Runs an `UPDATE` or `DELETE` on `table`: the rows it matched.
    async fn change(&mut self, table: Table, query: Query) -> Result<u64, DbError> {
        self.run(table, query)
            .await
            .map(|done| done.rows_affected())
    }

    /// Runs an `INSERT` into `table`: the ID of its row.
    async fn insert(&mut self, table: Table, query: Query) -> Result<u64, DbError> {
        self.run(table, query)
            .await
            .map(|done| Engine::inserted_id(&done))
    }

    /// The rows `query` selects.
    async fn rows(&mut self, query: Query) -> Result<Vec<crate::Row>, DbError> {
        self.statement(async |conn| query.fetch_all(conn).await.map_err(db_error))
            .await
    }

    /// The row `query` selects, if any.
    async fn optional(&mut self, query: Query) -> Result<Option<crate::Row>, DbError> {
        self.statement(async |conn| query.fetch_optional(conn).await.map_err(db_error))
            .await
    }

    /// The IDs `query` selects, as a column.
    async fn ids(&mut self, query: Query) -> Result<Vec<u64>, DbError> {
        self.rows(query)
            .await?
            .iter()
            .map(|row| row.try_get::<u64, _>(0).map_err(db_error))
            .collect()
    }

    /// The `COUNT(*)` `query` selects.
    async fn count(&mut self, query: Query) -> Result<u64, DbError> {
        let row = self
            .statement(async |conn| query.fetch_one(conn).await.map_err(db_error))
            .await?;
        row.try_get::<i64, _>(0)
            .map(i64::unsigned_abs)
            .map_err(db_error)
    }

    // Posts.

    /// `wp_insert_post()`'s `INSERT`: the new post's ID.
    pub async fn insert_post(&mut self, post: &PostFields) -> Result<u64, DbError> {
        let query = post.bind(self.sql(
            "INSERT INTO {prefix}posts (post_author, post_date, post_date_gmt, post_content, \
             post_content_filtered, post_title, post_excerpt, post_status, post_type, \
             comment_status, ping_status, post_password, post_name, to_ping, pinged, \
             post_modified, post_modified_gmt, post_parent, menu_order, post_mime_type, guid) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        ));
        self.insert(Table::Posts, query).await
    }

    /// `wp_insert_post()`'s `UPDATE` of an existing post.
    pub async fn update_post(&mut self, id: u64, post: &PostFields) -> Result<u64, DbError> {
        let query = post.bind(self.sql(
            "UPDATE {prefix}posts SET post_author = ?, post_date = ?, post_date_gmt = ?, \
             post_content = ?, post_content_filtered = ?, post_title = ?, post_excerpt = ?, \
             post_status = ?, post_type = ?, comment_status = ?, ping_status = ?, \
             post_password = ?, post_name = ?, to_ping = ?, pinged = ?, post_modified = ?, \
             post_modified_gmt = ?, post_parent = ?, menu_order = ?, post_mime_type = ?, \
             guid = ? WHERE ID = ?",
        ));
        self.change(Table::Posts, query.bind(id)).await
    }

    /// The slug `wp_insert_post()` gives a post saved without one.
    pub async fn set_post_name(&mut self, id: u64, name: &[u8]) -> Result<u64, DbError> {
        let query = self.sql("UPDATE {prefix}posts SET post_name = ? WHERE ID = ?");
        self.change(Table::Posts, query.bind(name).bind(id)).await
    }

    /// The GUID `wp_insert_post()` gives a post saved without one.
    pub async fn set_post_guid(&mut self, id: u64, guid: &[u8]) -> Result<u64, DbError> {
        let query = self.sql("UPDATE {prefix}posts SET guid = ? WHERE ID = ?");
        self.change(Table::Posts, query.bind(guid).bind(id)).await
    }

    /// `wp_publish_post()`'s status change.
    pub async fn set_post_status(&mut self, id: u64, status: &str) -> Result<u64, DbError> {
        let query = self.sql("UPDATE {prefix}posts SET post_status = ? WHERE ID = ?");
        self.change(Table::Posts, query.bind(status).bind(id)).await
    }

    /// `wp_update_comment_count_now()`'s `UPDATE`.
    pub async fn set_comment_count(&mut self, id: u64, count: u64) -> Result<u64, DbError> {
        let query = self.sql("UPDATE {prefix}posts SET comment_count = ? WHERE ID = ?");
        self.change(Table::Posts, query.bind(count).bind(id)).await
    }

    /// `wp_delete_post()`: the children of type `post_type` of `parent`
    /// move to `new_parent`.
    pub async fn reparent_posts(
        &mut self,
        parent: u64,
        post_type: &str,
        new_parent: u64,
    ) -> Result<u64, DbError> {
        let query = self.sql(
            "UPDATE {prefix}posts SET post_parent = ? WHERE post_parent = ? AND post_type = ?",
        );
        let query = query.bind(new_parent).bind(parent).bind(post_type);
        self.change(Table::Posts, query).await
    }

    /// `wp_media_attach_action()`'s `UPDATE`: `parent` becomes the parent of
    /// the attachments `ids`; the rows it changed, as `mysqli` counts them.
    pub async fn set_attachments_parent(
        &mut self,
        ids: &[u64],
        parent: u64,
    ) -> Result<u64, DbError> {
        let mut changed = Sql::new(
            "SELECT COUNT(*) FROM {prefix}posts WHERE post_type = 'attachment' \
             AND post_parent != ? AND ID IN (",
        );
        changed.bind(Bind::Int(parent));
        list(&mut changed, ids_or_none(ids));
        let changed = changed.query(&self.writer.prefix);
        let changed = self.count(changed).await?;
        let mut sql = Sql::new(
            "UPDATE {prefix}posts SET post_parent = ? \
             WHERE post_type = 'attachment' AND ID IN (",
        );
        sql.bind(Bind::Int(parent));
        list(&mut sql, ids_or_none(ids));
        let query = sql.query(&self.writer.prefix);
        self.change(Table::Posts, query).await?;
        Ok(changed)
    }

    /// `wp_delete_user()`: the posts of `author` pass to `new_author`.
    pub async fn reassign_posts(&mut self, author: u64, new_author: u64) -> Result<u64, DbError> {
        let query = self.sql("UPDATE {prefix}posts SET post_author = ? WHERE post_author = ?");
        self.change(Table::Posts, query.bind(new_author).bind(author))
            .await
    }

    /// `wp_delete_post()`'s `DELETE`.
    pub async fn delete_post(&mut self, id: u64) -> Result<u64, DbError> {
        let query = self.sql("DELETE FROM {prefix}posts WHERE ID = ?");
        self.change(Table::Posts, query.bind(id)).await
    }

    /// The children of type `post_type` of `parent`, as `wp_delete_post()`
    /// finds a post's revisions.
    pub async fn child_ids(&mut self, parent: u64, post_type: &str) -> Result<Vec<u64>, DbError> {
        let query =
            self.sql("SELECT ID FROM {prefix}posts WHERE post_parent = ? AND post_type = ?");
        self.ids(query.bind(parent).bind(post_type)).await
    }

    /// The author of `post`, none when there is no such post.
    pub async fn post_author(&mut self, post: u64) -> Result<Option<u64>, DbError> {
        let query = self.sql("SELECT post_author FROM {prefix}posts WHERE ID = ? LIMIT 1");
        Ok(self.ids(query.bind(post)).await?.first().copied())
    }

    // Comments.

    /// `wp_insert_comment()`'s `INSERT`: the new comment's ID.
    pub async fn insert_comment(&mut self, comment: &CommentFields) -> Result<u64, DbError> {
        let query = comment.bind(self.sql(
            "INSERT INTO {prefix}comments (comment_post_ID, comment_author, \
             comment_author_email, comment_author_url, comment_author_IP, comment_date, \
             comment_date_gmt, comment_content, comment_karma, comment_approved, comment_agent, \
             comment_type, comment_parent, user_id) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        ));
        self.insert(Table::Comments, query).await
    }

    /// `wp_update_comment()`'s `UPDATE`.
    pub async fn update_comment(
        &mut self,
        id: u64,
        comment: &CommentFields,
    ) -> Result<u64, DbError> {
        let query = comment.bind(self.sql(
            "UPDATE {prefix}comments SET comment_post_ID = ?, comment_author = ?, \
             comment_author_email = ?, comment_author_url = ?, comment_author_IP = ?, \
             comment_date = ?, comment_date_gmt = ?, comment_content = ?, comment_karma = ?, \
             comment_approved = ?, comment_agent = ?, comment_type = ?, comment_parent = ?, \
             user_id = ? WHERE comment_ID = ?",
        ));
        self.change(Table::Comments, query.bind(id)).await
    }

    /// `wp_set_comment_status()`'s `UPDATE`.
    pub async fn set_comment_status(&mut self, id: u64, status: &str) -> Result<u64, DbError> {
        let query =
            self.sql("UPDATE {prefix}comments SET comment_approved = ? WHERE comment_ID = ?");
        self.change(Table::Comments, query.bind(status).bind(id))
            .await
    }

    /// `wp_untrash_post_comments()`: the status of the comments `ids`.
    pub async fn set_comments_status(&mut self, ids: &[u64], status: &str) -> Result<u64, DbError> {
        let mut sql =
            Sql::new("UPDATE {prefix}comments SET comment_approved = ? WHERE comment_ID IN (");
        sql.bind(Bind::Text(status.to_string()));
        list(&mut sql, ids_or_none(ids));
        let query = sql.query(&self.writer.prefix);
        self.change(Table::Comments, query).await
    }

    /// `wp_trash_post_comments()`: every comment of `post` trashed with it.
    pub async fn trash_post_comments(&mut self, post: u64) -> Result<u64, DbError> {
        let query = self.sql(
            "UPDATE {prefix}comments SET comment_approved = 'post-trashed' \
             WHERE comment_post_ID = ?",
        );
        self.change(Table::Comments, query.bind(post)).await
    }

    /// `wp_delete_comment()`: the replies to `parent` move to `new_parent`.
    pub async fn reparent_comments(
        &mut self,
        parent: u64,
        new_parent: u64,
    ) -> Result<u64, DbError> {
        let query =
            self.sql("UPDATE {prefix}comments SET comment_parent = ? WHERE comment_parent = ?");
        self.change(Table::Comments, query.bind(new_parent).bind(parent))
            .await
    }

    /// `wp_delete_comment()`'s `DELETE`.
    pub async fn delete_comment(&mut self, id: u64) -> Result<u64, DbError> {
        let query = self.sql("DELETE FROM {prefix}comments WHERE comment_ID = ?");
        self.change(Table::Comments, query.bind(id)).await
    }

    /// The comments of `post`, newest ID first, as `wp_delete_post()`
    /// deletes them.
    pub async fn post_comment_ids(&mut self, post: u64) -> Result<Vec<u64>, DbError> {
        let query = self.sql(
            "SELECT comment_ID FROM {prefix}comments WHERE comment_post_ID = ? \
             ORDER BY comment_ID DESC",
        );
        self.ids(query.bind(post)).await
    }

    /// `get_comment()`: the comment `id`, as the action sees it.
    pub async fn comment(&mut self, id: u64) -> Result<Option<CommentFields>, DbError> {
        let mut sql = Sql::new(
            "SELECT comment_post_ID, comment_author, comment_author_email, comment_author_url, \
             comment_author_IP, comment_content, comment_karma, comment_approved, comment_agent, \
             comment_type, comment_parent, user_id",
        );
        crate::date_column(&mut sql, "comment_date");
        crate::date_column(&mut sql, "comment_date_gmt");
        sql.push(" FROM {prefix}comments WHERE comment_ID = ?");
        sql.bind(Bind::Int(id));
        let row = self.optional(sql.query(&self.writer.prefix)).await?;
        Ok(row.map(|row| CommentFields::read(&row)))
    }

    /// `get_page_of_comment()`'s count: the top level comments of every
    /// type on `post` from before `date_gmt` that are approved, or held and
    /// `unapproved`'s: written by that user, or by the author with that
    /// email.
    pub async fn comments_before(
        &mut self,
        post: u64,
        date_gmt: &str,
        unapproved: Option<Unapproved<'_>>,
    ) -> Result<u64, DbError> {
        let query = match unapproved {
            None => self.sql(
                "SELECT COUNT(*) FROM {prefix}comments WHERE comment_post_ID = ? \
                 AND comment_parent = 0 AND comment_date_gmt < ? AND comment_approved = '1'",
            ),
            Some(Unapproved::User(_)) => self.sql(
                "SELECT COUNT(*) FROM {prefix}comments WHERE comment_post_ID = ? \
                 AND comment_parent = 0 AND comment_date_gmt < ? \
                 AND ( comment_approved = '1' OR ( user_id = ? AND comment_approved = '0' ) )",
            ),
            Some(Unapproved::Email(_)) => self.sql(
                "SELECT COUNT(*) FROM {prefix}comments WHERE comment_post_ID = ? \
                 AND comment_parent = 0 AND comment_date_gmt < ? \
                 AND ( comment_approved = '1' \
                 OR ( comment_author_email = ? AND comment_approved = '0' ) )",
            ),
        };
        let query = query.bind(post).bind(date_gmt);
        self.count(match unapproved {
            None => query,
            Some(Unapproved::User(user)) => query.bind(user),
            Some(Unapproved::Email(email)) => query.bind(email),
        })
        .await
    }

    /// `wp_allow_comment()`'s duplicate: a comment not in the trash with
    /// the same post, parent, author, email (unless `email` is `None`) and
    /// content.
    pub async fn duplicate_comment(
        &mut self,
        (post, parent): (u64, u64),
        author: &[u8],
        email: Option<&[u8]>,
        content: &[u8],
    ) -> Result<Option<u64>, DbError> {
        let query = match email {
            Some(email) => self
                .sql(
                    "SELECT comment_ID FROM {prefix}comments WHERE comment_post_ID = ? \
                     AND comment_parent = ? AND comment_approved != 'trash' \
                     AND ( comment_author = ? AND comment_author_email = ? ) \
                     AND comment_content = ? LIMIT 1",
                )
                .bind(post)
                .bind(parent)
                .bind(author)
                .bind(email),
            None => self
                .sql(
                    "SELECT comment_ID FROM {prefix}comments WHERE comment_post_ID = ? \
                     AND comment_parent = ? AND comment_approved != 'trash' \
                     AND ( comment_author = ? ) AND comment_content = ? LIMIT 1",
                )
                .bind(post)
                .bind(parent)
                .bind(author),
        };
        Ok(self.ids(query.bind(content)).await?.first().copied())
    }

    /// `check_comment()`: whether the user `user` has an approved comment.
    pub async fn user_has_approved_comment(&mut self, user: u64) -> Result<bool, DbError> {
        let query = self.sql(
            "SELECT comment_ID FROM {prefix}comments WHERE user_id = ? \
             AND comment_approved = '1' LIMIT 1",
        );
        Ok(!self.ids(query.bind(user)).await?.is_empty())
    }

    /// `check_comment()`: whether `author` with `email` has an approved
    /// comment.
    pub async fn author_has_approved_comment(
        &mut self,
        author: &[u8],
        email: &[u8],
    ) -> Result<bool, DbError> {
        let query = self.sql(
            "SELECT comment_ID FROM {prefix}comments WHERE comment_author = ? \
             AND comment_author_email = ? AND comment_approved = '1' LIMIT 1",
        );
        Ok(!self.ids(query.bind(author).bind(email)).await?.is_empty())
    }

    /// `wp_check_comment_flood()`: the latest `comment_date_gmt` since
    /// `since` of the comments by the user `user` (by the IP address `ip`
    /// when logged out) or by `email`.
    pub async fn last_comment_date(
        &mut self,
        since: &str,
        (user, ip): (u64, &[u8]),
        email: &[u8],
    ) -> Result<Option<String>, DbError> {
        let mut sql = Sql::new("SELECT ");
        Engine::date_text(&mut sql, "comment_date_gmt");
        sql.push(" AS last");
        sql.bind(Bind::Text(since.to_string()));
        if user == 0 {
            sql.push(
                " FROM {prefix}comments WHERE comment_date_gmt >= ? \
                 AND ( comment_author_IP = ? OR comment_author_email = ? )",
            );
            sql.bind(Bind::Bytes(ip.to_vec()));
        } else {
            sql.push(
                " FROM {prefix}comments WHERE comment_date_gmt >= ? \
                 AND ( user_id = ? OR comment_author_email = ? )",
            );
            sql.bind(Bind::Int(user));
        }
        sql.push(" ORDER BY comment_date_gmt DESC LIMIT 1");
        sql.bind(Bind::Bytes(email.to_vec()));
        let row = self.optional(sql.query(&self.writer.prefix)).await?;
        Ok(row.map(|row| text(&row, "last")))
    }

    /// The approved and held notes replying to `parent`, newest first, as
    /// `wp_trash_comment()` finds a note's replies.
    pub async fn child_note_ids(&mut self, parent: u64) -> Result<Vec<u64>, DbError> {
        let query = self.sql(
            "SELECT comment_ID FROM {prefix}comments WHERE comment_parent = ? \
             AND comment_type = 'note' AND ( comment_approved = '0' OR comment_approved = '1' ) \
             ORDER BY comment_date_gmt DESC",
        );
        self.ids(query.bind(parent)).await
    }

    /// `wp_update_comment_count_now()`'s count: the approved comments of
    /// `post`, notes left out.
    pub async fn approved_comment_count(&mut self, post: u64) -> Result<u64, DbError> {
        let query = self.sql(
            "SELECT COUNT(*) FROM {prefix}comments WHERE comment_post_ID = ? \
             AND comment_approved = '1' AND comment_type != 'note'",
        );
        self.count(query.bind(post)).await
    }

    /// `get_comment_count( $post )`'s approved, held, spam, trashed and
    /// post-trashed comments, notes left out, of every post for 0, counted
    /// a status at a time as PHP counts them.
    pub async fn comment_counts(&mut self, post: u64) -> Result<[u64; 5], DbError> {
        let mut counts = [0; 5];
        let statuses = ["1", "0", "spam", "trash", "post-trashed"];
        for (count, status) in counts.iter_mut().zip(statuses) {
            let query = self.sql(
                "SELECT COUNT(*) FROM {prefix}comments WHERE comment_approved = ? \
                 AND comment_type != 'note' AND ( ? = 0 OR comment_post_ID = ? )",
            );
            *count = self.count(query.bind(status).bind(post).bind(post)).await?;
        }
        Ok(counts)
    }

    /// `edit_comment()`'s replies of `parents`, which are not empty, of any
    /// status, notes left out: one level of the subtree that moves with a
    /// comment.
    pub async fn comment_child_ids(&mut self, parents: &[u64]) -> Result<Vec<u64>, DbError> {
        let mut sql = Sql::new(
            "SELECT comment_ID FROM {prefix}comments WHERE comment_type != 'note' \
             AND comment_parent IN (",
        );
        list(&mut sql, parents.iter().copied().map(Bind::Int));
        let query = sql.query(&self.writer.prefix);
        self.ids(query).await
    }

    /// The comments Empty Spam and Empty Trash delete: of `status`, older
    /// than `before`, both compared as `edit-comments.php`'s text.
    pub async fn comment_ids_before(
        &mut self,
        status: &str,
        before: &str,
    ) -> Result<Vec<u64>, DbError> {
        let query = self.sql(
            "SELECT comment_ID FROM {prefix}comments \
             WHERE comment_approved = ? AND ? > comment_date_gmt",
        );
        self.ids(query.bind(status).bind(before)).await
    }

    // Users.

    /// `wp_insert_user()`'s `INSERT` of a new user `login`: its ID.
    pub async fn insert_user(&mut self, login: &[u8], user: &UserFields) -> Result<u64, DbError> {
        let query = user.bind(self.sql(
            "INSERT INTO {prefix}users (user_pass, user_nicename, user_email, user_url, \
             user_registered, user_activation_key, display_name, user_login) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        ));
        self.insert(Table::Users, query.bind(login)).await
    }

    /// `wp_insert_user()`'s `UPDATE` of an existing user.
    pub async fn update_user(&mut self, id: u64, user: &UserFields) -> Result<u64, DbError> {
        let query = user.bind(self.sql(
            "UPDATE {prefix}users SET user_pass = ?, user_nicename = ?, user_email = ?, \
             user_url = ?, user_registered = ?, user_activation_key = ?, display_name = ? \
             WHERE ID = ?",
        ));
        self.change(Table::Users, query.bind(id)).await
    }

    /// `get_userdata()`: the user's login and the columns `wp_insert_user()`
    /// writes.
    pub async fn user(&mut self, id: u64) -> Result<Option<(Vec<u8>, UserFields)>, DbError> {
        let mut sql =
            Sql::new("SELECT user_login, user_pass, user_nicename, user_email, user_url, ");
        Engine::date_text(&mut sql, "user_registered");
        sql.push(
            " AS registered, user_activation_key, display_name FROM {prefix}users WHERE ID = ?",
        );
        sql.bind(Bind::Int(id));
        let row = self.optional(sql.query(&self.writer.prefix)).await?;
        row.map_or(Ok(None), |row| {
            let mut c = Columns::new(&row);
            let login = c.bytes("user_login");
            let fields = UserFields {
                pass: c.text("user_pass"),
                nicename: c.bytes("user_nicename"),
                email: c.bytes("user_email"),
                url: c.bytes("user_url"),
                registered: c.text("registered"),
                activation_key: c.text("user_activation_key"),
                display_name: c.bytes("display_name"),
            };
            c.finish(Some((login, fields)))
        })
    }

    /// `get_user_by( 'login' )`'s lookup.
    pub async fn user_id_by_login(&mut self, login: &[u8]) -> Result<Option<u64>, DbError> {
        let query = self.sql("SELECT ID FROM {prefix}users WHERE user_login = ? LIMIT 1");
        Ok(self.ids(query.bind(login)).await?.into_iter().next())
    }

    /// `wp_insert_user()`'s check: a user other than `login` holding
    /// `nicename`.
    pub async fn nicename_owner(
        &mut self,
        nicename: &[u8],
        login: &[u8],
    ) -> Result<Option<u64>, DbError> {
        let query = self.sql(
            "SELECT ID FROM {prefix}users WHERE user_nicename = ? AND user_login != ? LIMIT 1",
        );
        let ids = self.ids(query.bind(nicename).bind(login)).await?;
        Ok(ids.into_iter().next())
    }

    /// `wp_update_user_counts()`'s count of users.
    pub async fn user_count(&mut self) -> Result<u64, DbError> {
        self.count(self.sql("SELECT COUNT(ID) FROM {prefix}users"))
            .await
    }

    /// `wp_delete_user()`: the posts of `author` of any of `types`.
    pub async fn author_post_ids_of_types(
        &mut self,
        author: u64,
        types: &[&str],
    ) -> Result<Vec<u64>, DbError> {
        let mut sql =
            Sql::new("SELECT ID FROM {prefix}posts WHERE post_author = ? AND post_type IN (");
        sql.bind(Bind::Int(author));
        list(&mut sql, types.iter().map(|t| Bind::Text((*t).to_string())));
        let query = sql.query(&self.writer.prefix);
        self.ids(query).await
    }

    /// `wp_delete_user()`: the links of `owner`.
    pub async fn owner_link_ids(&mut self, owner: u64) -> Result<Vec<u64>, DbError> {
        let query = self.sql("SELECT link_id FROM {prefix}links WHERE link_owner = ?");
        self.ids(query.bind(owner)).await
    }

    /// `get_user_by( 'email', $email )`'s user.
    pub async fn user_id_by_email(&mut self, email: &[u8]) -> Result<Option<u64>, DbError> {
        let query = self.sql("SELECT ID FROM {prefix}users WHERE user_email = ? LIMIT 1");
        Ok(self.ids(query.bind(email)).await?.first().copied())
    }

    /// `wp_delete_user()`: the links of `owner` pass to `new_owner`.
    pub async fn reassign_links(&mut self, owner: u64, new_owner: u64) -> Result<u64, DbError> {
        let query = self.sql("UPDATE {prefix}links SET link_owner = ? WHERE link_owner = ?");
        self.change(Table::Links, query.bind(new_owner).bind(owner))
            .await
    }

    /// `wp_delete_user()`'s `DELETE`.
    pub async fn delete_user(&mut self, id: u64) -> Result<u64, DbError> {
        let query = self.sql("DELETE FROM {prefix}users WHERE ID = ?");
        self.change(Table::Users, query.bind(id)).await
    }

    // Options.

    /// `add_option()`: inserts the option, or replaces it if it exists.
    pub async fn add_option(
        &mut self,
        name: &str,
        value: &[u8],
        autoload: &str,
    ) -> Result<u64, DbError> {
        let mut sql = Sql::new(
            "INSERT INTO {prefix}options (option_name, option_value, autoload) VALUES (?, ?, ?)",
        );
        sql.bind(Bind::Text(name.to_string()));
        sql.bind(Bind::Bytes(value.to_vec()));
        sql.bind(Bind::Text(autoload.to_string()));
        Engine::insert_or_update(&mut sql, &["option_name", "option_value", "autoload"]);
        let query = sql.query(&self.writer.prefix);
        self.change(Table::Options, query).await
    }

    /// `update_option()` keeping the option's autoload.
    pub async fn update_option(&mut self, name: &str, value: &[u8]) -> Result<u64, DbError> {
        let query = self.sql("UPDATE {prefix}options SET option_value = ? WHERE option_name = ?");
        self.change(Table::Options, query.bind(value).bind(name))
            .await
    }

    /// `update_option()` with a new autoload.
    pub async fn update_option_autoload(
        &mut self,
        name: &str,
        value: &[u8],
        autoload: &str,
    ) -> Result<u64, DbError> {
        let query = self
            .sql("UPDATE {prefix}options SET option_value = ?, autoload = ? WHERE option_name = ?");
        let query = query.bind(value).bind(autoload).bind(name);
        self.change(Table::Options, query).await
    }

    /// `delete_option()`'s `DELETE`.
    pub async fn delete_option(&mut self, name: &str) -> Result<u64, DbError> {
        let query = self.sql("DELETE FROM {prefix}options WHERE option_name = ?");
        self.change(Table::Options, query.bind(name)).await
    }

    /// `get_option()`'s value of option `name`, read inside the action.
    pub async fn option_value(&mut self, name: &str) -> Result<Option<Vec<u8>>, DbError> {
        let query =
            self.sql("SELECT option_value FROM {prefix}options WHERE option_name = ? LIMIT 1");
        let row = self.optional(query.bind(name)).await?;
        row.map(|row| {
            let mut c = Columns::new(&row);
            let value = c.bytes("option_value");
            c.finish(value)
        })
        .transpose()
    }

    /// The names and values of the options whose names start with `prefix`,
    /// in name order, as `delete_site_transient()`'s callers look for them.
    pub async fn options_starting(
        &mut self,
        prefix: &str,
    ) -> Result<Vec<(String, Vec<u8>)>, DbError> {
        let length = i64::try_from(prefix.len()).unwrap_or_default();
        let query = self.sql(
            "SELECT option_name, option_value FROM {prefix}options WHERE LEFT(option_name, ?) = ? ORDER BY option_name",
        );
        let rows = self.rows(query.bind(length).bind(prefix)).await?;
        rows.iter()
            .map(|row| Ok((text(row, "option_name"), bytes(row, "option_value"))))
            .collect()
    }

    /// The autoload of option `name`, as `update_option()` reads it.
    pub async fn option_autoload(&mut self, name: &str) -> Result<Option<String>, DbError> {
        let query = self.sql("SELECT autoload FROM {prefix}options WHERE option_name = ? LIMIT 1");
        let row = self.optional(query.bind(name)).await?;
        Ok(row.map(|row| text(&row, "autoload")))
    }
}

/// The columns of a post `wp_insert_post()` writes.
#[derive(Clone, Debug)]
pub struct PostFields {
    /// `post_author`.
    pub author: u64,
    /// `post_date`, `YYYY-MM-DD hh:mm:ss`.
    pub date: String,
    /// `post_date_gmt`.
    pub date_gmt: String,
    /// `post_content`.
    pub content: Vec<u8>,
    /// `post_content_filtered`.
    pub content_filtered: Vec<u8>,
    /// `post_title`.
    pub title: Vec<u8>,
    /// `post_excerpt`.
    pub excerpt: Vec<u8>,
    /// `post_status`.
    pub status: String,
    /// `post_type`.
    pub post_type: String,
    /// `comment_status`.
    pub comment_status: String,
    /// `ping_status`.
    pub ping_status: String,
    /// `post_password`.
    pub password: Vec<u8>,
    /// `post_name`.
    pub name: Vec<u8>,
    /// `to_ping`.
    pub to_ping: Vec<u8>,
    /// `pinged`.
    pub pinged: Vec<u8>,
    /// `post_modified`.
    pub modified: String,
    /// `post_modified_gmt`.
    pub modified_gmt: String,
    /// `post_parent`.
    pub parent: u64,
    /// `menu_order`.
    pub menu_order: i64,
    /// `post_mime_type`.
    pub mime_type: String,
    /// `guid`.
    pub guid: Vec<u8>,
}

/// Whose held comments a comment query adds to the approved ones.
#[derive(Clone, Copy, Debug)]
pub enum Unapproved<'a> {
    /// The comments of this user.
    User(u64),
    /// The comments of the author with this email.
    Email(&'a [u8]),
}

impl PostFields {
    fn bind(&self, query: Query) -> Query {
        query
            .bind(self.author)
            .bind(&self.date)
            .bind(&self.date_gmt)
            .bind(&self.content)
            .bind(&self.content_filtered)
            .bind(&self.title)
            .bind(&self.excerpt)
            .bind(&self.status)
            .bind(&self.post_type)
            .bind(&self.comment_status)
            .bind(&self.ping_status)
            .bind(&self.password)
            .bind(&self.name)
            .bind(&self.to_ping)
            .bind(&self.pinged)
            .bind(&self.modified)
            .bind(&self.modified_gmt)
            .bind(self.parent)
            .bind(self.menu_order)
            .bind(&self.mime_type)
            .bind(&self.guid)
    }
}

/// The columns of a comment `wp_insert_comment()` and
/// `wp_update_comment()` write.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommentFields {
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
    /// `comment_date`, `YYYY-MM-DD hh:mm:ss`.
    pub date: String,
    /// `comment_date_gmt`.
    pub date_gmt: String,
    /// `comment_content`.
    pub content: Vec<u8>,
    /// `comment_karma`.
    pub karma: i64,
    /// `comment_approved`: `0`, `1`, `spam`, `trash` and so on.
    pub approved: String,
    /// `comment_agent`.
    pub agent: Vec<u8>,
    /// `comment_type`.
    pub comment_type: String,
    /// `comment_parent`.
    pub parent: u64,
    /// `user_id`.
    pub user_id: u64,
}

impl CommentFields {
    fn read(row: &crate::Row) -> CommentFields {
        let number = |name: &str| row.try_get::<u64, _>(name).unwrap_or_default();
        CommentFields {
            post_id: number("comment_post_ID"),
            author: bytes(row, "comment_author"),
            author_email: bytes(row, "comment_author_email"),
            author_url: bytes(row, "comment_author_url"),
            author_ip: bytes(row, "comment_author_IP"),
            date: text(row, "comment_date"),
            date_gmt: text(row, "comment_date_gmt"),
            content: bytes(row, "comment_content"),
            karma: row.try_get::<i64, _>("comment_karma").unwrap_or_default(),
            approved: text(row, "comment_approved"),
            agent: bytes(row, "comment_agent"),
            comment_type: text(row, "comment_type"),
            parent: number("comment_parent"),
            user_id: number("user_id"),
        }
    }

    fn bind(&self, query: Query) -> Query {
        query
            .bind(self.post_id)
            .bind(&self.author)
            .bind(&self.author_email)
            .bind(&self.author_url)
            .bind(&self.author_ip)
            .bind(&self.date)
            .bind(&self.date_gmt)
            .bind(&self.content)
            .bind(self.karma)
            .bind(&self.approved)
            .bind(&self.agent)
            .bind(&self.comment_type)
            .bind(self.parent)
            .bind(self.user_id)
    }
}

/// The columns of a user `wp_insert_user()` writes, but the login, which
/// only an insert writes.
#[derive(Clone, Debug, Default)]
pub struct UserFields {
    /// `user_pass`, the password hash.
    pub pass: String,
    /// `user_nicename`.
    pub nicename: Vec<u8>,
    /// `user_email`.
    pub email: Vec<u8>,
    /// `user_url`.
    pub url: Vec<u8>,
    /// `user_registered`, `YYYY-MM-DD hh:mm:ss`.
    pub registered: String,
    /// `user_activation_key`.
    pub activation_key: String,
    /// `display_name`.
    pub display_name: Vec<u8>,
}

impl UserFields {
    fn bind(&self, query: Query) -> Query {
        query
            .bind(&self.pass)
            .bind(&self.nicename)
            .bind(&self.email)
            .bind(&self.url)
            .bind(&self.registered)
            .bind(&self.activation_key)
            .bind(&self.display_name)
    }
}

#[cfg(test)]
pub(crate) mod tests;

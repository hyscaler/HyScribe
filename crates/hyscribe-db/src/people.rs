// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_Comment_Query` and `WP_User_Query` as the REST API runs them.

use hyscribe_model::{
    CommentOrder, CommentQuery, CommentRow, USER_SEARCH_COLUMNS, User, UserOrder, UserQuery,
};
use sqlx::Row as _;

use crate::dialect::Dialect;
use crate::{
    Bind, Columns, Db, DbError, Engine, Row, Sql, comment, db_error, like, list, select_comments,
    user,
};

/// `values` as text binds.
fn texts<'a>(values: impl IntoIterator<Item = &'a String>) -> impl Iterator<Item = Bind> {
    values.into_iter().map(|value| Bind::Text(value.clone()))
}

pub(crate) fn ints(ids: &[u64]) -> impl Iterator<Item = Bind> + '_ {
    ids.iter().copied().map(Bind::Int)
}

/// ` AND column IN (...)`, or `NOT IN`, of `values` when there are any.
fn within(sql: &mut Sql, column: &'static str, not: bool, values: Vec<Bind>) {
    if values.is_empty() {
        return;
    }
    sql.push(" AND ");
    sql.push(column);
    sql.push(if not { " NOT IN (" } else { " IN (" });
    list(sql, values.into_iter());
}

/// The `WHERE` of a comment query.
fn comment_where(sql: &mut Sql, query: &CommentQuery) {
    sql.push(" WHERE 1=1");
    if let Some(approved) = &query.approved {
        within(sql, "comment_approved", false, texts(approved).collect());
    }
    let columns: [(&'static str, bool, &[u64]); 7] = [
        ("comment_ID", false, &query.include),
        ("comment_ID", true, &query.exclude),
        ("comment_parent", false, &query.parents),
        ("comment_parent", true, &query.excluded_parents),
        ("comment_post_ID", false, &query.posts),
        ("user_id", false, &query.authors),
        ("user_id", true, &query.excluded_authors),
    ];
    for (column, not, ids) in columns {
        within(sql, column, not, ints(ids).collect());
    }
    if let Some(email) = &query.author_email {
        sql.push(" AND comment_author_email = ?");
        sql.bind(Bind::Text(String::from_utf8_lossy(email).into_owned()));
    }
    within(sql, "comment_type", false, texts(&query.types).collect());
    within(
        sql,
        "comment_type",
        true,
        texts(&query.excluded_types).collect(),
    );
    if let Some(search) = &query.search {
        sql.push(
            " AND (comment_author LIKE ? OR comment_author_email LIKE ? OR comment_author_url LIKE ? \
             OR comment_author_IP LIKE ? OR comment_content LIKE ?)",
        );
        for _ in 0..5 {
            sql.bind(Bind::Text(like(search)));
        }
    }
    for (fragment, date) in [
        (" AND comment_date > ?", &query.after),
        (" AND comment_date < ?", &query.before),
    ] {
        if let Some(date) = date {
            sql.push(fragment);
            sql.bind(Bind::Text(date.clone()));
        }
    }
}

/// The column a comment order sorts by; none for the order of the IDs.
pub(crate) fn comment_column(order: CommentOrder) -> Option<&'static str> {
    Some(match order {
        CommentOrder::Date => "comment_date",
        CommentOrder::DateGmt => "comment_date_gmt",
        CommentOrder::Id => "comment_ID",
        CommentOrder::Post => "comment_post_ID",
        CommentOrder::Parent => "comment_parent",
        CommentOrder::Type => "comment_type",
        CommentOrder::Include => return None,
    })
}

/// The column a user order sorts by; none for the order of a list.
pub(crate) fn user_column(order: UserOrder) -> Option<&'static str> {
    Some(match order {
        UserOrder::Login => "user_login",
        UserOrder::Nicename => "user_nicename",
        UserOrder::Email => "user_email",
        UserOrder::Url => "user_url",
        UserOrder::Registered => "user_registered",
        UserOrder::DisplayName => "display_name",
        UserOrder::Id => "u.ID",
        UserOrder::Include | UserOrder::Nicenames => return None,
    })
}

/// A comment with the columns the REST API reads.
pub(crate) fn comment_row(row: &Row) -> Result<CommentRow, DbError> {
    comment(row).and_then(|comment| {
        let mut c = Columns::new(row);
        let approved = c.text("comment_approved");
        let agent = c.bytes("comment_agent");
        let note_status = c.bytes("note_status");
        c.finish(CommentRow {
            comment,
            approved,
            agent,
            note_status,
        })
    })
}

/// A limit and offset, when limited.
fn limit(sql: &mut Sql, limit: Option<u64>, offset: u64) {
    if let Some(limit) = limit {
        sql.push(" LIMIT ? OFFSET ?");
        sql.bind(Bind::Int(limit));
        sql.bind(Bind::Int(offset));
    }
}

/// The `WHERE` of a user query.
fn user_where(sql: &mut Sql, query: &UserQuery) {
    sql.push(" WHERE 1=1");
    if let Some(types) = &query.published {
        sql.push(
            " AND u.ID IN (SELECT DISTINCT post_author FROM {prefix}posts \
             WHERE post_status = 'publish' AND post_type IN (",
        );
        list(
            sql,
            texts(types).chain(std::iter::once(Bind::Text(String::new()))),
        );
        sql.push(")");
    }
    let nicenames = query.nicenames.iter();
    let nicenames = nicenames.map(|name| Bind::Text(String::from_utf8_lossy(name).into_owned()));
    within(sql, "user_nicename", false, nicenames.collect());
    if let Some(key) = &query.level_key {
        sql.push(
            " AND EXISTS (SELECT 1 FROM {prefix}usermeta l WHERE l.user_id = u.ID \
             AND l.meta_key = ? AND l.meta_value != '0')",
        );
        sql.bind(Bind::Text(key.clone()));
    }
    for (i, role) in query.roles.iter().enumerate() {
        sql.push(if i == 0 { " AND (" } else { " OR " });
        sql.push(
            "EXISTS (SELECT 1 FROM {prefix}usermeta r WHERE r.user_id = u.ID \
             AND r.meta_key = ? AND r.meta_value LIKE ?)",
        );
        sql.bind(Bind::Text(query.caps_key.clone()));
        sql.bind(Bind::Text(like(&format!("\"{role}\""))));
    }
    if !query.roles.is_empty() {
        sql.push(")");
    }
    if let Some((columns, text)) = &query.search {
        let columns = columns
            .iter()
            .filter_map(|name| USER_SEARCH_COLUMNS.iter().find(|column| *column == name));
        for (i, column) in columns.enumerate() {
            sql.push(if i == 0 { " AND (" } else { " OR " });
            sql.push(column);
            if *column == "ID" {
                sql.push(" = ?");
                sql.bind(Bind::Text(text.clone()));
            } else {
                sql.push(" LIKE ?");
                sql.bind(Bind::Text(like(text)));
            }
        }
        sql.push(")");
    }
    within(sql, "u.ID", false, ints(&query.include).collect());
    within(sql, "u.ID", true, ints(&query.exclude).collect());
}

impl Db {
    /// The comments `query` selects, in its order.
    pub async fn comment_query(&self, query: &CommentQuery) -> Result<Vec<CommentRow>, DbError> {
        let mut sql = select_comments(
            ", comment_approved, comment_agent, COALESCE((SELECT meta_value FROM {prefix}commentmeta m \
             WHERE m.comment_id = {prefix}comments.comment_ID AND m.meta_key = '_wp_note_status' \
             ORDER BY m.meta_id LIMIT 1), '') AS note_status FROM {prefix}comments",
        );
        comment_where(&mut sql, query);
        for (i, (order, descending)) in query.order.iter().enumerate() {
            sql.push(if i == 0 { " ORDER BY " } else { ", " });
            if let Some(column) = comment_column(*order) {
                sql.push(column);
            } else {
                let ids = ints(&query.include).chain(std::iter::once(Bind::Int(0)));
                Engine::list_order(&mut sql, "comment_ID", ids);
            }
            sql.push(if *descending { " DESC" } else { " ASC" });
        }
        limit(&mut sql, query.limit, query.offset);
        let rows = sql
            .query(&self.prefix)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter().map(comment_row).collect()
    }

    /// How many comments `query` selects, without its limit.
    pub async fn count_comment_query(&self, query: &CommentQuery) -> Result<u64, DbError> {
        let mut sql = Sql::new("SELECT COUNT(*) AS found FROM {prefix}comments");
        comment_where(&mut sql, query);
        self.count(sql).await
    }

    /// The users `query` selects, in its order.
    pub async fn user_query(&self, query: &UserQuery) -> Result<Vec<User>, DbError> {
        let mut sql = Sql::new(concat!(user_columns!(), " FROM {prefix}users u"));
        user_where(&mut sql, query);
        let (order, descending) = query.order;
        sql.push(" ORDER BY ");
        match (user_column(order), order) {
            (Some(column), _) => sql.push(column),
            (None, UserOrder::Include) => {
                let ids = ints(&query.include).chain(std::iter::once(Bind::Int(0)));
                Engine::list_order(&mut sql, "u.ID", ids);
            }
            (None, _) => {
                // PHP writes `FIELD( user_nicename, '' )` for no names.
                let names = query.nicenames.iter();
                let names = names.map(|name| String::from_utf8_lossy(name).into_owned());
                let mut names: Vec<String> = names.collect();
                if names.is_empty() {
                    names.push(String::new());
                }
                Engine::list_order(&mut sql, "user_nicename", names.into_iter().map(Bind::Text));
            }
        }
        sql.push(if descending { " DESC" } else { " ASC" });
        limit(&mut sql, query.limit, query.offset);
        let rows = sql
            .query(&self.prefix)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter().map(user).collect()
    }

    /// How many users `query` selects, without its limit.
    pub async fn count_user_query(&self, query: &UserQuery) -> Result<u64, DbError> {
        let mut sql = Sql::new("SELECT COUNT(*) AS found FROM {prefix}users u");
        user_where(&mut sql, query);
        self.count(sql).await
    }

    /// The user's `user_registered`, empty for no such user.
    pub async fn user_registered(&self, id: u64) -> Result<String, DbError> {
        let mut sql = Sql::new("SELECT ");
        Engine::date_text(&mut sql, "user_registered");
        sql.push(" AS registered FROM {prefix}users WHERE ID = ?");
        sql.bind(Bind::Int(id));
        let row = sql
            .query(&self.prefix)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;
        row.map_or(Ok(String::new()), |row| {
            let mut c = Columns::new(&row);
            let registered = c.text("registered");
            c.finish(registered)
        })
    }

    /// Whether `author` has posts of any of `types`, published, or private
    /// too where a type's flag is set: `count_user_posts()`.
    pub async fn has_posts(&self, author: u64, types: &[(String, bool)]) -> Result<bool, DbError> {
        let mut sql = Sql::new(
            "SELECT COUNT(*) AS found FROM {prefix}posts WHERE post_author = ? AND (1 = 0",
        );
        sql.bind(Bind::Int(author));
        for (post_type, private) in types {
            sql.push(" OR (post_type = ? AND (post_status = 'publish' OR (? = 1 AND post_status = 'private')))");
            sql.bind(Bind::Text(post_type.clone()));
            sql.bind(Bind::Int(u64::from(*private)));
        }
        sql.push(")");
        self.count(sql).await.map(|found| found > 0)
    }

    /// The `found` of a `COUNT(*)`.
    async fn count(&self, sql: Sql) -> Result<u64, DbError> {
        let row = sql.query(&self.prefix).fetch_one(&self.pool).await;
        row.and_then(|row| row.try_get::<i64, _>("found"))
            .map(i64::unsigned_abs)
            .map_err(db_error)
    }
}

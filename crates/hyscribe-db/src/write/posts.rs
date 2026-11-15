// SPDX-License-Identifier: AGPL-3.0-or-later

//! What `wp_insert_post()`, `wp_trash_post()` and `wp_delete_post()` read
//! inside their action, and the insert with a suggested ID.

use super::{PostFields, Table, Write};
use crate::dialect::Dialect;
use crate::{Bind, Columns, DbError, Engine, Row, Sql, date_column, db_error};

fn post_fields(row: &Row) -> Result<PostFields, DbError> {
    let mut c = Columns::new(row);
    let post = PostFields {
        author: c.unsigned("post_author"),
        date: c.text("post_date"),
        date_gmt: c.text("post_date_gmt"),
        content: c.bytes("post_content"),
        content_filtered: c.bytes("post_content_filtered"),
        title: c.bytes("post_title"),
        excerpt: c.bytes("post_excerpt"),
        status: c.text("post_status"),
        post_type: c.text("post_type"),
        comment_status: c.text("comment_status"),
        ping_status: c.text("ping_status"),
        password: c.bytes("post_password"),
        name: c.bytes("post_name"),
        to_ping: c.bytes("to_ping"),
        pinged: c.bytes("pinged"),
        modified: c.text("post_modified"),
        modified_gmt: c.text("post_modified_gmt"),
        parent: c.unsigned("post_parent"),
        menu_order: c.signed("menu_order"),
        mime_type: c.text("post_mime_type"),
        guid: c.bytes("guid"),
    };
    c.finish(post)
}

impl Write<'_> {
    /// `wp_insert_post()`'s `INSERT` with the suggested `import_id`.
    pub async fn insert_post_with_id(
        &mut self,
        id: u64,
        post: &PostFields,
    ) -> Result<u64, DbError> {
        let query = self
            .sql(
                "INSERT INTO {prefix}posts (ID, post_author, post_date, post_date_gmt, \
                 post_content, post_content_filtered, post_title, post_excerpt, post_status, \
                 post_type, comment_status, ping_status, post_password, post_name, to_ping, \
                 pinged, post_modified, post_modified_gmt, post_parent, menu_order, \
                 post_mime_type, guid) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(id);
        self.insert(Table::Posts, post.bind(query)).await
    }

    /// The columns `wp_insert_post()` writes of post `id`, as it is now in
    /// the action.
    pub async fn post(&mut self, id: u64) -> Result<Option<PostFields>, DbError> {
        let mut sql = Sql::new(
            "SELECT post_author, post_content, post_content_filtered, post_title, post_excerpt, \
             post_status, post_type, comment_status, ping_status, post_password, post_name, \
             to_ping, pinged, post_parent, post_mime_type, guid",
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
        sql.push(" AS menu_order FROM {prefix}posts WHERE ID = ?");
        sql.bind(Bind::Int(id));
        self.spend()?;
        let row = sql
            .query(&self.writer.prefix)
            .fetch_optional(&mut *self.tx)
            .await
            .map_err(db_error)?;
        row.as_ref().map(post_fields).transpose()
    }

    /// `wp_unique_post_slug()` for a flat type: whether another post of
    /// `post_type` has the name.
    pub async fn post_name_used(
        &mut self,
        name: &[u8],
        post_type: &str,
        id: u64,
    ) -> Result<bool, DbError> {
        let query = self.sql(
            "SELECT COUNT(*) FROM {prefix}posts WHERE post_name = ? AND post_type = ? AND ID != ?",
        );
        let query = query.bind(name).bind(post_type).bind(id);
        Ok(self.count(query).await? > 0)
    }

    /// `wp_unique_post_slug()` for a hierarchical type: whether another
    /// post of `post_type`, or an attachment, under `parent` has the name.
    pub async fn sibling_name_used(
        &mut self,
        name: &[u8],
        (post_type, parent): (&str, u64),
        id: u64,
    ) -> Result<bool, DbError> {
        let query = self.sql(
            "SELECT COUNT(*) FROM {prefix}posts WHERE post_name = ? \
             AND post_type IN (?, 'attachment') AND ID != ? AND post_parent = ?",
        );
        let query = query.bind(name).bind(post_type).bind(id).bind(parent);
        Ok(self.count(query).await? > 0)
    }

    /// The trashed posts, pages and attachments named `name` but `id`,
    /// newest first, as `wp_add_trashed_suffix_to_post_name_for_trashed_posts()`
    /// finds them.
    pub async fn trashed_post_ids_named(
        &mut self,
        name: &[u8],
        id: u64,
    ) -> Result<Vec<u64>, DbError> {
        let query = self.sql(
            "SELECT ID FROM {prefix}posts WHERE post_name = ? AND ID != ? \
             AND post_type IN ('post', 'page', 'attachment') AND post_status = 'trash' \
             ORDER BY post_date DESC",
        );
        self.ids(query.bind(name).bind(id)).await
    }

    /// `block_core_calendar_update_has_published_posts()`'s query.
    pub async fn published_post_exists(&mut self) -> Result<bool, DbError> {
        let query = self.sql(
            "SELECT COUNT(*) FROM {prefix}posts \
             WHERE post_type = 'post' AND post_status = 'publish'",
        );
        Ok(self.count(query).await? > 0)
    }

    /// `wp_get_post_revisions( $parent )`: newest first.
    pub async fn revision_ids(&mut self, parent: u64) -> Result<Vec<u64>, DbError> {
        let query = self.sql(
            "SELECT ID FROM {prefix}posts WHERE post_parent = ? AND post_type = 'revision' \
             AND post_status = 'inherit' ORDER BY post_date DESC, ID DESC",
        );
        self.ids(query.bind(parent)).await
    }

    /// The comments of `post` as `(comment_ID, comment_approved)`, in ID
    /// order, as `wp_trash_post_comments()` reads them.
    pub async fn post_comments(&mut self, post: u64) -> Result<Vec<(u64, String)>, DbError> {
        let query = self.sql(
            "SELECT comment_ID, comment_approved FROM {prefix}comments \
             WHERE comment_post_ID = ? ORDER BY comment_ID",
        );
        self.spend()?;
        let rows = query
            .bind(post)
            .fetch_all(&mut *self.tx)
            .await
            .map_err(db_error)?;
        rows.iter()
            .map(|row| {
                let mut c = Columns::new(row);
                let comment = (c.unsigned("comment_ID"), c.text("comment_approved"));
                c.finish(comment)
            })
            .collect()
    }
}

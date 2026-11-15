// SPDX-License-Identifier: AGPL-3.0-or-later

//! The meta functions' writes (`add_metadata()`, `update_metadata()`,
//! `delete_metadata()` and `delete_metadata_by_mid()`), for the four meta tables.

use super::{Table, Write};
use sqlx::Row as _;

use crate::{Columns, DbError, Sql, db_error, ids_or_none, list};

/// What a meta row belongs to, which names its table.
#[derive(Clone, Copy, Debug)]
pub enum Meta {
    /// `{prefix}postmeta`.
    Post,
    /// `{prefix}commentmeta`.
    Comment,
    /// `{prefix}termmeta`.
    Term,
    /// `{prefix}usermeta`.
    User,
}

/// The statements of one meta table.
struct Statements {
    table: Table,
    add: &'static str,
    update: &'static str,
    update_from: &'static str,
    delete: &'static str,
    ids: &'static str,
    ids_valued: &'static str,
    ids_of_key: &'static str,
    ids_of_key_valued: &'static str,
    object_ids: &'static str,
    object_meta: &'static str,
    value: &'static str,
}

/// The statements of the meta table `$table`, whose object column is
/// `$object` and whose ID column is `$id`.
macro_rules! statements {
    ($table:ident, $name:literal, $object:literal, $id:literal) => {
        Statements {
            table: Table::$table,
            add: concat!(
                "INSERT INTO {prefix}",
                $name,
                " (",
                $object,
                ", meta_key, meta_value) VALUES (?, ?, ?)"
            ),
            update: concat!(
                "UPDATE {prefix}",
                $name,
                " SET meta_value = ? WHERE ",
                $object,
                " = ? AND meta_key = ?"
            ),
            update_from: concat!(
                "UPDATE {prefix}",
                $name,
                " SET meta_value = ? WHERE ",
                $object,
                " = ? AND meta_key = ? AND meta_value = ?"
            ),
            delete: concat!("DELETE FROM {prefix}", $name, " WHERE ", $id, " IN ("),
            ids: concat!(
                "SELECT ",
                $id,
                " FROM {prefix}",
                $name,
                " WHERE meta_key = ? AND ",
                $object,
                " = ?"
            ),
            ids_valued: concat!(
                "SELECT ",
                $id,
                " FROM {prefix}",
                $name,
                " WHERE meta_key = ? AND ",
                $object,
                " = ? AND meta_value = ?"
            ),
            ids_of_key: concat!(
                "SELECT ",
                $id,
                " FROM {prefix}",
                $name,
                " WHERE meta_key = ?"
            ),
            ids_of_key_valued: concat!(
                "SELECT ",
                $id,
                " FROM {prefix}",
                $name,
                " WHERE meta_key = ? AND meta_value = ?"
            ),
            object_ids: concat!(
                "SELECT ",
                $id,
                " FROM {prefix}",
                $name,
                " WHERE ",
                $object,
                " = ?"
            ),
            object_meta: concat!(
                "SELECT meta_key, meta_value FROM {prefix}",
                $name,
                " WHERE ",
                $object,
                " = ? ORDER BY ",
                $id
            ),
            value: concat!(
                "SELECT meta_value FROM {prefix}",
                $name,
                " WHERE ",
                $object,
                " = ? AND meta_key = ? ORDER BY ",
                $id,
                " LIMIT 1"
            ),
        }
    };
}

const POST: Statements = statements!(Postmeta, "postmeta", "post_id", "meta_id");
const COMMENT: Statements = statements!(Commentmeta, "commentmeta", "comment_id", "meta_id");
const TERM: Statements = statements!(Termmeta, "termmeta", "term_id", "meta_id");
const USER: Statements = statements!(Usermeta, "usermeta", "user_id", "umeta_id");

impl Meta {
    fn statements(self) -> &'static Statements {
        match self {
            Meta::Post => &POST,
            Meta::Comment => &COMMENT,
            Meta::Term => &TERM,
            Meta::User => &USER,
        }
    }
}

impl Write<'_> {
    /// `add_metadata()`'s `INSERT`: the new row's ID. `None` stores NULL,
    /// as `wpdb` does for PHP's `null`.
    pub async fn add_meta(
        &mut self,
        meta: Meta,
        object: u64,
        key: &str,
        value: Option<&[u8]>,
    ) -> Result<u64, DbError> {
        let s = meta.statements();
        let query = self.sql(s.add).bind(object).bind(key).bind(value);
        self.insert(s.table, query).await
    }

    /// `update_metadata()`'s `UPDATE` of every `key` row of `object`.
    pub async fn update_meta(
        &mut self,
        meta: Meta,
        object: u64,
        key: &str,
        value: Option<&[u8]>,
    ) -> Result<u64, DbError> {
        let s = meta.statements();
        let query = self.sql(s.update).bind(value).bind(object).bind(key);
        self.change(s.table, query).await
    }

    /// `update_metadata()` with a previous value: only the `key` rows of
    /// `object` holding `previous`.
    pub async fn update_meta_from(
        &mut self,
        meta: Meta,
        (object, key): (u64, &str),
        previous: &[u8],
        value: Option<&[u8]>,
    ) -> Result<u64, DbError> {
        let s = meta.statements();
        let query = self.sql(s.update_from).bind(value).bind(object).bind(key);
        self.change(s.table, query.bind(previous)).await
    }

    /// `delete_metadata()`'s and `delete_metadata_by_mid()`'s `DELETE` of
    /// the rows `ids`.
    pub async fn delete_meta(&mut self, meta: Meta, ids: &[u64]) -> Result<u64, DbError> {
        let s = meta.statements();
        let mut sql = Sql::new(s.delete);
        list(&mut sql, ids_or_none(ids));
        let query = sql.query(&self.writer.prefix);
        self.change(s.table, query).await
    }

    /// The `key` rows of `object`, as `update_metadata()` and
    /// `delete_metadata()` find them.
    pub async fn meta_ids(
        &mut self,
        meta: Meta,
        object: u64,
        key: &str,
    ) -> Result<Vec<u64>, DbError> {
        let query = self.sql(meta.statements().ids).bind(key).bind(object);
        self.ids(query).await
    }

    /// The `key` rows of `object` holding `value`, as `delete_metadata()`
    /// finds them.
    pub async fn meta_ids_valued(
        &mut self,
        meta: Meta,
        (object, key): (u64, &str),
        value: &[u8],
    ) -> Result<Vec<u64>, DbError> {
        let query = self
            .sql(meta.statements().ids_valued)
            .bind(key)
            .bind(object);
        self.ids(query.bind(value)).await
    }

    /// The `key` rows of every object, as `delete_metadata()` finds them to
    /// delete them all.
    pub async fn meta_ids_of_key(&mut self, meta: Meta, key: &str) -> Result<Vec<u64>, DbError> {
        let query = self.sql(meta.statements().ids_of_key).bind(key);
        self.ids(query).await
    }

    /// The `key` rows of every object holding `value`.
    pub async fn meta_ids_of_key_valued(
        &mut self,
        meta: Meta,
        key: &str,
        value: &[u8],
    ) -> Result<Vec<u64>, DbError> {
        let query = self
            .sql(meta.statements().ids_of_key_valued)
            .bind(key)
            .bind(value);
        self.ids(query).await
    }

    /// The first `key` value of `object`, as `get_metadata()` reads a
    /// single one.
    pub async fn meta_value(
        &mut self,
        meta: Meta,
        object: u64,
        key: &str,
    ) -> Result<Option<Vec<u8>>, DbError> {
        self.spend()?;
        let query = self.sql(meta.statements().value).bind(object).bind(key);
        self.spend()?;
        let row = query
            .fetch_optional(&mut *self.tx)
            .await
            .map_err(db_error)?;
        Ok(row.map(|row| super::bytes(&row, "meta_value")))
    }

    /// Every meta row of `object`, as deleting the object finds them.
    pub async fn object_meta_ids(&mut self, meta: Meta, object: u64) -> Result<Vec<u64>, DbError> {
        let query = self.sql(meta.statements().object_ids).bind(object);
        self.ids(query).await
    }

    /// Every meta row of `object`, keys and values in ID order, as
    /// `update_meta_cache()` reads them; a NULL value is `None`.
    pub async fn object_meta(
        &mut self,
        meta: Meta,
        object: u64,
    ) -> Result<Vec<(Vec<u8>, Option<Vec<u8>>)>, DbError> {
        self.spend()?;
        let query = self.sql(meta.statements().object_meta).bind(object);
        let rows = query.fetch_all(&mut *self.tx).await.map_err(db_error)?;
        rows.iter()
            .map(|row| {
                let mut c = Columns::new(row);
                let key = c.bytes("meta_key");
                let value = c.keep(row.try_get_unchecked::<Option<Vec<u8>>, _>("meta_value"));
                c.finish((key, value))
            })
            .collect()
    }
}

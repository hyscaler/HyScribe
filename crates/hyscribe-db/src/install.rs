// SPDX-License-Identifier: AGPL-3.0-or-later

//! The database side of the new site path (ADR 0021): creating WordPress's
//! tables and their first rows in a database that has none of the prefix.
//! This is the only place HyScribe creates tables, and it only ever creates
//! and fills tables of the configured prefix that did not exist, and drops
//! those if the install fails, so it never changes a table that was there.
//! The statements are MySQL's and MariaDB's own, as `SHOW CREATE TABLE`
//! prints them for PHP's tables (ADR 0003); another engine adds its schema.

use hyscribe_config::Database;
use sqlx::{Connection as _, Row as _};

use crate::dialect::{Access, Dialect};
use crate::{Backend, DbError, Engine, PrefixSql as _, db_error};

/// How long a statement waits for a lock another connection holds, in
/// seconds: a drop that cannot get its table says so instead of waiting on.
const LOCK_WAIT: &str = "SET SESSION lock_wait_timeout = 10";

/// The rows of one table to insert, by column name; `None` is `NULL`.
#[derive(Clone, Debug)]
pub struct Rows {
    /// The table's name without the prefix.
    pub table: String,
    /// The columns each row gives, in order.
    pub columns: Vec<String>,
    /// The rows, each as long as `columns`.
    pub rows: Vec<Vec<Option<String>>>,
}

/// One writable connection that creates a site's tables.
pub struct Installer {
    conn: sqlx::pool::PoolConnection<Backend>,
    prefix: hyscribe_config::Prefix,
    /// The tables this installer made, with their prefix, in order.
    created: Vec<String>,
}

/// `name` as an identifier: letters, digits and underscores only, as a
/// table prefix is, so it is safe in a statement.
fn identifier(name: &str) -> Result<&str, DbError> {
    if !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        Ok(name)
    } else {
        Err(DbError(format!("{name:?} is not a table or column name")))
    }
}

impl Installer {
    /// Connects as the main user, with WordPress's SQL modes.
    pub async fn open(config: &Database) -> Result<Installer, DbError> {
        let pool = Engine::connect(config, Access::Write)
            .await
            .map_err(db_error)?;
        let mut conn = pool.acquire().await.map_err(db_error)?;
        sqlx::query(LOCK_WAIT)
            .execute(&mut *conn)
            .await
            .map_err(db_error)?;
        Ok(Installer {
            conn,
            prefix: config.prefix.clone(),
            created: Vec::new(),
        })
    }

    /// The tables of the database whose names start with the prefix,
    /// whatever their case, in name order.
    pub async fn existing(&mut self) -> Result<Vec<String>, DbError> {
        let rows = sqlx::query(
            "SELECT TABLE_NAME AS name FROM information_schema.TABLES \
             WHERE TABLE_SCHEMA = DATABASE() ORDER BY TABLE_NAME",
        )
        .fetch_all(&mut *self.conn)
        .await
        .map_err(db_error)?;
        let prefix = self.prefix.as_str().to_lowercase();
        Ok(rows
            .iter()
            .map(|row| {
                let name = row.try_get_unchecked::<Vec<u8>, _>("name");
                String::from_utf8_lossy(&name.unwrap_or_default()).into_owned()
            })
            .filter(|name| name.to_lowercase().starts_with(&prefix))
            .collect())
    }

    /// Runs one `CREATE TABLE` statement whose table is written
    /// `` `{prefix}name` ``, notes the table so a failed install drops it,
    /// and gives its name without the prefix.
    pub async fn create(&mut self, statement: &str) -> Result<String, DbError> {
        let name = statement
            .strip_prefix("CREATE TABLE `{prefix}")
            .and_then(|rest| rest.split_once('`'))
            .map(|(name, _)| name)
            .ok_or_else(|| DbError("a CREATE TABLE statement for a table of the prefix".into()))?;
        let table = format!("{}{}", self.prefix.as_str(), identifier(name)?);
        sqlx::raw_sql(self.prefix.sql_from_fragments(statement.to_string()))
            .execute(&mut *self.conn)
            .await
            .map_err(db_error)?;
        self.created.push(table);
        Ok(name.to_string())
    }

    /// Inserts every row of `tables`, in one transaction: all or none.
    pub async fn insert(&mut self, tables: &[Rows]) -> Result<(), DbError> {
        let mut transaction = self.conn.begin().await.map_err(db_error)?;
        for table in tables {
            let columns = table
                .columns
                .iter()
                .map(|column| identifier(column))
                .collect::<Result<Vec<_>, _>>()?;
            let marks = vec!["?"; columns.len()].join(", ");
            let statement = format!(
                "INSERT INTO {}{} ({}) VALUES ({marks})",
                self.prefix.as_str(),
                identifier(&table.table)?,
                columns.join(", ")
            );
            for row in &table.rows {
                let mut query = sqlx::query(sqlx::AssertSqlSafe(statement.clone()));
                for value in row {
                    query = query.bind(value.clone());
                }
                query.execute(&mut *transaction).await.map_err(db_error)?;
            }
        }
        transaction.commit().await.map_err(db_error)
    }

    /// Drops the tables of an install made earlier, by their names without
    /// the prefix, as [`Installer::create`] gave them: for a site whose
    /// later steps failed.
    pub async fn drop_tables(&mut self, names: &[String]) -> Result<(), DbError> {
        for name in names {
            let table = format!("{}{}", self.prefix.as_str(), identifier(name)?);
            self.created.push(table);
        }
        self.undo().await
    }

    /// Drops the tables this installer made, last first, after a failure.
    pub async fn undo(&mut self) -> Result<(), DbError> {
        // A table stays noted until it is dropped, so a drop that fails can
        // be tried again.
        while let Some(table) = self.created.last().cloned() {
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP TABLE IF EXISTS {table}")))
                .execute(&mut *self.conn)
                .await
                .map_err(db_error)?;
            self.created.pop();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

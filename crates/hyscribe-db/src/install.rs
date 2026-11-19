// SPDX-License-Identifier: AGPL-3.0-or-later

//! The database side of the new site path (ADR 0021): creating WordPress's
//! tables and their first rows in a database that has none of the prefix.
//! This is the only place HyScribe creates tables, and it only ever creates
//! and fills tables of the configured prefix that did not exist, and drops
//! those if the install fails, so it never changes a table that was there.
//! The statements are MySQL's and MariaDB's own, as `SHOW CREATE TABLE`
//! prints them for PHP's tables (ADR 0003); another engine adds its schema.

use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};

use hyscribe_config::Database;
use sqlx::Row as _;

use crate::dialect::{Access, Dialect};
use crate::{Backend, DbError, Engine, PrefixSql as _, db_error};

type Pool = sqlx::Pool<Backend>;

/// The budget of an installer whose statements do not fail on purpose.
const UNLIMITED: usize = usize::MAX;

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

/// A writable connection that creates a site's tables: one connection, whose
/// session waits ten seconds at most for a lock.
pub struct Installer {
    pool: Pool,
    prefix: hyscribe_config::Prefix,
    /// The tables this installer made, with their prefix, in order.
    created: Vec<String>,
    /// The statements left before each next one fails, [`UNLIMITED`] if not
    /// limited.
    budget: AtomicUsize,
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
        let pool = Engine::connect(config, Access::Install)
            .await
            .map_err(db_error)?;
        Ok(Installer {
            pool,
            prefix: config.prefix.clone(),
            created: Vec::new(),
            budget: AtomicUsize::new(UNLIMITED),
        })
    }

    /// Lets `statements` more statements run and fails each one after, so
    /// tests reach every error path; `None` lifts the limit.
    pub fn fail_after(&self, statements: Option<usize>) {
        self.budget
            .store(statements.unwrap_or(UNLIMITED), Ordering::SeqCst);
    }

    /// Runs one statement, unless the budget [`Installer::fail_after`] set
    /// is spent.
    async fn counted<T>(
        &self,
        statement: impl Future<Output = Result<T, sqlx::Error>>,
    ) -> Result<T, DbError> {
        let left = self.budget.load(Ordering::SeqCst);
        let spent = match left {
            0 => Err(DbError("the test's statement budget is spent".into())),
            UNLIMITED => Ok(()),
            left => {
                self.budget.store(left - 1, Ordering::SeqCst);
                Ok(())
            }
        };
        match spent {
            Ok(()) => statement.await.map_err(db_error),
            Err(err) => Err(err),
        }
    }

    /// The tables of the database whose names start with the prefix,
    /// whatever their case, in name order.
    pub async fn existing(&self) -> Result<Vec<String>, DbError> {
        let rows = sqlx::query(
            "SELECT TABLE_NAME AS name FROM information_schema.TABLES \
             WHERE TABLE_SCHEMA = DATABASE() ORDER BY TABLE_NAME",
        )
        .fetch_all(&self.pool);
        let rows = self.counted(rows).await?;
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
        let created = sqlx::raw_sql(self.prefix.sql_from_fragments(statement.to_string()))
            .execute(&self.pool);
        self.counted(created).await?;
        self.created.push(table);
        Ok(name.to_string())
    }

    /// Inserts every row of `tables`, in one transaction: all or none.
    pub async fn insert(&self, tables: &[Rows]) -> Result<(), DbError> {
        let mut transaction = self.counted(self.pool.begin()).await?;
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
                self.counted(query.execute(&mut *transaction)).await?;
            }
        }
        self.counted(transaction.commit()).await
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
            let drop = sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP TABLE IF EXISTS {table}")))
                .execute(&self.pool);
            self.counted(drop).await?;
            self.created.pop();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

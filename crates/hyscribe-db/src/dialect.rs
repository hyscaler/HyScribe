// SPDX-License-Identifier: AGPL-3.0-or-later

//! The SQL that cannot be written portably (ADR 0014): every query is
//! standard SQL but for the pieces a [`Dialect`] writes. MySQL and MariaDB
//! are the only engine, and their answers are the contract.

use std::future::Future;

use hyscribe_config::Database;
use sqlx::Executor;
use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions};

use crate::{Bind, Sql, list};

/// A part of a date [`Dialect::date_part`] extracts.
#[derive(Clone, Copy)]
pub(crate) enum DatePart {
    Year,
    Month,
    Day,
    Hour,
    Minute,
    Second,
}

/// An engine's SQL for what standard SQL lacks or answers differently.
/// It writes fixed fragments and binds values, never request text.
pub(crate) trait Dialect {
    /// The engine's `sqlx` driver, which gives the row and pool types.
    type Backend: sqlx::Database;

    /// A pool for `config`: read only, as the read user when there is one,
    /// or writable, as the main user in WordPress's SQL modes.
    fn connect(
        config: &Database,
        access: Access,
    ) -> impl Future<Output = Result<sqlx::Pool<Self::Backend>, sqlx::Error>> + Send;

    /// The ID an `INSERT` gave its row.
    fn inserted_id(result: &<Self::Backend as sqlx::Database>::QueryResult) -> u64;

    /// The names and engines of the tables of `tables` whose engine cannot
    /// roll back.
    fn rollback_less(sql: &mut Sql, tables: impl Iterator<Item = Bind>);

    /// The privileges of the connected user, as `privilege`, with each role
    /// it may take as `role <name>`.
    fn privileges(sql: &mut Sql);

    /// Ends an `INSERT`: a row whose key exists already takes `columns` from
    /// the row inserted instead.
    fn insert_or_update(sql: &mut Sql, columns: &[&'static str]);

    /// An `ORDER BY` term in a random order.
    fn random(sql: &mut Sql);

    /// An `ORDER BY` term in the random order `seed` repeats.
    fn seeded_random(sql: &mut Sql, seed: u64);

    /// An `ORDER BY` term: `column`'s position in `values`, 0 for none.
    fn list_order(sql: &mut Sql, column: &'static str, values: impl Iterator<Item = Bind>);

    /// The `part` of the date `date`.
    fn date_part(sql: &mut Sql, part: DatePart, date: &'static str);

    /// `_wp_mysql_week( date )`: the week of `date`, weeks starting on
    /// `start_of_week`.
    fn week(sql: &mut Sql, date: &'static str, start_of_week: i64);

    /// The condition that `date` written with the format `format` equals the
    /// number `time`, as `WP_Date_Query` compares a time.
    fn formatted_time(sql: &mut Sql, date: &'static str, format: String, time: String);

    /// The date `date` as text, `YYYY-MM-DD hh:mm:ss`.
    fn date_text(sql: &mut Sql, date: &'static str);

    /// The integer `value` writes, as a signed 64 bit integer.
    fn integer(sql: &mut Sql, value: impl FnOnce(&mut Sql));

    /// The condition that `column` does not match the regular expression
    /// `pattern`.
    fn not_matching(sql: &mut Sql, column: &'static str, pattern: String);
}

/// What a pool may do.
#[derive(Clone, Copy)]
pub(crate) enum Access {
    Read,
    Write,
    /// One writable connection that gives up on a lock another holds after
    /// ten seconds, for creating a site's tables (ADR 0021).
    Install,
}

/// `wpdb::set_sql_mode()`'s statement, as a literal, so another can follow it.
macro_rules! sql_mode {
    () => {
        "SET SESSION sql_mode = TRIM(BOTH ',' FROM \
         REGEXP_REPLACE(CONCAT(',', @@SESSION.sql_mode, ','), \
         ',(NO_ZERO_DATE|ONLY_FULL_GROUP_BY|STRICT_TRANS_TABLES|STRICT_ALL_TABLES|TRADITIONAL|ANSI)(?=,)', ''))"
    };
}

/// `wpdb::set_sql_mode()`: the session's SQL modes without those WordPress
/// does not work under, so values are written as PHP writes them.
pub(crate) const WORDPRESS_SQL_MODE: &str = sql_mode!();

/// The session of [`Access::Install`]: WordPress's SQL modes and a lock wait
/// limit, so a table another connection holds fails a drop instead of
/// waiting on.
const INSTALL_SESSION: &str = concat!(sql_mode!(), "; SET SESSION lock_wait_timeout = 10");

/// The connection a caller hands the writer to nest an action in its own
/// transaction (the write oracle).
pub type Connection = sqlx::MySqlConnection;

/// Turns MariaDB's snapshot isolation off for a writer's session where the
/// server has it, so an action writes over a row another client committed
/// since the action's first read, as PHP's statements do, instead of failing
/// with error 1020 (ADR 0012 amendment: the last save wins).
pub(crate) async fn last_save_wins(conn: &mut Connection) -> Result<(), sqlx::Error> {
    let show = "SHOW SESSION VARIABLES LIKE 'innodb_snapshot_isolation'";
    match conn.fetch_optional(show).await {
        Ok(Some(_)) => conn
            .execute("SET SESSION innodb_snapshot_isolation = OFF")
            .await
            .map(|_| ()),
        // MySQL and MariaDB before 10.6.18 have no such check.
        found => found.map(|_| ()),
    }
}

/// MySQL and MariaDB.
pub(crate) struct MySql;

impl Dialect for MySql {
    type Backend = sqlx::MySql;

    async fn connect(config: &Database, access: Access) -> Result<sqlx::MySqlPool, sqlx::Error> {
        let (user, password) = match (access, &config.read_user) {
            (Access::Read, Some(user)) => (user, &config.read_password),
            _ => (&config.user, &config.password),
        };
        let options = MySqlConnectOptions::new();
        let options = match &config.socket {
            Some(socket) => options.socket(socket),
            None => options.host(&config.host).port(config.port),
        };
        let options = options
            .database(&config.name)
            .username(user)
            .password(password.expose())
            .charset(&config.charset);
        MySqlPoolOptions::new()
            .max_connections(match access {
                Access::Install => 1,
                _ => 8,
            })
            // Report an unreachable database soon instead of retrying for 30s.
            .acquire_timeout(std::time::Duration::from_secs(5))
            .after_connect(move |conn, _| {
                Box::pin(async move {
                    // `raw_sql()`, which takes the install session's two statements.
                    let set = conn.execute(sqlx::raw_sql(match access {
                        Access::Read => "SET SESSION TRANSACTION READ ONLY",
                        Access::Write => WORDPRESS_SQL_MODE,
                        Access::Install => INSTALL_SESSION,
                    }));
                    let set = set.await;
                    match (set, access) {
                        (Ok(_), Access::Write) => last_save_wins(conn).await,
                        (set, _) => set.map(|_| ()),
                    }
                })
            })
            .connect_with(options)
            .await
    }

    fn inserted_id(result: &sqlx::mysql::MySqlQueryResult) -> u64 {
        result.last_insert_id()
    }

    fn rollback_less(sql: &mut Sql, tables: impl Iterator<Item = Bind>) {
        sql.dialect(
            "SELECT TABLE_NAME AS name, ENGINE AS engine FROM information_schema.TABLES \
             WHERE TABLE_SCHEMA = DATABASE() AND ENGINE <> 'InnoDB' AND TABLE_NAME IN (",
        );
        list(sql, tables);
    }

    fn privileges(sql: &mut Sql) {
        // `GRANTEE` is `'user'@'host'`; the host is what follows the last `@`.
        sql.dialect(
            "SELECT p.PRIVILEGE_TYPE AS privilege FROM (\
             SELECT GRANTEE, PRIVILEGE_TYPE FROM information_schema.USER_PRIVILEGES \
             UNION ALL SELECT GRANTEE, PRIVILEGE_TYPE FROM information_schema.SCHEMA_PRIVILEGES \
             UNION ALL SELECT GRANTEE, PRIVILEGE_TYPE FROM information_schema.TABLE_PRIVILEGES \
             UNION ALL SELECT GRANTEE, PRIVILEGE_TYPE FROM information_schema.COLUMN_PRIVILEGES\
             ) p WHERE p.GRANTEE = CONCAT('''', SUBSTRING(CURRENT_USER(), 1, \
             CHAR_LENGTH(CURRENT_USER()) - CHAR_LENGTH(SUBSTRING_INDEX(CURRENT_USER(), '@', -1)) - 1), \
             '''@''', SUBSTRING_INDEX(CURRENT_USER(), '@', -1), '''') \
             UNION SELECT CONCAT('role ', ROLE_NAME) FROM information_schema.APPLICABLE_ROLES \
             ORDER BY privilege",
        );
    }

    fn insert_or_update(sql: &mut Sql, columns: &[&'static str]) {
        sql.dialect(" ON DUPLICATE KEY UPDATE ");
        for (i, column) in columns.iter().enumerate() {
            sql.push(if i == 0 { "" } else { ", " });
            sql.push(column);
            sql.dialect(" = VALUES(");
            sql.push(column);
            sql.push(")");
        }
    }

    fn random(sql: &mut Sql) {
        sql.dialect("RAND()");
    }

    fn seeded_random(sql: &mut Sql, seed: u64) {
        // `WP_Query`'s `RAND(n)`: the sequence is MySQL's own.
        sql.dialect("RAND(?)");
        sql.bind(Bind::Int(seed));
    }

    fn list_order(sql: &mut Sql, column: &'static str, values: impl Iterator<Item = Bind>) {
        // PHP's own `FIELD()`, so ties sort as in its plan; a `CASE` gives the same positions.
        sql.dialect("FIELD(");
        sql.push(column);
        sql.push(", ");
        list(sql, values);
    }

    fn date_part(sql: &mut Sql, part: DatePart, date: &'static str) {
        sql.dialect(match part {
            DatePart::Year => "YEAR(",
            DatePart::Month => "MONTH(",
            DatePart::Day => "DAYOFMONTH(",
            DatePart::Hour => "HOUR(",
            DatePart::Minute => "MINUTE(",
            DatePart::Second => "SECOND(",
        });
        sql.push(date);
        sql.push(")");
    }

    fn week(sql: &mut Sql, date: &'static str, start_of_week: i64) {
        match start_of_week {
            1 => {
                sql.dialect("WEEK(");
                sql.push(date);
                sql.push(", 1)");
            }
            2..=6 => {
                sql.dialect("WEEK(DATE_SUB(");
                sql.push(date);
                sql.dialect(", INTERVAL ? DAY), 0)");
                sql.bind(Bind::Int(start_of_week.unsigned_abs()));
            }
            _ => {
                sql.dialect("WEEK(");
                sql.push(date);
                sql.push(", 0)");
            }
        }
    }

    fn formatted_time(sql: &mut Sql, date: &'static str, format: String, time: String) {
        sql.dialect("DATE_FORMAT(");
        sql.push(date);
        sql.dialect(", ?) = CAST(? AS DECIMAL(65, 6))");
        sql.bind(Bind::Text(format));
        sql.bind(Bind::Text(time));
    }

    fn date_text(sql: &mut Sql, date: &'static str) {
        sql.push("CAST(");
        sql.push(date);
        sql.dialect(" AS CHAR)");
    }

    fn integer(sql: &mut Sql, value: impl FnOnce(&mut Sql)) {
        sql.push("CAST(");
        value(sql);
        sql.dialect(" AS SIGNED)");
    }

    fn not_matching(sql: &mut Sql, column: &'static str, pattern: String) {
        sql.push(column);
        sql.dialect(" NOT REGEXP ?");
        sql.bind(Bind::Text(pattern));
    }
}

impl Sql {
    /// Appends a fragment only this module may write: engine specific SQL.
    fn dialect(&mut self, fragment: &'static str) {
        self.text.push_str(fragment);
        #[cfg(test)]
        self.portable.push_str("/* dialect */");
    }
}

# 0014: Portable SQL behind a database layer

- Status: Accepted in principle (2026-11-14); the dialect design below is Proposed
- Date: 2026-11-14

## Context

HyScribe reads and writes WordPress's MySQL or MariaDB database (ADR 0003). Decision: SQLite and PostgreSQL are not in scope now but must stay doable later. Today `hyscribe-db` builds its SQL for MySQL only: backtick quoting, `LIMIT ?, ?`, `FIELD()` and `RAND()` in orders, `CAST(... AS SIGNED)` and `CAST(... AS CHAR)`, and `sqlx`'s MySQL row and pool types. Each such construct makes a later engine a rewrite rather than an addition. At the same time, answers must match PHP WordPress on MySQL exactly, including the orders, collations and type conversions MySQL applies.

## Decision

- **One layer owns SQL.** Only `hyscribe-db` writes SQL, as it does now (no string-built SQL anywhere, ADR 0006); every other crate asks it through typed methods.
- **Portable by default.** Queries use standard SQL that MySQL, MariaDB, SQLite and PostgreSQL all accept: `LIMIT ? OFFSET ?`, standard `CAST`, `CASE` instead of `IF()` and `FIELD()`, unquoted or double quoted identifiers where the schema allows.
- **A `Dialect` for what cannot be portable.** Engine specific pieces (random order, collation sensitive comparisons, insert-or-update, identifier quoting, row and pool types) sit behind one `Dialect` trait in `hyscribe-db`, with MySQL and MariaDB as its only implementation for now. Adding an engine means adding a `Dialect`, not editing queries.
- **MySQL behaviour stays the contract.** Where WordPress's answers depend on MySQL semantics (case-insensitive collations, string to number conversion, tie order), the MySQL dialect keeps them exactly; the PHP goldens keep checking it. A later engine must reproduce them in its dialect or document the difference.
- **Guarded.** A test lists the SQL the layer emits and fails on MySQL-only syntax outside the MySQL dialect.

## The dialect

`hyscribe-db/src/dialect.rs` holds the `Dialect` trait and its one implementation, `MySql`. Queries are assembled from fixed fragments (`Sql::push`); only the dialect module can append an engine specific fragment, and it binds values, never request text. The MySQL dialect writes exactly the SQL the queries wrote before, so MySQL's answers are unchanged:

- **Row and pool types:** the `sqlx` driver (`Backend`), from which `Row` and the pool come, and `connect()`, which opens a pool: for reads it puts each connection in read only mode (`SET SESSION TRANSACTION READ ONLY`), as the read only user when there is one; for the writer (ADR 0010) it drops the SQL modes `wpdb::set_sql_mode()` drops, so values are stored as PHP stores them
- **Random orders:** `RAND()` and `RAND(?)`, whose sequence for a seed is MySQL's own
- **Explicit list orders:** `FIELD(column, ...)`, kept rather than a `CASE` so the statement, and with it the plan that orders ties, stays the one PHP runs
- **Date parts:** `YEAR()`, `MONTH()`, `DAYOFMONTH()`, `HOUR()`, `MINUTE()`, `SECOND()`
- **Weeks:** `_wp_mysql_week()`'s `WEEK(date, mode)` and `DATE_SUB(date, INTERVAL ? DAY)`
- **Time comparisons:** `WP_Date_Query`'s `DATE_FORMAT(date, ?) = CAST(? AS DECIMAL(65, 6))`
- **Casts:** a date as text (`CAST(... AS CHAR)`, `YYYY-MM-DD hh:mm:ss` with zero dates kept) and a signed integer (`CAST(... AS SIGNED)`)
- **Regular expressions:** `NOT REGEXP ?`
- **Insert-or-update:** `ON DUPLICATE KEY UPDATE column = VALUES(column)`, as `add_option()` and the term order of `wp_set_object_terms()` write it
- **Inserted IDs:** the ID an `INSERT` gave its row (`last_insert_id`)
- **Table engines:** the tables whose engine cannot roll back, from `information_schema.TABLES`, which the writer refuses (ADR 0012)

Everything else is standard SQL: `LIMIT ? OFFSET ?`, `COUNT(CASE WHEN ... THEN 1 END)` instead of summing comparisons, `? = 1` instead of a bound integer used as a condition, `CASE` instead of `GREATEST()`, and months ordered by their year and month instead of by a column the `DISTINCT` leaves out. No query used backticks, `IF()`, `BINARY`, `COLLATE` or `SQL_CALC_FOUND_ROWS`.

What a later dialect must also reproduce, because the queries rely on MySQL semantics without MySQL syntax: case and accent insensitive comparison under the site's collation (`=`, `IN`, `LIKE` and `ORDER BY` on text), the backslash as the `LIKE` escape (SQLite needs `ESCAPE`), the regular expression flavour of the patterns callers pass, and the row order MySQL gives statements without an `ORDER BY` or with ties.

The guard is `emitted_sql_is_portable` in `hyscribe-db`: every statement the crate's tests run, reads and writes, is recorded with each dialect fragment replaced by a marker, and the test fails on any MySQL or MariaDB only construct in what is left; `the_guard_finds_only_the_dialect` checks that each dialect piece is caught when written outside the dialect.

## Consequences

- The existing queries are moved to portable forms and the `Dialect` in one refactor, before phase 2 adds the write queries, with every golden unchanged
- No new engine is supported by this ADR; it only keeps the door open
- A small cost in indirection inside `hyscribe-db`; none outside it

# 0012: The admin's writes

- Status: Accepted (2026-11-14), with the amendments below
- Date: 2026-11-13

## Context

The MVP ([MVP.md](../MVP.md), phase 2) needs the admin to save posts, terms, comments, users and options. ADR 0010 gives HyScribe a writer that exists only in primary mode, with one reviewed method per ported write. The admin's actions write several rows each (a post with its meta, term relationships and counts), PHP WordPress takes no transactions, and PHP WordPress, WP-CLI or a cron job of another install may still write to the same database while HyScribe runs.

## Decision

- Every admin write goes through ADR 0010's writer: primary mode only, one method per ported WordPress function (`wp_insert_post()`, `wp_update_post()`, `wp_set_comment_status()` and so on), fixed statements with bound values, refused when `db_version` is newer than supported (ADR 0003)
- One action is one transaction: the rows an action writes are committed together or not at all. The rows at the end are the rows PHP leaves for the same action, so PHP WordPress reading them sees no difference
- Counts are recomputed with PHP's own queries (`_update_term_count_on_object_type()`, `wp_update_comment_count_now()`, `wp_update_term_count_now()`), never incremented, so HyScribe and a concurrent PHP writer converge on the same numbers
- No extra locking beyond what PHP does: concurrent edits behave as they do in PHP, the last save wins, and the editor's post lock is the `_edit_lock` meta PHP uses
- HyScribe keeps no cache of writable state across requests, so a change PHP makes is seen on the next request
- Writes need transactional tables: the setup wizard and the start of primary mode check the engine of the tables an action writes, and refuse writes to a table on any engine other than InnoDB (MyISAM, Aria, MEMORY) with a message saying how to convert it, rather than write without the all or nothing guarantee
- Every action checks its nonce and capabilities as PHP does before the transaction starts, and the write oracle ([MVP.md](../MVP.md), 2.3) compares each action's rows with PHP's before it ships

## Consequences

- A failed action leaves nothing half written, which PHP cannot promise; the end state of a successful one is still exactly PHP's
- Running PHP WordPress and HyScribe on one database at the same time is as safe as running two PHP servers on it, no more; if the PHP side uses a persistent object cache (Redis, Memcached), HyScribe's writes do not clear it and PHP may show stale data until it expires, so the wizard warns when it finds one
- Each ported write function is a reviewed method and a write oracle case, which keeps the write surface visible but makes porting slower than a general query builder would

## Amendment (2026-11-25): the last save wins on MariaDB 11.6.2 and later

A reported failure: `GET /hs-admin/` answered 500 with error 1020 ("Record has changed since last read in table 'wp_options'") while PHP WordPress loaded its dashboard on the same database. MariaDB 11.6.2 turned `innodb_snapshot_isolation` on by default. Under it, a `REPEATABLE READ` transaction that writes or locks a row whose newest version committed after the transaction's read view opened fails with 1020. Every action opens its read view with its first statement (the `db_version` check), so any row PHP commits during an action, such as the `cron` option both runtimes schedule events in on a dashboard load, failed the HyScribe action that wrote it next, even a row the action never read. MySQL and older MariaDB servers have no such check: the write applies to the newest row.

- Every writer connection turns `innodb_snapshot_isolation` off for its session where the server has the variable, so writes behave the same on every supported server: an action writes over a row another client committed meanwhile, the last save wins, as the decision above says
- The isolation level stays `REPEATABLE READ`, and the global setting is not touched; the reader and the installer's connections are unchanged
- Why this is safe: the bullet above already makes concurrent saves behave as in PHP, which takes no transactions and loads its options once at the start of a request, so what PHP writes back can be older than what HyScribe's action read. Turning the check off restores the behaviour of MySQL and of MariaDB before 11.6.2, which HyScribe already runs on; all or nothing commits, row locks and deadlock detection are unchanged
- No retry: rerunning an action would need every action body to be rerunnable, and some send mail, write files or call extensions before they commit. A deadlock (1213) or lock wait timeout (1205) still fails the action as before, rolled back whole; PHP's single statements hold row locks for milliseconds, and InnoDB rolls back the transaction that changed fewer rows, which is usually PHP's statement
- `SET SESSION TRANSACTION ISOLATION LEVEL READ COMMITTED` was not chosen: with a statement based binary log InnoDB refuses writes under it

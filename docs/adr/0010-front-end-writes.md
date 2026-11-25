# 0010: Front end writes go through a separate, gated writer

- Status: Accepted (2026-11-14), with the amendments below
- Date: 2026-10-28

## Context

Decision: HyScribe makes the database writes PHP WordPress makes while serving page views, starting with the `wp_navigation` post the navigation fallback creates. `hyscribe-db` is read only by design: every connection runs `SET SESSION TRANSACTION READ ONLY` and the crate has no write methods, so shadow mode cannot write (ARCHITECTURE.md: shadow never writes, primary writes). ADR 0003 limits writes to values PHP WordPress would write and refuses them when `db_version` is newer than supported.

## Decision

- Reads keep the read only pool. Writes use a second pool that exists only in `primary` mode; in `shadow` mode there is no writer at all, so a write is a no-op by construction, not by a check
- `hyscribe-db` gains one method per ported write (first: `insert_post` with the columns `wp_insert_post()` fills), each a fixed statement with bound values (ADR 0006). There is no general "execute" method
- The writer refuses every write when `db_version` is outside the supported range: newer than the newest supported version (ADR 0003), or older than the 7.0 floor
- `hyscribe-render` decides what to write, as the ported PHP does (the fallback post's title, slug, content and dates), and asks through write methods on the `Site` trait. Rendering never depends on whether the write happened: the fallback renders the same page list either way, as PHP does
- Write statements are portable SQL through ADR 0014's `Dialect`, like reads; insert-or-update is a dialect piece
- Write tests run against the fixture database inside a transaction that is rolled back, so the read tests always see the fixture as built, and each written row is compared with the row PHP writes for the same action

## Consequences

- A fresh HyScribe install in the default `shadow` mode still never writes
- Each new front end write is a reviewed, named method, visible in one place
- The writable connection uses the main database credentials. An optional read only database user in `.env` (`DB_READ_USER`, `DB_READ_PASSWORD`) serves every read, and everything in shadow mode, so a fault in the read path cannot write even in primary mode; without it, reads use the main user on a read only session as today

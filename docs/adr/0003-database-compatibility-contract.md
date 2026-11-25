# 0003: Database compatibility contract

- Status: Accepted (2026-10-26, accepted 2026-11-10)
- Date: 2026-10-26

## Context

The core promise is that an existing WordPress database works unchanged and remains usable by PHP WordPress at any time.

## Decision

- HyScribe never alters WordPress core tables or indexes
- All writes must be values PHP WordPress could have written for the same action, including PHP-serialized formats byte for byte
- HyScribe-only state goes in tables named `{prefix}hyscribe_*`, created only when needed and safe to drop
- Writes are refused when `db_version` is newer than the newest supported version
- First run defaults to shadow (read-only) mode

## Consequences

- Some data structures will be less convenient than a native Rust design would choose
- Every write path needs a write-compatibility test against PHP WordPress
- Switching back to PHP WordPress stays a supported, tested operation

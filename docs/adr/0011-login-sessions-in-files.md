# 0011: Login sessions in files

- Status: Accepted
- Date: 2026-11-12

## Context

HyScribe keeps its own login sessions, separate from PHP WordPress, and logging in needs primary mode. WordPress keeps sessions in the `session_tokens` user meta, which a separate HyScribe session cannot reuse, and ADR 0003 forbids writing data PHP WordPress would not write, so a HyScribe-owned meta key would break that contract.

## Decision

- Sessions live in files under the installation folder, `sessions/`, one file per user, holding only hashed tokens with their expiry, login time, IP and user agent, as `session_tokens` holds them
- The folder is `0700` and the files `0600`, as `run/` and `logs/` are; expired sessions are dropped when a user's file is next written
- The cookies have HyScribe's own names and are signed with the site's salts in WordPress's format, so they never collide with a PHP WordPress login on the same domain

## Consequences

- ADR 0003 holds: nothing in the database changes when a user logs in to HyScribe
- Sessions survive restarts; several HyScribe processes on one database would not share them, which the single binary deployment (ADR 0005) does not need
- A backup of the database does not carry HyScribe sessions; restoring one logs everyone out of HyScribe, which is safe

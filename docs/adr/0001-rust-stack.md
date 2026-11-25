# 0001: Rust stack

- Status: Accepted (2026-10-26, accepted 2026-11-10 without the template engine)
- Date: 2026-10-26

## Context

HyScribe must ship as a single binary, talk to MySQL/MariaDB with a table prefix only known at runtime, serve HTTP efficiently, and render the themes it serves.

## Decision

- tokio for async runtime
- axum with tower and tower-http for HTTP, middleware and static files
- sqlx for database access using runtime-built queries (compile-time checked macros cannot handle a dynamic prefix)
- clap for the CLI, tracing for logs and spans
- Static musl builds for Linux release binaries

## Consequences

- Mature, widely used crates with good async support
- No compile-time SQL verification; compensate with integration tests against fixture databases
- No template engine yet: block themes are rendered by the block port, and how classic themes are templated (M7.2) gets its own ADR

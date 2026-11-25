# Architecture decision records

One file per decision, numbered, never renumbered. Status is one of: Proposed, Accepted, Superseded by NNNN, Rejected.

| ADR | Title | Status |
| --- | --- | --- |
| [0001](0001-rust-stack.md) | Rust stack | Accepted |
| [0002](0002-wasm-extension-runtime.md) | WebAssembly components as the extension runtime | Accepted |
| [0003](0003-database-compatibility-contract.md) | Database compatibility contract | Accepted |
| [0004](0004-licensing.md) | Licensing (AGPL-3.0-or-later) | Accepted |
| [0005](0005-single-binary-distribution.md) | Single binary distribution | Accepted |
| [0006](0006-security-first.md) | Security is the first priority | Accepted |
| [0007](0007-full-test-coverage.md) | Full test coverage | Accepted |
| [0008](0008-hs-admin-emulates-wp-admin.md) | hs-admin emulates wp-admin | Accepted |
| [0009](0009-rendering-reads-through-a-site-trait.md) | Rendering reads site data through a trait | Accepted |
| [0010](0010-front-end-writes.md) | Front end writes go through a separate, gated writer | Accepted |
| [0011](0011-login-sessions-in-files.md) | Login sessions in files | Accepted |
| [0012](0012-admin-writes.md) | The admin's writes | Accepted |
| [0013](0013-rest-server.md) | The REST server | Accepted (2026-11-16) |
| [0014](0014-portable-sql.md) | Portable SQL behind a database layer | Accepted in principle |
| [0015](0015-image-decoding-and-resizing.md) | Image decoding and resizing for uploads | Accepted (2026-11-16) |
| [0016](0016-mail-transport.md) | The mail transport | Accepted (2026-11-16) |
| [0017](0017-cron-in-the-service.md) | WP-Cron in the service | Accepted (2026-11-16) |
| [0018](0018-login-rate-limits.md) | Login rate limits in memory | Accepted (2026-11-16) |
| [0019](0019-outbound-http-client.md) | The outbound HTTP client | Accepted |
| [0020](0020-non-image-uploads.md) | Non-image uploads | Proposed (scope accepted) |
| [0021](0021-new-site-install.md) | The new site install | Accepted |
| [0022](0022-extension-host.md) | The extension host | Accepted (2026-11-20) |
| [0023](0023-classic-themes.md) | The classic theme model | Accepted |
| [0024](0024-registry-and-package-signing.md) | The hosted registry and package signing | Accepted |

## Template

```markdown
# NNNN: Title

- Status: Proposed
- Date: YYYY-MM-DD

## Context

## Decision

## Consequences
```

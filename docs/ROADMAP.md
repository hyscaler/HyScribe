# Roadmap

Milestones are ordered so that something useful ships early: the wizard and compatibility report (M2) are valuable on their own before HyScribe can serve a single page. Each milestone has exit criteria that must be demonstrable, not just merged.

The work between today and the minimum viable product, a drop-in replacement for a default install, is planned in [MVP.md](MVP.md). Themes (M7) and plugins with the extension runtime (M6) follow the MVP there as phases 10 and 11.

Security is the first priority ([ADR 0006](adr/0006-security-first.md)). Every milestone exit additionally requires: an updated threat model, a security review of the milestone's code, all parsers added in the milestone fuzzed in CI, and no open critical or high findings. It also requires full test coverage of the milestone's code with no surviving mutants ([ADR 0007](adr/0007-full-test-coverage.md)).

## v0.1.0 (2026-11-26)

The MVP of [MVP.md](MVP.md) is met (2026-11-20, [ACCEPTANCE.md](ACCEPTANCE.md)): on the default WordPress install, HyScribe drops in against the same database, serves it from the release archive, writes to it with the same checks as PHP, and hands it back to PHP WordPress with no check failing. Work since then is phases 10 (themes) and 11 (plugins). The live checklist is [PROGRESS.md](../PROGRESS.md); what still answers 501 is in [operators/not-supported.md](operators/not-supported.md).

Legend: ✅ done, 🚧 in progress, 🔜 not started.

| Milestone | Status | Done | Left |
| --- | --- | --- | --- |
| MVP: a drop-in replacement for a default install | ✅ Done | Every criterion of [MVP.md](MVP.md) passes on the default install (2026-11-20) | |
| M0 Planning | ✅ Done | The planning documents, the ADRs, the licence (ADR 0004) | |
| M1 Foundations | 🚧 In progress | The workspace and its gates (100% coverage, mutation testing, supply chain checks, secret scanning, fuzzing), `.env` configuration, the service commands, reproducible release archives with checksums, an SBOM and build provenance, the database pool with any table prefix, the PHP serialize codec, the reference environment | Typed read models for the remaining core tables, reading every row of the default install from a live database, more fixtures (another prefix, latin1, multisite) |
| M2 Setup wizard and compatibility scanner | 🚧 In progress | The browser and terminal wizard for an existing site, the `wp-config.php` reader | The new site installer, the compatibility scan and its report, `hyscribe doctor` |
| M3 Read-only front end | 🚧 In progress | The default install's pages match PHP byte for byte (98 of 99 whole pages; the other is a bad preview nonce), with routing, feeds, sitemaps, core blocks, global styles and shadow mode | The same on more fixtures; classic themes come with M7 |
| M4 Authentication and users | 🚧 In progress | Passwords in every WordPress format, HyScribe's own sessions, login limits, login and logout, the lost and reset password forms, registration, roles and capabilities | The login actions still answering 501 |
| M5 Write path | 🚧 In progress | REST writes, the admin's actions, the block editor's and site editor's saves, the media library and uploads of every type PHP allows | PDF previews, some audio and video metadata, media trash and the other actions still answering 501 |
| M6 Extension SDK and runtime | 🚧 In progress | The sandboxed WebAssembly runtime (ADR 0022), SDK 0.1 to 0.3, the package format, installing, activating, deactivating and deleting plugins, signed packages and the hosted registry's client (ADR 0024) | The rest of the SDK (admin pages, the settings API, uploads, rewrite rules, plugin tables), the three reference plugins, the runtime's security review, the registry's keys and address |
| M7 Theme system | 🚧 In progress | The block themes Twenty Twenty-Two to Twenty-Five as data, switching them, child block themes, the classic theme model (ADR 0023), Hello Elementor's template tags and head | The rest of Hello Elementor, Twenty Twenty-One, installing themes, nav menus, widgets and the Customizer |
| M8 Agent porting pipeline | 🔜 Not started |  | All of it |
| M9 Breadth | 🚧 In progress | WP-Cron's core jobs in the service (ADR 0017) | Multisite, XML-RPC, translations, object and page caches |
| M10 1.0 hardening | 🔜 Not started | Signed reproducible releases with an SBOM and provenance exist from M1 | The external audit, a long fuzzing campaign, benchmarks, more platforms, the upgrade path, the documentation site |

## M0: Planning

- Planning documents, ADRs, design decisions made
- **Exit:** blocking design decisions made, ADRs 0001 to 0003 accepted, license chosen (done: ADR 0004)

## M1: Foundations

- Cargo workspace and CI (fmt, clippy, tests)
- Security baseline in CI: `cargo-deny` (advisories, AGPL-compatible licenses, sources), `cargo-audit`, `cargo-vet`, secret scanning, `#![forbid(unsafe_code)]` by default, fuzz targets for every parser from the first parser onward
- Initial threat model
- Test baseline in CI: `cargo-llvm-cov` gate at 100% lines and regions, `cargo-mutants` on changed code, `proptest`, `coverage-exclusions.md`
- `.env` config loading with environment overrides
- Self-managed service commands (`start`, `service start|status|log|restart|stop`)
- Release pipeline producing static musl archives and `SHA256SUMS.txt` from day one
- MySQL/MariaDB connection pool with dynamic table prefix
- PHP `serialize` / `unserialize` codec with round-trip fidelity
- Typed read models for core tables (posts, meta, terms, users, options, comments)
- Reference environment: containerized PHP WordPress + MariaDB with fixture sites
- **Exit:** read every row of the default install fixture (then the other fixtures) and re-serialize option and meta values byte-identically

## M2: Setup wizard and compatibility scanner

- First-run setup mode: browser wizard with one-time access secret (default) and terminal wizard
- New site path: `/hs-admin/install` with the same steps as WordPress's installer, creating the same database as WordPress for the same answers, plus `hyscribe/uploads/`
- Folder layout from [DEPLOYMENT.md](DEPLOYMENT.md#folder-layout) created on demand
- `wp-config.php` parser (DB credentials, `$table_prefix`, salts, `WP_HOME`/`WP_SITEURL`, multisite constants)
- Schema and environment probe (db_version, charset, multisite, active plugins, theme, permalinks)
- Compatibility report (terminal, JSON, HTML) backed by a registry file
- `hyscribe doctor` command to re-run the scan
- **Exit:** extract the archive, `./hyscribe service start`, complete the browser wizard against each fixture site; reports correct and stable; S4 met

## M3: Read-only front end

- HTTP server, static file serving for `wp-content`
- Rewrite and query engine: permalinks, archives, pagination, search, `?p=`, `?page_id=`, attachments, 404s, canonical redirects
- Template hierarchy resolution
- Content pipeline: block parser, wpautop, shortcode parser, embeds, excerpts
- Feeds (RSS2, Atom, comments), core sitemaps, `robots.txt`
- Internal hook bus (actions and filters) used by core itself
- Port of one default block theme
- Shadow mode (read-only, never writes)
- **Exit:** success criterion S1 on the fixture site in shadow mode

## M4: Authentication and users

- Password hash verification for all WordPress formats; hashing that PHP WordPress accepts
- Auth cookie compatibility using the site's salts, session tokens in usermeta
- Roles and capabilities from `{prefix}user_roles`, `map_meta_cap` semantics
- Login, logout, password reset, application passwords
- **Exit:** S2 met in both directions

## M5: Write path

- REST API `wp/v2` parity for posts, pages, media, users, terms, comments, settings
- hs-admin emulating `wp-admin` ([ADR 0008](adr/0008-hs-admin-emulates-wp-admin.md), [ADMIN.md](ADMIN.md)), screens compared against PHP `wp-admin`
- Block editor integration
- Media uploads with WordPress-compatible file naming, intermediate sizes and attachment metadata
- Comments, revisions, autosaves, trash, scheduled posts
- **Exit:** S3 and S7 met on the fixture sites

## M6: Extension SDK and runtime

- `hyscribe-sdk` crate with WordPress-shaped APIs
- Extension runtime per ADR 0002, capability manifest, install/activate/deactivate lifecycle
- Mapping of `active_plugins` to installed extensions
- Three first-party reference plugins ported by hand to prove the SDK
- **Exit:** reference plugins pass differential tests; SDK API reviewed and versioned 0.1

## M7: Theme system

- Block themes: `theme.json`, templates, template parts, patterns, global styles
- Classic theme model in Rust (template files, `functions` equivalent, menus, widgets, customizer mods)
- Child themes (`template` vs `stylesheet`)
- **Exit:** at least one block theme and one classic theme ported and passing differential tests

## M8: Agent porting pipeline

- Static analyzer for PHP plugins and themes (hooks, shortcodes, blocks, REST routes, options, tables, cron)
- Port spec format and agent workflow (analyze, plan, generate, test, fix)
- Differential test harness wired into the pipeline
- Public registry of ports with compatibility levels
- **Exit:** agents port the agreed pilot set with no hand-written code beyond review fixes

## M9: Breadth

- Multisite (subdomain and subdirectory)
- WP-Cron equivalent scheduler reading the `cron` option
- XML-RPC (at least for pingbacks and legacy clients)
- Internationalization: `.mo`/`.po` and JSON translations, `WPLANG`/`locale`
- Object cache and page cache integration
- **Exit:** multisite fixture passes S1 and S7

## M10: 1.0 hardening

- External security audit; long-running fuzzing campaign; signed reproducible releases with SBOM and provenance
- Performance benchmarks against PHP WordPress
- Packaging: `hyscribe-latest` archives for all 1.0 platforms, optional container image, boot autostart, upgrade path
- Documentation site
- **Exit:** all success criteria S0 to S7 met; release candidate cut

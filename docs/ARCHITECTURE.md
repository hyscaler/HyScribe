# Architecture (proposed)

This is a starting design for review. Decisions marked with an ADR are proposed until accepted.

## Shape of the system

```
                    +------------------------------------------------+
  HTTP request ---> | hyscribe (single binary)                       |
                    |                                                |
                    |  http: axum router, static files, middleware   |
                    |    |                                           |
                    |  routing: rewrite rules -> query vars -> query |
                    |    |                                           |
                    |  core: posts, terms, users, options, comments  |
                    |    |        ^                                  |
                    |    |        | hooks (actions / filters)        |
                    |    |        v                                  |
                    |  extensions: plugins + themes (WASM sandbox)   |
                    |    |                                           |
                    |  render: blocks, shortcodes, wpautop, templates|
                    |    |                                           |
                    |  db: sqlx pool, prefix-aware queries, PHP serde|
                    +----|-------------------------------------------+
                         v
              WordPress MySQL/MariaDB      hyscribe/ folder (plugins, themes, uploads, ...)
                                           + existing wp-content/uploads for existing sites
```

## Technology stack (ADR 0001)

| Concern | Choice | Notes |
| --- | --- | --- |
| Async runtime | tokio | |
| HTTP | axum + tower + tower-http | static files via `ServeDir`, compression, tracing |
| Database | sqlx (MySQL/MariaDB) | runtime-checked queries, because the table prefix is dynamic; standard SQL with engine specific pieces in one `Dialect` (ADR 0014) |
| Templates | none yet | block themes need none; classic themes (M7.2) get their own ADR |
| Extensions | wasmtime with the WebAssembly Component Model | see ADR 0002 |
| Terminal wizard | inquire or dialoguer, plus ratatui if needed | |
| Config | `.env` next to the binary (dotenvy) | real environment variables override it (ADR 0005) |
| Embedded assets | rust-embed | admin UI, `wp-includes` assets, default theme compiled into the binary |
| Images | `image` crate, optional libvips binding | must reproduce WordPress size naming |
| Observability | tracing, tracing-subscriber, OpenTelemetry export | |
| CLI | clap | `start`, `service start\|status\|log\|restart\|stop`, `doctor`, `ext`, `port` |

## Cargo workspace layout

```
crates/
  hyscribe/              binary: CLI entry point, wires everything together
  hyscribe-config/       .env model, wp-config.php parser
  hyscribe-write/        WordPress's core write functions over the writer (MVP 2.4)
  hyscribe-service/      self-managed background service (pidfile, logfile, signals)
  hyscribe-assets/       embedded wp-includes assets, admin build, default theme
  hyscribe-db/           pool, prefix handling, core table models, portable queries, SQL dialect (ADR 0014)
  hyscribe-phpser/       PHP serialize/unserialize with byte-exact round trip
  hyscribe-core/         domain logic: posts, terms, users, options, caps, cron
  hyscribe-hooks/        action/filter bus with priorities and accepted args
  hyscribe-routing/      rewrite rules, query vars, WP_Query equivalent
  hyscribe-render/       block parser, wpautop, shortcodes, kses, embeds, templates
  hyscribe-auth/         password hashes, auth cookies, nonces, app passwords
  hyscribe-rest/         wp-json namespace and wp/v2 endpoints
  hyscribe-media/        uploads, image sizes, attachment metadata
  hyscribe-wizard/       setup wizard and compatibility scanner
  hyscribe-ext-host/     extension runtime, manifests, capabilities, lifecycle
  hyscribe-sdk/          the crate extension authors depend on (guest side)
  hyscribe-testkit/      fixtures, differential test harness, HTML normalizer
themes/                  the block themes' data built into the binary, by WordPress slug
hs-admin/                hs-admin screens and assets emulating wp-admin (ADR 0008)
reference/               scripts that build the reference environment and fixtures
fixtures/                generated fixture dumps, starting with the default install
fuzz/                    cargo-fuzz targets, one per parser
tools/
  porter/                agent porting pipeline
  php-analyzer/          static analysis of PHP plugins and themes
registry/                compatibility registry data (slug -> port, level)
docs/
```

## Request lifecycle

1. axum receives the request; static paths are served directly: `/wp-content/plugins/<slug>/` and `/wp-content/themes/<slug>/` from the matching `hyscribe/plugins` or `hyscribe/themes` folder, `/wp-content/uploads/` from the uploads folder, `/wp-includes/` from embedded assets
2. Special endpoints are dispatched: `/wp-json/*`, `/wp-login.php`, `/wp-admin/*`, `/xmlrpc.php`, `/wp-cron.php`, feeds, sitemaps
3. Otherwise the rewrite engine matches the path against rules generated from `permalink_structure` and registered rewrites, producing query vars
4. The query engine (WP_Query equivalent) resolves posts, sets the conditional flags (`is_single`, `is_archive` and so on)
5. Template hierarchy picks a template from the active theme (child first, then parent)
6. Rendering runs the content pipeline with filters applied at the same points WordPress applies them
7. Response headers, canonical redirects, caching headers, then send

## Hooks as the central contract

WordPress behaviour is mostly defined by which filters run, in what order, with what arguments. HyScribe core must fire the same named hooks at the same points with equivalent arguments, so that ported plugins behave the same. A hook catalogue (generated from WordPress core source) is a planned artifact of M3, recording for each hook: name, type, arguments, call site, and whether HyScribe implements it.

## State HyScribe owns

- The `hyscribe/` folder: `.env`, `plugins/`, `themes/`, `languages/`, `uploads/` (new sites), `packages/`, `reports/`, `cache/`, `logs/`, `run/` (see [DEPLOYMENT.md](DEPLOYMENT.md#folder-layout))
- Optional tables named `{prefix}hyscribe_*` for state WordPress has no place for

Nothing else. See [COMPATIBILITY.md](COMPATIBILITY.md).

## Deployment modes

| Mode | Description |
| --- | --- |
| Shadow | Read-only. Never writes. Used to compare against a live PHP site |
| Primary | HyScribe serves the site and writes to the database |

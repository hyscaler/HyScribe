# Goals

## Vision

Why this matters (PHP2Shell, plugin vulnerability growth, agent-driven exploitation, PHP performance and operations) is set out in [docs/VISION.md](docs/VISION.md).

Any existing WordPress site can be served by HyScribe by downloading one Rust binary, pointing it at the site's database and `wp-content` directory, and answering a wizard. Content, URLs, users, media and settings keep working. The site gets faster, safer and cheaper to host, and the owner can return to PHP WordPress at any moment because HyScribe never breaks the data.

## Primary goals

### G0. Security first

Security is the top priority, ahead of every other goal ([ADR 0006](docs/adr/0006-security-first.md), [docs/SECURITY_MODEL.md](docs/SECURITY_MODEL.md)).

- Remove whole vulnerability classes by design: memory safety, no code execution from disk, no PHP object deserialization, typed parameterized queries
- Sandbox every plugin and theme with declared, deny-by-default capabilities
- Secure defaults: attack-prone WordPress behaviours (XML-RPC, pingbacks, user enumeration) off unless enabled
- Ports never reproduce the vulnerabilities of the plugins they replace
- Signed, reproducible releases and a vetted dependency tree
- Conflicts are settled in this order: security, data compatibility, behaviour compatibility, performance, convenience

### G0.1 Full test coverage

- The entire application ships with full test coverage, gated in CI from the first commit of code ([ADR 0007](docs/adr/0007-full-test-coverage.md))
- Mutation testing proves the tests catch real defects
- PHP WordPress is the oracle for compatibility tests

### G1. Drop-in replacement

- Serve an existing WordPress site from its existing database without migration
- Preserve every public URL: permalinks, archives, feeds, sitemaps, attachments, `?p=ID` style links
- Keep existing user logins, passwords and roles working
- Serve `wp-content/uploads` and theme assets unchanged

### G2. Single binary, wizard first

- Install like WordPress: download `hyscribe-latest.tar.gz`, extract, run `./hyscribe service start`, finish in the browser
- One statically linked binary per platform (Linux amd64 and arm64 first) with no runtime dependencies besides MySQL or MariaDB
- The binary runs itself as a service; Docker is optional, never required
- First run with no config starts a setup wizard (browser or terminal)
- Wizard collects database connection details, or reads them from an existing `wp-config.php`, and can also create a new site
- Wizard writes `.env` next to the binary and starts serving in the same process

### G3. Detect and report

- Inspect the database: WordPress schema version, table prefix, charset, multisite
- Inspect configuration: active plugins, active theme (and parent theme), widgets, permalink structure
- Produce a compatibility report with three states per item: **supported**, **partial**, **missing**
- For missing items, point to a registry entry, a porting path, or a known gap

### G4. Plugins and themes as Rust packages

- Plugins and themes are Rust crates built against a published HyScribe SDK
- The SDK mirrors WordPress concepts (actions, filters, shortcodes, blocks, REST routes, admin pages, options, cron) so ports map cleanly
- Extensions install without recompiling the core binary (see [docs/EXTENSIONS.md](docs/EXTENSIONS.md) for the proposed runtime)

### G5. Agent-driven porting

- Provide a repeatable pipeline where AI agents convert a WordPress plugin or theme (PHP) into a HyScribe extension (Rust)
- Every port is verified by differential tests against the original running on PHP WordPress
- A registry maps WordPress slugs to HyScribe ports and their compatibility level

### G6. 100% backwards compatibility with data

- Read any WordPress 7.0+ database
- Every write HyScribe makes must be readable by PHP WordPress with identical meaning
- No changes to WordPress core table schemas. HyScribe-only state lives in prefixed, separate tables or in options/meta that WordPress ignores
- Static files in `wp-content` are never moved or renamed; new uploads follow WordPress naming and metadata rules

### G7. Familiar admin

- hs-admin emulates `wp-admin` as closely as possible: same screens, menus, URLs (under `/hs-admin/`), markup and behaviour ([ADR 0008](docs/adr/0008-hs-admin-emulates-wp-admin.md))
- Anyone who can use WordPress can use HyScribe without retraining

## Secondary goals

- Significantly lower memory and latency than PHP WordPress with a comparable page cache disabled
- Observability built in: structured logs, metrics, request tracing
- Shadow mode: run HyScribe read-only beside PHP WordPress on the same database to compare output before switching

## Non-goals (for 1.0)

- Executing PHP code or arbitrary PHP plugins at runtime
- Browser code editors for plugins and themes, and installing PHP code from the admin
- Supporting WordPress versions older than 7.0 (older sites upgrade with PHP WordPress first)
- Hosting control panel features (DNS, email, TLS issuance beyond optional ACME)

## Success criteria

| ID | Criterion |
| --- | --- |
| S0 | Before 1.0: external security audit passed with no open critical or high findings, every parser fuzzed in CI, no `unsafe` outside the reviewed allowlist, all release blockers in the security model cleared |
| S0.1 | 100% line and region coverage across the whole application (branch coverage once supported on stable), no surviving mutants in security-critical crates, every route, hook and parser tested |
| S1 | A fresh WordPress install with default theme and Theme Unit Test data renders through HyScribe with zero differences after HTML normalization |
| S2 | A user created in PHP WordPress logs into HyScribe with the same password, and vice versa |
| S3 | A post and an image upload created in HyScribe appear correctly in PHP WordPress, including generated image sizes |
| S4 | The wizard goes from empty directory to running site in under 5 minutes for a single-site install |
| S5 | The compatibility report correctly classifies the active plugins of a sample of real sites |
| S6 | At least the agreed "top N" plugins have verified ports in the registry |
| S7 | Switching the same database back to PHP WordPress shows no data loss or corruption |
| S8 | Every hs-admin screen matches `wp-admin` on the default install fixture (normalized DOM and screenshot comparison) |

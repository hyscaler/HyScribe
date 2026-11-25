# 0021: The new site install

- Status: Accepted (scope decided 2026-11-19, criterion S4 of GOALS.md; creating tables in the wizard and pretty permalinks on first start accepted)
- Date: 2026-11-19

## Context

The setup wizard handled an existing WordPress database only. S4 asks it to go from an empty directory to a running site, and the wizard's plan has a new site path (docs/SETUP_WIZARD.md). A new site must stay openable by PHP WordPress (ADR 0003): the tables and rows must be the ones PHP's installer, `wp_install()`, leaves, so a person can switch to PHP the day after.

## Decision

### What is installed

Given a database with no table of the configured prefix, the install creates WordPress 7.1.3's twelve core tables and the rows `wp_install()` writes: the options of `populate_options()` and the roles of `populate_roles()`, the first administrator, the Uncategorized category, the Hello world post and its comment, the Sample Page, the draft Privacy Policy page, the widget and sidebar options, the cron events and Twenty Twenty-Five as the theme.

- **Copied from PHP, not rewritten.** `reference/install/golden.sh` runs `wp_install()` in the PHP oracle on an empty database, as `wp-admin/install.php` calls it, and writes what PHP made: each table's `SHOW CREATE TABLE` (`crates/hyscribe-write/src/install/schema.sql`) and every row with the values that depend on the answers or the clock masked as `%name%` (`rows.tsv`). HyScribe's install fills the masks in. The tables are PHP's byte for byte, `AUTO_INCREMENT` included, and no core table schema is defined or changed by HyScribe (ADR 0003)
- **Checked against PHP, not against itself.** Four more cases (`reference/install/cases.list`) are recorded with their answers as stored, not masked: a title with markup and quotes, a login with a dot, a space and an `@`, a site over https on another port with a path and public off, multibyte text, an empty title, a made password and a table prefix of its own. The test installs each into `wordpress_saves` and compares every row and every `SHOW CREATE TABLE` with PHP's
- **Pretty permalinks.** `wp_install_maybe_enable_pretty_permalinks()` asks the site itself, over HTTP, whether pretty permalinks work, and keeps them only if its own answer carries the `X-Pingback` header. HyScribe is going to serve the site, so the install writes what PHP writes when that probe succeeds, at the same moment, as part of the install: `permalink_structure` is `/%year%/%monthnum%/%day%/%postname%/` and `rewrite_rules` is the array `flush_rewrite_rules()` stores, nothing else changes (no marker is kept, and a site HyScribe did not install is never touched). The oracle answers the probe with the `X-Pingback` header for every recorded case, so `rows.tsv` and the case goldens carry both rows. The rules differ in one way with the site address: a home URL with a path has no `robots.txt`, `favicon.ico` and sitemap rules, so `golden.sh` also records the `subfolder` case's value (`rewrite_rules_subfolder.txt`) and the install uses it for an address with a path
- **Answers.** The same as PHP's install screen, with PHP's checks and messages (`Plan::new`): site title, user name (`sanitize_user( $name, true )` must leave it unchanged), password twice (an empty one is replaced by a made one of 12 characters, shown once, as PHP shows it), email, and whether to discourage search engines. PHP takes the site address from the request, so the wizard asks for it: only `http` or `https`, a host, a port and a path of letters, digits and `._~/-`, so no answer needs escaping where it reaches an option or a post
- **Where it runs.** Only in the setup wizard (the terminal, or the browser wizard behind its one-time secret), never from a route of the running site, and only in a database with no table of the prefix, whatever its case. A new site is written to, so it is always set up in primary mode, and the shadow question is not asked

### How it writes

- `hyscribe-db`'s `Installer` is the only place HyScribe creates tables. It runs the recorded `CREATE TABLE` statements, then inserts every row in one transaction with bound values. The only text put into a statement is the prefix and the table and column names of the recorded data, each checked to be letters, digits and underscores
- It has one connection, whose session gives up on a lock another connection holds after ten seconds (`lock_wait_timeout`), so a drop that something else blocks reports it instead of waiting on; MariaDB's default wait is a year. Like `Write`, it counts its statements against a budget tests spend (`fail_after`), so every error path is reached
- DDL cannot roll back, so a failure drops the tables this install made, and nothing else: while setting up, after the checks that follow the install, and when `.env` cannot be written. The error says whether they were dropped
- The password is hashed (`wp_hash_password()`, bcrypt over HMAC-SHA384) and made from the kernel's random source before any table exists

### What PHP does that this does not

- **The notification mail** (`wp_new_blog_notification()`): no mail is set up yet. The made password is shown on screen instead
- **Languages:** English only, as PHP's install of `en_US`; `WPLANG` is not written

### The test of S4

`the_new_site_wizard_reaches_a_home_page_within_s4_s_time` runs the browser wizard end to end against an empty database: setup mode serves its pages, installs the site, writes `.env`, and the same process answers `/` with 200 and the new site's title and first post, and the first post at its pretty URL, all inside the five minutes S4 sets (it takes a few seconds, most of it bcrypt).

## Consequences

- A new site is made in seconds, and PHP WordPress can open it
- The data files change only when the oracle's installer does: `reference/install/golden.sh` records them again, and the tests show the difference
- Another database engine needs its own recorded schema (ADR 0014)
- A site HyScribe installs has pretty permalinks and its rewrite rules from the first request, as a PHP site whose probe succeeds does, and its first post is at `/<year>/<month>/<day>/hello-world/`

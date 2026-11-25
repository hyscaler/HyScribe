# Setup wizard and compatibility scanner

## User experience

The wizard runs on the first start of the binary (see [DEPLOYMENT.md](DEPLOYMENT.md)). By default it runs in the browser behind a one-time access secret; the same steps are available in the terminal. The session below is the planned flow, with the compatibility scan (M2), not built yet; "Built so far" below says what runs today, and "A new site" shows its flow:

```
$ ./hyscribe service start
No .env found. Set up in the browser or in the terminal? [browser/terminal]  terminal

? Do you have an existing WordPress install on this machine?  Yes
? Path to the WordPress directory:  /var/www/example.com
  Found wp-config.php: database example_wp on localhost, prefix wp_
? Use these database settings?  Yes
  Connected to MariaDB 10.11, charset utf8mb4
  WordPress database version maps to WordPress 6.8
  Single site, permalinks /%postname%/

Scanning site...
  Theme      twentytwentyfive              supported
  Plugin     akismet/akismet.php           supported (port: hyscribe-akismet 0.3)
  Plugin     contact-form-7/wp-contact...  partial   (forms render; file uploads missing)
  Plugin     some-premium/plugin.php       missing   (no port; run: hyscribe port some-premium)

? Start in shadow mode (read-only) first?  Yes
Wrote /opt/hyscribe/.env
Service started, listening on http://127.0.0.1:8080  (shadow mode)
```

In the browser flow the binary gives a URL and secret instead, and every page redirects to `/installation/step-1` until setup completes (see "Setup mode, the browser wizard" below).

## Wizard steps

1. **Existing or new.** Existing WordPress site (continue below), or new site: ask for an empty database, then what PHP's install screen asks (site title, administrator user name, password, email, search engine visibility) and the site's address, and install WordPress 7.1.3 as `wp_install()` does ([ADR 0021](adr/0021-new-site-install.md)). Uploads go to `hyscribe/uploads/`. A new site stays openable by PHP WordPress.
2. **Locate install.** Ask for the WordPress directory, or skip if only a database is available.
3. **Read `wp-config.php`.** Parse without executing PHP: `DB_NAME`, `DB_USER`, `DB_PASSWORD`, `DB_HOST` (including port and socket forms), `DB_CHARSET`, `DB_COLLATE`, `$table_prefix`, the eight auth keys and salts, `WP_HOME`, `WP_SITEURL`, `WP_CONTENT_DIR`, `UPLOADS`, `MULTISITE` and related constants, and the capability constants `DISALLOW_UNFILTERED_HTML` and `ALLOW_UNFILTERED_UPLOADS`, written to `.env` under the same names when they are literal `true` or `false`. Values that depend on runtime PHP expressions are flagged and asked for manually.
4. **Manual entry fallback.** Host, port or socket, database, user, password, prefix, TLS options.
5. **Connect and probe.** Server version, charset and collation, privileges (can it write?), presence of core tables for the prefix.
6. **Identify WordPress.** `db_version` option, `siteurl` and `home`, multisite detection.
7. **Scan.** See below.
8. **Choose mode.** Shadow (default for first run) or primary.
9. **Network.** "Running HyScribe behind a proxy?" sets `BIND_HOST`, `PORT` and `BEHIND_PROXY`.
10. **Write config.** `hyscribe/.env`; secrets may instead be left to real environment variables.
11. **Start.** Print the URL and the report location.

## Built so far (MVP 8.4)

The wizard for an existing site or a new one, without the scan, in the terminal (`./hyscribe setup`) and in the browser (setup mode). Both ask the same steps through the same code. The scan is not built yet.

- **Where:** `start` or `service start` without `.env` (and without `DB_NAME` in the environment) asks "Set up in the browser or in the terminal? [browser/terminal]" when standard input is a terminal; browser is the default, and the only choice for a start that is not interactive. The terminal choice runs the terminal wizard, then starts the site
- **Existing `.env`:** the terminal wizard asks "Replace it? [y/N]" first; anything but yes keeps it and writes nothing. It refuses to run as root, like the server commands
- **Locate install:** the default answer is the first `wp-config.php` found in the installation's parent folder or a folder beside it. The answer may be the file, the WordPress folder, `none` for manual entry, or `new` for a new site (see "A new site" below). A folder is searched as `wp-load.php` searches: the folder, then the one above unless that holds `wp-settings.php`. The browser wizard also takes the contents of `wp-config.php` pasted into a form, read the same way
- **Read `wp-config.php`** (`hyscribe-config`'s `setup::import`, fuzz target `wpconfig_parse`): `DB_NAME`, `DB_USER`, `DB_PASSWORD`, `DB_HOST` split as `wpdb` splits it (a socket is copied as `localhost:/path` for `localhost`, the only host `mysqli` takes one for; a port outside 1 to 65535 is asked for instead), `DB_CHARSET` (`utf8` becomes `utf8mb4`, as `wpdb::determine_charset()` does), `$table_prefix`, the keys and salts (the sample's placeholder counts as unset), `WP_HOME`, `WP_SITEURL`, `DISALLOW_UNFILTERED_HTML`, `ALLOW_UNFILTERED_UPLOADS`, `MULTISITE`, `DISABLE_WP_CRON` and `ALTERNATE_WP_CRON` (ADR 0017) when they are literal `true` or `false`, and `EMPTY_TRASH_DAYS` when it is a literal whole number of days. A value PHP computes, or defines only under a condition, is listed and asked for; for the constants it is not copied and the safer value (for `EMPTY_TRASH_DAYS`, WordPress's 30) holds. `MULTISITE=true` is refused. `WP_CONTENT_DIR` locates `object-cache.php` and the uploads folder, and is asked for when PHP computes it
- **Manual entry:** database name, user, password, host, port and prefix. A Unix socket is given as the host, `localhost:/run/mysqld/mysqld.sock`, as `wp-config.php` writes it. TLS options are not offered: `hyscribe-db` connects without TLS for now
- **Keys and salts** not copied are generated, 64 characters each, as `wp_generate_password()` makes them
- **Mode:** "Start in shadow mode (read only) first? [Y/n]"; then an optional `DB_READ_USER` and its password
- **Network:** "Running HyScribe behind a proxy? [y/N]" (`BEHIND_PROXY`), the address to listen on (`BIND_HOST`, default `127.0.0.1`) and the port (`PORT`, default `8080`). The browser wizard offers the address it was opened on
- **Uploads:** `UPLOADS_DIR`, by default the folder a literal `UPLOADS` gives in the WordPress folder (`ABSPATH . UPLOADS`, as `wp_upload_dir()` takes it; pasted or computed, it is noted instead), else `wp-content/uploads`; an empty answer leaves media to `hyscribe/uploads`. A folder that does not exist yet is a warning
- **First run checks**, which only read (the new site's install, below, is the one thing the wizard writes to the database): both users connect; `db_version` is in the supported range (older sites are told to upgrade with PHP WordPress first); `DB_READ_USER`, when given, has `SELECT` and nothing more, roles included; in primary mode, every table the writer writes is InnoDB (ADR 0012), while in shadow mode the tables that would be refused are listed as warnings; a `wp-content/object-cache.php` drop-in gives a warning that HyScribe's writes do not clear a persistent object cache
- **Write `.env`:** values double quoted so any text reads back; written to `.env.new` with mode `0600`, then renamed into place, or hard linked when nothing may be replaced, so an `.env` that appeared meanwhile is never overwritten (the browser wizard never replaces one)
- Passwords are read with the terminal's echo off; no password, key or salt is printed

### A new site

Answer `new` to the path question (the terminal), or tick "This is a new site" at step 2 (the browser). It is the only time the wizard writes to the database, and it does so only when the database has no table of the prefix: otherwise it stops and names them ("choose another database or prefix, or set up the existing site instead"), and changes nothing.

```
? Path to the WordPress folder or its wp-config.php (none if there is none, new for a new site) [...]: new
? Database name: example_wp
  ... user, password, host, port and table prefix, as for manual entry
? Database user granted only SELECT, for every read (empty for none):
? Running HyScribe behind a proxy? [y/N]:
? Address to listen on [127.0.0.1]:
? Port to listen on [8080]:
? Site title: My Site
? Administrator user name: owner
? Administrator password (empty for a generated one):
? Password again:
? Administrator email address: owner@example.org
? Discourage search engines from indexing this site? [y/N]:
? Site address, where visitors will open it [http://127.0.0.1:8080]:
? The site's uploads folder (empty for hyscribe/uploads):
  Keys and salts: 0 copied, 8 generated
  Installed WordPress 7.1.3's 12 tables and first content in example_wp
  Connected to example_wp on localhost:3306, db_version 61833
```

- **Questions:** PHP's install screen's, with its checks and messages (a user name `sanitize_user( $name, true )` leaves unchanged, matching passwords, a valid email); a refused set is asked again, three times at most. The site address is asked because PHP takes it from the request: `http` or `https`, a host, an optional port and path. The shadow mode question is not asked: a new site is written to, so it is in primary mode. An empty password becomes a made one of 12 characters, printed once in the report (the terminal's, or the last page of the browser wizard), as PHP shows it
- **What it makes:** the twelve core tables as `SHOW CREATE TABLE` prints them for PHP's, and the rows of `wp_install()`: the options of `populate_options()`, the roles, the administrator, the Uncategorized category, the Hello world post and comment, the Sample Page, the draft Privacy Policy page, the widgets, the cron events and Twenty Twenty-Five as the theme. As PHP leaves a site whose pretty permalink probe succeeds, it also sets `/%year%/%monthnum%/%day%/%postname%/` and the rewrite rules. Difference from PHP: no notification mail
- **If something fails after the tables are made** (the checks that follow, or writing `.env`), the tables are dropped again and the error says so
- **Checked** against PHP by `reference/install/golden.sh` (five cases, every row and every `SHOW CREATE TABLE`), and by a timed test of S4 that runs the browser wizard from an empty database to a home page that answers 200

### Setup mode, the browser wizard

- **Address:** setup mode listens where the site would, on `BIND_HOST` and `PORT` from the environment, by default `127.0.0.1:8080`. On a remote server, use an SSH tunnel, or start once with `BIND_HOST=0.0.0.0`
- **Secret:** at start it gives the address and a one-time setup secret, 32 letters and digits drawn from the kernel. When a terminal runs the command (`start` with standard output a terminal, or `service start` at one) it is printed there. Otherwise it is written to its own file, `logs/setup-secret` (mode `0600`, in the `0700` `logs/` folder), and the log only says where; setup mode deletes the file when it ends, by finishing, a signal, an error or its timeout, and the next start removes one a killed server left. The secret never reaches `logs/hyscribe.log`. The secret is typed into the first page's form, never put in a URL, so it stays out of browser history, proxies' logs and `Referer` headers. It works once: it then gives that browser a session cookie (`HttpOnly`, `SameSite=Strict`, path `/installation`), and no other browser can use it. A lost session needs a restart, which gives a new secret
- **Pages:** `/installation/step-1` (the secret), `step-2` (the WordPress folder, the pasted `wp-config.php`, neither, or a new site) and `step-3` (what `wp-config.php` did not give, the mode, the read user, the network and the uploads folder, then the checks and `.env`; for a new site the database details and PHP's install questions in place of the mode). Every other path redirects to step 1, or to step 2 once signed in
- **Forms:** each carries a token compared in constant time: step 1's must also be in its cookie, the others' is the session's. Bodies are URL encoded forms of at most 32 fields and about 3 MiB (fuzz target `setup_form`); anything else is refused
- **Nothing secret is sent back:** answers are held on the server; password fields are always empty, and a page shown again after a failed check repeats only the other answers
- **Headers:** `Cache-Control: no-store`, `Referrer-Policy: no-referrer`, `X-Content-Type-Options: nosniff`, and a Content Security Policy with no script and no outside resource, `frame-ancestors 'none'` and `form-action 'self'`
- **Timeout:** setup mode stops after 30 minutes without `.env`, and the process exits with an error; a new start gives a new secret
- **After setup:** once `.env` is written, the last page shows the checks' report, setup mode stops, and the same process loads `.env` and serves the site on the address it sets. SIGTERM or SIGINT stops setup mode as it stops the site

## What the scanner checks

| Item | Source |
| --- | --- |
| Active plugins | `active_plugins` option; `active_sitewide_plugins` site meta for multisite |
| Installed plugins | Directory listing of `wp-content/plugins`, plugin header parsing (including `License`) |
| Must-use plugins and drop-ins | `wp-content/mu-plugins`, `object-cache.php`, `advanced-cache.php`, `db.php` |
| Theme | `template` and `stylesheet` options; `style.css` headers; block theme if `theme.json` and `templates/` exist |
| Widgets and menus | `sidebars_widgets`, `widget_*` options, `nav_menu` terms and `nav_menu_locations` theme mod |
| Content features | Shortcodes and block names actually used in `post_content` (sampled or full scan) |
| Custom post types and taxonomies | Distinct `post_type` and `taxonomy` values, mapped to the plugin or theme that registers them |
| Plugin tables | Tables with the prefix that are not core tables |
| Cron | Hooks scheduled in the `cron` option |
| Permalinks and rewrites | `permalink_structure`, `category_base`, `tag_base`, `rewrite_rules` |

Scanning content for used shortcodes and blocks matters because a plugin that is active but unused is less important than one whose shortcode appears in 400 posts. The report ranks missing items by impact.

## Classification

| State | Meaning |
| --- | --- |
| supported | A registry port exists at full compatibility, or core HyScribe covers it |
| partial | A port exists with documented gaps; gaps listed in the report |
| missing | No port. Report shows impact (posts affected, URLs affected) and the porting command |
| not needed | Plugins whose function HyScribe provides natively (for example, caching or security plugins) |
| license blocked | No port possible without relicensing: the plugin is GPLv2-only or not GPL (see ADR 0004) |

## Outputs

- Terminal summary
- `hyscribe/reports/<date>-report.json` (machine readable, stable schema, for CI and the porting pipeline)
- `hyscribe/reports/<date>-report.html` (shareable)

## Safety

- The wizard and scanner never write to the database, except the new site's install into a database with no table of its prefix (ADR 0021)
- Credentials are only written to `.env` with file mode `0600`, or kept in environment variables if the user chooses
- The browser wizard requires the one-time secret given on start, and holds answers on the server, never in the browser

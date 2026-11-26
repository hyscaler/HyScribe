# Deployment

HyScribe installs the way WordPress does: download an archive, extract it, open a browser. The difference is that there is no web server, PHP or extensions to install first. The archive holds one binary, and that binary runs itself as a service.

## Quickstart

```bash
curl -fsSLO https://github.com/hyscaler/HyScribe/releases/latest/download/hyscribe-latest-linux-amd64.tar.gz
tar -xzf hyscribe-latest-linux-amd64.tar.gz && cd hyscribe
./hyscribe service start      # asks: browser wizard or terminal wizard
```

On arm64 take `hyscribe-latest-linux-arm64.tar.gz`; on macOS, from the release after v0.1.0, `hyscribe-latest-darwin-arm64.tar.gz` (Apple Silicon) or `hyscribe-latest-darwin-amd64.tar.gz` (Intel), and read [macOS](#macos) first.

With the browser wizard, the binary prints a URL and a one-time access secret. Open the URL, enter the secret, and follow the steps. When setup finishes the same process starts serving the site.

### A new site

With no WordPress site to take over, the wizard can make one. Create an empty database and a user with all privileges on it (MariaDB or MySQL), then at step 2 of the browser wizard tick "This is a new site", or answer `new` to the terminal wizard's first question. It asks for the database details, the site title, the first administrator (user name, password, email), whether to discourage search engines, and the site's address, and installs WordPress 7.1.3 as PHP's installer does ([ADR 0021](adr/0021-new-site-install.md)): the twelve core tables, the Hello world post and Sample Page, the Twenty Twenty-Five theme. The site starts in primary mode, since a new site is written to, and PHP WordPress can open the same database.

- The database must have no table with the prefix: the install stops and names them otherwise, and changes nothing. Use another database or prefix for a new site beside an old one
- An empty password makes one of 12 characters, shown once on the last page or in the terminal
- Plain permalinks (`?p=1`) are set, as PHP sets them when its own probe cannot reach the site; choose a structure in Settings, Permalinks. No notification mail is sent
- If a later check fails, the tables made are dropped again, so the same wizard can be run again

## Release artifacts

| Artifact | Contents |
| --- | --- |
| `hyscribe_<version>_<os>_<arch>.tar.gz` (`.zip` on Windows) | `hyscribe/` folder with the binary, `LICENSE`, `THIRD-PARTY-NOTICES`, `README.txt`, `.env.example` and the operator documentation in `docs/` |
| `hyscribe-latest-<os>-<arch>.tar.gz` | Stable alias of the newest release, the equivalent of WordPress `latest.tar.gz` |
| `SHA256SUMS.txt` | Checksums for every archive |
| `hyscribe-sbom.cdx.json`, `hyscribe-sbom-darwin.cdx.json` | CycloneDX SBOMs of the Linux and the macOS binaries, which link a few different crates |
| Container image (optional) | Same binary on a minimal base, for operators who prefer Docker |

`LICENSE` ships in every archive because the AGPL requires a copy of the license to accompany each distribution. `THIRD-PARTY-NOTICES`, written by `scripts/third-party-notices.sh` when the archive is built, holds the licences of what the binary embeds: WordPress's files and its themes, with where their source is, and every Rust crate it links, with the licence files each ships.

### Platforms

- Linux amd64 and arm64 (`linux_amd64`, `linux_arm64`), statically linked with musl so it runs on any distribution
- macOS 11 or later, Apple Silicon and Intel (`darwin_arm64`, `darwin_amd64`), from the release after v0.1.0: see below
- Later: Windows amd64

### macOS

The macOS archives hold the same files as the Linux ones: the plain binary in a `tar.gz`, with no installer, disk image or app bundle. They are built on macOS for each release, never cross-compiled, and each is installed and started by the release's smoke test before it is published.

- **Not signed by Apple.** The binaries carry no Developer ID signature and are not notarised yet; the linker's ad hoc signature is all they have. A copy fetched with `curl`, as in the quickstart, is not marked as downloaded and starts like any other program. A copy downloaded with a browser is quarantined, and `tar` passes that mark on to the files it unpacks; macOS then refuses to start the binary, saying it cannot verify it. Remove the mark with `xattr -d com.apple.quarantine hyscribe/hyscribe`, or try to start it once and then allow it under System Settings, Privacy & Security ("Open Anyway"). Verify the download first with `SHA256SUMS.txt` and `gh attestation verify`: until the binaries are signed, that is what says the file is the project's
- **Linked against the system.** A macOS binary cannot be fully static: it uses two libraries every macOS has (`libSystem` and `libiconv`) and nothing that has to be installed
- **Certificates.** Outbound HTTPS and the mail relay's TLS are checked against the bundle macOS ships in `/etc/ssl/cert.pem`. Certificates added to the Keychain are not read
- **Starting at boot.** As on Linux the service does not start at boot by itself. macOS's own way is a launchd job, which HyScribe neither writes nor documents yet
- **What is tested.** The release's smoke test (install, `service start`, `status`, a page of the default install, `stop`) runs for both architectures on macOS 15, the Intel binary under Rosetta on an Apple Silicon machine, and CI runs the tests that need no database there, for both architectures the same way. The full suite (the PHP and MariaDB oracle, coverage, the switch-back suite, screenshots, fuzzing) runs on Linux only, and no macOS release older than 15 is tested, so treat macOS as a platform for trying HyScribe and for development rather than for hosting

## What is inside the binary

Everything needed to serve a site, embedded at build time:

- HTTP server, setup wizard (terminal and browser), admin interface
- WordPress `wp-includes` scripts, styles and block library assets that themes and content reference
- A default block theme, so a fresh site renders without downloading anything
- Extension runtime (wasmtime): planned, M6

Runtime dependencies: none, other than a reachable MySQL or MariaDB server.

## Folder layout

The `hyscribe/` folder is the whole installation, just as the WordPress folder is for PHP WordPress. Everything HyScribe installs, generates or writes lives inside it, in a folder named for what it holds. The binary only creates these folders when first needed.

What the archive holds and the binary creates today:

```
hyscribe/
  hyscribe                    the binary
  LICENSE
  THIRD-PARTY-NOTICES         licences of the embedded WordPress files, themes and crates
  README.txt
  .env                        configuration, written by the wizard (mode 0600)
  .env.example                every configuration key, documented
  docs/                       the operator documentation (docs/operators/ in the repository)
  logs/                       hyscribe.log and rotated logs; setup-secret while setup mode runs without a terminal
  sessions/                   login sessions, one file per user (ADR 0011)
  run/                        hyscribe.pid
```

Planned, each created by the feature that needs it once that feature is built:

```
hyscribe/
  plugins/                    installed plugins, one folder per WordPress slug (M6)
    contact-form-7/
      hyscribe.ext.toml       manifest
      extension.wasm          the component, compiled in memory when loaded (ADR 0022)
      public/                 css, js, images (served at /wp-content/plugins/contact-form-7/)
      templates/
      languages/
      faults.txt              the extension's last faults and whether it is disabled (ADR 0022); absent while it has none
      approved.ext.toml       the manifest as approved at activation (ADR 0022); absent while it is not active
      origin.txt              who signed the package and its SHA-256, and a revocation once the registry revokes it (ADR 0024)
  themes/                     installed themes, one folder per WordPress slug (M6, M7)
    twentytwentyfive/
      hyscribe.ext.toml
      extension.wasm          only if the theme has logic
      theme.json
      templates/  parts/  patterns/
      public/                 style.css, fonts, images (served at /wp-content/themes/twentytwentyfive/)
  languages/                  core and site translations (.mo, .json) (M9)
  uploads/                    media for new sites (served at /wp-content/uploads/) (M5, MVP phase 6)
  packages/                   installed .hyx archives kept for rollback, each with its .minisig signature when it had one (ADR 0024)
  registry/                   the hosted registry's key list and index, verified, with their signatures and the highest serials accepted in state (ADR 0024)
  reports/                    compatibility reports from the wizard and `doctor` (M2)
  cache/                      page and object cache, safe to delete (M9)
```

Naming rules:

- Plugin and theme folders use the WordPress slug they replace (`contact-form-7`, not `hyscribe-contact-form`), so `active_plugins`, `template` and `stylesheet` resolve to folders the same way they do in WordPress
- Public URLs keep their WordPress paths (`/wp-content/plugins/<slug>/...`, `/wp-content/themes/<slug>/...`, `/wp-content/uploads/...`), so links already stored in post content keep working; the folders on disk are simply named without the `wp-content` prefix

### Existing sites

Extract HyScribe anywhere (next to the WordPress folder is simplest). Plugins, themes, translations, cache, logs and reports all live in `hyscribe/`. The only thing left in place is the existing media library: `.env` records the path to the existing `wp-content/uploads`, because moving it would break the switch back to PHP WordPress. Theme files not shipped in a theme package (for example images in an untouched child theme) fall back to the existing `wp-content/themes/<slug>/`.

### New sites

Everything, including `uploads/`, is in `hyscribe/`. The wizard records `upload_path` so PHP WordPress can find the media if the site ever moves to it.

## Configuration

- `.env` in the binary's folder, written by the wizard; `.env.example` documents every key
- Real environment variables always win over `.env`, so containers and CI can inject settings
- Main keys: database connection (`DB_HOST` may name a Unix socket as `wp-config.php` does, `localhost:/run/mysqld/mysqld.sock`, and `DB_PORT` is then not used; only `localhost` takes a socket, as for PHP's `mysqli`), table prefix, `DB_READ_USER` and `DB_READ_PASSWORD` (optional: a database user granted only `SELECT`, which then serves every read and all of shadow mode, so a fault in the read path cannot write even in primary mode; without it reads use the main user on a read only session, ADR 0010), `UPLOADS_DIR` (existing sites only; the wizard takes it from a literal `UPLOADS` in `wp-config.php`, relative to the WordPress folder as `wp_upload_dir()` takes it), auth keys and salts (read from `wp-config.php` or generated), `BIND_HOST`, `PORT`, `BEHIND_PROXY`, `TRUSTED_PROXIES` (the peers `X-Forwarded-For` is believed from, loopback by default), `REQUEST_TIME_LIMIT` (seconds a request may run before it answers 503 and its unfinished write rolls back, 30 by default), `REGISTRATION_LIMIT_ADDRESS` (accounts per client address and hour by registration, 3 by default, `-1` for no cap), `MODE` (`shadow` or `primary`), `WP_HOME` and `WP_SITEURL` (override the stored home and site addresses, as the WordPress constants do), `REST_PUBLIC_USERS` (`on` by default; `off` answers visitors of the public users REST endpoint as a site where no user has published posts), `DISALLOW_UNFILTERED_HTML`, `ALLOW_UNFILTERED_UPLOADS` and `MULTISITE` (the capability constants, see below), `LOGIN_LIMIT_ADDRESS`, `LOGIN_LIMIT_ACCOUNT` and `LOGIN_LIMIT_WINDOW` (the login rate limits, see below), `EMPTY_TRASH_DAYS` (WordPress's constant: 30 by default, 0 deletes posts and comments without the trash, and hs-admin and the REST API then offer only permanent deletes, as WordPress does; the wizard copies a literal number from `wp-config.php`), `UPLOAD_MAX_SIZE` (the upload limit, see below), `SMTP_HOST`, `SMTP_PORT`, `SMTP_TLS`, `SMTP_USER`, `SMTP_PASSWORD` and `MAIL_FROM` (the mail relay, see below)
- Login rate limits (docs/SECURITY_MODEL.md, "Login rate limits"): `LOGIN_LIMIT_ADDRESS` (default 5) wrong guesses from one client address within `LOGIN_LIMIT_WINDOW` seconds (default 900) refuse that address for the window, doubled for each refusal of the day before, up to a day; `LOGIN_LIMIT_ACCOUNT` (default 10) wrong guesses at one account in the window refuse it for the window, except from the last 8 addresses it logged in from since the start; one address checks one password at a time. The limits are whole numbers from 1 to 1000, the window from 1 to 86400; another value stops the start. Behind a proxy, set `BEHIND_PROXY=true`, or every client counts as the proxy's address
- Upload limit: `UPLOAD_MAX_SIZE`, 64 MB by default, takes the place of PHP's `upload_max_filesize` and `post_max_size`. It is a whole number of bytes, or one with a `K`, `M` or `G` suffix (either case) as `php.ini` writes it (`512K`, `64M`, `2G`), at least one byte; any other value stops the start. hs-admin prints it wherever WordPress prints `wp_max_upload_size()`: the media uploader and templates (Add New Media, the media library, the site icon on Settings > General), plupload's `max_file_size`, and Site Health's Info tab. The upload handler refuses larger files
- Mail (ADR 0016): `SMTP_HOST` names the relay every message goes to, over SMTP; unset, no mail is sent, each message is logged as not sent and the start log says mail is off. `SMTP_TLS` is `starttls` (the default: STARTTLS required, never a plain text fallback), `tls` (TLS from the first byte) or `none` (plain text, only to `localhost`, `127.0.0.1` or `::1`); `SMTP_PORT` defaults to 587, 465 or 25 by that mode. `SMTP_USER` and `SMTP_PASSWORD` log in with SMTP AUTH and are refused with `none`. The relay's certificate is checked against the host's certificate store and must match `SMTP_HOST`. `MAIL_FROM` is the sender address of every message, WordPress's default sender (`wordpress@` and the site's host without `www.`) when unset; a `From` header or plugin only sets the name shown. HyScribe's own limits: 3 password reset mails per account and 10 per client address in an hour, and 60 messages a minute in all, the rest waiting in a bounded queue. A send failure never fails the request; it is logged with the message's kind and the recipient's domain, never its subject, body or address
- Capability constants: the `wp-config.php` constants of the same names, which the setup wizard copies from a literal `define()`. Each takes `true`, `yes`, `on` or `1`, or `false`, `no`, `off` or `0`, in any case, as every yes or no key does (`BEHIND_PROXY`, `REST_PUBLIC_USERS`, `HTTP_OUTBOUND`); any other value stops the start with an error. A missing or empty key takes the safer value: `DISALLOW_UNFILTERED_HTML=true` (no one may post unfiltered HTML, where a default WordPress install lets administrators and editors), `ALLOW_UNFILTERED_UPLOADS=false` and `MULTISITE=false`. `MULTISITE=true` is refused at start, since multisite networks are not supported (M9). `./hyscribe setup` copies them; a value PHP computes is not copied, so the safer value holds until the owner sets it
- WP-Cron (ADR 0017): `DISABLE_WP_CRON` and `ALTERNATE_WP_CRON`, yes or no keys as the capability constants and copied the same way, both false by default. In primary mode the service runs the due events of the six core hooks HyScribe ports once a minute, behind PHP's `doing_cron` lock, unless `DISABLE_WP_CRON=true`; then `./hyscribe cron run` from the system's cron runs them. Cron is off in shadow mode, and when `object-cache.php` sits in the `wp-content` folder above `UPLOADS_DIR`. `ALTERNATE_WP_CRON` is ignored with a line in the log
- Image limits (ADR 0015): `IMAGE_MAX_BYTES` (64 MB), `IMAGE_MAX_WIDTH` and `IMAGE_MAX_HEIGHT` (32000 pixels), `IMAGE_MAX_AREA` (256000000 pixels) and `IMAGE_MAX_MEMORY` (1 GB), the PHP oracle's ImageMagick policy by default. Sizes in bytes take a `K`, `M` or `G` suffix as `php.ini` writes them; an image over a limit is kept with no sizes. Uploads are written to `UPLOADS_DIR`, or `uploads/` in the installation's folder for a new site, and served at `/wp-content/uploads/`
- Extension packages (ADR 0024): `hyscribe plugin install <file>` takes a package whose signature `<file>.minisig` verifies with a key of the hosted registry's key list kept in `registry/` (`keys.json` and `keys.json.<n>.minisig`, and `index.json` with `index.json.minisig` for revocations, copied there from the registry's `v1/` folder) or with a key in `EXTENSION_TRUSTED_KEYS`: minisign public keys an operator trusts for packages they build themselves, comma separated, read from `.env` or the environment by the command; a key that does not read refuses the install. Any other package installs only with `--unsigned`. The registry's root keys are compiled into the binary; until the registry's keys exist the release carries none, so only `EXTENSION_TRUSTED_KEYS` and `--unsigned` install anything
- The hosted registry (ADR 0024): `EXTENSION_REGISTRY`, the registry's base URL, `https:` only (an `http:` URL stops the start); not set by default until the project's registry has its address. `hyscribe plugin install <name>[@<version>]` fetches the key list and index (kept verified in `registry/`, applying their revocations), the extension's version document and the newest compatible version's package (to `packages/.download-<id>-<version>`, removed after), checks its size, SHA-256 and signature, and installs it with `origin.txt` saying `registry`; `--downgrade` allows an older version, `--allow-new-grants` other grants or limits than the approved ones, `--replace-local` replacing a plugin installed from a local file. `hyscribe plugin update --check` lists the updates and revocations, `hyscribe plugin update <id>...` or `--all` installs them (same flags but `--replace-local`). These commands fetch from the registry's host only, whatever `HTTP_OUTBOUND` says, and send nothing about the site. With `EXTENSION_UPDATE_CHECK=on` (and `HTTP_OUTBOUND=on`) the service itself fetches the key list and index at its start and every 12 hours, applies their revocations and logs the updates, installing nothing; the Plugins screen shows the updates and revocations from the kept index
- No config yet means the binary boots into setup mode, the browser wizard (see below)

## Service commands

```
./hyscribe setup                 the terminal setup wizard: writes .env
./hyscribe start                 run in the foreground (also the default with no arguments)
./hyscribe service start         detach into the background (run/hyscribe.pid, logs/hyscribe.log)
./hyscribe service status        exit code 0 when running, 3 when not
./hyscribe service log [-n N] [-f]
./hyscribe service restart
./hyscribe service stop          SIGTERM, then SIGKILL after 15 seconds
./hyscribe cron run              run the due cron events once (for DISABLE_WP_CRON sites)
./hyscribe plugin install <file> check a local .hyx package and its signature, and unpack it into plugins/<slug>/ (--unsigned for a package no trusted key signed)
./hyscribe plugin delete <id>    remove plugins/<id>/ and its kept packages; refused while the plugin is active
```

Planned, not in the binary yet:

```
./hyscribe doctor                re-run the compatibility scan (M2)
./hyscribe theme install <file|slug>    unpack into themes/<slug>/ (M6)
```

- The running server holds an exclusive lock on `run/hyscribe.pid`. Holding the lock is what "running" means, so a pid file left by a crash never blocks a start, and `stop` only signals the pid written by the lock holder. The server writes its pid when it takes the lock and a `listening` line once it accepts connections; until then `status` says `starting (pid N)`
- `setup`, `start`, `service start`, `service restart`, `plugin install` and `plugin delete` refuse to run as root
- `service start` reports `started` only once the server listens (the site, or the setup wizard in setup mode), waiting up to 30 seconds. If the server exits first, or does not listen in time (it is then stopped), it says so with the last log lines, which give the reason, and exits with 1
- `logs/hyscribe.log` is rotated at `service start` once it reaches 10 MiB, keeping `hyscribe.log.1` to `hyscribe.log.5`; `log -f` follows until the server stops
- `run/`, `logs/` and `sessions/` are created with mode `0700`, the pid file, logs and session files with `0600`

The service does not start at boot by itself. Add `@reboot /path/to/hyscribe service start` to the crontab, or use the container image. A generated systemd unit is under consideration. On macOS nothing is provided either; a launchd job is the system's way.

## Setup modes

- **Browser wizard** (default, and the only option for non-interactive starts): the server boots into setup mode, gives a one-time access secret, and redirects every page to `/installation/step-1` until setup completes. The secret closes the window in which a stranger could claim a fresh install before the owner does; setup mode stops after 30 minutes without `.env`. Answers are held on the server, never in the browser. The secret is printed when a terminal runs the command; otherwise it is written to `logs/setup-secret` (mode `0600`), which setup mode deletes when it ends, whether by finishing, a stop or its timeout. It is never written to `logs/hyscribe.log`. Its security design is in [SETUP_WIZARD.md](SETUP_WIZARD.md).
- **Terminal wizard** (`./hyscribe setup`, for existing sites and new ones; see [SETUP_WIZARD.md](SETUP_WIZARD.md)): the same steps asked in the terminal, with the same live checks and hidden password input.
- **No wizard:** write `.env` by hand from `.env.example` before the first start.

Steps and the compatibility scan are described in [SETUP_WIZARD.md](SETUP_WIZARD.md).

## Behind a reverse proxy, or not

- Default: HyScribe serves plain HTTP on `127.0.0.1:8080`, so nothing is reachable from outside until you decide. Put nginx, Caddy or Traefik in front for TLS and set `BEHIND_PROXY=true` so forwarded headers are trusted. For the browser wizard on a remote server, use an SSH tunnel or start once with `BIND_HOST=0.0.0.0`
- Direct: binding to 80 and 443 needs root or `CAP_NET_BIND_SERVICE`; built-in TLS with ACME is a secondary goal
- The wizard asks "Running HyScribe behind a proxy?" and sets these keys

## Database changes on boot

Unlike products that run migrations on start, HyScribe never alters WordPress tables (ADR 0003). On boot it only checks `db_version` and creates `{prefix}hyscribe_*` tables if a feature needs them.

## Upgrades

1. Download the new archive and replace the binary; every other file in `hyscribe/` is kept
2. `./hyscribe service restart`

A built-in `hyscribe update` command is under consideration.

## Uninstall or switch back to PHP WordPress

`./hyscribe service stop`, then delete the `hyscribe/` folder (for a new site, move `uploads/` out first). The database and the original `wp-content` remain valid for PHP WordPress. Optionally drop the `{prefix}hyscribe_*` tables.

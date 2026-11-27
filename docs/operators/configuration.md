# Configuration

HyScribe reads its settings from `.env` in the `hyscribe/` folder, next to the binary. The setup wizard writes it; `.env.example` in the archive lists every key with a comment.

## Rules

- One `KEY=value` per line; `#` starts a comment. Values may be double quoted, as the wizard writes them
- `.env` must be readable by its owner only (`chmod 600 .env`). Any group or other permission bit stops the start
- Real environment variables override `.env`, key by key, so a container or a supervisor can inject a setting
- `HYSCRIBE_HOME`, from the environment only, names another folder as the installation's, where `.env`, `logs/` and the rest live instead of the binary's folder: an absolute path to an existing folder, or the start stops with an error. The container image sets it to `/hyscribe` ([docker.md](docker.md))
- No `.env` and no `DB_NAME` in the environment means the site is not set up: the binary starts the setup wizard instead of the site
- A key set to an empty value counts as not set
- Every yes or no key (`BEHIND_PROXY`, `REST_PUBLIC_USERS`, `AUTHOR_ARCHIVES`, `HTTP_OUTBOUND`, `EXTENSION_UPDATE_CHECK` and the WordPress constants below) takes `true`, `yes`, `on` or `1`, and `false`, `no`, `off` or `0`, in any case
- A value that cannot be used (a port of 0, `MODE=read`, `BEHIND_PROXY=maybe`) stops the start with an error naming the key. Secret values are never printed
- Settings are read once at start. After editing `.env`, run `./hyscribe service restart`

Check a configuration without starting anything:

```
./hyscribe config check                  the .env next to the binary
./hyscribe config check --dir /path      the .env in another folder
```

It prints a summary with no passwords, keys or salts, or the first problem it finds, and exits with 1 on a problem.

## Every key

The default is what HyScribe uses when the key is missing or empty. A test in `crates/hyscribe-config` fails when a key the loader reads is missing from this table, when the table names a key the loader does not read, or when a default here differs from the loader's.

### Database

| Key | Default | What it does |
| --- | --- | --- |
| `DB_HOST` | `localhost` | Database server, as `DB_HOST` in `wp-config.php`. A Unix socket is written `localhost:/run/mysqld/mysqld.sock`; only `localhost` takes a socket, as for PHP's `mysqli` |
| `DB_PORT` | `3306` | Database port, 1 to 65535. Not used with a socket |
| `DB_NAME` | required | Database name |
| `DB_USER` | required | Database user. In primary mode it needs the rights PHP WordPress's user has |
| `DB_PASSWORD` | not set | Its password |
| `DB_PREFIX` | `wp_` | `$table_prefix` from `wp-config.php`. Letters, digits and underscores only |
| `DB_CHARSET` | `utf8mb4` | Connection character set. The wizard turns `utf8` from `wp-config.php` into `utf8mb4`, as WordPress does |
| `DB_READ_USER` | not set | Optional user granted only `SELECT`. When set, every read and all of shadow mode use it, so a fault in a read cannot write. Without it, reads use `DB_USER` on a read only session |
| `DB_READ_PASSWORD` | not set | Its password. Setting it without `DB_READ_USER` stops the start |

TLS to the database server is not supported yet: HyScribe connects over TCP or a Unix socket without TLS.

### Network

| Key | Default | What it does |
| --- | --- | --- |
| `BIND_HOST` | `127.0.0.1` | IP address to listen on. The default keeps the site private until a proxy is in front. A host name is refused |
| `PORT` | `8080` | Port to listen on, 1 to 65535. Ports below 1024 need `CAP_NET_BIND_SERVICE`, and HyScribe refuses to run as root |
| `BEHIND_PROXY` | `false` | `true` when a reverse proxy is in front: the last `X-Forwarded-For` address then names the client for login limits, comment flood checks, mail limits, the registration cap and a comment's stored address, when the connection comes from a peer in `TRUSTED_PROXIES` |
| `TRUSTED_PROXIES` | `127.0.0.1,::1` | With `BEHIND_PROXY` on, the peers `X-Forwarded-For` is believed from: up to 256 comma separated IP addresses or CIDR ranges, such as `10.0.0.0/8` or `fd00::/8`. A connection from any other peer counts as its own address, whatever header it sends, so a client that reaches the port directly cannot name its address. An invalid entry stops the start |
| `REQUEST_TIME_LIMIT` | `30` | Seconds a request may run, from 1 to 86400. A request still running past it answers 503 and its unfinished write rolls back, so a write is never left half done. Cron and the mail queue run outside requests and are not cut by it |

### Site

| Key | Default | What it does |
| --- | --- | --- |
| `MODE` | `shadow` | `shadow` serves the site read only and never writes. `primary` serves and writes. Logins and hs-admin work in primary mode only |
| `WP_HOME` | not set | Overrides the `home` option, as the constant in `wp-config.php` does. Useful to try a copy of a site on another address |
| `WP_SITEURL` | not set | Overrides the `siteurl` option, as the constant does. An `https:` address marks login cookies secure |
| `UPLOADS_DIR` | not set | An existing site's uploads folder, left where it is. Uploads are written there and served at `/wp-content/uploads/`; without it, in `uploads/` in the installation's folder |
| `REST_PUBLIC_USERS` | `on` | `on` answers visitors at `/wp-json/wp/v2/users` as WordPress does, listing every user with published posts. `off` answers visitors as a site where no user has published posts |
| `AUTHOR_ARCHIVES` | `off` | `on` serves author archives (`?author=N`, `?author_name=`, `/author/<nicename>/`, their pages and feeds) as WordPress does, and redirects `?author=N` to `/author/<nicename>/` under pretty permalinks. This reveals each author's nicename, which WordPress derives from the login name, to anyone who counts up the IDs, which is why it is off by default ([security.md](security.md)). `off` answers all of them 404, so the author links themes print lead to a 404 |
| `HTTP_OUTBOUND` | `off` | `on` lets the server fetch from other sites, to public addresses only (see [security.md](security.md)). `off` makes no outbound request at all |
| `EMBED_ALLOWED_HOSTS` | `providers` | Where embeds may fetch from. `providers` (also unset or empty): WordPress's oEmbed providers only. `providers,media.example.org`: the providers and these hosts, whose pages are also searched for an oEmbed link. Host names alone (no `providers`): only those hosts. `none`: no embed fetches. Names are comma separated, without scheme, port or path, matched exactly. A URL outside the list stays a plain link, nothing is fetched and no `_oembed_` row is written. Fetching also needs `HTTP_OUTBOUND=on` |
| `EXTENSION_TRUSTED_KEYS` | not set | minisign public keys trusted for extension packages you build yourself, comma separated: `hyscribe plugin install <file>` takes a package whose signature `<file>.minisig` one of them made. A key that does not read refuses the install. These keys never stand in for the hosted registry |
| `EXTENSION_REGISTRY` | not set | The hosted registry's base URL, `https:` only (an `http:` URL stops the start): a mirror or a registry of your own. `hyscribe plugin install <name>` and `hyscribe plugin update` fetch from it whatever `HTTP_OUTBOUND` says; what they install is still checked against the root keys compiled into the binary, so a mirror can withhold files but forge none. Not set, the commands say so, until the project's own registry has its address |
| `EXTENSION_UPDATE_CHECK` | `off` | `on` makes the service fetch the registry's key list and index at its start and every 12 hours, verify them against the root keys compiled into the binary, keep them in `registry/` and stop any installed extension the registry has revoked. It installs nothing: the Plugins screen shows the updates, and `hyscribe plugin update <id>` installs them. Nothing about the site is sent. Needs `HTTP_OUTBOUND=on` and `EXTENSION_REGISTRY`; without either the start log says the check is off |
| `RSS_ALLOWED_HOSTS` | not set | The hosts the RSS block may fetch feeds from, comma separated host names (no scheme, port or path), matched exactly. Empty: every RSS block shows WordPress's error for a feed it cannot reach. Fetching also needs `HTTP_OUTBOUND=on` |
| `UPLOAD_MAX_SIZE` | `64M` | The largest upload, in place of PHP's `upload_max_filesize` and `post_max_size`: whole bytes, or a number with `K`, `M` or `G` (either case), at least one byte. hs-admin shows it where WordPress shows its upload limit |
| `IMAGE_MAX_BYTES` | `64M` | The largest image whose sizes are made, as a size like `UPLOAD_MAX_SIZE`. A larger image is kept with no sizes |
| `IMAGE_MAX_WIDTH` | `32000` | The widest image whose sizes are made, in pixels, from 1 to 4294967295 |
| `IMAGE_MAX_HEIGHT` | `32000` | The tallest image whose sizes are made, in pixels, from 1 to 4294967295 |
| `IMAGE_MAX_AREA` | `256000000` | The most pixels an image whose sizes are made may have |
| `IMAGE_MAX_MEMORY` | `1G` | The most memory decoding one image may take, as a size. The defaults are the PHP oracle's ImageMagick policy |

### WordPress constants

The wizard copies these from `wp-config.php` when they are literal values there. A missing key takes the safer value, which for `DISALLOW_UNFILTERED_HTML` differs from a default WordPress install.

| Key | Default | What it does |
| --- | --- | --- |
| `DISALLOW_UNFILTERED_HTML` | `true` | No one, administrators included, may save unfiltered HTML. Set `false` for WordPress's own behaviour |
| `ALLOW_UNFILTERED_UPLOADS` | `false` | As the WordPress constant |
| `MULTISITE` | `false` | `true` stops the start: multisite networks are not supported |
| `EMPTY_TRASH_DAYS` | `30` | Days posts and comments stay in the trash. `0` deletes them at once, and hs-admin and the REST API then offer only permanent deletes, as WordPress does |
| `MEDIA_TRASH` | `false` | Whether media files go to the trash before they are deleted, as `wp-config.php`'s constant of the same name. hs-admin's Media screens, the edit screens and the General settings answer 501 while it is `true` |
| `DISABLE_WP_CRON` | `false` | `true` stops the service's once a minute run of WordPress's scheduled core jobs (publishing scheduled posts, emptying the trash, deleting expired transients and old auto-drafts and exports, failing stale privacy requests). Run `./hyscribe cron run` from the system's cron instead. Same values as above |
| `ALTERNATE_WP_CRON` | `false` | Accepted and ignored, with a line in the log: HyScribe makes no loopback request to replace. Same values |

### Login limits

HyScribe's own; PHP WordPress has none. Details in [security.md](security.md).

| Key | Default | What it does |
| --- | --- | --- |
| `LOGIN_LIMIT_ADDRESS` | `5` | Wrong passwords from one client address within the window before that address is refused. 1 to 1000 |
| `LOGIN_LIMIT_ACCOUNT` | `10` | Wrong passwords at one account within the window before it is refused, except from its recent login addresses. 1 to 1000 |
| `LOGIN_LIMIT_WINDOW` | `900` | The window, in seconds. 1 to 86400 |
| `REGISTRATION_LIMIT_ADDRESS` | `3` | Accounts one client address may create by registration in an hour, 0 to 1000. Past it, registration answers WordPress's own registration error and writes nothing. `-1` turns the cap off. Only accounts really created count |

### Mail

HyScribe sends mail to an SMTP relay you name, never through a local `sendmail` (ADR 0016). Without `SMTP_HOST` no mail is sent: each message is logged as not sent, the start log says mail is off, and users cannot reset their own passwords, as on a PHP host whose mail is broken. The relay's certificate must match `SMTP_HOST` and is checked against the host's certificate store; there is no setting to skip the check.

| Key | Default | What it does |
| --- | --- | --- |
| `SMTP_HOST` | not set | The relay's host name or IP address. Unset sends no mail |
| `SMTP_PORT` | not set | The relay's port: 587 for `starttls`, 465 for `tls` and 25 for `none` when unset |
| `SMTP_TLS` | `starttls` | `starttls` requires STARTTLS (a relay that does not offer it fails the send, never falling back to plain text), `tls` speaks TLS from the first byte, `none` is plain text and only for a relay on this machine (`localhost`, `127.0.0.1` or `::1`) |
| `SMTP_USER` | not set | The login for SMTP AUTH; unset logs in as no one. Refused with `SMTP_TLS=none`, so credentials never travel unencrypted |
| `SMTP_PASSWORD` | not set | Its password. Never logged |
| `MAIL_FROM` | not set | The sender address of every message, one plain address such as `wordpress@example.com`. Unset uses WordPress's default sender, `wordpress@` and the site's host without `www.`. A plugin or a message's `From` header changes only the name shown |

Limits (HyScribe's own; PHP WordPress has none): at most 3 password reset mails per account and 10 per client address in an hour, refused past that as a failed send; at most 60 messages a minute in all, the rest waiting in a queue of 1,000 that refuses only when full. A password reset waits up to 30 seconds for its mail; other messages are sent in the background. Each message is logged on one line with its kind, the recipient's domain, the outcome, the relay's reply code and the time taken, never its subject, body or full address.

### Keys and salts

The eight keys and salts of `wp-config.php`. Copy them from the existing site so its users' logins keep working; the wizard copies them, and generates any it cannot copy. A key that is not set falls back to the one stored in the database options (`auth_key` and so on), as WordPress does; with neither, logins fail.

| Key | Default | What it does |
| --- | --- | --- |
| `AUTH_KEY` | not set | As in `wp-config.php` |
| `SECURE_AUTH_KEY` | not set | As in `wp-config.php` |
| `LOGGED_IN_KEY` | not set | As in `wp-config.php` |
| `NONCE_KEY` | not set | As in `wp-config.php` |
| `AUTH_SALT` | not set | As in `wp-config.php` |
| `SECURE_AUTH_SALT` | not set | As in `wp-config.php` |
| `LOGGED_IN_SALT` | not set | As in `wp-config.php` |
| `NONCE_SALT` | not set | As in `wp-config.php` |

## Not configurable yet

- The request line limit (8,191 bytes, as Apache) and the 30 second header timeout
- Log rotation size and count

# Running as a service

The binary runs itself in the background; no init script is needed to start it.

## Commands

Run them from the `hyscribe/` folder, as the user that owns it.

| Command | What it does |
| --- | --- |
| `./hyscribe service start` | Starts the server in the background, output to `logs/hyscribe.log`. Prints `started (pid N)` once it listens, or `already running (pid N)` |
| `./hyscribe service stop` | SIGTERM, then SIGKILL after 15 seconds. Prints `stopped (pid N)` |
| `./hyscribe service restart` | Stop, then start. Needed after any `.env` change |
| `./hyscribe service status` | `running (pid N)`, or `starting (pid N)` before it listens, and the last log line, exit code 0; `not running`, exit code 3 |
| `./hyscribe service log [-n N] [-f]` | The last N log lines (default 20); `-f` keeps following until the server stops |
| `./hyscribe start` | Runs the server in the foreground until SIGTERM or SIGINT. Also what the binary does with no command |
| `./hyscribe setup` | The terminal setup wizard, see [install.md](install.md) |
| `./hyscribe config check [--dir DIR]` | Checks the configuration without starting, see [configuration.md](configuration.md) |
| `./hyscribe cron run` | Runs the due events of WordPress's core scheduled jobs once and exits, for sites with `DISABLE_WP_CRON=true` (see [maintenance.md](maintenance.md)). Needs `.env` in primary mode, refuses root, and prints a line for each job it runs |
| `./hyscribe plugin install <file>` | Checks a local `.hyx` extension package and its signature `<file>.minisig` against `EXTENSION_TRUSTED_KEYS` (or `--unsigned`), keeps it in `packages/` and unpacks it into `plugins/<id>/`; activate it from the Plugins screen. Given a name, `<name>[@<version>]`, it installs from the hosted registry at `EXTENSION_REGISTRY` (see [configuration.md](configuration.md)) |
| `./hyscribe plugin update --check \| <id>... \| --all` | Lists or installs the updates the hosted registry offers for extensions installed from it |
| `./hyscribe plugin delete <id>` | Removes `plugins/<id>/` and its kept packages; refused while the extension is active. Writes nothing to the database |
| `./hyscribe version` | Version, license and source location |
| `./hyscribe help [command]` | Help for every command |

Exit codes: 0 on success, 1 on an error (printed as `error: ...`), 2 for a command line it does not understand, 3 from `service status` when nothing runs.

## How it behaves

- "Running" means holding the lock on `run/hyscribe.pid`. A pid file left by a crash never blocks a start, and `stop` only signals the process that holds the lock
- `service start` returns once the server listens: it has connected to the database and opened its port (in setup mode, the wizard's port). It waits up to 30 seconds. If the server exits first, or does not listen in time, it prints the reason with the last log lines and exits with 1; a server that did not listen in time is stopped
- `service stop` also stops a server that is still starting
- `setup`, `start`, `service start` and `service restart` refuse to run as root
- Environment variables set when `service start` runs are passed on to the server, and override `.env`
- A server stopped by a signal logs `stopped on signal N` as its last line

## Starting at boot

HyScribe does not install itself into the init system. Either:

- add `@reboot /path/to/hyscribe/hyscribe service start` to the service user's crontab, or
- have a process supervisor run `/path/to/hyscribe/hyscribe start` in the foreground as the service user, with the `hyscribe/` folder as working directory, stopped with SIGTERM

On macOS the supervisor is launchd; HyScribe ships no launchd job yet.

## Behind a reverse proxy

- Keep `BIND_HOST=127.0.0.1` and point the proxy at `http://127.0.0.1:8080`
- Set `BEHIND_PROXY=true`, or every visitor counts as the proxy's address for login limits and comment flood checks. The proxy must append the client's address to `X-Forwarded-For`; HyScribe reads every `X-Forwarded-For` line in order as one list and takes its last entry, the one the proxy appended, but only from a peer listed in `TRUSTED_PROXIES` (default `127.0.0.1,::1`); list the proxy's address there if it connects from another machine
- This assumes HyScribe is reachable only through one proxy, which appends exactly one entry, either to the client's header (nginx's `$proxy_add_x_forwarded_for`, Apache's `mod_proxy`, Caddy, Traefik) or as a separate line (HAProxy's `option forwardfor`). Behind a chain of proxies or a CDN in front of the proxy, make the last proxy replace the header with the address it trusts, or the last entry names the CDN rather than the client
- The entry may be `ip`, `ip:port`, `[ipv6]` or `[ipv6]:port`. An entry that names no address, such as `unknown`, refuses that request's login guesses and logs `login refused: the proxy's last X-Forwarded-For entry ... names no address`; fix the proxy's format if you see it
- Terminate HTTPS at the proxy. HyScribe treats the site as HTTPS when its `siteurl` (or `WP_SITEURL`) starts with `https:`, and then marks login cookies secure
- HyScribe serves `/wp-content/uploads/` itself, from `UPLOADS_DIR` or `hyscribe/uploads/`. A proxy may serve that folder directly instead, if it keeps HyScribe's headers: `nosniff`, and files other than images, audio, video, PDF and text sent as downloads (see [security.md](security.md))
- `/wp-login.php` and `/wp-admin/...` answer `307` redirects to `/hs-login` and `/hs-admin/...`, keeping the query, so old links and bookmarks work

## Logs

- Everything the server prints goes to `logs/hyscribe.log`, mode `0600`, in a `0700` folder
- At each `service start`, a log of 10 MiB or more is rotated to `hyscribe.log.1`, keeping `.1` to `.5`
- `./hyscribe start` in the foreground prints to the terminal instead

What it logs:

- At start: the version, pid, mode and address, then `outbound fetches off`, `outbound fetches on` or `outbound fetches on over HTTP only: no certificate store was found`
- `writes refused: ...` at start in primary mode, for each table or `db_version` HyScribe will not write to
- `cron: ...` at start when the scheduled jobs do not run in the service (shadow mode, `DISABLE_WP_CRON`, an `object-cache.php`) or `ALTERNATE_WP_CRON` is set, and later for each run that does something, waits for PHP's lock or fails
- In setup mode: the setup address and where the one-time secret is, `logs/setup-secret`, never the secret itself. That file is mode `0600` and deleted when setup mode ends. `service start` at a terminal prints the secret there instead, and `./hyscribe start` at a terminal prints it to the terminal and writes no file
- `database error: ...` when a query fails; visitors see an error page without the details
- `stopped on signal N` at the end

It does not log requests. Use the reverse proxy's access log for those. Passwords, keys, salts, nonces and cookies are never logged.

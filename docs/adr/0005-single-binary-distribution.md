# 0005: Single binary distribution

- Status: Accepted
- Date: 2026-10-26

## Context

WordPress is installed by extracting `latest.tar.gz` and opening a browser. HyScribe should be at least that simple, without requiring a web server, PHP, or Docker. A single self-contained binary that runs itself as a service and sets itself up through a browser or terminal wizard gives operators that.

## Decision

- The primary distribution is a per-platform archive (`hyscribe_<version>_<os>_<arch>.tar.gz` plus a `hyscribe-latest-*` alias) containing one statically linked binary, `LICENSE` and `README.txt`
- Everything needed at runtime is embedded in the binary: admin UI, `wp-includes` assets, a default theme, the extension runtime
- The binary manages itself as a background service (`service start|status|log|restart|stop`) with its pidfile in `run/` and logs in `logs/`
- First start without configuration boots setup mode: browser wizard protected by a one-time secret (default) or terminal wizard
- Configuration lives in `.env` next to the binary; real environment variables override it
- The `hyscribe/` folder holds the whole installation in plainly named folders: `plugins/<slug>`, `themes/<slug>`, `languages/`, `uploads/` (new sites), `packages/`, `reports/`, `cache/`, `logs/`, `run/`. Only an existing site's media library stays in its original `wp-content/uploads`
- WordPress URL paths (`/wp-content/...`) are preserved and mapped onto these folders
- Docker images are an optional, secondary distribution of the same binary

## Consequences

- Install, upgrade and removal are file operations on one folder
- Binary size grows with embedded assets; acceptable, but tracked (risk R13)
- `wp-includes` assets are bundled, which settles where `wp-includes` assets come from
- The configuration format changes from the earlier `hyscribe.toml` draft to `.env`
- Boot autostart and self-update are not covered yet

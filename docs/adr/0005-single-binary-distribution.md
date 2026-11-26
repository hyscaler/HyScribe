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

## Amendment (2026-11-26): macOS archives

- macOS joins Linux as a release platform: `hyscribe_<version>_darwin_arm64.tar.gz` (Apple Silicon) and `hyscribe_<version>_darwin_amd64.tar.gz` (Intel), each with its `hyscribe-latest-darwin-<arch>.tar.gz` alias, holding the same files as the Linux archives. The deliverable stays the plain binary in a `tar.gz`: no `.dmg`, no `.pkg`, no app bundle
- They are built on macOS, on a GitHub-hosted runner, and never cross-compiled from Linux: the build then uses Apple's own linker and SDK, on the system the binary is for, where it can be started and tested. The Linux archives stay on the project's self-hosted runners
- "Statically linked" cannot hold on macOS, which has no stable system call interface and no static C library: the binary links the system's own libraries and nothing else. The rule that matters is kept, no runtime dependency to install other than the database
- The build targets macOS 11.0, the first release for Apple Silicon, for both architectures (`MACOSX_DEPLOYMENT_TARGET` in `scripts/package.sh`)
- Each archive is smoke-tested on the runner before it is uploaded: installed, started against the default install fixture in a MariaDB installed on the runner (`reference/default-install/db-native.sh`, since the runner has no Docker), a page fetched, stopped. The Intel binary runs under Rosetta, since the runner is an Apple Silicon machine
- Each archive is built twice on the runner and the bytes compared, as for Linux, and they are the same. The Linux builds take their compiler and C toolchain from one pinned image; the macOS bytes also depend on the runner image's Xcode, SDK and Homebrew tools (GNU tar), which only the image's label (`macos-15`) pins, so a later image may give other bytes for the same commit
- The binaries are not signed with an Apple Developer ID or notarised. Until they are, a copy downloaded with a browser has to be allowed by its user (docs/DEPLOYMENT.md, "macOS"). Checked on the macOS 15 runner: a file fetched with `curl` carries no quarantine mark, `tar` copies an archive's mark to what it unpacks, `spctl` rejects the binary, and `xattr -d com.apple.quarantine` removes the mark; the refusal to start is macOS's documented behaviour and could not be shown on the runner, where a marked binary still ran from the job's shell. Signing needs an Apple developer account and its certificate in the release workflow, a decision still open
- The certificate store on macOS is the bundle the system ships (`/etc/ssl/cert.pem`), read by the same code as on Linux, not the Keychain: reading the Keychain would need the Security framework through FFI, which ADR 0019 avoided
- Boot autostart stays out of scope here too (a launchd job on macOS)

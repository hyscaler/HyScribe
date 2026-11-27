# Install

HyScribe is one binary in a `tar.gz` (a `.zip` on Windows). There is no web server, PHP or extension to install. It needs a MySQL or MariaDB server, normally the one your WordPress site already uses.

HyScribe is not yet production ready: it is an early release that has had no external security audit, and some screens and routes still answer `501 Not Implemented` ([not-supported.md](not-supported.md)). Install it on a copy of your site first, and keep backups of the database and uploads.

## Requirements

- Linux on amd64 or arm64. The binary is static, so any distribution works
- Or macOS 11 or later, on Apple Silicon or Intel: see [On macOS](#on-macos) below
- Or Windows 10, Windows Server 2016 or later, on amd64: see [On Windows](#on-windows) below
- A MySQL or MariaDB database holding a WordPress 7.0 or 7.1 site (`db_version` 61833). Older sites: upgrade with PHP WordPress first
- An unprivileged user account to run it: HyScribe refuses to run as root
- A reverse proxy (nginx, Caddy, Traefik) for HTTPS. HyScribe serves plain HTTP

## Download and verify

Each release on GitHub (`hyscaler/HyScribe`) has:

- `hyscribe_<version>_<os>_<arch>.tar.gz`, `<os>` being `linux` or `darwin` (macOS) and `<arch>` being `amd64` or `arm64` (Apple Silicon is `arm64`), and `hyscribe_<version>_windows_amd64.zip`
- `hyscribe-latest-<os>-<arch>.tar.gz` and `hyscribe-latest-windows-amd64.zip`, the same archives under names that stay the same
- `SHA256SUMS.txt`, an SBOM for each system (`hyscribe-sbom.cdx.json` for Linux, `hyscribe-sbom-darwin.cdx.json` for macOS, `hyscribe-sbom-windows.cdx.json` for Windows) and signed build provenance

The latest release's archives download from fixed addresses:

```
curl -fLO https://github.com/hyscaler/HyScribe/releases/latest/download/hyscribe-latest-linux-amd64.tar.gz
curl -fLO https://github.com/hyscaler/HyScribe/releases/latest/download/SHA256SUMS.txt
```

On arm64, take `hyscribe-latest-linux-arm64.tar.gz` instead. A given version's files are at `https://github.com/hyscaler/HyScribe/releases/download/v<version>/`.

Then check the archive against the checksums and its build provenance (the second needs the GitHub CLI, `gh`):

```
sha256sum -c SHA256SUMS.txt --ignore-missing
gh attestation verify hyscribe-latest-linux-amd64.tar.gz --repo hyscaler/HyScribe
```

`sha256sum` must print `OK` for the archive, and `gh` must find an attestation made by the repository's release workflow. To build the archive from source instead, run `scripts/package.sh x86_64-unknown-linux-musl dist` (it needs the Rust target and clang).

## Extract

```
tar -xzf hyscribe_<version>_linux_amd64.tar.gz
cd hyscribe
./hyscribe version
```

The `hyscribe/` folder is the whole installation. Put it anywhere the service user owns, for example next to the WordPress folder. It holds:

- `hyscribe`: the binary
- `LICENSE`, `NOTICE`, `README.txt`
- `THIRD-PARTY-NOTICES`: the licences of what the binary embeds (WordPress's files, its themes and the Rust crates) and where their source is
- `.env.example`: every configuration key
- `docs/`: these pages

The binary creates the rest when first needed, each with owner only permissions:

- `.env`: the configuration, mode `0600`
- `logs/`: `hyscribe.log` and its rotated copies, and `setup-secret` while setup mode runs without a terminal
- `run/`: `hyscribe.pid`
- `sessions/`: login sessions, one file per user (primary mode)

## Set up

There are three ways to write `.env`. All of them read an existing site's `wp-config.php` without running it.

### Browser wizard

```
./hyscribe service start
```

Without `.env`, the server starts in setup mode. At a terminal, `service start` prints a line like:

```
Setup mode: Open http://127.0.0.1:8080/installation/step-1 and enter the one-time setup secret <32 letters and digits>
```

- Run without a terminal, it says the line is in `logs/setup-secret` instead (`cat logs/setup-secret`). That file is mode `0600` and is deleted when setup mode ends, however it ends. The secret never goes into `logs/hyscribe.log`
- `./hyscribe start` in the foreground at a terminal prints the line itself and writes no file

- Setup mode listens on `BIND_HOST` and `PORT` from the environment, `127.0.0.1:8080` by default. On a remote server, use an SSH tunnel (`ssh -L 8080:127.0.0.1:8080 server`), or start once with `BIND_HOST=0.0.0.0 ./hyscribe service start`
- Every page redirects to `/installation/step-1` until setup is done. Enter the secret there; it works once, for one browser
- Step 2 asks for the WordPress folder, or the contents of `wp-config.php` pasted in, or neither for manual entry
- Step 3 asks for what `wp-config.php` did not give, the mode, the optional read only database user, the network and the uploads folder, then runs the checks and writes `.env`
- The same process then serves the site. After 30 minutes without `.env` it stops; start again for a new secret. A lost browser session also needs a restart

At a terminal, `./hyscribe start` and `./hyscribe service start` without `.env` first ask `No .env found. Set up in the browser or in the terminal? [browser/terminal]`. Browser is the default, and the only choice when standard input is not a terminal. `terminal` runs the terminal wizard below, then starts the site.

### Terminal wizard

```
./hyscribe setup
```

It asks, in order:

1. `<path>/.env exists. Replace it? [y/N]`, only when it exists. Anything but yes writes nothing
2. The WordPress folder or its `wp-config.php`, defaulting to the first one found in the folder above `hyscribe/` or in a folder beside it; `none` for manual entry (database name, user, password, host, port, table prefix)
3. Whatever `wp-config.php` does not give as a literal value; computed values are listed and asked for
4. `Start in shadow mode (read only) first? [Y/n]`
5. A database user granted only `SELECT`, and its password (empty for none)
6. `Running HyScribe behind a proxy? [y/N]`, the address (default `127.0.0.1`) and the port (default `8080`)
7. The site's uploads folder (empty for `hyscribe/uploads`)

Passwords are read with echo off, and no password, key or salt is printed. Keys and salts missing from `wp-config.php` are generated. Then start the site with `./hyscribe service start`.

### Checks both wizards run

They only read, and stop on a failure:

- Both database users connect
- `db_version` is one this release supports
- The read only user, when given, has `SELECT` and nothing more, roles included
- In primary mode, every table HyScribe writes is InnoDB; in shadow mode the others are listed as warnings
- A `wp-content/object-cache.php` drop-in is a warning: HyScribe's writes do not clear a persistent object cache, so PHP WordPress may show stale data until it expires

### By hand

```
cp .env.example .env
chmod 600 .env
$EDITOR .env
./hyscribe config check
```

Every key is in [configuration.md](configuration.md). Copy the eight keys and salts from `wp-config.php`, or existing logins stop working.

## Start

```
./hyscribe service start
./hyscribe service status
```

The log's first lines say the version, the mode and the address:

```
hyscribe 0.1.0 running (pid 12345) in shadow (read only) mode, listening on http://127.0.0.1:8080
outbound fetches off
```

## On macOS

The steps above are the same on macOS, with these differences:

- Take `hyscribe-latest-darwin-arm64.tar.gz` on Apple Silicon and `hyscribe-latest-darwin-amd64.tar.gz` on an Intel Mac
- Check the download with `shasum -a 256 -c SHA256SUMS.txt --ignore-missing` where there is no `sha256sum`; `gh attestation verify` works as on Linux
- The binary is not signed or notarised by Apple yet. Fetched with `curl` as above, it starts normally. Downloaded with a browser, the archive is quarantined and `tar` passes that on to the binary, which macOS then refuses to start, saying it cannot verify it. After checking the download as above, remove the mark with `xattr -d com.apple.quarantine hyscribe/hyscribe`, or try to start it once and then choose "Open Anyway" under System Settings, Privacy & Security
- Outbound HTTPS and the mail relay are checked against the certificates macOS ships in `/etc/ssl/cert.pem`, not against the Keychain
- Nothing starts HyScribe at boot: [service.md](service.md) describes cron and process supervisors, and a launchd job is not provided
- macOS is tested less than Linux: each release's archive is installed, started and stopped on macOS 15, but the full test suite runs on Linux only. Use it to try HyScribe or to develop, and host sites on Linux
- To build the archive from source, on a Mac: `scripts/package.sh aarch64-apple-darwin dist` (or `x86_64-apple-darwin`), with Xcode's command line tools, the Rust target and GNU tar (`brew install gnu-tar`)

## On Windows

The binary is `hyscribe.exe` in `hyscribe-latest-windows-amd64.zip`; the rest of this page applies, with these differences. In PowerShell:

```
cd $HOME
curl.exe -fLO https://github.com/hyscaler/HyScribe/releases/latest/download/hyscribe-latest-windows-amd64.zip
curl.exe -fLO https://github.com/hyscaler/HyScribe/releases/latest/download/SHA256SUMS.txt
(Get-FileHash hyscribe-latest-windows-amd64.zip -Algorithm SHA256).Hash
Select-String windows-amd64.zip SHA256SUMS.txt
gh attestation verify hyscribe-latest-windows-amd64.zip --repo hyscaler/HyScribe
Unblock-File hyscribe-latest-windows-amd64.zip
Expand-Archive hyscribe-latest-windows-amd64.zip -DestinationPath .
cd hyscribe
.\hyscribe.exe version
.\hyscribe.exe service start
```

- The two hashes must be the same (letter case aside); `certutil -hashfile hyscribe-latest-windows-amd64.zip SHA256` prints it too. `gh` must find an attestation made by the repository's release workflow
- The binary is not signed yet. A zip downloaded with a browser is marked as coming from the internet, and Windows SmartScreen may then stop the binary's first start with "Windows protected your PC". After checking the download as above, choose "More info" and then "Run anyway", or run `Unblock-File` on the zip before unzipping it, as above. A copy fetched with `curl.exe` carries no such mark
- Keep the `hyscribe` folder inside your own profile folder (`C:\Users\<you>`), as above. HyScribe sets and checks no file permissions on Windows: `.env`, the session files and the logs take the folder's, and a profile folder is open only to its user, administrators and the system. Elsewhere, restrict the folder yourself, for example `icacls hyscribe /inheritance:r /grant:r "${env:USERNAME}:(OI)(CI)F" "SYSTEM:(OI)(CI)F" "Administrators:(OI)(CI)F"`
- HyScribe cannot tell an administrator account on Windows, so it does not refuse one as it refuses root elsewhere: run it from an ordinary account
- `DB_HOST` is a host or `host:port`: a socket is refused, and named pipes are not supported
- Windows's own certificate store is not read. For outbound HTTPS and the mail relay's TLS, set `SSL_CERT_FILE` in the environment HyScribe starts in to a PEM bundle of trusted certificates (curl's `cacert.pem`, for example); without it the log says HTTPS fetches are off
- `service start` runs the server in the background with no window; `service stop` asks it to stop through a named pipe, then ends it after 15 seconds. Nothing starts it at boot: [service.md](service.md) shows a scheduled task
- Windows is tested less than Linux: each release's archive is unzipped, started and stopped on Windows Server 2025, but the full test suite runs on Linux only. Use it to try HyScribe or to develop, and host sites on Linux
- To build the archive from source, on Windows: `scripts/package.sh x86_64-pc-windows-msvc dist` in Git for Windows's bash, with Visual Studio's C++ build tools, the Rust target and Python 3

Next: [service.md](service.md) to keep it running, [switching.md](switching.md) to move a site over.

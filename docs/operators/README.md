# HyScribe for operators

HyScribe serves an existing WordPress database from one Rust binary, with no PHP. These pages are for site owners and hosting operators who know WordPress; no Rust is needed. The release archive ships them in `hyscribe/docs/`.

HyScribe is not finished. Read [not-supported.md](not-supported.md) before moving a live site.

## Pages

- [install.md](install.md): download, verify, extract and run the setup wizard in the browser or the terminal
- [configuration.md](configuration.md): every `.env` key with its default
- [service.md](service.md): the service commands, starting at boot, reverse proxies and logs
- [switching.md](switching.md): moving a site over in shadow mode, then primary mode, and switching back to PHP WordPress
- [maintenance.md](maintenance.md): backups and upgrades
- [security.md](security.md): defaults, outbound fetches, uploads, login limits and sessions
- [not-supported.md](not-supported.md): everything that answers 501, and the other gaps

## In one minute

```
tar -xzf hyscribe-latest-linux-amd64.tar.gz && cd hyscribe
./hyscribe service start        # no .env yet: asks browser or terminal setup, then prints the setup address and one-time secret
./hyscribe service status
```

It listens on `127.0.0.1:8080` and starts read only (shadow mode). Put a reverse proxy with HTTPS in front, and switch to primary mode when the site is ready.

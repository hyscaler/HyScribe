# Security notes for operators

HyScribe's defaults are safe for someone who never reads this page. This page says what they are and what changing them means. Report vulnerabilities to security@hyscribe.com.

## Defaults

- Listens on `127.0.0.1` only, so nothing is reachable from outside until a proxy is in front or `BIND_HOST` changes
- Starts in shadow mode: read only, no writer at all
- Refuses to run as root, and refuses an `.env` that group or others can read
- No PHP. Nothing from disk runs but the extension packages an operator installs with `./hyscribe plugin install`, compiled in memory and run sandboxed, each call within time and memory limits ([ADR 0022](https://github.com/hyscaler/HyScribe/blob/main/docs/adr/0022-extension-host.md))
- `DISALLOW_UNFILTERED_HTML=true`: no one, administrators included, saves unfiltered HTML. WordPress lets administrators and editors; set `false` only if they need to embed scripts
- Author archives (`?author=N`, `?author_name=`, `/author/<nicename>/`) answer 404, so they do not give away login names. `AUTHOR_ARCHIVES=on` serves them as WordPress does: `?author=N` then redirects to `/author/<nicename>/` under pretty permalinks, and the nicename, which WordPress derives from the login name, is shown to anyone who counts up the IDs. Themes link every post's author name to the archive, so with the key off those links lead to a 404. Turn it on only if you accept that the login names of authors with posts can be found; the login rate limits still apply to guesses at their passwords
- XML-RPC, pingbacks and application passwords are not served. A trackback is answered that trackbacks are closed, and nothing is stored

## Outbound fetches

`HTTP_OUTBOUND=off` by default: the server makes no request to another site, so features that need one answer 501 (link previews in the editor, `url-details`) or show what WordPress shows when a fetch fails (embeds keep their URL, the RSS block shows its error). With `on`:

- Only `http` and `https` URLs, and only to public addresses. Every address a host resolves to is checked, on every redirect, and the connection goes to the checked address, so DNS rebinding cannot reach loopback, private, link local or cloud metadata addresses. The site's own host is not exempt
- At most 30 seconds, 4 MiB and 5 redirects per fetch; compressed answers are not decoded
- TLS is verified against the host's certificate store (on macOS the bundle in `/etc/ssl/cert.pem`, not the Keychain), never skipped. Without a store, the log says `outbound fetches on over HTTP only: no certificate store was found` and HTTPS fetches fail
- Only `Host`, `User-Agent`, `Accept-Encoding` and `Connection` headers are sent
- There is no general list of allowed hosts: any public address can be fetched. Firewall the server's outbound traffic if it must reach fewer
- The RSS block fetches only from the hosts in `RSS_ALLOWED_HOSTS`, empty by default
- Embeds fetch only from the hosts in `EMBED_ALLOWED_HOSTS`: by default WordPress's oEmbed providers, no discovery on other pages; hosts you add also get discovery. A URL outside the list stays a plain link and writes no cache row. They fetch the URL a post's author put alone on its line, on a page view when no cached embed exists; the result is cached in the post's meta in primary mode, and untrusted providers' HTML is sandboxed as WordPress sandboxes it

## Uploads

hs-admin's upload forms and the REST media route take every type in WordPress's default allowed list; images get their sizes made. The few formats whose metadata is not read yet answer 501 ([not-supported.md](not-supported.md)).

- Uploaded files are never run. They are served as static files with `nosniff`; anything but an image, audio, video, PDF or text file is sent as a download, so an uploaded HTML or SVG page cannot run script on the site
- A file is written under a temporary name inside `uploads/` and then given its name, never outside the folder; `.php`, `.html` and other types WordPress refuses are refused
- A request body over `UPLOAD_MAX_SIZE` (and a 1 MiB allowance for its form) is refused with 413 before more of it is read; at most 16 uploads are read at once
- Images are decoded only by memory safe Rust code on a pool of workers, with limits checked from the header before decoding: by default 32,000 pixels a side, 256,000,000 pixels, 1 GiB of working memory and no file over 64 MiB (`IMAGE_MAX_*`, see [configuration.md](configuration.md)). An image over a limit is kept without resized copies

## Login limits

PHP WordPress has none; HyScribe limits password guessing at `/hs-login`, the only place a password is checked.

- An address with `LOGIN_LIMIT_ADDRESS` (5) wrong passwords within `LOGIN_LIMIT_WINDOW` (900 seconds) is refused for the window, doubled for each refusal earlier that day, up to a day. That is about 40 guesses a day from one address
- An account with `LOGIN_LIMIT_ACCOUNT` (10) wrong passwords in the window, from any addresses, is refused for the window, but not from the last 8 addresses it logged in from, so an attacker cannot lock its owner out
- One address checks one password at a time
- A refused guess is answered exactly as a wrong password
- IPv6 addresses count by their /64. Behind a proxy, set `BEHIND_PROXY=true`, or every visitor shares the proxy's address and one attacker locks everyone out
- `X-Forwarded-For` is believed only from the peers in `TRUSTED_PROXIES` (loopback by default). A client that reaches the port directly counts as its own address, whatever it sends, so it cannot dodge the limits by naming new ones. Add your proxy's address if it is not on this machine
- With registration open, an address may create `REGISTRATION_LIMIT_ADDRESS` (3) accounts an hour; past that, registration answers WordPress's own registration error and writes nothing. `-1` turns the cap off
- The counts live in memory only: a restart clears them
- Password reset mails are limited to 3 per account and 10 per client address an hour (see [configuration.md](configuration.md#mail))

## Request time limit

PHP stops a request at `max_execution_time`; HyScribe stops one at `REQUEST_TIME_LIMIT` (30 seconds). A request still running then answers 503, and a write it had not committed rolls back, so nothing half written stays. A write committed just before the limit still stands, though the client sees the 503. Cron and the mail queue run outside requests and are not cut by it.

## Sessions and cookies

- Login sessions are files under `sessions/`, mode `0600`, holding only a hash of each token. Deleting a user's file logs them out everywhere
- Cookies are WordPress's format signed with the site's keys and salts, `HttpOnly`, `SameSite=Lax`, and `Secure` when the site address is `https:`
- Changing a password ends that user's other sessions
- Rotating the keys and salts in `.env` logs everyone out

## Setup mode

- The one-time setup secret is printed to the terminal when one runs the command. Otherwise it is in `logs/setup-secret` (mode `0600`), which is deleted when setup mode ends; it is never written to `logs/hyscribe.log`. It works once, for one browser
- Setup mode stops after 30 minutes without `.env`
- With `BIND_HOST=0.0.0.0`, anyone who can reach the port sees the setup page, but cannot pass it without the secret. Prefer an SSH tunnel

## Files

- `.env` holds the database password and the keys: mode `0600`, owned by the service user. Its permissions are checked at start only
- `logs/`, `run/` and `sessions/` are `0700`, their files `0600`
- Run HyScribe as its own unprivileged user, and give the database user only what PHP WordPress's user needs; better, add a `DB_READ_USER` with only `SELECT`

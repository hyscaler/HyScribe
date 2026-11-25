# 0019: The outbound HTTP client

- Status: Accepted (2026-11-17)
- Date: 2026-11-17

## Context

Several features fetch from other sites: the editor's link previews (`wp-block-editor/v1/url-details`, MVP 4.4), the RSS block (behind its allowlist, decided 2026-11-10), embeds (`wp_oembed_get()`), the dashboard's Events and News once the owner turns them on, and HyScribe's own release notice. Decision on the stack: `hyper`'s client with `rustls`, over the host's certificate store, a host without one fetching nothing over HTTPS and saying why. This ADR is the client's design and its crypto provider, which was left to it.

A server that fetches URLs its users give it is a server side request forgery (SSRF) target: a URL can point at the machine itself, the private network behind it or a cloud metadata service (`169.254.169.254`, `fd00:ec2::254`), which hands out credentials.

What PHP WordPress does (`wp_safe_remote_get()`, `reject_unsafe_urls`):

- `wp_http_validate_url()` allows HTTP and HTTPS only, no user or password in the URL, ports 80, 443 and 8080 (or the home URL's port on its host), and refuses hosts that are, or `gethostbyname()` resolves to, loopback, private, link-local, CGNAT, documentation, benchmarking, multicast or reserved IPv4 addresses
- The site's own host is exempt from the address check, whatever it resolves to
- The check resolves the name once, then cURL resolves it again to connect: a name that answers a public address first and a private one second (DNS rebinding) passes the check and connects to the private one
- Only IPv4 is checked: an IPv6 answer is not
- Each redirect's `Location` passes the same check (`WP_Http::validate_redirects()`), with the same gap
- The filters `http_request_host_is_external` and `http_allowed_safe_ports` can widen the checks, and `http_request_args` can turn `reject_unsafe_urls` off
- `WP_HTTP_BLOCK_EXTERNAL` blocks every host but the site's own and `WP_ACCESSIBLE_HOSTS`

## Decision

Accepted on 2026-11-17 as proposed, with all six of its decisions taken as recommended: the client and `ring`; trusted-publisher cargo vet entries for the TLS crates and `openssl-probe` in place of `rustls-native-certs`; `HTTP_OUTBOUND` off by default; the stricter SSRF rules, with the two widening WordPress filters only allowed to narrow; url-details answering 501 while fetches are off; visitors refused url-details before any lookup.

### The client

- A crate of its own, `hyscribe-fetch`: `get( url, home, args )` standing for `wp_safe_remote_get()`, with WordPress's `timeout`, `redirection`, `user-agent` and `limit_response_size` arguments and their defaults (5 seconds, 5 redirects)
- `hyper`'s HTTP/1.1 client connection (`hyper::client::conn::http1`), one connection per request and per redirect, closed after it: no pool, so no connection outlives the check that opened it, and no HTTP/2
- TLS through `rustls` and `tokio-rustls` with the `ring` provider, TLS 1.2 and 1.3, the certificate checked against the host's name; no setting skips verification
- The host's certificate store, read once at start where OpenSSL finds it (`openssl-probe`: its bundle file, else its certificate directories), parsed by `rustls-pki-types`. This is what `rustls-native-certs` reads on Linux; using `openssl-probe` directly leaves out `rustls-native-certs` and its macOS and Windows dependencies, which HyScribe never builds and cargo vet would need audited (decision 2)
- A host without a store fetches over HTTP only; the start log says `outbound fetches on over HTTP only: no certificate store was found`

### SSRF rules

Every hop, the URL and each redirect, passes all of these before a connection is made:

- WordPress's checks, as `wp_http_validate_url()` makes them: HTTP or HTTPS only, no user or password, no `:#?[]` in the host, ports 80, 443 and 8080, or the home URL's own port on its own host
- Stricter: the host is a plain DNS name or an IPv4 address (letters, digits, `-`, `.`, `_`); a URL without a scheme is refused
- The host is resolved once, and every address it resolves to must be public; one private answer refuses the hop (a name answering both is refused)
- The client connects to an address it checked, never resolving again, so DNS rebinding cannot swap the address after the check
- Public means: for IPv4, outside every range `wp_http_validate_url()` lists; for IPv6, global unicast (`2000::/3`) outside the IETF block with Teredo, documentation and 6to4, and an IPv4 address carried in an IPv4-mapped or NAT64 address checked as IPv4. Loopback, unique local (`fd00:ec2::254` included), link-local and multicast are refused
- No exemption for the site's own host: a site on a private address cannot fetch from itself
- A redirect's `Location` is made absolute as `WP_Http::make_absolute_url()` makes it and checked as a new URL; 300 to 303 and 307 to 399 are followed, as Requests follows them
- Proxies are not supported, so no proxy setting can route around the checks

### Limits

- Time: the `timeout` argument, at most 30 seconds, for the whole fetch, redirects included
- Size: the body is cut at `limit_response_size`, and at 4 MiB whatever the arguments; no `Accept-Encoding` but `identity` is sent, so nothing is decompressed
- Redirects: the `redirection` argument, at most 5; `redirection` 0 returns the redirect itself, as Requests does

### Nothing sent but the request

- Each request carries `Host`, `User-Agent`, `Accept-Encoding: identity` and `Connection: close` only
- No cookies, no `Authorization`, no `Referer`, no client certificate; a URL with a user or password is refused

### Hook points

- `http_request_args` and `pre_http_request` run under their names, with WordPress's arguments, in `WP_Http::request()`'s order: the arguments filtered, then a pre-answer, which ends the request without a connection
- Plain internal calls until the extension runtime exists (docs/EXTENSIONS.md). No hook skips a check: the limits apply to the filtered arguments, and the address rules are not arguments a filter can change
- `http_request_host_is_external` and `http_allowed_safe_ports` would only ever narrow the rules, never widen them (decision 4)

### The `.env` switch

- `HTTP_OUTBOUND`: `off` (default) makes no outbound client at all, so nothing can fetch; `on` makes one. Any other value stops the start
- The start log says which, and whether HTTPS can be fetched
- Proposed default: `off`, as the security model denies outbound network by default (decision 3)

### url-details

- `WP_REST_URL_Details_Controller` ported: the permission check (`edit_posts`, or the edit capability of any post type the REST API shows), `wp_http_validate_url()` as the argument's check, the user agent `WP-URLDetails/7.1.3 (+home)`, 150 KB read, the head's title, icon, description and Open Graph image read with WordPress's own regexes, matched by hand in linear time and fuzzed (`url_details`)
- Cached as PHP caches it: the site transient `g_url_details_response_` and the URL's MD5, for an hour, written as `set_site_transient()` writes it, and an expired one deleted on read as `get_site_transient()` deletes it. Shadow mode writes nothing, so it fetches every time
- A URL the client refuses on its first hop (an address that is not public, or a name that does not resolve) answers `rest_invalid_param`, as PHP's argument check answers it; a refused redirect answers `no_response`, as PHP does
- Visitors and users without the capability are refused before any lookup: PHP's argument check resolves names for anyone, which tells a visitor whether a name resolves; HyScribe answers them the permission error instead
- While `HTTP_OUTBOUND` is off, the route answers 501, as today; once accepted, off could answer as PHP answers a blocked request (`no_response`) instead (decision 5)

### Dependencies and cargo vet

New crates: `rustls`, `rustls-webpki`, `rustls-pki-types`, `ring`, `untrusted`, `tokio-rustls`, `openssl-probe`, `getrandom` 0.2, `zeroize`, `cc` (a build dependency of `ring`) with `find-msvc-tools` and `shlex`, and the `windows-*` crates `ring` names for Windows only. `hyper` gains its `client` feature, which pulls `want` and `try-lock`.

- Covered by imported audits: `untrusted`, `zeroize`, `shlex`, `want` and `try-lock`
- Trusted publishers, as Mozilla's, ISRG's, Bytecode Alliance's or Zcash's imported sets trust them: Joe Birr-Pixton (`ring`, `rustls`, `rustls-webpki`, `rustls-pki-types`), Dirkjan Ochtman (`tokio-rustls`, `openssl-probe`), Kenny Kerr (`windows-sys` and the `windows-*` crates, which also retires the exemption of `windows-sys` 0.61), rust-lang-owner (`cc`, pinned to 1.4.5, the newest they published) and Josh Stone (`find-msvc-tools`). These are `trusted` entries in `supply-chain/audits.toml`, not exemptions (decision 2)
- Reviewed and certified by HyScribe: `getrandom` 0.2.17 (the Linux path and every backend's I/O) and the `rustls-webpki` 0.103.16 to 0.103.17 delta (a certificate transparency parser)
- `cargo deny` allows every licence (`ring` is Apache-2.0 AND ISC)
- `ring` over `aws-lc-rs`: `aws-lc-rs` needs CMake and a C toolchain at build time, has no imported audit, and its FIPS build adds Go; `ring` is the provider `rustls` and `lettre` (ADR 0016) both offer, so the binary carries one

### Decisions

1. The client as above: one HTTP/1.1 connection per hop, no pool, no proxies, `ring` as the crypto provider
2. The supply chain: `trusted` publisher entries for the TLS and build crates instead of reviews of their 250,000 lines, mirroring the imported sets' trust; and `openssl-probe` in place of `rustls-native-certs`, which was named, for the same certificate store on Linux without its macOS and Windows crates
3. `HTTP_OUTBOUND` off by default, or on
4. The SSRF rules, stricter than WordPress: no exemption for the site's own host, IPv6 answers checked, every address of a name checked, and the two widening filters only ever narrowing
5. While fetches are off, url-details answers 501, or as PHP answers a blocked request; and whether link previews, which fetch any URL an editor pastes, need the RSS block's allowlist too (the security model's rule for fetches from core) or only the address rules
6. Visitors refused url-details before any DNS lookup, an intended difference from PHP's order

## Consequences

- url-details is built and tested behind `HTTP_OUTBOUND`; it answers 501 until this is accepted and the switch is on
- The RSS block, embeds, the dashboard's wordpress.org calls and the release notice fetch through this client, each with its own decision on what it fetches and from where. Embeds fetch only from an allowlist (2026-11-19): by default the hosts of WordPress's oEmbed providers, editable through `EMBED_ALLOWED_HOSTS`; other URLs stay plain links. Every redirect hop of an embed or RSS fetch must also be an allowed host, checked by a caller given predicate before the hop is resolved (`Client::get_within`), on top of the address checks; a hop off the list stops the fetch (no cache row, a plain link or the feed's unreachable error), a deliberate difference from PHP (2026-11-19)
- The mail transport (ADR 0016) shares `rustls` and `ring`
- `ring` compiles C and assembly, so the release build needs a C compiler for each musl target: `scripts/package.sh` uses clang, which targets both without a musl cross toolchain (found 2026-11-17)
- A site on a private address, behind a reverse proxy on the same network, cannot preview links to itself or other private hosts, where PHP could
- New `trusted` entries end on 2027-11-16 and need renewing, or audits

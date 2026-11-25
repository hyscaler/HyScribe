# 0024: The hosted registry and package signing

- Status: Accepted (2026-11-22, all eleven questions as recommended)
- Date: 2026-11-22

## Context

Extension distribution is decided in part (2026-11-21): extensions are distributed through a hosted registry the project runs, with no Git based index. Left to this ADR: how packages are signed and how the binary verifies them, the registry's API as HyScribe consumes it, and `hyscribe plugin install <name>` beside the local file. The security model already asks for it: "Packages signed; unsigned packages need an explicit operator override", "only embedded code and signed extension packages run", and a security review of every port "before a port enters the registry" (docs/SECURITY_MODEL.md).

What exists (ADR 0022 and its 11.3 amendment):

- `hyscribe plugin install <file>` reads a local `.hyx` (at most 256 MiB), loads its component once with its grants, keeps it as `packages/<id>-<version>.hyx` and swaps it into `plugins/<id>/`, keeping `faults.txt` and `approved.ext.toml`. A package that does not load changes nothing. The 11.3 amendment reserves the place for a signature check: before the package is read
- `hyscribe plugin delete <id>` removes the folder and the kept packages; the Plugins screen never installs, updates or deletes, as `install_plugins`, `update_plugins` and `delete_plugins` are never granted (docs/ADMIN.md)
- An update that asks for more grants or limits needs approval again, and the extension does not run until it is activated again
- Every outbound request goes through `hyscribe-fetch` under ADR 0019: no client unless `HTTP_OUTBOUND=on`, every address of every hop public, 30 seconds, 4 MiB, redirects checked, only `Host`, `User-Agent`, `Accept-Encoding` and `Connection` sent

What PHP WordPress does:

- `wp_update_plugins()` posts every installed plugin's headers, the site's locale and its WordPress version to `api.wordpress.org/plugins/update-check/1.1/`, with the home URL in the user agent (`WordPress/7.1.3; https://example.com/`), twice a day, and stores the answer in the `update_plugins` site transient
- The Plugins screen prints an update row under each plugin that transient names (`wp_plugin_update_rows()`), the `Update Available` view and the menu's count; all three only for users with `update_plugins`, so a user without it sees none of them
- Packages are zip files over HTTPS. WordPress 5.2 added Ed25519 signatures, checked against `wp_trusted_keys()` for downloads from wordpress.org hosts, but a missing or failed signature soft fails by default (`wp_signature_softfail`) and the package installs anyway

HyScribe's position differs on three points: a plugin runs only inside the sandbox with what it was granted, files never change from the browser, and an outbound call is denied unless switched on. The registry has to keep all three.

## Decision

### Signature format: minisign, verified with crates already audited

- Signatures are minisign signatures, the prehashed form (`ED`): Ed25519 over the BLAKE2b-512 of the file, plus a global Ed25519 signature over the first signature and a signed "trusted comment". Keys and signatures are minisign's text formats: a public key is the base64 of `Ed`, an 8 byte key id and the 32 byte key; a signature file is four lines (untrusted comment, signature, trusted comment, global signature)
- Signers use the standard `minisign` tool or any compatible one, offline, outside this repository; nothing on the signing side is a HyScribe dependency
- HyScribe verifies with its own reader of those formats over crates already in the lockfile and covered by cargo vet: `ring` 0.17 (`ring::signature::ED25519`, trusted publisher since ADR 0019), `blake2` 0.10.6 (audited `safe-to-deploy` by HyScribe for `hyscribe-auth`) and `base64` 0.23.1 (audited `safe-to-deploy`). SHA-256 for the registry's documents comes from `ring::digest`. No crate is added, and no exemption grows (question 1)
- The legacy minisign form (`Ed`, the whole file signed without a prehash) is refused, so a 256 MiB package is hashed as it streams and never held for the signature check
- The reader takes at most 4 KiB, exactly four lines, each field its exact length, the trusted comment printable ASCII, and refuses anything else; it gets a fuzz target (`ext_signature`), as every parser does

Alternatives looked at and not taken:

- `minisign-verify`: small and dependency free, but not in the tree, without an imported audit, and it carries its own Ed25519 and BLAKE2b code where `ring` and `blake2` are already trusted here
- `ed25519-dalek`: brings `curve25519-dalek` and its tree for what `ring` already does
- Sigstore (cosign, Rekor): a large dependency tree and an online transparency log on every verification, against a self-contained binary that verifies offline
- OpenPGP: a large format and parsers with a long vulnerability history
- TUF through `tough`: the right model, but a large tree; this design takes TUF's ideas (offline roots, expiring signed metadata, rollback protection) at the size HyScribe needs

### Keys: offline roots in the binary, online keys in a signed key list

- Root keys: three Ed25519 keys, held offline by different people, their public keys compiled into the binary. They sign one document only, the key list, and any two of the three must sign it (question 2)
- The key list (`keys.json`): a `serial`, an `expires` time (at most 13 months after signing, so a root signature is renewed every year), and the online keys, each with its key id, public key, what it may sign (`index`, `packages`) and the times it may sign between (`not_before`, `not_after`, at most one year apart); and `revoked`, the ids of keys that no longer count for anything
- Online keys sign the index and the packages. They live with the registry's signing step, never with the web server that serves the files: serving the files needs no secret
- Rotating an online key: a new key list, signed by two roots, names the new key and gives the old one a `not_after`; packages signed by the old key while it was valid stay valid
- Revoking an online key: the key list names it under `revoked`; every package it signed stops verifying, and the registry signs the good ones again with a new key (only `.minisig` files change, never the packages)
- Rotating or replacing a root key: only by a HyScribe release, which carries the new set. A lost root leaves two, which still sign; a stolen root alone cannot sign a key list
- Rollback: the binary keeps the highest key list serial it has accepted, and refuses a lower one or one past its `expires`

### Who signs, and what is signed

- The registry signs, once a package passed its review (the security model's review of each port, its CVE check and the porting pipeline's gates). Authors do not sign with keys of their own in this version (question 3)
- A package's signature file is `<id>-<version>.hyx.minisig`, detached, because a zip cannot sign itself. Its trusted comment binds the package to its name, so a signature for one package cannot be moved to another:

  ```
  trusted comment: hyscribe-package id=contact-form-7 version=0.1.0 sdk=0.2 sha256=<64 hex> signed=1795161600
  ```

- Verifying a package: the signature's key id names a key of the accepted key list, allowed to sign `packages`, not revoked, and `signed` lies within its `not_before` and `not_after`; both signatures verify; the trusted comment's `sha256` is the file's; and once the package is read, its manifest's `id`, `version` and `sdk` are the comment's. Any mismatch refuses the package before `Hyx::read`, the place ADR 0022's amendment reserved
- The index is signed the same way (`hyscribe-index serial=<n> expires=<unix>`), and the key list by two roots (`hyscribe-keys serial=<n> expires=<unix>`)

### The registry's API, as HyScribe reads it

Static files under one HTTPS base URL, versioned by path, so the registry can be served by any static host or cache and a mirror needs nothing but a copy. HyScribe only reads; it never posts anything to the registry.

| Path | What it is | Verified by |
| --- | --- | --- |
| `v1/keys.json` and `v1/keys.json.<n>.minisig` (`n` 1 to 3) | The key list | Two signatures by distinct compiled in roots |
| `v1/index.json` and `.minisig` | Every extension: id, kind, and its latest version for each SDK, with the SHA-256 and size of its version document; revoked versions | An online key allowed to sign `index` |
| `v1/extensions/<id>.json` | One extension's versions: `version`, `sdk`, `requires_hyscribe` (the oldest binary it needs), `replaces`, `replaces_versions`, `size`, `sha256`, `released`, a page on the registry's site, and `revoked` with a reason | Its SHA-256 and size in the signed index |
| `v1/packages/<id>/<id>-<version>.hyx` and `.minisig` | The package | Its size and SHA-256 in the version document, then its own signature |

- The index carries a `serial`, which never goes down (kept like the key list's), and an `expires` time, 7 days after signing; the registry signs a new one at least daily. A past `expires` refuses the index: no install by name and no offers until a fresh one is fetched, so a stale or frozen copy cannot hide a revocation or an update for long
- Documents are JSON, read by `hyscribe-phpser`'s bounded `json_decode()` (already fuzzed through `rest_serve`) with every field checked: ids are slugs (`manifest::slug`), versions the manifest's version characters, hashes 64 lowercase hex digits, times integers, URLs paths under the base. Size caps: 64 KiB for the key list, 4 MiB for the index (ADR 0019's cap), 1 MiB for a version document. Fuzz target `registry_index` over the three readers
- Verified copies are kept in `hyscribe/registry/` (`keys.json`, `index.json` and their signatures, re-verified when read), with `state`, two lines `keys <serial>` and `index <serial>`, written whole and renamed into place as `faults.txt` is. Nothing goes into the database (ADR 0003)

### Outbound calls under ADR 0019

- Every request goes through `hyscribe-fetch` with `get_within`, its predicate allowing the registry's host only: every rule of ADR 0019 holds (every address of every hop public, the connection made to the address checked, no proxies, TLS verified against the host's store), and a redirect off the registry's host stops the request
- The base URL must be HTTPS; an `http:` registry URL stops the start
- What is sent: `Host`, `User-Agent: HyScribe/<version>`, `Accept-Encoding: identity`, `Connection: close`. No site URL, no list of installed extensions, no locale, no user or visitor data, unlike WordPress's update check. The background check fetches only the key list and the index; a version document is fetched only by a command that installs or updates that extension, so the registry learns which extension a site has only when the operator installs or updates it (question 11)
- The key list and the index are read under ADR 0019's limits (30 seconds, 4 MiB). A package is larger than 4 MiB allowed: its download is streamed to `hyscribe/packages/.download-<id>-<version>`, refused past the size the version document gives (at most 256 MiB, the package format's limit), hashed as it arrives, with 30 seconds at most between two reads and 15 minutes in all; it is renamed into place only once its size, SHA-256 and signature verify, and removed otherwise. This is `hyscribe-fetch`'s one caller with a larger limit, a function of its own beside `get_within`, so no other caller can ask for it (question 7)
- When fetches may happen (question 6):
  - The commands below are run by the operator on the server, an explicit request like running `curl`: they fetch from the registry whatever `HTTP_OUTBOUND` says, and only from the registry
  - The service checks for updates only with `HTTP_OUTBOUND=on` and a new key `EXTENSION_UPDATE_CHECK=on` (off by default): a task of the service, like cron's (ADR 0017) but not a WordPress cron event, fetching the key list and the index every 12 hours, as `wp_update_plugins()` runs, writing only `hyscribe/registry/`
- `EXTENSION_REGISTRY` sets another base URL (a mirror, or a registry of one's own), HTTPS only; the compiled in roots still decide what is trusted, so a mirror can withhold files but cannot forge any (question 8)

### Installing by name

`hyscribe plugin install <name>[@<version>]`, beside `hyscribe plugin install <file>`. An argument ending in `.hyx` or holding a `/` is a file; any other is a name, which must be a slug.

1. Fetch and verify the key list and the index (or use the kept copies when a fresh index is under 12 hours old)
2. Choose the newest version of `<name>` (or the one asked for) that is not revoked, whose `sdk` this binary links (0.1 and 0.2 today) and whose `requires_hyscribe` is not newer than the binary; if none fits, say why (a newer SDK, a newer HyScribe, revoked)
3. Fetch the version document and check it against the index; download the package and its signature, checked as above
4. Run the install of ADR 0022's amendment on the verified bytes: read, load once with its grants, keep in `packages/` (now with its `.minisig` beside it), swap into `plugins/<id>/`
5. Write `plugins/<id>/origin.txt`, a name no package can carry (the format allows nothing else at the top): `registry <key id> <sha256>`, `signed <key id> <sha256>` for a signed local file, `trusted <key id> <sha256>` for an operator's key, or `unsigned <sha256>`
6. Print what the extension asks for, grants, sensitive hooks and limits, and that it runs once activated from the Plugins screen

Asking for a version older than the installed one installs it only with `--downgrade`, so a typo cannot roll back a fix; a revoked version is never installed by name.

`hyscribe theme install <name>` (10.7) is the same with `kind = "theme"` into `themes/<id>/`.

### Local files, signed or not

- `hyscribe plugin install <file>` looks for `<file>.minisig` beside it. Found, it must verify as a registry package does, against the kept key list (fetched first if there is none, which an offline server can copy in by hand), and `origin.txt` says `signed`
- Not found, or the package is refused by its signature: the install is refused unless the command says `--unsigned`, the operator's explicit override the security model asks for (question 4). An unsigned package still passes every other check (the format, the manifest, the component loading with its grants, approval at activation): signing says where a package came from, the sandbox limits what it can do
- `EXTENSION_TRUSTED_KEYS`: minisign public keys an operator trusts for packages they build themselves (a site's own extensions, or a developer's test builds), comma separated; a package signed by one of them installs without `--unsigned`, its trusted comment checked as above, and `origin.txt` says `trusted`. These keys never sign an index and never stand in for the registry (question 4)
- The Plugins screen says so in HyScribe's own part of the row (where `after_plugin_row_meta` prints, beside the approval note): "Unsigned package, installed from a local file by the server's operator", or "Signed by a key this site trusts"
- `hyscribe plugin delete` also removes the kept `.minisig` files of the packages it removes

### Revocation

- The index lists revoked versions (id, version, SHA-256, a reason), and the key list revoked keys
- When a fetched index revokes an installed version, or the key list revokes the key `origin.txt` names (and no newer signature from a valid key is found for the same SHA-256), the extension stops loading at once: the runtime treats it like a disabled extension, so `active_plugins` is untouched and PHP WordPress keeps the original plugin active on a switch back (ADR 0003). The Plugins screen shows the reason in an error row (question 5)
- The state is kept as a line `revoked <unix seconds> <reason>` in `origin.txt`, read when the extension is loaded, so a restart without network keeps it refused
- The way out is an update to a version that is not revoked (`hyscribe plugin update <id>`), or deleting it; an operator who insists can reinstall the kept package with `--unsigned`, which the screen then says
- Unsigned and operator-signed packages are not in the registry's lists, so nothing revokes them

### Updates

- `hyscribe plugin update --check` fetches the key list and the index and lists the installed extensions with a newer compatible version, its SDK and whether it asks for more; it installs nothing
- `hyscribe plugin update <id> ...` or `--all` installs the newest compatible version of each, by the install above. An extension installed `unsigned` or `trusted` is never offered an update from the registry, since nothing says it is the registry's extension of that id; installing by name over it says so and replaces it only with `--replace-local`
- An update whose manifest asks for more than the approved one (a grant, a sensitive hook, an outbound host, a limit) would stop a running extension until it is activated again, so it is refused unless the command says `--allow-new-grants`, and the command prints what is new (question 10)
- No automatic installs: the service checks and the screen tells, the operator installs. Requests in flight finish on the old component, as EXTENSIONS.md says

### Updates on the Plugins screen

WordPress's update UI is shown only to users with `update_plugins`, which HyScribe never grants, so today the screen correctly prints none of it. Offers are shown as follows (question 9):

- PHP's cached offers in the `update_plugins` site transient, written by a PHP WordPress on the same database for the PHP plugins, are never read for the screen and never written: they offer wordpress.org packages HyScribe cannot install, as decided for core's notice
- In their place, an extension with a newer compatible version in the kept index gets an update row in PHP's own markup and classes (`tr.plugin-update-tr`, `div.update-message.notice.inline.notice-warning.notice-alt`, and `update` on the plugin's row), so the screen's CSS and script treat it as WordPress's. Its text is PHP's string for a user who may not update, "There is a new version of %1$s available. View version %2$s details.", the link going to the extension's page on the registry's site (a new tab, `rel="noopener noreferrer"`), followed by HyScribe's sentence: "Install it on the server with `hyscribe plugin update <id>`." An update asking for more says so in the same row
- A revoked installed version gets the row with `notice-error` and the registry's reason instead
- Shown to users with `activate_plugins`, who are the ones that see the Plugins screen. No menu count and no `Update Available` view, which PHP gates on `update_plugins` too; these stay as PHP prints them for a user without it
- The rows come from the kept, verified index only; rendering a page never fetches. An index past its `expires` gives no rows, and the screen says the update information is out of date
- Dashboard > Updates (`update-core.php`) stays refused as PHP refuses it without `update_core`

### What this changes elsewhere

- `hyscribe-ext` gains the signature reader and the verification (no network), with `ring`, `blake2` and `base64` as direct dependencies, all already in the lockfile
- A new crate `hyscribe-registry` reads and keeps the key list, the index and the version documents, chooses versions, downloads through `hyscribe-fetch` and hands verified bytes to `hyscribe_ext::installed::install`; it holds the compiled in roots
- `hyscribe-fetch` gains the streamed download with its own limits
- The switch-back suite's `INSTALL` lines and `hyscribe-ext`'s tests sign the test extension with a test key passed through `EXTENSION_TRUSTED_KEYS`, so the signed path is the one exercised, with one case for `--unsigned` and one for each refusal
- Tests run against a registry served from a local folder over HTTPS with test roots; the compiled in roots are swapped for test ones in test builds only, never by configuration

### Questions

1. Signatures in minisign's prehashed format, verified by HyScribe's own small reader over `ring`, `blake2` and `base64`, all already audited or trusted here, rather than taking `minisign-verify` (which would need a HyScribe audit and brings its own Ed25519 code) or a larger system such as Sigstore or TUF through `tough`. Recommendation: minisign format, own reader, no new crate
2. The key hierarchy: three offline root keys compiled into the binary, any two of them signing the key list, which expires after at most 13 months; online keys valid for at most a year, signing the index and the packages. Recommendation: as described. Simpler alternative: one root key and one online key, at the cost of a single point of loss or theft
3. Who signs: the registry alone, after its review, in this version; author keys (a second signature saying who built a package) could come later without changing the format. Recommendation: the registry alone
4. Unsigned and locally built packages: refused unless the command says `--unsigned`, and an `EXTENSION_TRUSTED_KEYS` setting for keys an operator trusts for their own builds. Recommendation: both; no `.env` switch that turns signature checks off for every install
5. Revocation of an installed version or of its signing key: the extension stops loading at once, `active_plugins` untouched, the screen saying why; or only a warning while it keeps running. Recommendation: stop loading (fail closed), as a disabled extension does
6. When fetches happen: the operator's commands fetch from the registry whatever `HTTP_OUTBOUND` says; the service's background check needs `HTTP_OUTBOUND=on` and `EXTENSION_UPDATE_CHECK=on`, off by default. Recommendation: as described; the alternative is to make the commands need `HTTP_OUTBOUND=on` too, which would force a site to let its server fetch link previews and embeds to install a plugin
7. Package downloads beyond ADR 0019's limits: streamed to disk up to the signed size (at most 256 MiB), 30 seconds between reads, 15 minutes in all, from the registry's host only, through a function of its own. Recommendation: accept, as an amendment to ADR 0019 for this caller only
8. `EXTENSION_REGISTRY` to point at a mirror or another registry, HTTPS only, still verified against the compiled in roots. Recommendation: allow it
9. The Plugins screen: never show PHP's cached `update_plugins` offers (extending the core rule to plugins), and show the registry's offers in PHP's update row markup with the command to run, to users with `activate_plugins`, without a menu count or an `Update Available` view. Recommendation: as described
10. Updates installed only from the command line, no automatic updates in this version, and an update asking for more grants or limits refused without `--allow-new-grants`. Recommendation: as described; automatic security updates by the service can be a later decision once the registry has run for a while
11. Privacy: no site URL, installed list, locale or user data sent; version documents fetched only on install or update. Recommendation: as described, an intended difference from WordPress's update check

## Consequences

- Every package that runs came from the registry's review, a key the operator chose to trust, or a command that said `--unsigned`; the Plugins screen says which
- A stolen online key is limited by its year of validity and ends with one revocation; a stolen root alone can sign nothing; serving the registry's files needs no secret, so a compromised web host can only withhold files, which the index's 7 day expiry turns into a visible "out of date" rather than a silent freeze
- No crate is added; `ring`'s trusted entry (until 2027-11-16) and the `blake2` and `base64` audits now also cover package verification, and new versions of them need the same care
- Two new parsers (the signature reader and the registry's documents) and their fuzz targets; the streamed download is new attack surface in `hyscribe-fetch`, reviewed with 11.9
- `hyscribe plugin install <file>` without a signature now needs `--unsigned`, a change for anyone installing local builds today (the tests among them)
- The registry has to be run with care: offline roots, a signing step apart from the web host, a fresh index at least daily. If no index is signed for 7 days, no site can install by name or see offers until one is
- Nothing is written to the database; what the registry gives a site is in `hyscribe/registry/`, `packages/` and the extension's `origin.txt`
- The same root keys and formats could sign HyScribe's own releases for a self-update, which is left to its own decision

## Amendment: signatures and local installs as built (2026-11-22)

Built on `adr-0024-signing`: the signature reader, the key list and index, `origin.txt`, `hyscribe plugin install <file>` with `--unsigned` and `EXTENSION_TRUSTED_KEYS`, and revocation. Install by name, updates, version documents, the service's check and the Plugins screen rows follow.

- **Document fields.** `keys.json` is `{"serial", "expires", "keys": [{"id", "public_key", "may_sign": ["index", "packages"], "not_before", "not_after"}], "revoked": [<key id>]}`. `index.json` is `{"serial", "expires", "extensions": [{"id", "kind", "latest": {<sdk>: <version>}, "sha256", "size"}], "revoked": [{"id", "version", "sha256", "reason"}]}`. A key id is minisign's: 16 uppercase hex digits, the key's 8 id bytes read as a little endian number, and `id` must equal the id inside `public_key`. A member missing, unknown or out of its bounds refuses the document. A revocation reason is printable ASCII of at most 1000 bytes. The index's extension entries are checked but not kept until install by name reads them; the `registry` word of `origin.txt` comes with install by name too, so `signed` and `trusted` are the only signed origins today.
- **Expiry bound.** A key list whose `expires` lies more than 13 months (13 times 31 days) after the time it is checked is refused, as is one past its `expires`. An online key's window is at most 366 days.
- **When revocation applies.** The installed plugins are checked against the kept key list and index whenever a command reads them; today that is `hyscribe plugin install` of a file signed by a registry key. The service's update check and `hyscribe plugin update` will apply it too once they exist.
- **A bad kept registry fails closed.** A kept key list or index that does not verify, has expired or goes back refuses the install of a registry-signed local file. The command does not fall back to installing without the index's revocations.
- **Extensions installed before this amendment** have no `origin.txt` and load as before. A file that exists but does not read refuses the extension, as a revocation does.
- **Back-dating.** An online key's window is checked against the `signed` time in the package's trusted comment, which the key's own global signature covers. A stolen key can therefore sign a package dated inside its old window after the window has ended. Only revoking the key in the key list stops this, so a lost online key must always be revoked, not just left to expire.

## Amendment: install by name and updates as built (2026-11-22)

Built on `adr-0024-registry`, after the amendment above.

- **Version documents.** `v1/extensions/<id>.json` is `{"id", "versions": [{"version", "sdk", "requires_hyscribe", "replaces", "replaces_versions", "size", "sha256", "released", "page", "revoked"}]}`, its versions oldest first (`released` never goes down, no version twice). `requires_hyscribe` is a HyScribe release number, three decimal numbers (`0.1.0`), compared number by number with the binary's own. `replaces`, `replaces_versions` and `revoked` are a string or `null`; `page` is a path under the base URL (letters, digits and `-._~/`, relative, no empty, `.` or `..` segment, at most 255 bytes). The document must have the size and SHA-256 its entry in the signed index gives, and the entry's `id`. The index's entries are kept since this slice, with the size and SHA-256 of each version document.
- **The package download.** `hyscribe_fetch::Client::download` streams a package to a file: HTTPS only, every hop on the first URL's host (a redirect elsewhere stops it), every address rule of ADR 0019, a 200 answer of exactly the signed size (at most 256 MiB), 30 seconds at most for each hop and between two reads, 15 minutes in all. It answers the file's SHA-256; any file already at the path is removed first, and the file is removed on any failure. WordPress's request hooks (`http_request_args`, `pre_http_request`) do not run on it, as it is no WordPress request. Its body reading also gives `get` a separate step, read within the same deadline as before.
- **Install by name and updates.** `hyscribe plugin install <name>[@<version>]` and `hyscribe plugin update --check | <id>... | --all` always fetch a fresh key list and index (the kept copies are not reused within 12 hours), verify them, keep them in `registry/` (refused if either serial goes back, `state` written last) and apply their revocations to the installed plugins before doing anything else. The versions of a version document are ordered by their place in it: "newer" means listed later, and an installed version the document no longer lists counts as newer than any, so going to a listed one needs `--downgrade`. Replacing a plugin installed from a local file (unsigned, by an operator's key, or from before ADR 0024) needs `--replace-local` and is never a downgrade. `origin.txt` says `registry <key id> <sha256>` for a package installed by name; a local file signed by a registry key keeps `signed`, and both count as the registry's for updates and revocation.
- **New grants.** The runtime runs an extension only while its grants and limits equal the approved ones, so any difference, fewer grants included, stops it until it is activated again. An update whose grants or limits differ from the approved ones is therefore refused without `--allow-new-grants`, listing what is new (or "fewer"). A plugin never activated has nothing approved and nothing is refused.
- **Before the package is installed** its manifest must give the version document's `version`, `sdk`, `replaces` and `replaces_versions`; its signature is checked (the key allowed to sign packages, the version not revoked) before the package is read.
- **`--check` and the index alone.** `--check` fetches only the key list and the index, as the ADR says, so it offers the version the index's `latest` gives for the newest SDK this binary links when it differs from the installed one. It cannot tell from the index whether that version needs a newer HyScribe or asks for more grants; `hyscribe plugin update <id>` says so when it refuses.
- **No default registry yet.** Without `EXTENSION_REGISTRY` the commands say the hosted registry has no address yet, as the compiled-in roots stay empty until the registry's keys are created.

## Amendment: the update check and the Plugins screen as built (2026-11-23)

Built on `adr-0024-update-check`, after the amendments above. With it every part of this ADR is built.

- **The service's check.** `EXTENSION_UPDATE_CHECK` (off by default) runs only with `HTTP_OUTBOUND=on` and `EXTENSION_REGISTRY` set; with either missing the start log says the check is off and why, and the service starts. The check is a task of its own beside the server and cron, in both modes (it writes nothing to the database), sharing the log a call at a time. It runs at the start and then every 12 hours; no time of the last check is kept, so a restart checks again. Each check does what `hyscribe plugin update --check` does: a fresh key list and index, verified, kept in `registry/` (refused if either serial goes back), their revocations written to `origin.txt`, and the offers logged as `update check:` lines; it installs nothing. A check is bounded at 300 seconds beyond its fetches' own 30 seconds, a failure is logged as `update check: failed: <why>`, and stopping the service ends a check under way (its files are written in one step after its last fetch, so a stopped check leaves nothing half kept). The runtime reloads an extension whose `origin.txt` changed, so a revocation the check writes stops the extension on the next request.
- **The screen reads, never writes.** The Plugins screen reads the kept key list and index at the request's time against the compiled-in roots (`kept::shown`): checked against `state` like a command's read, but `state` is never raised. A kept index that has expired, does not verify or goes back gives no offers and a notice of HyScribe's own above the list ("The update information from the plugin registry is out of date"); a site with no kept key list or index (one not using the registry) gets neither.
- **Offers.** An extension the registry signed (`registry` or `signed` in `origin.txt`) is offered what `--check` offers: the index's `latest` for the newest SDK this binary links, when it is not the installed version. The index cannot tell whether that version asks for more grants or needs a newer HyScribe, so the row cannot say so either; `hyscribe plugin update <id>` says so when it refuses.
- **The update row is PHP's.** `wp_plugin_update_row()`'s markup for a user without `update_plugins` (`tr.plugin-update-tr`, its `id`, `data-slug` and `data-plugin`, the colspan of the columns shown, `active` beside an active plugin), recorded by `reference/plugins/golden.sh` in the variants `x24update`, `x24active`, `x24revoked` and `x24hidden`, where a must-use filter supplies the `update_plugins` transient and hooks the row as `wp_plugin_update_rows()` would; HyScribe's sentence is printed where `in_plugin_update_message-{$file}` fires. Three differences are intended and named in the test: the plugin's row gets `update` (PHP adds it only for users with `update_plugins`), the details link opens in a new tab with `rel="noopener noreferrer"` instead of PHP's ThickBox frame, and a revoked version's first sentence is the revocation.
- **Where the details link goes.** The index carries no page, and a version document, which does, is fetched only by a command that installs or updates (question 11), so the link goes to the extension's `Plugin URI` from its installed manifest: PHP's own fallback when an offer gives no URL. Linking to the registry's page instead would need the index to carry it.
- **Revoked rows.** A `revoked` line in `origin.txt` gives the row with `notice-error` (PHP's class for an offer the site cannot take), the reason escaped, and the commands that update or delete it; it takes the place of any offer. It needs no index, so it shows while the index is out of date.
- **Origins.** HyScribe's own part of the row says "Unsigned package, installed from a local file by the server's operator" or "Signed by a key this site trusts", on every row of such a package, active or not; a package the registry signed, or one installed before ADR 0024, says nothing.
- **Escaping.** Everything the row prints from the index, the manifest or `origin.txt` goes through `esc_html`, `esc_attr` or `esc_url`, the plugin's name through the screen's `wp_kses` as before.

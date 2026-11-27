# History

Dated log of project events and decisions, newest first. Add an entry whenever a decision is made, a milestone changes state, or direction shifts. Link ADRs where they exist.


## 2026-11-27: Windows archive

- Decision ([ADR 0005](docs/adr/0005-single-binary-distribution.md), amendment of 2026-11-27): Windows amd64 is a release platform from the release after v0.1.0, as `hyscribe_<version>_windows_amd64.zip` with its `hyscribe-latest-windows-amd64.zip` alias: the same files as the other archives, the binary being `hyscribe.exe`, with no installer or MSI. Built on a GitHub-hosted Windows runner (`windows-2025`) with Visual Studio's toolchain and the C runtime linked in, never cross-compiled; unsigned for now; arm64 Windows not built
- The service keeps its design and commands rather than becoming a Windows service: a detached background process holding the lock that decides "running", the lock on `run/hyscribe.lock` beside the pid file because Windows locks refuse reads, and in place of signals a named pipe of the server's that only its own user, administrators and the system can write to (a stop request taken as SIGTERM, then a kill request as SIGKILL). Everything stays in safe Rust; no `unsafe` was added
- Ported behind `cfg(windows)` with Unix unchanged: file modes (none on Windows; files take the folder's access list), the `.env` mode check (not run), the console's echo for the terminal wizard's passwords (through `winapi-util`, already in the tree), the kernel's random source (`getrandom`, already audited, where Unix reads `/dev/urandom`), the extension runtime's file stamps, `DB_HOST` sockets (refused: the driver has no named pipes), upload names Windows would misread (device names, `:`, trailing dots and spaces, refused there), and Site Health's build target. Not matched and documented instead: the root refusal (an administrator is not told apart), free disk space, and Windows's certificate store (only a PEM bundle named by `SSL_CERT_FILE`)
- Line endings: `.gitattributes` (`* -text`) checks every file out as stored, so the templates, goldens and embedded files keep LF on Windows
- CI's `windows` job runs clippy, a build, the tests of the crates that need no database and the Windows tests of `hyscribe-service` and `hyscribe-http`; the release's `build-windows` job builds the zip twice and compares the bytes (the same), checks that no C runtime has to be installed, and smoke-tests it in Git for Windows's bash against the default install fixture in MariaDB 11.8's zip for Windows. The release carries a third SBOM, for Windows

## 2026-11-27: A container image, built from the release archives

- Decision ([ADR 0005](docs/adr/0005-single-binary-distribution.md), amendment): the optional container image of the decision is built, as `docker.io/hyscribe/hyscribe` and `ghcr.io/hyscaler/hyscribe` (the same digests), for `linux/amd64` and `linux/arm64`, tagged `<version>`, `<major>.<minor>` and `latest`. The archives stay the primary distribution; macOS and Windows users run the Linux image under Docker Desktop
- Assembled, not compiled: `docker/Dockerfile` copies the binary, licences and docs out of the two Linux release archives onto distroless's `static-debian13` (`nonroot`, pinned by digest, CA certificates and no shell). It has no `RUN`, so the arm64 image needs no emulation, and `scripts/docker-image.sh` checks that each architecture's binary is its archive's byte for byte. Timestamps come from the commit and BuildKit's provenance is left out, so a rebuild without cache gives the same digest, which the release compares
- `HYSCRIBE_HOME`: the installation's folder was the binary's own, which in a container would put the binary in the data volume and keep an old one there past every upgrade. The environment variable now names another folder (absolute and existing, or the start stops); the image sets it to its volume `/hyscribe`. Archive installs are unchanged
- Publishing: the release workflow's publish job, for a `v*` tag only and after every gate, pushes to GitHub Container Registry with the job's own token (`packages: write` there only), attests the image, and copies the same manifest to Docker Hub with `docker buildx imagetools create` when the secrets `DOCKERHUB_USERNAME` and `DOCKERHUB_TOKEN` are set; without them it logs a notice and the release goes on. Manual runs build and check the image and push nothing
- Tested on the self-hosted Linux runners: CI's package job and the release's amd64 build make the image from the fresh archive and run `scripts/docker-smoke-test.sh` against the default install fixture: an existing site from the environment, then a new site through the browser wizard answered with curl, a restart from the volume, the health check and clean stops, as the image's user on a read only root filesystem
- Trying it: `docker/compose.yaml` runs the image with MariaDB 11.8 on `127.0.0.1:8080`, the password in Compose's own `.env`; `compose.existing.yaml` adds a copy of an existing site's dump and `wp-content`. `docs/operators/docker.md` covers both, upgrades, backups of the volume, logs and the security defaults
- Changed the same day, before any image was pushed: the image is built per published release by `docker.yml` from the release's own archives on GitHub (each archive's build attestation and its `SHA256SUMS.txt` line checked, then `ADD --checksum`), on Debian `trixie-slim` pinned by digest with Debian's CA certificates, for amd64 and arm64 with every build step on the build host's platform (no emulation). The release and CI workflows no longer build it, so the distroless base and the release job's publishing above are replaced. v0.1.0 gets no image, as its binary has no `HYSCRIBE_HOME`; the first release after this one is the first with an image.

## 2026-11-27: Extension REST routes served (SDK 0.5)

- Decision ([ADR 0022](docs/adr/0022-extension-host.md), amendment): an extension's `register_rest_route()` routes are served through hyscribe-rest after core's routes, so no extension can shadow a core route, and core namespaces are refused at registration; a permission callback is required and allows only on `true`; an answer may set only a listed set of headers; a fault inside a route gives PHP's fatal error answer and counts as any callback's fault. SDK 0.5 (WIT `@0.5.0`; 0.1 to 0.4 still link) adds `rest::response` and `rest::error`.
- Tested against PHP: `reference/rest/extension.list` with the plugin the test extension stands in for, 240 cases (40 requests as a visitor and five roles), all matching but PHP's JSON fatal error body, which also carries the PHP error and stack. Not served yet: batches to an extension route (501), route schemas, `_embed` of extension links, request headers beyond the content type.

## 2026-11-27: Three small differences from PHP closed: slash runs, X-Redirect-By, and pagination arithmetic

- Slash runs in the path under plain permalinks: `redirect_canonical()` collapses `//` in the path for every request, and HyScribe did so only under pretty permalinks. `//` answered 200 where PHP redirects 301 to `/`, `//index.php` redirected to `//`, and `/blog//` on a site in a folder answered 200. The plain path now strips a trailing `/index.php`, replaces an inner `/index.php/`, adds the front page's slash and collapses runs of slashes in that order, as PHP does, and a request such as `//blog/` outside a site's folder is redirected into it. `reference/pages/default.list` and `folder.list` gained 55 cases (plain, `/%postname%/` and a date structure, root and folder sites, with and without query strings) recorded from PHP.
- `X-Redirect-By: WordPress` was sent by the login, admin and comment redirects and missing from the others PHP makes with `wp_redirect()`: the canonical 301, the feed 301, the favicon 302 and the trackback 302 now send it too, through one helper. The value is PHP's default; ADR 0008 keeps it unbranded on purpose. `reference/pages/golden.sh` and `reference/trackback/golden.sh` now record the header as a last column, and the tests compare it. Redirects PHP does not make through `wp_redirect()` (the `/wp-login.php` and `/wp-admin` forwards to hs-login and hs-admin, and the setup wizard's) are unchanged.
- REST pagination: an `_envelope` holds `X-WP-Total` and `X-WP-TotalPages` as strings for comments and as numbers for every other collection, as PHP's controllers set them. An `offset` past the integers, such as `9223372036854775807` on tags or users, now gives the page PHP computes (`(int) ceil( ( $offset / $per_page ) + 1 )`, with its float arithmetic and the cast that wraps to a negative number), so the `Link` header carries `rel="next"` with a negative page where HyScribe sent `rel="prev"`. `reference/rest/pagination.list` records 50 requests for each identity.

## 2026-11-26: macOS archives, and the workflows arranged for two runners

- Decision ([ADR 0005](docs/adr/0005-single-binary-distribution.md), amendment): macOS is a release platform from the release after v0.1.0, as `hyscribe_<version>_darwin_arm64.tar.gz` and `hyscribe_<version>_darwin_amd64.tar.gz` with their `hyscribe-latest-darwin-<arch>.tar.gz` aliases. The same plain binary in a `tar.gz` as on Linux; no disk image, installer or app bundle. Built on a GitHub-hosted macOS runner (`macos-15`) with the Rust of the Linux image, never cross-compiled; the Linux builds stay on the self-hosted runners
- What had to change for macOS was small, since the code already kept to what Unix systems share: outbound TLS found no certificates (the probe HyScribe uses does not know macOS's bundle, `/etc/ssl/cert.pem`, which is now the fallback when the probe names no file; nothing changes on Linux, where the probe already looks there), Site Health's build target read `aarch64-macos` (now `aarch64-apple-darwin`, the one item compiled for macOS only), and the service's tests ran `/bin/true`, which macOS keeps elsewhere. `scripts/package.sh` and `scripts/third-party-notices.sh` keep to what GNU and BSD tools share and to bash 3.2; the archive is written by GNU tar on both systems (Homebrew's on macOS), and the notices list the crates each system links, which differ by three. The Linux notices are byte for byte what they were
- The macOS binary links `libSystem` and `libiconv`, targets macOS 11.0, and is not signed with a Developer ID or notarised: a copy downloaded with a browser is quarantined and has to be allowed by its user, one fetched with `curl` is not. The documentation says so and how to allow it. Signing is an open decision
- Smoke test on macOS: the runner has no Docker, so `reference/default-install/db-native.sh` loads the default install fixture into a MariaDB 11.8 installed on the runner, and `scripts/smoke-test.sh` runs unchanged against it for both archives, the Intel one under Rosetta. A rebuild on the runner gives the same bytes for both architectures. CI's `macos` job runs clippy and builds the binary for both targets, and runs the tests of the fifteen crates that need neither the fixture database nor the Theme Test Data, natively and under Rosetta (650 tests each). Everything else runs on Linux only (docs/TESTING.md, "What runs on macOS")
- CI for two runners: the Linux jobs are two chains, coverage, switch-back and screenshots, and checks, package and fuzz, so two jobs are ready at any time and none waits for a runner. `CARGO_BUILD_JOBS` and `RUST_TEST_THREADS` are 12 in every Linux job, the processor time a job gets, where cargo counted every core of the machine
- The release waits for the gates: a `v*` tag starts CI only, and CI's last job calls the release workflow once every other job has passed, so nothing is built for publishing before the gates are green. A called workflow was chosen over a workflow started by CI's completion because the release then keeps the tag as its ref (the version check and the build provenance name it) and waits through an ordinary `needs` in the same run. The release's manual dry run is unchanged, and a manual CI run can rehearse the whole tag path with `release_dry_run`, publishing nothing. The release now carries an SBOM for each system

## 2026-11-26: v0.1.0, the first release

- The repository is public and `v0.1.0` is tagged on `main`. The tag's CI run passed all six jobs (checks, package, coverage, the switch-back suite, the screenshot comparison and a short run of all 64 fuzz targets), and the release run built the x86_64 and arm64 archives reproducibly, attested them and published the release with its checksums and SBOM
- The install commands of the README were run against the published release: the download, the checksum and `gh attestation verify` all pass
- The licence file carries one line above the licence text, naming the project and pointing to `NOTICE`; `NOTICE` says whose trademarks HyScaler and HyScribe are. `AGENTS.md` now holds the project instructions, with `CLAUDE.md` and `GEMINI.md` as links to it
- Next for the workflows: chain the CI jobs, and make the release wait for CI

## 2026-11-25: The open findings of the writes' security review closed (S-12 to S-15)

- The five findings docs/SECURITY_MODEL.md still listed as open were checked again against the code as it is now. Two had changed under their own wording: the extension runtime the first waited for exists, and the claim of the fourth, that no value a request controls reaches a REST header, did not hold
- S-15, fixed: the names of a collection's query parameters reach its pagination `Link` header unencoded, as in PHP (`urlencode_deep()` encodes values only), and a header the `http` crate cannot carry turned the whole answer into an empty 200. No header could be added that way, as such a value is refused whole. The REST server now collapses a header's white space as `WP_REST_Server::send_header()` does, and the HTTP layer leaves out a header it still cannot carry, keeping the status, the other headers and the body. PHP under Apache answers 500 for those bytes, a difference listed in docs/COMPATIBILITY.md. Recorded from PHP: ten requests added to `reference/rest/search.list` and two writes to `reference/rest/invalid-input.list`, which now records over `invalid-input.sql` so a new post gets the ID the tests give it
- The other places `hyscribe-http` sets a header from a value that is not constant were read for the same pattern, and one had it: a canonical or feed redirect whose location kept a control byte from a nested query key was sent with its status and no `Location`. Both now go through `wp_sanitize_redirect()`, as `wp_redirect()` sends them. The rest hold: the login, admin and comment redirects are sanitized already and replace a value they cannot carry with an empty one, the redirects to hs-admin and the login carry the request's own path and query, which a parsed URI holds only in bytes a header carries, and the front end's `Link` headers and the static files' headers leave such a header out. A test walks the front end's, the feeds' and the admin's redirects with control bytes in the query. Known gap: for a nested key that is only control bytes PHP's canonical redirect numbers the key (`v[0]`) where this one leaves it empty (`v[]`)
- S-12, fixed (ADR 0022's 2026-11-25 amendment): "a hook never skips a check" is now held by the grants. The hooks that can answer a REST request before its checks or replace what they let through, that change a save or an upload after its checks, or whose answer decides a check are sensitive hooks: named as a plain action or filter they refuse the package. Sensitive rather than refused, as plugins use them for real work and the sensitive grant is the ADR's answer to a hook that changes who may do what. No callback runs on the REST hooks among them today, their values not crossing yet; the test extension attaches to two of them and a REST answer and a save stay as they were
- An extension's REST routes are recorded by the runtime and not yet served by the REST server; docs/EXTENSIONS.md and the SDK's documentation now say so
- S-13, accepted with tests: every upload route is recorded against PHP with names that step out of the folder, absolute and backslashed paths, a NUL, a line feed, a leading dot, only dots, a device name, a name over 255 bytes and one that is not UTF-8 (38 cases added to `reference/upload-routes/cases.list`, whose golden now also keeps each new attachment's title, GUID and `_wp_attached_file`, and whose script fails if a file appears outside the uploads folder). The file, the rows and the answers are PHP's, and nothing is written outside the folder. One difference was found and fixed: PHP reads a form part's header line as a C string, so a NUL ends it, and the multipart reader now does the same
- S-14, accepted: names that are not UTF-8 sanitize as in PHP by now. The one difference left is the stricter way: a file sent to `wp/v2/media` as a form under such a name is refused with nothing stored, where PHP stores the file, fails to insert its attachment and leaves the file
- S-16 stays as it was: not a security finding
## 2026-11-25: The front end for logged in users

- "View" and "Preview" from hs-admin answered 404 for anything unpublished: the front end never read the login cookie, so every request was a visitor's. It now finds the user as `determine_current_user` does outside the admin (the auth cookie, else the logged in cookie, through the cookie verification hs-admin uses) and applies `WP_Query::get_posts()`'s check of a singular query's post with the ported `map_meta_cap()`: drafts, pending and scheduled posts are previewed by who may edit them (dated now, unless scheduled), private posts shown to who may read them, trash, auto-drafts and revisions to nobody, a post of an unregistered status to who may edit it
- Lists follow `WP_Query` for a logged in user too: the main query, query blocks, feeds and adjacent post links add private posts by `read_private_posts` (else the user's own), a search finds password protected posts, a private post the user may read gets its pretty link, the body has `logged-in` and the footer no speculation rules; `_set_preview()` shows the latest autosave for a verified preview nonce, which also keeps a published post's preview from its canonical redirect, for a visitor as well
- `nocache_headers()` now go out with every front end answer to a logged in user, as `WP::send_headers()` sends them; a visitor's answers keep their headers (PHP also sends them for a visitor's 404, a redirect from one and a password protected post, which is left for later)
- Matched to PHP by `reference/private-views/golden.sh`: 553 requests as a visitor and as an administrator, editor, author, contributor and subscriber over posts and pages of every status, their own and others', plain and pretty, with a page whose query block lists posts, comment feeds, author archives and a preview nonce of another post; a tampered, expired or deleted user's cookie gets a visitor's answer byte for byte, and a user whose role was lowered gets the new role's
- Reviewed as access control before the merge, each finding first as a request PHP answers otherwise. The comment feed of one post (`?p=<id>&feed=rss2`) listed the approved comments of a private post or a draft to anyone, a visitor included, and had since before this work: `WP_Query` loads a singular feed's comments only after its status check, and the feed now lists them only for a post the user is shown. A `pagename` finds pages only again: the port had let an attachment stored as published or private through to the attachment page. A feed is rendered as its user reads it, so a private post they may read has its pretty link there as on a page
- Not ported: the toolbar and the comment form of a logged in user on the front end (the golden's users turn the toolbar off in their profiles, its posts have comments closed), trackbacks and attachment pages for a logged in user, and `_set_preview()`'s term, thumbnail and meta filters

## 2026-11-25: Author archives behind `AUTHOR_ARCHIVES` (ADR 0006 amendment)

- A comparison of the release binary with PHP WordPress 7.1.3 found `/author/admin/` and `/?author=1` answering 404 on HyScribe and 200 on PHP. This was the deliberate refusal of ADR 0006 and docs/SECURITY_MODEL.md (author archives give away login names), not a missing port
- Decision (2026-11-25): build the planned opt-in key. `AUTHOR_ARCHIVES` is off by default and read as every yes or no key; off, every author-shaped request answers the same 404 as before, which a test walks (`?author=1`, `?author=1,2`, `?author=-1`, `?author_name=admin`, `?p=1&author=1`, `/author/admin/` with its page and feed)
- On, author archives follow `WP_Query`: `author` as its sorted IDs (`author__in`, or `author__not_in` for any that is not positive, a numeric zero asking for none), `author_name` as its sanitized last path part, both applying together; `handle_404()` keeps the archive of an existing author with no posts; `redirect_canonical()` sends `?author=N` to `/author/<nicename>/` only for an author with a published post of any type and not past the first page; the template hierarchy (`author-<nicename>`, `author-<id>`, `author`), the document and query titles (`the_author`'s ent2ncr), body classes, the author feed link (for an ID of 0, the first post's author, as `$authordata` is left), the REST link to `/wp/v2/users/<id>` and the queried object ID the pages list compares
- The classic theme host answers `is_author()`, and `the_archive_title()` and `the_archive_description()` of `$authordata`, as PHP does
- Matched to PHP by `reference/pages/golden.sh authors` (73 requests over `reference/pages/authors.sql`), the Theme Test Data lists of the five block themes (their ten author archives each, recorded earlier and no longer skipped) and `reference/theme-tags/golden.sh` (eleven author archives of `reference/theme-tags/authors.list`)
- Not ported, answering as with the key off: an author with any query var but `paged` and `feed` (a search, a date, a term, a post); `is_author()` naming an author answers 501. The query pagination block's pretty links are a gap on every archive, listed in `pages-authors-known.tsv`

## 2026-11-25: MP4 and QuickTime uploads (ADR 0020 amendment)

- The second half of the MP3 and MP4 decision: `hyscribe-image`'s `getid3` reads MP4, M4A, M4V, QuickTime and 3GPP files as getID3's `audio-video.quicktime` module and `analyze()`'s sums do, for what `wp_read_video_metadata()` and `wp_read_audio_metadata()` keep: length, size, codec, data format, bitrates, the audio array, `created_timestamp` and the first cover picture, made an attachment of its own as an MP3's is. Taken under the GNU GPL version 3, as the MP3 port is
- Matched to PHP by `reference/getid3/golden.sh` over the corpus (ten more ffmpeg files) and the built files of `reference/getid3/mp4-samples.py`, by the upload routes' golden and by the write oracle's MP4, M4A, MOV and M4V actions; what depends on PHP's memory limit, reaches another getID3 module or is not ported answers 501 (listed in the amendment). Fuzz target `upload_quicktime`
- A video attachment's content now starts with the player `prepend_attachment()` makes (`wp_video_shortcode()` with its size and poster), through the shortcode port; the REST media answer no longer refuses a playable video, and the upload routes' answers for MP4, MOV and M4V uploads match PHP's
## 2026-11-25: Writes no longer fail on a row PHP changed meanwhile (ADR 0012 amendment)

- `GET /hs-admin/` answered 500 with MariaDB error 1020 while PHP WordPress loaded its dashboard on the same database. Reproduced with two connections on the fixture's MariaDB 11.8 (`REPEATABLE-READ`, `innodb_snapshot_isolation` on, the default since 11.6.2): an action began, another client committed a change to the `cron` option, and the action's update of that row failed with 1020, whether or not the action had read it
- Every writer connection now turns `innodb_snapshot_isolation` off for its session where the server has it, so an action writes over a row committed meanwhile and the last save wins, as ADR 0012 already decided and as on MySQL and older MariaDB; the isolation level is unchanged. Actions are not retried: some have side effects outside the database, and deadlocks still roll an action back whole
- Test: `a_row_changed_by_another_client_is_written_over` in `hyscribe-db`, which failed with 1020 before the change
## 2026-11-25: Porting help in the welcome banner and the footer (ADR 0008 amendment)

- The welcome banner links the release notes of HyScribe's version on GitHub instead of the About page, asks people trying HyScribe out to write to contact@hyscribe.com for help porting custom themes or plugins, and takes the brand's colours and mark; the footer adds a `mailto:` link whose subject names the site's domain (from the stored home URL) and whose body names the home URL and HyScribe's version
- Done in hs-admin and in the oracle's look plugin alike, and the admin goldens are re-recorded from PHP
## 2026-11-25: The site folder in the admin's own URLs

- hs-admin answered with the path below the site's folder as `$_SERVER['REQUEST_URI']`, so what PHP builds from it lost the folder of a site at `http://host/blog`: the `redirect_to` of the login redirect (`auth_redirect()`), the canonical link (`wp_admin_canonical_url()`), the `_wp_http_referer` fields and every link or redirect made from the request's query (a page past the last, the list screens' links, an extension settings page's `settings_fields()`). The request URI now carries the folder, from the `siteurl` option, and the path below it only routes
- A login posted to the URL a visitor was sent to answers 200 and not a redirect when its query carries `reauth`: `wp-login.php` redirects only without it (`! $reauth`), and the form's own action has no query, so a browser is redirected; checked, not a bug
- No PHP oracle serves from a folder, so the expected values are derived from the source of the functions named in the tests (`crates/hyscribe-http/src/admin/tests/folder.rs`)

## 2026-11-25: Archive, site and 404 titles are texturized

- A category's or tag's name goes through `single_cat_title` and `single_tag_title` (`wptexturize()` and `strip_tags()`) and another taxonomy's name as it is, then through `document_title`'s `wptexturize()`, `convert_chars()` and `esc_html()`, as `wp_get_document_title()` does; the term name had been escaped first, so its quotes printed as `&quot;`
- The site's name and tagline in the title go through the `bloginfo` filters (`wptexturize()`, `convert_chars()`, `esc_html()`), as `get_bloginfo( 'name', 'display' )` does, and the 404 title is built by `document_title` like the others; before, a name or tagline with quotes, an apostrophe, dashes, an ellipsis or an ampersand was escaped only
- Matched to PHP by `reference/pages/golden.sh titles`, which now also records category, tag and post format archives with such names (plain and pretty), and the home page, a post, a category, a search and a 404 under a site name and tagline of that kind

## 2026-11-24: MP3 uploads (ADR 0020 amendment)

- Decision: MP3 and MP4 files must upload. MP3 is ported first: `hyscribe-image`'s `getid3` reads the frame header, the Xing, Info and VBRI headers, the LAME tag and the ID3v1 and ID3v2 tags as getID3 does, for what `wp_read_audio_metadata()` keeps; an audio attachment's title and text come from its tags as `media_handle_upload()` writes them, and a cover picture becomes an attachment of its own and the audio's thumbnail, as `wp_generate_attachment_metadata()` makes it
- getID3 is offered under several licences; the port is taken under the GNU GPL version 3, which the AGPL may be combined with (ADR 0020 amendment, read as ADR 0004 reads WordPress's licence)
- Matched to PHP by `reference/getid3/golden.sh` over the corpus and 506 built MP3 files (`reference/getid3/mp3-samples.py`), by the upload routes' golden and by the write oracle's `upload-*-mp3*` actions; what getID3 would warn about, rescan or read another way answers 501. Fuzz target `upload_getid3`

## 2026-11-24: The mark in the post editor's publish panel (ADR 0008 amendment)

- The editor bundle's WordPress logo fallback (`svg.components-site-icon` in the publish panel, shown when the site has no Site Icon) is painted over with the HyScribe mark by a style rule on the block editors' pages, in hs-admin and in the oracle's look plugin; the bundle stays unmodified and a Site Icon (an `img`) is never covered
- Checked in the 7.1.3 bundles: the post editor's back button and the site editor's site hub draw arrows, so only the publish panel shows the logo; the editor goldens are re-recorded

## 2026-11-24: SDK 0.4, admin pages and the settings API (11.4)

- The WIT package is `hyscribe:sdk@0.4.0` with a new `admin` interface: `add_menu_page()`, `add_submenu_page()` and `add_options_page()` with its kin, `register_setting()`, `add_settings_section()`, `add_settings_field()`, `settings_fields()`, `do_settings_sections()` and `submit_button()`, registrations taken in `init` only (ADR 0022 amendment)
- hs-admin's menu adds the active extensions' pages as `admin_menu` adds a PHP plugin's (`$admin_page_hooks`, `$_registered_pages`, positions, the copied parent entry, plugin links and their current classes, the command palette's `menu_page_url()`), and serves a page at `admin?page=<slug>` or under its core menu (`options-general?page=<slug>`) after `user_can_access_admin_page()`, its body what its callback answers; checked against PHP on the Plugins screen views and six page views as the administrator and an editor (`reference/plugins/golden.sh`, `extension-admin.tsv`)
- No new grant: pages show only to users with their capability, which the host checks; a setting needs its option's grant; slugs are restricted to letters, digits, `_` and `-`
- A page's callback runs only when it is on the page's hook for the user, as `admin.php` checks: a top level page asked under another menu by a user without its capability answers PHP's "Cannot load" 500, where the callback had run. `settings_fields()` prints only a settings group the extension registered, and a page's icon is a plain `dashicons-` class
- Not yet: the save through `options.php` (501), `add_users_page()`, hidden pages, other query arguments, `add_settings_error()`, a setting's `default`, other extensions' callbacks on a page hook, the current user and nonces through `auth` on a page, admin-ajax handlers

## 2026-11-24: A home URL in a folder: redirects, login, admin and bundled files

- Canonical redirects under plain permalinks keep the home URL's folder once (`/blog` goes to `/blog/`, not `/blog/blog/`), checked against PHP's redirects for a `home` in `/blog` (`reference/pages/folder.list`)
- The login, hs-admin, its loaders, the admin redirects and the bundled and uploaded files answer under the home folder (`/blog/hs-login`, `/blog/wp-includes/...`); a request outside the folder, beside it (`/blogs`), doubled or climbing out of it reaches none of them
- PHP's REST index under the folder is compared (`reference/pages/folder-rest.list`); its login and static files have no PHP counterpart in the oracle, which serves its files from its root, so those are covered by tests of HyScribe's own behaviour alone

## 2026-11-24: hs-admin's footer credits HyScaler (ADR 0008 amendment)

- The admin footer's sentence now reads "Thank you for creating with HyScribe. HyScribe is developed and supported by HyScaler", HyScaler linked to hyscaler.com, inside the same span and still through `admin_footer_text`: the attribution `NOTICE` asks a copy or fork to keep, shown in the interface's Appropriate Legal Notices
- Admin footer only; the oracle's look plugin maps the same string, so the admin goldens stay differential and are re-recorded

## 2026-11-24: List search forms and the bare admin URL

- `/hs-admin` without its slash answers a 301 to `/hs-admin/` with the query kept, as Apache's directory slash does for `/wp-admin`, in place of an empty 501
- The Posts, Pages, Comments, Users and Media lists answer their search forms as a browser submits them: the nonce, the referer and the idle `action`, `action2` and `filter_action` fields no longer turn the request away, and the redirect PHP makes without the nonce and the referer is made after the screen's own checks. The Categories and Tags search forms send `post_type=post`, which is read. `reference/list-search/golden.sh` records PHP's answers for each role
- An author asking for the Pages list gets `edit.php`'s own refusal ("not allowed to edit posts in this post type") as PHP gives it, the menu's only for users who cannot edit posts at all
- Not ported, as before: a Plugins search with a term

## 2026-11-24: The template tags of classic templates, and a theme's public files (10.5)

- The host answers the tags Hello Elementor 3.5.2's templates call, for any classic theme (`hyscribe_render::classic`, over the block renderers' ports): `bloginfo` and `get_bloginfo`, `home_url`, `esc_url`, `esc_attr`, `esc_html`, `wp_kses_post`, `the_title`, `get_the_title`, `get_permalink`, `the_permalink`, `get_the_ID`, `get_post_type`, `post_class`, `the_content`, `the_excerpt`, `has_post_thumbnail`, `get_the_post_thumbnail`, `wp_link_pages`, `has_tag`, `the_tags`, `post_type_supports`, `comments_open`, `the_archive_title`, `the_archive_description`, `get_search_query`, `$wp_query->max_num_pages` (as `max_num_pages`), `previous_posts_link`, `next_posts_link`, and the comments: `comments_template()` sets the comments up for the theme's own `comments.php`, then `have_comments`, `get_comments_number`, `wp_list_comments`, `the_comments_navigation` and `comment_form`
- The global post follows the loop and `wp_reset_postdata()`; content, excerpts and images count on through the page as PHP's globals do
- Checked against PHP: `reference/theme-tags/golden.sh` records the new tags on all 132 pages, over the default install and over content with a featured image, a tag, threaded and paged comments and an untitled post in pages; all match, Hello's `comments.php` played by a stand-in from the tags
- `wp_link_pages()` adds `preview=true` on a preview, as `get_preview_post_link()` does (the post content block too)
- An installed theme's files are served at `/wp-content/themes/<slug>/...` from its package's `public/`, with the embedded themes' content types, `Last-Modified`, an `ETag` and conditional requests; a path outside `public/`, a hidden file, a link leading out and PHP source are not found
- Not yet: the compat comments template for a theme without `comments.php`, comments on a password protected post, a term's archive description, `get_pagenum_link()` under `index.php` permalinks, the Themes screen for an installed classic theme

## 2026-11-24: PDFs upload without previews (ADR 0020 amendment)

- Decision: a PDF uploads without `-pdf.jpg` previews, as PHP WordPress writes it on a host without Imagick and Ghostscript; `_wp_attachment_metadata` holds `filesize` alone, and no PDF renderer is added
- Oracles: the upload goldens add the must-use plugin `reference/uploads/no-pdf-preview.php` (no image editor while a PDF's metadata is made); the upload routes and two write oracle actions match PHP row for row

## 2026-11-24: Word, Excel and PowerPoint 97 to 2003 uploads (ADR 0020 amendment)

- Decision: the old Office files upload. `hyscribe-image`'s `fileinfo` reads Compound File Binary files as libmagic's `file_trycdf()` types them (summary information, then entry names, then streams), ported from the source of the libmagic PHP's `fileinfo` carries; `.doc`, `.xls`, `.ppt` and their kin on WordPress's list get the attachment of any other document
- 501 stays where libmagic could read otherwise: damaged or cut files, larger sectors or tables, unusual properties, and files libmagic's earlier tar or SIMH tape tests may take. Bounded and linear, fuzzed by `upload_fileinfo`
- Oracles: about 670 Compound File Binary samples in `fileinfo.tsv`, the type checks of the three corpus files under other names, the upload routes and seven write oracle actions

## 2026-11-23: A HyScribe colour scheme by default (ADR 0008 amendment)

- A HyScribe colour scheme is the default in place of Coffee for users without a scheme, with `modern` or an unknown one (nothing is written): WordPress's own colour Sass with the brand's colours as variables (`crates/hyscribe-assets/colors/hyscribe/colors.scss`), compiled by `reference/admin-colors/build.sh` with pinned dart-sass, rtlcss and clean-css after it rebuilds core's Coffee byte for byte, served under `/hs-admin/css/colors/hyscribe/` with an `ETag` only
- Ink menus and toolbar, Scribe Blue highlight, links and buttons, Navy submenus, Sky focus text, Nib Gold bubbles, Paper background; every text pair on the menu and toolbar meets WCAG AA (ratios in the amendment). The picker lists HyScribe by name among the others, as WordPress sorts them; the colour scheme save accepts it
- The oracle's look plugin registers the same scheme and `look.sh` copies the same files, so the goldens stay differential; the scheme-dependent goldens are re-recorded

## 2026-11-23: hs-admin carries the HyScribe mark (ADR 0008 amendment)

- `/hs-login` shows the mark (`brand/hyscribe-mark.svg`) at WordPress's logo size in place of the text mark, its link text still "HyScribe"; the admin bar has a `hyscribe` item with the small mark, one link to the Dashboard, where the WordPress logo menu was; admin and login pages of a site without a Site Icon take the small mark as their icon, while the front end and `/favicon.ico` keep PHP's behaviour
- The binary embeds the marks from `brand/` (never copied into a source folder, as they are not covered by the AGPL) and serves them under `/hs-admin/images/`, with an `ETag` and no `Last-Modified`
- The oracle's look plugin makes the same changes and `look.sh` copies the same marks into its `wp-admin/images/`, so the admin, login and screenshot goldens stay differential; they are re-recorded. The default colour scheme stays Coffee until the HyScribe scheme follows

## 2026-11-23: No third-party theme in the binary; the setup calls (10.5)

- Decision: the release binary carries no third-party plugin or theme. This reverses ADR 0023's earlier decision that `scripts/package.sh` builds the theme components into the binary; should HyScribe ever carry a theme, writing it into `hyscribe/themes/<slug>/` at start is the accepted way (ADR 0023 amendment)
- Hello Elementor 3.5.2's port (its `functions.php` and eleven templates as askama templates, its version write over a port of PHP's `version_compare()` matching PHP on 3,481 pairs, a fuzz target, its build script and the write oracle of its version write) moves to a separate repository of ports, built and shipped as its own package later; it depends on `hyscribe-sdk` by path until the SDK is published
- Kept in HyScribe, for any classic theme: the host fires `after_setup_theme` once a request's extensions load, answers `is_admin()` outside a page (true for hs-admin and wp-admin's PHP files), and takes `register_nav_menus()`, `add_editor_style()` (adding `menus` and `editor-style` as PHP does) and `$content_width`; it serves `get_queried_object_id`, `get_post_field` of the queried post's excerpt and `wp_strip_all_tags`
- SDK: the test host answers a tag by name and arguments (`get_bloginfo[Text("name")]`) and can refuse a conditional tag, so a port's every failure path is tested; `Raw::is_empty` for PHP's `if ( $markup )`
- Tests: the host's classic head tests play Hello's callbacks with a stand-in that reads the description through the served tags, matching PHP on all 68 pages of `reference/classic-head`; Hello stays pinned as the PHP oracle of those goldens. `basic-toml`'s advisory ignore leaves `deny.toml`, as no crate of HyScribe reads an `askama.toml`
- Not yet: the other template tags classic templates call (`bloginfo`, `esc_url`, `the_title`, `the_content`, `post_class`, the comments template, archive and search tags and the rest), serving a classic theme's `public/` files, whole page goldens and installing theme packages

## 2026-11-23: A classic theme's head and footer (10.5)

- Actions that print: `hyscribe_model::hooks::do_action_printing` runs core's callbacks among the extensions' by priority (core's first at a priority), and an extension's callback prints the string it answers (`Dispatch::printing`, `Request::printing`); `wp_head`, `wp_footer`, `wp_enqueue_scripts` and `wp_body_open` fire this way; the SDK test host's `Host::action` answers what the callbacks print
- `wp_head()` and `wp_footer()` for a classic theme with a `theme.json` (`hyscribe_render::head::classic`): the title tag and feed links as the theme supports them, robots, oEmbed links, the styles core enqueues for a classic theme loading block assets on demand, the extensions' stylesheets, REST links, generator, canonical and shortlink, the footer's scripts; WordPress 6.9's template output buffer is ported as it acts on these pages: the global styles the footer builds go where the head left their placeholder, and styles enqueued after the head go before `</head>` once the page is done. The theme's `theme.json` is read from its package's `public/` and its global styles built with the site's own, kept until either changes
- Checked against PHP: `reference/classic-head/golden.sh` renders a template of `wp_head()` and `wp_footer()` only, with Hello Elementor active, over the 101 requests of `reference/pages/default.list` and a post with an excerpt; all 68 pages its template loader reaches match, Hello's callbacks played by a stand-in for its port (its three stylesheets, its description meta tag)
- Assets: an extension's stylesheet for every medium with no dependencies is registered and enqueued over its package's files (`/wp-content/themes/<slug>/` for the active theme, `/wp-content/plugins/<slug>/` otherwise), and `wp_enqueue_script()` takes the scripts core registers (`comment-reply`)
- A theme's fault while rendering answers PHP's critical error page with status 500 (the golden's last line)
- The head's `wp_custom_css_cb` writes `custom_css_post_id` -1 into `theme_mods_<stylesheet>` once, as PHP does (write oracle action `theme-classic-page-view`); it is skipped without `current_theme`, where PHP names the theme from its headers, which HyScribe does not read for a classic theme yet
- Not yet: late core block styles (no served tag enqueues one), font faces, a site icon, a `custom_css` post's styles, a classic theme without `theme.json` (its `wp_head()` answers not served), scripts and inline styles of extensions, the other setup calls (`register_nav_menus`, `add_editor_style`, `$content_width`, image sizes)

## 2026-11-23: The update check and the Plugins screen's update rows (ADR 0024, last slice)

- `EXTENSION_UPDATE_CHECK` (off by default): with `HTTP_OUTBOUND=on` and `EXTENSION_REGISTRY`, a service task fetches the key list and index at the start and every 12 hours, keeps them, applies their revocations and logs the offers, installing nothing; 300 seconds at most, failures logged, a stop ends a check under way
- The Plugins screen shows an update from the kept index in PHP's `wp_plugin_update_row()` markup for a user who may not update, with `hyscribe plugin update <id>` where `in_plugin_update_message` prints, an error row for a revoked version, where an unsigned or operator-signed package came from, and a notice when the kept index is out of date; it reads `registry/` without writing. Checked against PHP in `reference/plugins/golden.sh`'s `x24` variants (all four match but for the intended differences)
- As built choices in a third amendment to ADR 0024: the details link goes to the extension's `Plugin URI` in a new tab (the index has no page, and version documents stay fetched only on install), the plugin row gets `update`, a restart checks again (no last check time kept)

## 2026-11-23: The brand kit comes into the product

- The logo (light and dark), the mark and the small mark are copied into `brand/` from HyScribe's brand kit, with a notice that the HyScribe name and logo are not covered by the AGPL and that a distributed fork takes another name and mark; the README shows the logo, short "Works today" and "Not yet" lists and, at its foot, that notice and the WordPress trademark notice
- Before the first release, hs-admin's own branding surfaces take the brand: the mark on `/hs-login` and in the admin bar, the small mark as the admin's favicon, and a HyScribe colour scheme in the brand's colours as the default in place of Coffee (as the brand guide asks: those surfaces carry the HyScribe mark, never the W)

## 2026-11-23: CI for releases only

- CI and the release workflow run for a `v*` tag and by hand only; day to day the same gates run before every merge
- The workflows read the runner's paths from the repository variables `HYSCRIBE_CI_WORK`, `HYSCRIBE_CI_DOCKER` and `HYSCRIBE_CI_FIXTURES`, so no machine detail is in the repository; the work, caches and Docker of a job are freed after it; the compile cache is capped at 10 GB
- The screenshot job removes its fixture network (leftover networks had filled Docker's address pools and failed the release smoke test); arm64 release builds compile wasmtime's C helpers against musl's own headers

## 2026-11-23: Decisions for the Hello port's head and faults (ADR 0023)

- A classic page view writes `custom_css_post_id` -1 into `theme_mods_<stylesheet>` as PHP's head does; action callbacks may return output the host prints where the action fires (SDK 0.3); a theme fault answers PHP's critical error page with status 500
- Round review after the registry and template tag merges found no dead code; `Remote::url` narrowed to the crate

## 2026-11-22: Install by name and updates from the hosted registry (ADR 0024)

- Version documents (`v1/extensions/<id>.json`) read and checked against the size and SHA-256 the signed index gives; fuzzed in `registry_index`
- `hyscribe-fetch` gains `Client::download`, the streamed package download of ADR 0024's amendment of ADR 0019 (HTTPS, one host, the signed size up to 256 MiB, 30 seconds between reads, 15 minutes in all, to a file); hops now return the answer with its body unread, so `get` and the download share every check
- `hyscribe plugin install <name>[@<version>]` and `hyscribe plugin update --check | <id>... | --all` (`--downgrade`, `--allow-new-grants`, `--replace-local`), through `EXTENSION_REGISTRY` (HTTPS only, unset until the project's registry has its address); the key list and index are refreshed, kept and their revocations applied on every run; `origin.txt` gains `registry`
- As built choices recorded in a second amendment to ADR 0024: version order is the version document's order, any change of grants or limits needs `--allow-new-grants` (the runtime stops on any difference), `--check` reads only the index

## 2026-11-22: The host answers a classic theme's template tags (10.5)

- The front end renders the active classic theme's page through its component: the template the classic hierarchy picks (`index.php` when none other), its calls answered over the page by `hyscribe_render::classic`; a database error of a tag or a fault of the theme answers the error page
- Served: `add_theme_support()` as request state; the main loop (`have_posts`, `the_post`, `rewind_posts`, `wp_reset_postdata`, `in_the_loop`); the conditional tags; `language_attributes`, `body_class` (with `wp-custom-logo` and `wp-embed-responsive` as the theme supports them, block themes included), `wp_body_open`, `has_custom_logo` and `the_custom_logo` over the site logo block's `get_custom_logo()` port (with its `get_custom_logo` filter), `wp_nav_menu` with `echo` false; `get_header()` and the other part functions find their file as `locate_template()` does and fire their actions
- Checked against PHP: `reference/theme-tags/golden.sh` records every tag where PHP's loader picked Hello's template, on the 101 requests of `reference/pages/default.list` and again with a logo and two menus; all 132 pages that reach the loader match (author archives stay 404)
- Not yet: `wp_head()` and `wp_footer()` for a classic theme (WordPress 6.9 and later hoist late block styles into the head through the template output buffer, and the head writes `custom_css_post_id` -1 into `theme_mods_<stylesheet>` on a page view), theme mods through `get_theme_mod` and its kin, the other setup calls (`register_nav_menus`, `add_editor_style`, `$content_width` and the rest answer not served), `body_class( $css_class )`, `is_admin` outside a page, and assets

## 2026-11-22: Package signatures and local installs (ADR 0024)

- `hyscribe-ext` reads minisign signatures in the prehashed form only, over `ring`, `blake2` and `base64` (no crate added, no exemption grown); minisign's own test vector verifies; fuzz target `ext_signature`
- New crate `hyscribe-registry`: the key list (two of three compiled-in roots, online keys with what they may sign and a window of at most a year, revoked keys, expiry) and the index (an online key allowed to sign it, revoked versions, expiry), read through the bounded `json_decode()`; verified copies kept in `registry/` with the highest serials in `state`; fuzz target `registry_index`
- The compiled-in root keys are an empty placeholder: until the three offline roots exist, a release verifies no key list, so only `EXTENSION_TRUSTED_KEYS` and `--unsigned` install packages
- `hyscribe plugin install <file>` needs `<file>.minisig` verifying against the kept key list or a key in the new `EXTENSION_TRUSTED_KEYS`, or `--unsigned`; the trusted comment binds the package's id, version, SDK and SHA-256, checked against its manifest. `plugins/<id>/origin.txt` records the signer and the SHA-256; the kept signature sits beside the kept package and `plugin delete` removes it
- Revocation: a version the kept index revokes, or one signed by a key the key list revokes, gets `revoked <time> <reason>` in its `origin.txt` (applied when the command reads the kept registry) and stops loading (`Fault::Revoked`), `active_plugins` untouched; a missing `origin.txt` (an extension installed before ADR 0024) loads as before
- A key list expiring more than 13 months after it is checked is refused; the as built choices (document fields, when revocation applies, fail closed on a bad kept registry, back-dating within a key's window) are recorded in an amendment to ADR 0024

## 2026-11-22: Decisions for the Hello Elementor port (ADR 0023 amendment)

- The SDK's escaper is written afresh and checked against PHP goldens, keeping the SDK's licence; first-party theme components are built by `scripts/package.sh` before the binary; raw bytes keep ADR 0023's markers

## 2026-11-22: `basic-toml`'s advisory ignored

- askama's `config` feature, which reads a theme port's `askama.toml` where the SDK's escaper is named, brings `basic-toml`, marked unmaintained (RUSTSEC-2026-0374). The decision is to ignore that advisory in `deny.toml`: the crate runs only at build time inside askama's proc macro, reading the port's own file, and is unmaintained rather than vulnerable. The entry is removed when askama drops it (ADR 0023's Hello amendment)

## 2026-11-22: askama tied to the SDK (10.5)

- `hyscribe_sdk::page`: `Esc`, the escaper ports name in `askama.toml`, written afresh from `esc_html()`'s behaviour (ADR 0023's decisions) and matching all 1,815 cases of PHP's `esc_html()` golden; `Html`, the host's markup, printed as it is and kept aside behind markers; `Text` for host text; `render` and `splice`, the splicer fuzzed by `sdk_template_splice`; `check_port` for a port's tests, refusing `safe`, askama's `Safe` and its kin, a template's own `escape` and marker characters
- Markup put through a filter that takes text prints a marker character, never its bytes
- Supply chain: askama, askama_derive, askama_macros and askama_parser 0.16.1 are audited as deltas from the imported 0.16.0 audits; winnow's publisher (Ed Page, trusted by the ISRG, Mozilla and Bytecode Alliance sets) is trusted, which removes winnow's `safe-to-run` exemption
- To decide: askama reads a port's `askama.toml`, where the escaper is named, only with its `config` feature, which brings `basic-toml`, marked unmaintained (RUSTSEC-2026-0374) and refused by `cargo deny`. The SDK's own tests go without it; Hello's port needs it, or another way to name the escaper

## 2026-11-22: Hello Elementor pinned, `wp_nav_menu()` ported (10.5)

- Hello Elementor 3.5.2 is pinned in `themes/pinned.tsv` with the archive SHA-256 ADR 0023 recorded. Its licence check (ADR 0004): the theme's own code is GPL-3.0-or-later and may be ported under AGPL-3.0-or-later; Composer's autoloader (MIT), the admin app bundles (MIT libraries) and the GPL-3 notifications package are not ported; nothing is GPL-2.0-only (`themes/README.md`)
- `wp_nav_menu()` with `Walker_Nav_Menu` in `hyscribe-render`, over the classic menu items the navigation fallback already sets up: menus by ID, slug, name or theme location, containers, `items_wrap`, spacing, depth, orphans, the request's unique menu IDs and `_nav_menu_item_id_use_once()`, `aria-current`, `rel="privacy-policy"` and `wp_nav_menu_remove_menu_item_has_children_class()`. 30 cases match PHP (`reference/nav-menu/golden.sh`), Hello's two locations among them. `fallback_cb` `wp_page_menu`, and IDs numbered past PHP's integers, answer not ported. Its filters pass objects and cross to no extension yet
- Found on the way: the menu items' title attribute and description now get core's `wptexturize` and `strip_tags`, as PHP's default filters give them, which the navigation fallback's conversion lacked
- To decide: the SDK is Apache-2.0 OR MIT, and ADR 0023 has the SDK carry ports of `esc_html()` and the other escaping functions as askama's escaper; porting WordPress's GPL code into the SDK would put GPL code under the SDK's licence, so where the escaper lives (a clean implementation checked against PHP, a GPL crate beside the SDK, or a host call) waits on an answer. How the first-party theme components are built into the binary (question 3: embedded) is not designed yet either

## 2026-11-22: ADR 0024 accepted, SDK 0.2's grants and the deactivate difference confirmed

- ADR 0024 (package signing and the hosted registry) accepted with all eleven questions as recommended
- SDK 0.2: an extension may change granted capabilities on WordPress's built-in roles, and may send its own credential headers to its granted hosts only (ADR 0022 amendment)
- An extension whose deactivation traps is deactivated and logged, unlike PHP, which keeps a plugin active when its deactivation fatals
- Release: `main` is created and receives `development` through a reviewed pull request; the first `v*` tag is created and the repository's visibility changed

## 2026-11-22: The classic theme foundation of 10.5 (ADR 0023)

- SDK 0.3 (`hyscribe:sdk@0.3.0`) adds the `theme` world: `template` (the loop, conditional, printing and value tags by WordPress's name, the template part functions), `theme-setup` (ADR 0023's `theme` interface, renamed as a WIT package cannot name an interface and a world alike) and `customize`, and the `theme-guest` export's `render`; template tags are named rather than one WIT function each, so the interface grows without a type change
- A manifest of `kind = "theme"` has no `replaces` and none of the display keys, and needs SDK 0.3; a component loads only as its manifest's kind; the SDK builds the theme world with its `theme` feature and exports a theme with `theme!`
- The SDK's test host (`hyscribe_sdk::test`): built natively, the SDK's functions answer from a site in memory, so a port's own code is covered and mutation tested without wasm
- The embedder loads the active theme's component per request after the plugins, from `themes/<stylesheet>/`, a child only with its parent installed; a theme renders in a second instance under one budget and fault ledger with its callbacks' instance
- The classic template hierarchy (`hyscribe_ext::locate`) matches PHP for Twenty Twenty-One on all 101 requests of the default install's page list (`reference/themes/hierarchy-golden.sh`)
- To confirm: the theme world as SDK 0.3 rather than inside the unreleased 0.2, the interface name `theme-setup`, named template tags, and admin actions running over the plugins only

## 2026-11-22: ADR 0024 proposed (package signing and the hosted registry)

- The remaining design of extension distribution: minisign signatures checked by HyScribe's own reader over crates already vetted, offline root keys signing a key list of online keys, a signed static index, `hyscribe plugin install <name>` and `hyscribe plugin update`, revocation, and update rows on the Plugins screen
- Eleven questions, each with a recommendation; nothing is built until it is accepted

## 2026-11-22: SDK 0.2, its first slices (11.4)

- The WIT package is `hyscribe:sdk@0.2.0`; the host links each 0.1 interface under both versions, and a component whose imports name another SDK than its manifest's `sdk` is refused
- Time inside host calls no longer counts against a call's or an extension's request budget (ADR 0023's question 5): host functions are timed and the epoch deadline's callback extends the deadline by that time; `REQUEST_TIME_LIMIT` bounds the rest
- Transients (`[transients] names` and `site`), roles and capabilities (`[roles] names`, never WordPress's own roles, and `capabilities`) and outbound requests to granted hosts (`[http] hosts`) are host calls under their grants, driven by the test extension; meta is now served over the database too
- `hyscribe-write` gains `get_transient()` and `get_site_transient()` with their expiry on read, `delete_site_transient()` and `WP_Roles`' writes, matched row for row by write oracle actions `transient-expired`, `site-transient-delete`, `roles-add`, `roles-caps` and `roles-remove`
- `hyscribe-fetch` sends an extension's own method, headers and body under every rule of ADR 0019, following redirects for GET and HEAD only and only to granted hosts; fetches are off in admin actions
- Left for the rest of 11.4: admin pages and the settings API, uploads folders and multipart bodies, page templates and `template_include`, rewrite rules and query variables, plugin owned tables and migrations, the transient hooks (ADR 0022's 11.4 amendment)

## 2026-11-21: No WordPress name left in hs-admin, hs-login and the setup wizard (ADR 0008 amendment)

- Requirement: the admin front end names WordPress nowhere: the footer's version line now shows `HyScribe x.y.z` (`update_footer`), with the goldens' footer version mapped to the running one by `hyscribe_testkit::hs_admin_page` and masked in the hashed Theme Test Data pages, so a release does not stale them
- Strings of WordPress's script bundles (the site editor's document title, the post editor's welcome guide, the inserter's pattern help, the comment preview and the site icon help) get HyScribe wording through WordPress's own i18n mechanism: the `<handle>-js-translations` inline script with `wp.i18n.setLocaleData()` for `wp-block-editor`, `wp-block-library`, `wp-edit-post` and `wp-edit-site`, printed before their tags as `WP_Scripts` prints translations; the bundles' bytes are the pinned release's. The Classic, Shortcode and Comment Edit Link block descriptions are reworded in the editor's block definitions
- The oracle's look plugin gives PHP the same through `update_footer`, `pre_load_script_translations` and `gettext_with_context`; the admin goldens were recorded again for the editors, the dashboard, plugins, settings, profile, users and the Theme Test Data hashes, and the footer and translations tags spliced into the others (the splice reproduced each full recording byte for byte)
- The setup wizard says HyScribe; "WordPress" stays only where it names the PHP install a site comes from or switches back to, and the WordPress versions a database must match
- Kept as names of other services or software: the embed block's WordPress and WordPress.tv variations, the social link block's WordPress service, the bundles' unseen private API and deprecation strings, the `X-Redirect-By` header, and stored content and theme headers

## 2026-11-21: `hyscribe plugin delete <id>`

- Decision: plugins can be deleted from the command line; the Plugins screen keeps refusing delete as PHP does under HyScribe's constants (`delete_plugins` is never granted)
- An id that is not a slug is refused before any path is built, and one that is not installed is reported; a plugin `active_plugins` names (any entry under `<id>/`) is refused with a note to deactivate it first and nothing changes. `active_plugins` is only read, and only for an installed plugin; an unreadable or non-list option refuses too
- Otherwise `plugins/<id>/` is renamed to `plugins/.<id>.deleting` (never loaded) and removed, then the kept packages that name this id (a package of `<id>-more` shares the file name prefix, so each is read for its id); links are removed themselves and never followed; a failure is reported after the rest is done, and running the command again finishes the removal
- Nothing is written to the database: PHP's `delete_plugins()` changes `uninstall_plugins` and the `update_plugins` transient only for the PHP plugin's own files, which stay for a switch back, and it never touches `recently_activated`, so no write oracle case applies (ADR 0022's 11.3 amendment)

## 2026-11-21: Extensions run per request, and their lifecycle exports (11.3)

- Each request runs the extensions `active_plugins` names that are installed and approved, with the grants and limits of their approval: `hyscribe_ext::Runtime` loads each once per installed version (about 70 ms for the test extension) and finds the set unchanged in about 1.1 µs a request; the embedder sets the request's dispatcher for each poll of its future and carries it into its blocking tasks, so concurrent requests never share an instance or a scope
- The runtime's site serves options over the database, a page view's calls each committed on their own and an action's inside the action's transaction; meta, queries, cron, mail, the current user and assets answer `failed` until served (ADR 0022's 11.3 amendment)
- The Plugins screen runs the `activate` export where PHP fires `activate_{$plugin}`, between `activate_plugin` and `activated_plugin`, and `deactivate` between `deactivate_plugin` and `deactivated_plugin`; an activation whose component does not load or whose export fails leaves the plugin inactive and answers PHP's redirect to the screen's fatal error notice, now ported; a failing deactivate export is logged and the plugin deactivated anyway, a deliberate difference from PHP, which keeps it active
- New hook points `body_class` and `admin_footer_text`; the test extension's filters on them, its activation writing `ext_active` and its deactivation deleting it are matched against the PHP plugin it stands in for, which now does the same (`reference/plugins/golden.sh`, write oracle `admin-plugins-*`); the switch-back suite checks the option PHP finds
- Proved end to end: installed and activated, the test extension changes the front end's body classes and hs-admin's footer as PHP's plugin does, two requests at once each count from one, three traps disable it until it is activated again, and deactivated it changes nothing

## 2026-11-21: ADR 0023 accepted

ADR 0023 was accepted as proposed: askama templates compiled into each theme's component, Hello Elementor and Twenty Twenty-One as embedded components on the extension runtime, a second rendering instance per request, host call time bounded by `REQUEST_TIME_LIMIT` rather than the component budgets (ADR 0022 to be amended), per-request theme features, children compiled with their parents, the Customizer ported without drafts, scheduling, theme previews, lock takeover or shared links, no starter content, and Hello's admin page waiting for SDK 0.2.

## 2026-11-21: ADR 0023, the classic theme model, proposed (10.4)

- Classic themes are components on the extension runtime (`kind = "theme"`), templates and `functions.php` logic alike; Hello Elementor 3.5.2 and Twenty Twenty-One 2.9 were read for it
- The template engine proposed is askama, compiled into the theme's component, so the server parses no templates; the SDK keeps pages as bytes (values spliced in after rendering, so content that is not UTF-8 prints as PHP prints it) and escapes with the ported `esc_html()` by default, raw output only for the SDK's `Html` type. minijinja and tera in the host, minijinja in the component, hand-ported Rust, sailfish and maud were compared and set aside
- A `theme` world adds the loop and template tags, theme features, the Customizer's registrations and a `render` export; theme features are request state set in `after_setup_theme`; a theme gets a rendering instance beside its callbacks' instance, and time in host calls stops counting against a component's budget
- Nav menus (`wp_nav_menu()`, Appearance > Menus), widget areas (`dynamic_sidebar()`, the block widgets editor), theme mods and the Customizer (screen, preview and publish saves, with drafts, scheduling, theme previews and starter content left out) are ported in the host and matched against PHP; child classic themes compile with their parent into one component
- Ten questions are listed in the ADR; installing third party classic themes still waits on a decision

## 2026-11-21: Child block themes (10.3)

- A child theme (`Template:` in its `style.css`) resolves as PHP does: its templates and parts over its parent's, its `theme.json` merged over the parent's (lists such as `templateParts` and `customTemplates` merge by position, and a preset list the child sets replaces the parent's), its style variations added to the parent's with a parent file of a name the child has left out, the parent's stylesheet enqueued from the parent's URL with the child's version, `get_theme_file_uri()` for `file:./` fonts, and the body classes `wp-theme-<template>` and `wp-child-theme-<stylesheet>`; the speculation rules' exclusions name the child's folder before the parent's, as WordPress's swapped `template` and `stylesheet` contexts do
- Themes screen: a child's entry names its parent, and the parent of the active theme has no delete action; activating a child writes `template` as its parent and `stylesheet` as itself (`switch_theme()`), five write oracle actions cover switching to and from it
- The fixture is our own, `hyscribe-fixture-child`, a child of Twenty Twenty-Five with an overriding 404 template, footer part and pattern, a custom page template, a theme.json and two style variations, one named as a parent's. It is not a shipped theme: its source is `reference/themes/fixtures/` (mounted in the oracle), its export `fixtures/themes/`, and `hyscribe-render` embeds it only with the `fixture-themes` feature, which the tests of the crates that read themes switch on, so a shipped build lists the four themes it has. The oracle lists it among the installed themes, and the Themes screen and admin saves goldens were recorded again with it
- The child matches PHP on 101 of 101 default install pages and 747 of 747 Theme Test Data pages, its 396 REST answers (themes, global styles and their variations, block types, patterns, templates and parts, as each role), its 10 theme files (their times are the checkout's, so not recorded) and the Themes screen as the active theme
- Not ported for a child: the Site Health Info tab, the post and site editors' pages (editor stylesheets of both themes) and the REST theme routes of the parent theme; they answer 501 or are untested. A switch involving a child makes PHP keep a `theme_roots` transient of the folder it scans, which HyScribe does not write

## 2026-11-21: The plugin lifecycle (11.3)

- `hyscribe plugin install <file>` checks a local `.hyx` package, loads its component once, keeps it in `packages/` and unpacks it into `plugins/<slug>/`; only local files until distribution and signing are decided
- The manifest is read by the host's own strict TOML subset reader and the package by its own zip reader over `miniz_oxide` and `crc32fast`, both bounded, with fuzz targets `ext_manifest` and `ext_package`; no TOML or zip crate enters the tree. The grant tables (`[meta]`, `[taxonomies]`, `[rest]`) are read, and `[extension]` gains the plugin's displayed headers; recorded in ADR 0022's 11.3 amendment
- The Plugins screen lists installed extensions as PHP lists the plugins they stand in for (12 of 12 recorded views match), with what each asks for shown before activation; activate and deactivate write `active_plugins` and `recently_activated` row for row as PHP (write oracle), approve or revoke the grants in `approved.ext.toml`, and delete is refused as PHP refuses it under HyScribe's constants; a view keeps `recently_activated` to the last week as PHP writes it
- The switch-back suite installs and activates the test extension beside the site's own Hello Dolly, and PHP finds both active
- Not yet: the embedder loading active extensions per request, and the `activate` and `deactivate` exports at activation

## 2026-11-20: hs-admin gets its own look (ADR 0008 amendment)

- Requirement: hs-admin is not to be mistaken for wp-admin: Coffee is the color scheme for a user whose stored scheme is missing or WordPress's Default (`modern`), and the picker leaves Default out; nothing is written for it, and a save stores what the user picks, as PHP would
- hs-admin's and hs-login's own screen text says HyScribe instead of WordPress (titles, footer, help, dashboard widgets, Site Health, login messages); the W logo menu is gone from the admin bar and the login screen shows a HyScribe text mark; content, markup, classes, hooks and URLs are unchanged
- The oracle stays differential: `reference/admin-look/hyscribe-look.php`, a must-use plugin working only through WordPress's filters, is added by every admin, login and screenshot golden script, and the admin goldens are recorded again with it
- Left as they are for now: strings inside WordPress's JavaScript bundles (the block editor's document title among them), the footer's version line and the setup wizard's mentions of the imported WordPress site

## 2026-11-20: Theme switching, and every path follows the active theme (10.2)

- The Themes screen lists the embedded themes, active first; activating one writes the rows PHP's `switch_theme()` writes, and the next request runs `check_theme_switched()`'s follow ups (menus and widgets carried over, rewrite rules flushed), each matched row for row by 21 write oracle actions in `reference/writes/actions-themes.php`
- REST (themes, global styles, block types, patterns, templates and template parts), the dashboard, Site Health and the post and site editors now read the active theme instead of Twenty Twenty-Five, with goldens recorded from PHP with Twenty Twenty-Four and Twenty Twenty-Three active; each theme's `registered.json` gains the data those paths need
- Only switches between embedded themes are served; a site whose theme is not embedded answers 501 on activate. Pattern caches are matched by the theme slug in their patterns, as HyScribe does not know PHP's folder hash
- A pattern block naming an unregistered pattern outside `core/` is left as is, as PHP does, instead of answering 501

## 2026-11-20: Twenty Twenty-Two pinned and served as data (10.1 done)

- Decision applied: `themes/pinned.tsv` pins Twenty Twenty-Two 2.2 and Twenty Twenty-One 2.9 from wordpress.org by version and SHA-256; `reference/themes/pin.sh` downloads, verifies and unpacks them for the oracle, and `hyscribe-assets`'s build script verifies the same archives (`HYSCRIBE_THEME_ZIPS` for offline builds); Twenty Twenty-One is pinned only, its port is 10.6
- Twenty Twenty-Two matches PHP on 101 of 101 default install pages, 747 of 747 Theme Test Data pages and its 69 theme files; about 6.8 MB of embedded data
- Ported on the way: the legacy `core/post-comments` block through `comments_template()` (Walker_Comment's HTML5 items, comment pages, the classic form, the `cpage` the canonical link reads), the `wp-block-styles` theme stylesheets, header images counted in the viewport; the comments goldens gain legacy cases

## 2026-11-20: The extension runtime and SDK 0.1 (11.2)

- `hyscribe-ext` runs WebAssembly extensions as ADR 0022 sets: one engine per process, a pre-instantiated package per extension, a fresh instance per request, each call bounded by the smaller of its own budget and what is left of the request's, 16 MiB through a resource limiter; a manifest asking past the ceiling is refused, not clamped
- Faults are counted in `hyscribe/plugins/<slug>/faults.txt` (written whole, mode 0600, renamed into place, fail closed); new fuzz target `ext_faults`
- `hyscribe-sdk` 0.1 (Apache-2.0 OR MIT): hooks, options, meta, post types and taxonomies, queries, REST routes, shortcodes, blocks, assets, cron, mail, the current user, capabilities, nonces and i18n, each host call checked against the manifest's grants; the spike guest is replaced by a test extension built on the SDK
- Hook points with scalar, string, PHP value or string list values reach extensions; object valued hooks wait for their PHP shapes to be checked
- The host's sensitive hook list gains `retrieve_password_key`, `wp_set_password`, `password_reset`, `after_password_reset`, `wp_mail_succeeded` and `wp_mail_failed`, firing a sensitive hook needs the grant too, and queries refuse `post_type: any`; stricter than the ADR, waiting on confirmation
- Five WIT records cross as wasmtime values rather than derived types, recorded in the ADR amendment; `dlmalloc` and `prettyplease` publishers trusted in `cargo vet` until 2027-11-18

## 2026-11-20: ADR 0022, the extension host, accepted (11.1)

- The answers to its ten questions are written in as decisions; the draft numbered 0020 is renumbered 0022
- A manifest may ask for limits above the defaults (50 ms a call, 250 ms an extension a request, 16 MiB) up to a fixed ceiling of 500 ms, 2 s and 64 MiB, approved at activation; anything else refuses the package
- Fault state lives in `hyscribe/plugins/<slug>/faults.txt`, never in the database; security sensitive hooks need their own `[hooks] sensitive` grant
- `itertools` and `cpp_demangle` audited by HyScribe; the spike's guest is built from source (`crates/hyscribe-ext/guest/build.sh`, the `wasm32-wasip2` target); fuzz target `ext_load`
- Decision: Twenty Twenty-Two and Twenty Twenty-One, no longer bundled with 7.1.3, are pinned from wordpress.org at a version and checksum and installed in the oracle

## 2026-11-20: Twenty Twenty-Four and Twenty Twenty-Three served as data (10.1)

- Each `themes/<slug>/` exported from the oracle is embedded, and the `stylesheet` option picks the theme; no Rust names a theme; the export also records post formats and the stylesheets a theme enqueues
- Both themes match PHP on the default install (101 of 101) and the Theme Test Data (747 of 747), and their 49 and 44 theme files match; the binary grows by 5.9 MB
- Core behaviour ported on the way: the query block's style handle, the hidden author avatar counted as an image, an attachment's status for the post date binding, comment page numbers under two pages, template part content filters
- Twenty Twenty-Two is not bundled with WordPress 7.1.3, so it waits on a decision; REST, admin and write paths still read Twenty Twenty-Five until theme switching (10.2)

## 2026-11-20: The plan for themes and plugins (phases 10 and 11)

- Phase 10: the bundled block themes first (10.1 to 10.3, no runtime needed), then the classic theme ADR, Hello Elementor and Twenty Twenty-One, then installing themes
- Phase 11: the extension host ADR (renumbered 0022) waits on its ten questions; then the runtime, the lifecycle and SDK 0.2, and the reference plugins smallest first: Contact Form 7, Yoast SEO, Elementor's front end, then its editor; security and a joint acceptance run close both phases

## 2026-11-20: The MVP is met (9.1)

- At `54a73e35` every GOALS criterion passes: S1 (101 of 101 default install pages, 747 of 747 Theme Test Data pages), S2, S3, S4 (a new site in 3.65 s), S7 (the switch-back suite with 0 check failures on both fixtures), S8 (55 of 58 screens, the 3 others listed), coverage at 100%, `cargo deny` and `cargo vet`
- CI runs every gate on each push; all six jobs passed
- Next: themes (phase 10) and plugins (phase 11)

## 2026-11-20: The Theme Test Data page hashes are answered at the day they were recorded

- CI's first run after midnight failed 28 Theme Test Data pages: their calendar block marks today, and the hashes were recorded on 2026-11-19; no test wrote to the database
- Under test, `now()` reads a pinned time for the hashed pages only, from `crates/hyscribe-http/tests/golden/pages-ttd.clock`, which `reference/pages/golden.sh ttd` writes for the day it records

## 2026-11-20: The site editor screenshot settles before capture (S8)

- The fourth acceptance run's last gap, the site editor screen, was a capture race: the editor fetches the navigation fallback, menus and template parts after the load event, so a slow machine captured it while still drawing; `capture.py` now waits until no spinner shows; three runs, locally and in CI, match 55 of 58 with the 3 listed exceptions
- HyScribe's navigation fallback route answers as PHP does with `_embed=true&_locale=user`, now in its goldens

## 2026-11-20: The release workflow proven by a dry run

- `release.yml` takes a `workflow_dispatch` with `dry_run` (default true): both architectures build, the x86_64 build checks reproducibility, smoke tests the archive and runs the switch-back suite, and the publish job makes the SBOM and checksums; attestation and the GitHub release run only on a `v*` tag push
- The switch-back step uses the Docker relay and the literal host path as CI does; unproven until the first tag: the draft release, the provenance attestation and the tag version check

## 2026-11-20: The switch-back suite and the screenshot comparison in CI

- New CI jobs `switch-back` (the default install and the Theme Test Data, about 4 minutes) and `screenshots` (about 6 minutes) run after `coverage` on every push to `development` and `main`; `fuzz` waits for both and runs last; their logs and reports are uploaded on failure
- Needed: a Docker relay inside the job container for the test user (`scripts/ci-docker-proxy.sh`), the checkout's host path written literally in steps (the runner rewrites `_work` to `/__w` in step environments), a port forward for `php.sh`, and writable copied uploads in `reference/switch-back/run.sh`
- `release.yml`'s switch-back step needs the first two of these before a tag can run it

## 2026-11-20: Trusted proxies written as IPv4-mapped addresses

- Found by the `trusted_proxies` fuzz target in CI: an entry such as `::ffff:192.0.2.1` never matched, since a peer's mapped address is read as IPv4 while the entry stayed IPv6; such an entry (with a prefix of 96 or more) now names its IPv4 address, and a wider IPv6 range still holds the mapped address

## 2026-11-20: The navigation fallback REST route

- `GET wp-block-editor/v1/navigation-fallback` answers as PHP does, on the same fallback decision as the page view, and writes the same post with the asking user as author (goldens `reference/rest/navigation-fallback.list` and `navigation-fallback-existing.list`, write oracle actions `rest-navigation-fallback-*`)
- The screenshot comparison no longer seeds the navigation post: both sides start with a visitor home page view, which writes it alike; PHP's other write of it in that run came from Site Health's loopback test, which HyScribe does not run

## 2026-11-20: The block patterns route, and theme URLs in pattern content

- `GET wp/v2/block-patterns/patterns` answers as PHP does with remote patterns off: core's registered patterns, then the theme's, with `_fields` and the contexts; no wordpress.org call and nothing written (golden `reference/rest/block-patterns.list`, oracle recorded with `should_load_remote_block_patterns` off since a blocked fetch writes a site transient)
- Theme pattern content no longer carries the export's address: URLs follow the site's `siteurl` (`hyscribe_render::theme::under_site`) on the front end, in the templates routes and in the patterns route
- The screenshot comparison seeds the navigation post on both sides, so both number their auto-drafts alike; 55 of 58 screens match, the 3 left are the listed exceptions

## 2026-11-19: The preview nonce check (`_show_post_preview()`)

- Front end requests with `preview_id` and `preview_nonce` that fail `wp_verify_nonce( ..., 'post_preview_<id>' )` for the current user answer PHP's 403 `wp_die()` page before routing
- The default install's whole pages: 101 of 101 match PHP (`pages-default-known.tsv` empty)
- It runs on every entry point that loads WordPress, as PHP's `init` does: hs-admin, `admin-ajax.php`, `admin-post.php`, `wp-comments-post.php`, hs-login, cron, REST, `xmlrpc.php` and the other PHP files, answering `wp_die()` by its handler (page, Ajax text, JSON, XML, an empty body for `xmlrpc.php`); the loaders do not run `init` and are untouched (golden `reference/preview-nonce/golden.sh`, 64 requests)

## 2026-11-19: Third MVP acceptance run (9.1)

- Run at `805f5d07`; result and evidence in [docs/ACCEPTANCE.md](docs/ACCEPTANCE.md): S2, S3, S4, S7 pass, S1 passes on the Theme Test Data (747 of 747) with one known default install difference, S8 matches 51 of 58 screens pixel for pixel
- Verdict: not met against the literal GOALS wording (S1's one request, S8's 6 listed differences and one stale `expected.list` entry) and not gated in CI for S7 and S8
- Two `rest::tests::uploads` tests failed once under concurrent load at `956053aa` and passed alone; not reproduced

## 2026-11-19: The navigation fallback's writes and classic menu items

- When no menu converts, a page view writes the `page-list` post PHP writes ("Navigation"), as no user, in primary mode; a menu with items not ported writes nothing and shows the page list
- Classic menu items of posts, pages, terms and the `post` archive are set up as `wp_setup_nav_menu_item()` does, with the classes of `_wp_menu_item_classes_by_context()` for the request
- `wp_get_nav_menus` and `wp_get_nav_menu_items` exposed on `Hooks`; 18 `navigation-fallback-*` write oracle actions and two default install page goldens

## 2026-11-19: CI on a self-hosted runner

- GitHub Actions runs on a self-hosted runner (outbound polling only), on pushes to `development` and `main` and by hand; no pull request trigger
- Jobs run in a fresh container per job pinned by digest: checks (fmt, clippy, SPDX, deny, audit, vet, gitleaks), coverage at 100% over the fixtures, the musl package with its smoke test, and every fuzz target for 30 seconds; mutation testing stays out of CI
- Fixes the new CI exposed: the SPDX header of `.cargo/mutants.toml`, the `admin_actions` fuzz target, reviewed `.gitleaksignore` fingerprints, and the `mail_headers` and `duotone_color` fuzz checks
- `release.yml` moved the same way, not yet run on a tag

## 2026-11-19: New site path of the setup wizard (S4, ADR 0021)

- The wizard installs WordPress 7.1.3 as `wp_install()` does into an empty database: tables and rows copied from PHP's installer (`reference/install/golden.sh`), matched row for row and `SHOW CREATE TABLE` for `SHOW CREATE TABLE` on 5 cases
- HyScribe creates tables only here, only for a prefix with no table, and drops them again if a later step fails
- A timed test runs the browser wizard from an empty database to a home page answering 200 within S4's time
- A new site gets pretty permalinks, as PHP after a successful probe: the install step itself writes `permalink_structure` and PHP's `rewrite_rules` (a home URL with a path has its own recorded value), so no first start marker is written; no notification mail yet

## 2026-11-19: All 747 Theme Test Data pages match PHP

- The loading and duotone differences were stale entries: only the RSS error text differed
- The classic menu navigation fallback is ported with its `wp_navigation` write through `wp_insert_post` as no user, checked by the write oracle actions `navigation-fallback-*`; menu items other than custom links keep the page list for now
- The audio attachment page enqueues MediaElement as `wp_audio_shortcode()` does, and the footer prints scripts in enqueue order

## 2026-11-19: The sweep's surviving mutants closed

- 11 mutants left by the mutation sweep killed with tests (render hooks, autop after shortcodes, mediaelement styles, attachment template names, post meta reads)
- Three `vec![]` replacements of hook defaults in `outbound.rs` excluded as equivalent (`.cargo/mutants.toml`, `coverage-exclusions.md`)

## 2026-11-19: The block editor's persisted preferences (`POST users/me`)

- A user's `meta` is ported for `persisted_preferences` only (`update_value()`): validated by its schema, written after the roles, and its errors answered once the user is written, as PHP does; a password for the current user still answers 501
- Goldens: `user-writes.list` extended, `user-preferences.list` over stored rows, write actions `rest-users-preferences*`
- The editor no longer gets a 501 each time it opens; S8's screenshot differences left are block patterns, `navigation-fallback` and Site Health's PHP-only sections

## 2026-11-19: Theme Test Data RSS blocks and the random gallery compared

- The RSS blocks read the url-details test site on both sides (`reference/pages/ttd-feeds.sql`, `feed-themes.xml`, `feed-news.xml`), with `pages.test` the one allowed feed host
- Post 1031's `orderby="rand"` gallery (14 pages) is compared by its items in any order, `loading="lazy"` kept by position (`reference/pages/ttd-body.py`, `hyscribe-testkit::unshuffle`)
- 743 of 747 Theme Test Data requests match; 4 remain known (the navigation fallback from a classic menu 3, an attachment page 1)

## 2026-11-19: Screenshot comparison of hs-admin against wp-admin (S8)

- `reference/screenshots/golden.sh` photographs 58 screens for administrator, editor, subscriber and visitor in a headless Chromium (Playwright's image, pinned by digest), PHP and HyScribe on one fixture under the same host name
- Tolerance: 16 of 255 per colour channel and 0.01 percent of pixels, plus the same set of requests; `expected.list` records the differences by design
- 50 screens match pixel for pixel; the 8 left come from unported routes (block patterns; `POST users/me`; `navigation-fallback`), Site Health's PHP-only sections and the media grid's address without `.php` (ADR 0008). No HTML or CSS difference found

## 2026-11-19: Trusted proxies, a request time limit and a registration cap

- `X-Forwarded-For` is read only from peers in `TRUSTED_PROXIES` (loopback by default, addresses or CIDR ranges, fuzz target `trusted_proxies`), through the one `comment_post::client_ip` every limit and record uses; security review S-10 closed
- A request past `REQUEST_TIME_LIMIT` (30 seconds) answers 503 and its uncommitted write rolls back; cron and the mail queue are not cut; S-11 closed
- Registration is capped per client address by `REGISTRATION_LIMIT_ADDRESS` (3 an hour, `-1` for none), counting only accounts created; past it, PHP's `registerfail` error and nothing written

## 2026-11-19: The trash schedule, REST media update and delete, the global styles post

- In primary mode every logged-in hs-admin request first schedules what PHP's `admin.php` and `init` would: `wp_scheduled_delete`, `delete_expired_transients` and the two privacy events, the `cron` option matching the write oracle row for row (ADR 0017 amendment), so a site only HyScribe has served still empties its trash
- `POST`, `PUT`, `PATCH` and `DELETE` on `/wp/v2/media/<id>` are ported through the media library's `wp_delete_attachment()`, files removed only after the rows commit (9 write oracle actions, 228 REST cases)
- A themes read whose links need the user global styles post creates it as PHP does (`rest-themes-global-styles`, 54 REST cases)
- The switch-back suite no longer needs its SQL workarounds: 154 actions and 72 pages on the default install, 39 and 31 on the Theme Test Data
- Still 501: `featured_media` and a negative `post` on a media update, media trash and restore, deleting a logo, site icon, header or classic menu attachment

## 2026-11-19: Redirects on the allowlist, braces in global styles

- Every redirect hop of an embed or RSS fetch after the first must be an allowed host (`Client::get_within` in `hyscribe-fetch`, the predicate run after the URL checks and before resolving); a refused hop leaves an embed a plain link and a feed PHP's unreachable error, caching nothing. Embed redirects may reach the listed hosts and the oEmbed provider endpoint hosts
- A global styles value containing `{` or `}` is unsafe and dropped; two PHP goldens that keep such a value are checked against the difference HyScribe means to give
- Both are deliberate differences from PHP, documented in SECURITY_MODEL, COMPATIBILITY and ADR 0019

## 2026-11-19: Uploads of every type in the default allowed list

- `wp_check_filetype_and_ext()` ported whole over a bounded, memory safe stand in for libmagic (`hyscribe-image` `fileinfo`, fuzz target `upload_fileinfo`) that answers only where every type libmagic may give leads to one verdict: 196 of 222 PHP type checks match and none of 1,068 answered libmagic samples disagree; getID3 is ported for PCM WAVE and the formats WordPress's copy has no module for
- 107 upload route cases for 6 identities and 15 new write oracle actions match PHP; images, audio, video, PDF and plain text are served with nosniff, everything else downloads
- Still 501: PDF previews (PHP renders them through Ghostscript), the getID3 metadata of MP3, MP4 and MOV, Ogg, FLAC, ASF, Matroska, AVI and FLV, Word 97 and other OLE files, bytes libmagic calls `data`; design in ADR 0020 (non-image uploads), proposed

## 2026-11-19: The acceptance gaps closed in one batch

- Media library (MVP 6.4): the attachment edit screen's save, attach and detach, `wp_delete_attachment()`; 43 write oracle actions and 66 Ajax goldens match PHP; a song's empty metadata is removed as PHP does. `MEDIA_TRASH` is an `.env` key awaiting confirmation
- Comments: posts on paged comments (`get_page_of_comment()` counting the user's or cookie email's held comments), multipart comment posts, and commenter cookies (`sanitize_comment_cookies()`, the form, held comments, comment pages)
- Profile and Edit User over HTTPS with application passwords; every admin page in a secure context prints `__clientSideMediaProcessing` as PHP does; Updates is refused as PHP refuses it; missing plugins are shown without PHP's write
- Core shortcodes (`[caption]`, `[gallery]`, `[audio]`, `[video]`), every feed enclosure, `wpautop` restored after block content; Theme Test Data term ties told apart in the golden fixture and the tag cloud on `WP_Term_Query`'s SQL
- `WP_Duotone`, attachment pages and their redirect, feed image loading: the Theme Test Data's known differences fall from 221 to 51 requests
- Block editor: footnotes `meta`, comment notes, `author_ip`, the sticky password check
- Registration (`register_new_user()`, through the login limits) and the personal data confirmation link
- Embeds fetch only from `EMBED_ALLOWED_HOSTS`, by default WordPress's oEmbed providers
- The Plugins screen (its help naming `hyscribe/plugins/`) and Site Health's Status tab with HyScribe's own six checks
- The comment-writing tests share one lock, `COMMENT_ROWS`

## 2026-11-19: Scaled and rotated uploads on every route

- `wp/v2/media` no longer refuses an image PHP scales (`-scaled`, `big_image_size_threshold`) or rotates (`-rotated`, EXIF orientation); `missing_image_sizes` reads the original image's size from the uploads folder as `wp_get_missing_image_subsizes()` does, names that leave the attachment's folder staying 501; the async routes already matched PHP
- Corpus grows by 7 files (large PNGs in RGB, indexed and RGBA, both threshold edges, orientations 3 and 8); the upload routes golden has 49 cases for every role and 7 new write oracle actions match row for row
- Decision open: ADR 0015's palette PNG SSIM bound is 0.95, and the scaled 64 colour PNG reaches 0.9493 as the quantizers differ (ImageMagick octree against NeuQuant); 0.94 is set for that file only

## 2026-11-19: The switch-back suite covers uploads, site editor saves and cron (S7)

- HyScribe now makes REST and async uploads with their sizes, template, template part and global styles saves, a cron-published scheduled post and a cron-emptied trash, and PHP WordPress reads and serves each correctly: 142 actions and 70 pages on the default install, a REST subset and 29 pages over the Theme Test Data, PHP's debug log empty
- Found: hs-admin never schedules `wp_scheduled_delete` as PHP's `admin.php` does; REST media update and delete answer 501; HyScribe cannot create the user global styles post (PHP does on the site editor's first view)

## 2026-11-19: The site editor's saves on the front end, and global styles saves (MVP 7.2, 7.3)

- The front end uses the site editor's saved templates and template parts in place of the theme's files, applies the site's saved global styles, and renders the navigation menus a block names, as PHP does: 12 edited-site pages, 99 default-install pages, 36 page template cases and 123 navigation cases match byte for byte
- Global styles save through `wp/v2/global-styles/<id>` with `WP_Theme_JSON::remove_insecure_properties()` and `wp_filter_global_styles_post()` ported (23 and 8 cases against PHP, 96 REST answers); the write layer runs the filter before kses on every post's content as `content_save_pre` does; theme.json's CSS check now shares the kses crate's `safecss_filter_attr()`, and `sanitize_html_class()` moved there; the `theme_json` fuzz target runs the filter
- The privacy policy page gets its template and body class in PHP's order; a navigation block whose menu reference is 0 uses the global post; one `names_term()` replaces three copies of the theme term filter
- Decisions open: whether HyScribe filters global styles CSS more strictly than PHP (a value such as `red;} body{x:y}` passes PHP's check); custom CSS is dropped on save, as no one has `unfiltered_html`

## 2026-11-19: The site editor's REST writes (MVP 4.3)

- Templates and template parts (create, theme file customise, update, revert, trash, delete), pattern meta and categories, and autosaves, matched against PHP for a visitor and every role: 147 template cases, 180 pattern cases and 60 autosave cases, with 22 write oracle actions row for row; simple schema `pattern` ported; the templates REST hooks fire as PHP fires them, `rest_after_insert_` tested through a test-only log of fired actions
- An autosave writes only the fields `fields()` reads; the after-save fields it never wrote were removed
- Still 501: pattern meta PHP refuses only after the write, autosave `meta`, reverting a template with no customised post or theme file, templates the read path cannot answer, text `wpdb` refuses

## 2026-11-18: Embeds and the RSS block (ADR 0019)

- `core/embed` ports `WP_Embed::autoembed()` wherever PHP runs it, with `WP_oEmbed` over WordPress's 55 providers (exported into `wordpress/wp-includes/oembed-providers.json`), discovery, `data2html`, `wp_filter_oembed_result` and the YouTube handler; the `_oembed_` post meta cache is written in primary mode only, and while fetches are off an uncached URL stays as it is with nothing written; 35 of 35 PHP cases match
- `core/rss` ports `fetch_feed()` over SimplePie 1.9's data tree and its transient cache, byte for byte with PHP's; it fetches only from `RSS_ALLOWED_HOSTS`, empty by default; 23 of 23 PHP cases match; two Theme Test Data pages now match PHP
- A new linear-time XML reader refuses internal DTD subsets, expands no entities and stops at depth 256 (fuzz target `xml_reader`); `hyscribe-fetch` returns response headers; the outbound arguments carry the feed timeout, the discovery size limit and the redirects, each tested
- Decisions open: an allowlist for embeds, no `{{unknown}}` meta written while fetches are off, a cached feed from a host off the allowlist

## 2026-11-18: Second security review of the writes (MVP 9.2)

- The threat model in `docs/SECURITY_MODEL.md` updated for the write path, the REST API, uploads, mail and outbound fetches, with a findings table (S-1 to S-16) and a table of every parser of untrusted input and its fuzz target; new fuzz targets `auth_cookie`, `forwarded_for` and `upload_filetype`, and `text_fields` covers `sanitize_email`, `is_email` and `sanitize_title`
- Fixed, each from a failing test: `post-new?x=page` wrote an auto-draft and an edit lock without the capability check (medium, S-1); no time limit on reading a request body, now 60 seconds then 408, and 413 past 4 MiB (medium, S-2); `options?action=dismiss` without a nonce; the block editor taking over another user's fresh edit lock; `/users/me` writing the user a body's `id` named; a disallowed word's comment going to trash with the trash off instead of spam; logins now clear a pending reset key and rehash old hashes; terms and roles repeated in a REST list written once each
- Open: trusting `X-Forwarded-For` from any peer behind `BEHIND_PROXY` (S-10), and no limit on a request's total time (S-11)

## 2026-11-18: lettre 0.11.24, the test databases in memory, a lean review

- `lettre` 0.11.24 for RUSTSEC-2026-0565: a malicious or forged SMTP reply line without an end could exhaust memory, and STARTTLS, the default, lets a man in the middle forge one; 0.11.24 caps a reply at 100,000 bytes and refuses bytes sent before the TLS handshake. Delta audits for `lettre` and `email-encoding` 0.4.3
- `db.sh` keeps MariaDB's data in a tmpfs (`HYSCRIBE_DB_TMPFS`, 4g by default, `off` to disable): the whole clippy and coverage gate went from about 45 minutes to 11, and the test databases' writes no longer reach disk
- Lean review: four unused `theme.json` accessors removed; no unused dependencies, placeholders or unread goldens found
- Decision: the finished wordpress.org calls are held out of development for now

## 2026-11-18: REST indexes, oEmbed and batch; change notices; operator docs (MVP 4.4, 8.2, 9.3)

- REST: the namespace indexes (`/wp/v2`, `/oembed/1.0`, `/wp-site-health/v1`, `/wp-block-editor/v1`, `/wp-abilities/v1`) from the exported descriptions, oEmbed for this site's own posts and pages (JSON and XML, the proxy with PHP's `edit_posts` check and no outbound fetch), `batch/v1` with PHP's validation and its 25 request limit, each write in its own action, and the sidebars, widgets and widget types read as a block theme answers them; matched against PHP for a visitor and every role. Serving the namespace indexes before every route in them is ported is an ADR 0013 amendment awaiting a decision
- `wp_update_user()` reports its change notices: Edit User, the profile and REST user updates send `password_change_email` and `email_change_email` once committed, matching PHP (8 cases in `mail-flows.tsv`), including PHP's comparison of the address slashed; the personal data request mails wait for their actions (501)
- The refused feed cases (`?feed=nope` with and without `If-None-Match`, `_atom`, `comments-`, a refused feed's redirect) recorded against PHP; HyScribe already matched
- Operator documentation (9.3) checked against the binary and brought up to cron, the block editor, the site editor page, mail, uploads and the REST writes; switching covers a rehearsal on a copy, PHP beside HyScribe and the steps back; backups have restore commands
- Merge: a batch's writes go through the uploads' `finished()`, each next write made in its own action with its own outbox, sent once it commits

## 2026-11-18: Uploads (MVP 6.2)

- Image uploads through `async-upload.php`, Add New Media and `wp/v2/media` (raw body and multipart), with the sizes made on the worker pool, served from `/wp-content/uploads/` with `nosniff`; matched to PHP over 204 HTTP cases (34 cases as a visitor and each role) and 15 write oracle actions; AVIF differs only in its sizes (ADR 0015)
- `UPLOAD_MAX_SIZE` is enforced before buffering and the `IMAGE_MAX_*` keys are read from `.env`; uploaded files are served and never run; anything but an image, audio, video, PDF or text file is a download
- Still 501: files PHP checks with fileinfo (text, PDF, audio, video), the async uploader's `fetch`, `post_data`, `Content-MD5`, uploads that would make a `-scaled` or `-rotated` copy, a custom `upload_path`
- `db.sh up` removes a killed container's stale socket first and gives up after five minutes instead of waiting for ever
- Decisions open: HTML and SVG served as downloads, 413 for a body over the limit, which fileinfo types to port

## 2026-11-18: Unknown feeds, array searches and the write oracle's time window

- `do_feed()`'s feed choice ported: `comments-` and leading underscores dropped, an empty name or `feed` meaning rss2, an unknown type refused with `_xml_wp_die_handler()`'s 404 after `send_headers()` and `redirect_canonical()`; an array `s` is an empty search, as `WP_Query::parse_query()` makes it. `/?feed=nope` and `/?s[]=hello` now match PHP byte for byte and left the whole-page known list
- The write oracle's time window opens five seconds before its first database clock reading, for dates as for Unix times: tests read their request's time before `check_writes` connects, which is what made `%now%` fail under load (not clock skew)
- Tests with tight time limits given room on a loaded machine (linear-time checks 10 s, the fetch timeouts 30 s, the login waiting line's watchdog 60 s)

## 2026-11-18: The mail transport (MVP 8.2, ADR 0016)

- `lettre` over STARTTLS (required) or TLS from the first byte, with SMTP AUTH, over the host's certificates; `SMTP_HOST`, `SMTP_PORT`, `SMTP_TLS`, `SMTP_USER`, `SMTP_PASSWORD` and `MAIL_FROM` in `.env`; without `SMTP_HOST` mail is off and each message is logged as not sent
- One `wp_mail()` running WordPress's filters and actions in 7.1.3's order (`wp_mail` before `pre_wp_mail`), a `phpmailer_init` stand in; a CR, LF or NUL in a recipient, subject, header or sender name refuses the message
- Sent after the action commits: the Users list's and Edit User's password resets, the lost password mail (waited for, with PHP's errors), new user notices, the profile's email change confirmation, the new admin email confirmation and change notice, the reset form's admin notice, and the front end's comment notices
- A bounded queue (1,000, 60 a minute), resets limited to 3 per account and 10 per client address an hour; one log line per message, never the body
- Supply chain: lettre and its dependencies audited; `0BSD` allowed, and `CDLA-Permissive-2.0` for `webpki-roots` only (to be confirmed); fuzz target `mail_headers`
- Not wired yet: password and email change notices for another user's edit, the personal data request mails (in progress)

## 2026-11-18: Themes and plugins join the road map

- Decision: themes (phase 10, from M7) and plugins with the extension runtime (phase 11, from M6) are added to `docs/MVP.md` after the MVP's acceptance, and the plugin work starts now, alongside the MVP's last phases
- The reference plugins of 11.4 are the three most installed open source plugins on wordpress.org: Elementor, Yoast SEO and Contact Form 7
- Open: how extensions are distributed and signed

## 2026-11-18: The site editor's page (MVP 7.1)

- `site-editor.php` is served in primary mode once its first view wrote the custom CSS lookup and the theme's global styles post, as PHP does; all 9 administrator variants of the page golden match byte for byte, and the first view's rows match the write oracle's `admin-site-editor`
- `get_block_editor_settings()` takes the editor kind (post or site); the two editors share their template part areas, preloads, global styles and block style assets; `reference/site-editor/parts.php` folded into `reference/post-edit/editor-parts.php`
- Still 501: the page in shadow mode (its first view writes), arguments that are not strings, posts that are neither posts nor pages, preloads not ported
- `a_server_that_never_listens_times_out` waits 10 seconds instead of 3, as a loaded machine missed the fake server's `ready` line

## 2026-11-18: The terms', settings', comments' and users' REST writes (MVP 4.3)

- Categories and tags (create, update, delete), settings (update), comments (create, update and status changes, trash, delete, following `EMPTY_TRASH_DAYS`) and users (create, update, delete with `reassign`, `me`) on the two-step plan design, matched against PHP for a visitor and every role (about 700 cases) and 11 write oracle actions row for row
- `allow_and_insert_comment()` and `add_role()` added to `hyscribe-write`; the client address and user agent reach REST; bug fixed: the REST `Site::can()` answered for the current user whoever was asked
- The write oracle masks a new ID only as a whole number (it had masked the 127 of 127.0.0.1)
- Still 501: `meta`, comment notes and `author_ip`, a new password for oneself; visitors' REST comments are refused, as PHP's default filter does

## 2026-11-17: The rest of the security review fixed

- Entity decoding is linear; behind `BEHIND_PROXY` the client is the last entry of all `X-Forwarded-For` lines, `ip:port` and bracketed forms are parsed, and an entry naming no address gets no login turn and is not counted, never falling back to the proxy's address
- Post text that is not valid UTF-8 is refused as wpdb refuses it (`db_insert_error`, `db_update_error`); a REST `id` below 1 answers 404; post passwords are compared with `hash_equals`; SECURITY_MODEL.md corrected
- Open: comments and term descriptions with invalid UTF-8 are stored as given, where PHP strips the bad bytes

## 2026-11-17: The block editor's saves (MVP 5.2)

- Mapped from 7.1.3's editor, core-data and api-fetch sources: every save is one POST with a PUT or DELETE override, `_locale=user`, the ID, the changed fields and the content; all 34 requests match PHP for every identity (204 cases) and 11 write oracle actions row for row, the scheduled post's `publish_future_post` event included
- Ported `set_post_thumbnail()`, `set_post_format()` and `check_template()` with the `theme_templates` filters; the meta box save does not apply to a default install
- Still 501: inline new terms, footnotes `meta`, autosaves, creating a custom template

## 2026-11-17: The block editor page (MVP 5.1)

- `post-new` (post or page) and `post.php?action=edit` serve the block editor page in primary mode: `get_block_editor_settings()`, the server block definitions, categories, bindings sources, the meta boxes area and the script and style closure match PHP's 12 editor pages byte for byte apart from preload data; every preload is answered by `hyscribe-rest`
- The first view writes before rendering (auto-draft, edit lock, the theme's global styles post with its `wp_theme` term, the custom CSS lookup), only once every preload it does not write is answered, matching 4 write oracle goldens; the theme pattern cache transient is not written

## 2026-11-17: Image findings of the security review fixed

- Each decoder's own reading of the size must equal the header's and pass the limits before any pixel buffer is allocated (the JPEG `FF 00` bypass); PNG metadata has its own 16 MiB limit outside the pixel budget; EXIF is bounded to its APP1 segment, IPTC parsing is linear, the limit arithmetic is checked, and a GIF counts the RGBA bytes it allocates; hostile cases seed the image fuzz targets
- Release builds unwind (`panic = "unwind"`, was `abort` since M1 with no recorded reason), so a decoder panic ends only its image job; `hyscribe-image` refuses to build with `abort`; decision pending

## 2026-11-17: Lost password and reset flows (MVP 8.3)

- The lost password and reset password forms of `/hs-login` write as PHP does: `retrieve_password()` (moved to `hyscribe-write`, shared with hs-admin) stores `time():wp_fast_hash()`, `check_password_reset_key()` with the one day expiry and a new `wp_verify_fast_hash()`, `reset_password()` clears the key and ends every session of the user; their hooks fire under their names; 42 requests match byte for byte and two write oracles row for row
- Mails are built and taken as sent until the transport (ADR 0016) is wired; lost password requests are not rate limited yet (question)

## 2026-11-17: ADR 0018's amendments built

- The login limits run as `hyscribe_login_limits` at priority 10 of the `authenticate` chain, ahead of core's password checks at 20; callbacks are named, so an extension can add or replace one; the address and account refusals and the CPU cap sit inside `wp_check_password()`, where no callback can skip them
- `wp_login_failed` and `wp_login` fire under their names; the limits count from the first and trust the address from the second (`wp_login` fires before HyScribe sets its cookies, where PHP fires it after)
- Site wide, at most one password check per CPU runs at once, the rest wait up to 2 seconds in a line of 8 per CPU, past which a guess is answered as wrong without bcrypt and not counted; a guess with an empty username is no longer counted

## 2026-11-17: The switch-back suite covers every merged write (MVP 8.5)

- 124 actions (from 53) and 50 pages: REST posts, pages, navigation and synced patterns compared with PHP's own REST reads, deletes and leftovers, revisions, heartbeat locks, every comment moderation path, the Trash off after a restart with `EMPTY_TRASH_DAYS=0`; PHP reads everything back with no notices
- Found and fixed: a trashed post's REST `permalink_template` was the pretty one where PHP's is plain

## 2026-11-17: WP-Cron runs in the service (ADR 0017)

- In primary mode the service runs the due events of the six core hooks HyScribe ports once a minute, behind PHP's `doing_cron` transient in its own format; each event is one action, a failing job keeps its event and its hook waits until restart, other events are left as they are
- Off in shadow mode, with `DISABLE_WP_CRON`, or beside an `object-cache.php` found above `UPLOADS_DIR`; `ALTERNATE_WP_CRON` is ignored with a log line; `hyscribe cron run` does one run; the scheduling functions keep WordPress's names and the `cron_schedules` filter point is added

## 2026-11-17: The site editor's first part (MVP 7.1)

- `site-editor.php`'s older addresses redirect as PHP's `_wp_get_site_editor_redirection_url()` does, refusals and the "Invalid post type." 500 match; every one of its REST preloads is served as PHP serves it; the settings it adds are ported; the default template types and part areas move to `hyscribe-render`
- The page and its first view's writes are recorded but answer 501 until the block editor's page pieces (MVP 5.1) are merged

## 2026-11-17: Release and operations fixes

- `scripts/package.sh` builds `ring` with clang for both musl targets; CI and the release workflow install it
- `service start` reports started only once the server listens (a `listening` line in `run/hyscribe.pid`) and reports a failure with the log's reason
- The setup secret goes to the terminal or to `logs/setup-secret` (0600), deleted when setup mode ends, never to `hyscribe.log`
- One parser reads every yes or no `.env` key (`true`/`yes`/`on`/`1`, `false`/`no`/`off`/`0`, any case)
- The docs mark `doctor`, `plugin install`, `theme install`, the planned folders, the wizard's scan and the XML-RPC key as planned, with their milestones

## 2026-11-17: hs-admin URLs without .php, built

- The router answers a 301 from `/hs-admin/<screen>.php` to `/hs-admin/<screen>` for GET and HEAD and serves other methods in place; `hyscribe_admin::menu::clean_links` cleans every admin page and redirect in one place, including encoded and JSON-escaped forms; goldens map `wp-admin/<file>.php` to `hs-admin/<file>` in `hyscribe_testkit::hs_admin_page`, so none was re-recorded

## 2026-11-17: Whole-page tests over the Theme Test Data (M3.12)

- 747 Theme Test Data requests built from its database (stored as hashes) and 98 more default install requests (`reference/pages/`); 511 Theme Test Data pages match; pages that still differ are listed in `pages-*-known.tsv` and must keep differing until fixed, so a fix shrinks the list
- The golden script makes outbound HTTP fail and stops the classic menu navigation fallback, so pages are reproducible
- Fixed: comment links on paged comments (`get_page_of_comment()`), document title filters, `page-parent`, theme block style CSS, relative stylesheet links, the button style on password forms, feeds counted as `is_home()`
- Left: core shortcodes, image loading priority, attachment pages and their redirect, oEmbed, the RSS block, duotone, later enclosures, the classic menu fallback, term name ties in MariaDB order, the preview nonce `wp_die`, array query vars, unknown feeds

## 2026-11-17: Operator documentation (MVP 9.3)

- `docs/operators/` (install, configuration, service, switching over and back, backups and upgrades, security, not supported), shipped in the release archive as `hyscribe/docs/` and checked by the smoke test; `hyscribe-config`'s `operator_docs` test fails when a key the loader reads is undocumented or its default differs
- Found on the way: `ring` (from the outbound client) needs a musl C compiler for the release build; the smoke test's `curl | grep -q` failed under pipefail (fixed); the setup secret stays in `logs/hyscribe.log` after setup; `service start` reports started before the server listens; several docs describe commands, folders and keys that do not exist yet

## 2026-11-17: UPLOAD_MAX_SIZE and Site Health's Info tab

- `UPLOAD_MAX_SIZE` in `.env`, 64 MB by default, printed wherever hs-admin prints `wp_max_upload_size()`; enforcing it is the upload handler's
- Site Health's Info tab is served with WordPress's WordPress, Active Theme, Media and Database sections as they apply and a HyScribe section in place of server and PHP; left out: the wordpress.org reachability row, the theme folder, PHP ini rows, the database client and limits, the constants and filesystem sections (to be confirmed); the Status tab stays 501

## 2026-11-17: hs-admin URLs without `.php` (ADR 0008 amended)

- Decision: hs-admin's screens are addressed without `.php` (`/hs-admin/edit`); HyScribe prints the clean form, the `.php` form answers a 301 to it (POSTs served in place), and goldens map `wp-admin/<file>.php` to `hs-admin/<file>`; public endpoints keep WordPress's names

## 2026-11-17: ADR 0019 accepted

- ADR 0019 was accepted (the outbound HTTP client) as proposed, with all six of its decisions as recommended: `HTTP_OUTBOUND` off by default, SSRF checks on every resolved address of every hop, WordPress's widening filters only allowed to narrow, trusted-publisher cargo vet entries for the TLS crates and `openssl-probe` for the host's certificates, url-details answering 501 while fetches are off and refused to visitors before any lookup

## 2026-11-17: Outbound HTTP client proposed (ADR 0019), MVP 4.4

- ADR 0019 proposed: `hyscribe-fetch` on hyper and rustls with the ring provider over the host's certificates; SSRF checks on every resolved address of every hop, connecting to the address checked; 30 s, 4 MiB, 5 redirects; `http_request_args` and `pre_http_request` under their names; `HTTP_OUTBOUND` off by default; trusted-publisher cargo vet entries for the TLS crates
- `wp/v2/block-renderer` serves 14 dynamic blocks matching PHP for each role; `wp-block-editor/v1/url-details` is built and matches PHP behind `HTTP_OUTBOUND`, answering 501 until the ADR is accepted

## 2026-11-17: Trash off, database sockets and UPLOADS

- With `EMPTY_TRASH_DAYS` at 0, hs-admin, the REST API and the trash actions follow PHP: Delete Permanently instead of Trash, no Trash views, `rest_trash_not_supported`; golden variants named `e0…` run PHP with the constant at 0 (`as-role.php`)
- Bug fixed: `wp_trash_post()` and `wp_trash_comment()` trashed even with the Trash off; they now delete forced, as PHP's do
- A Unix socket is given in `.env` as wp-config.php gives it, `DB_HOST=localhost:/path/to/socket`, only for `localhost` as for mysqli; both wizards copy it, and a literal `UPLOADS` gives the uploads default
- Open: wp-config's plain `localhost` means PHP's default socket, while HyScribe connects over TCP to `localhost:3306`

## 2026-11-17: Navigation and synced pattern REST writes, REST hooks (MVP 4.3)

- `wp/v2/navigation` and `wp/v2/blocks` create, update, trash and delete on the two-step plan design; WordPress's six REST hooks (`rest_pre_dispatch`, `rest_pre_insert_`, `rest_insert_`, `rest_after_insert_`, `rest_prepare_`, `rest_delete_{post_type}`) fire at their points from one place in `hyscribe-rest`
- `rest_invalid_json` is ported with PHP's JSON error codes and messages
- Bug fixed: a draft's unset `modified_gmt` used the stored `gmt_offset` instead of a named zone's offset
- Templates, template parts, global styles, autosaves, and pattern meta and categories still answer 501

## 2026-11-17: hyscribe-image (MVP 6.1, 6.3)

- Own header and IPTC readers with fuzz targets, EXIF through kamadak-exif, zune-jpeg, png, gif, image-webp and jpeg-encoder with SIMD off, each fully audited; ImageMagick's policy limits are checked from the header before any decoder allocates
- An Imagick-step resampler and `wp_create_image_subsizes()` with `-scaled`, `-rotated` and EXIF orientation match PHP's names, dimensions and metadata on 42 cases; pixels within PSNR 38.8 dB and SSIM 0.953
- Differences from PHP, left open: sizes keep only the ICC profile (ADR 0015), so GPS data is dropped; CMYK JPEGs and 16-bit PNGs come out as 8-bit RGB; BMP gets no sizes; the unsharp mask is left out as it changes no pixel with WordPress's parameters

## 2026-11-17: The browser setup wizard (MVP 8.4)

- `start` without `.env` serves only `/installation/step-1` to `step-3` on the configured address; the one-time secret is typed into a form, never put in a URL, and works once, then one `SameSite=Strict` session; every form carries a token; setup mode stops after 30 minutes; nothing secret goes back to the browser
- `EMPTY_TRASH_DAYS` is a `.env` key, read by trashing and the scheduled deletion

## 2026-11-16: The REST index (ADR 0013)

- Built the decision: the index is served from the site's options and the exported route data without routes; requests that would list routes or namespaces answer 501, and so does `authentication` on an https site, where PHP lists application passwords; `hyscribe_rest::Request` carries the `Host` header and the request's time

## 2026-11-16: `wp_fast_hash()` on the audited `blake2` crate

- Decision: certify RustCrypto's `blake2` 0.10.6 with `cargo vet` after a full source review (no build script or I/O, `unsafe` limited to a sound integer byte view, the SIMD and assembly features left off) and use it for `wp_fast_hash()` as `Blake2bMac<U30>` with default features off; the hand-written BLAKE2b is removed and the 11 PHP goldens still match

## 2026-11-16: Decisions on ADRs 0013, 0015, 0016, 0017 and 0018; WordPress hook points

- ADR 0015 accepted: the codec crates with SIMD off and HyScribe's own resampler; IJG allowed for `jpeg-encoder`; AVIF uploads kept without sizes (not refused), a safe decoder a follow-up; WebP sizes lossless for now; limits default to the oracle's ImageMagick policy (32,000 wide and high, 256,000,000 pixels, 1 GiB, measured), set in `.env` and checked from the header; resizing in a worker pool sized to the CPUs; a `WP_Image_Editor` shaped interface and WordPress's image hooks
- ADR 0016 accepted: the setting names, `MAIL_FROM` defaulting to `wordpress@` and the host without `www.`; `lettre`, certified by HyScribe with cargo vet; the per account reset limit kept, a bounded queue delaying mail over the overall limit; background sending where PHP ignores the result; the commenter's IP address in notifications; one `wp_mail()` running WordPress's filters in order, and a `phpmailer_init` stand in to replace the transport
- ADR 0017 accepted as proposed: the wizard copies `DISABLE_WP_CRON` and `ALTERNATE_WP_CRON`; events stay keyed by hook name; `wp_schedule_event`, `wp_next_scheduled`, `wp_unschedule_event` and `cron_schedules` under their names
- ADR 0018 accepted with its defaults: the limits become a step of the `authenticate` chain; `authenticate`, `wp_login_failed` and `wp_login` fire under their names; concurrent password checks capped site wide at the CPU count
- ADR 0013's writes amendment accepted: `rest_invalid_json` ported instead of 501; WordPress's REST hooks at their points; `register_rest_route()` shaped registration, refusing a route without `permission_callback`
- New rule (docs/EXTENSIONS.md, CLAUDE.md): every subsystem exposes WordPress's filters and actions under their names and argument order, plain internal calls until M6, never skipping a check and bounded in time

## 2026-11-16: Revisions, their restore and the heartbeat (MVP 5.3, first part)

- `wp_text_diff()` is ported from Text_Diff, which is LGPL-2.1; decision: LGPL-2.1 code may be ported, taken under its section 3 as GPL version 2 or later with the original notice kept (ADR 0004)
- The heartbeat refreshes the edit lock as PHP writes it; admin-ajax gives every cookie the hour's grace `wp_doing_ajax()` gives it
- Three text diff helpers whose mutants change no output over 800,000 random pairs are excluded from mutation testing (`.cargo/mutants.toml`, coverage-exclusions.md)

## 2026-11-16: Login rate limits (ADR 0018 proposed)

- `/hs-login` limits password guesses, a deliberate difference from PHP, which has none: a refused guess is answered as a wrong password before its bcrypt check; 5 failures per address per 15 minutes, doubling up to a day; 10 per account per window, except from the account's last 8 login addresses; one check at a time per address
- State is in memory only (ADR 0003) and bounded; keys `LOGIN_LIMIT_ADDRESS`, `LOGIN_LIMIT_ACCOUNT` and `LOGIN_LIMIT_WINDOW`

## 2026-11-16: The first REST writes (MVP 4.3); the REST index decided

- REST writes answer in two steps: `hyscribe-rest` runs PHP's checks and gives a plan, `hyscribe-http` writes it in one writer action and answers from the reloaded site; shadow mode answers 403; nothing is answered 501 after a commit (ADR 0013's writes amendment, proposed)
- Decision on ADR 0013's index amendment: serve the index from the exported route data, answering 501 when `_fields` asks for `routes` or `namespaces` or gives none, so the block editor's preload of the site fields is served

## 2026-11-16: WP-Cron's core jobs; ADR 0017 proposed (MVP 8.1)

- The core jobs and the `cron` option's bookkeeping are ported and matched against PHP; `wp_publish_post` is kept, as cron's `publish_future_post` calls it; the write oracle masks rescheduled times as `%unix+<interval>%`
- ADR 0017 proposes running them in the service in primary mode behind PHP's `doing_cron` lock, leaving update checks and plugins' events to PHP, with `DISABLE_WP_CRON` in `.env`; nothing runs them until a decision is made

## 2026-11-16: The Permalinks view refreshes the rewrite rules

- In primary mode each view of the Permalinks screen writes `rewrite_rules` as PHP's `flush_rewrite_rules()` does, in the same transaction as the view's other writes; refusals, shadow mode and `.htaccess` write nothing
- The switch-back suite now changes the structure through numeric permalinks and a static front page and passes; its `check.php` generates PHP's rules with `$matches[n]`, so a stored array can match (only empty rules had been checked before)

## 2026-11-16: Internal security review of the write path (MVP 9.2)

- Fixed (high): a Customizer flag on `admin-ajax.php` skipped every check before the ported Ajax handlers; the Customizer is not ported, so such requests answer 501
- Fixed (high): a profile update carrying a pending email argument skipped its own nonce and `edit_user` check; an update now always runs its checks. A divergence from PHP: a valid dismissal nonce with `action=update` answers 403 where PHP dismisses the notice
- Fixed: Screen Options is handled before a screen's own actions, an array role writes nothing, and the moderation keys' duplicate check is linear
- Open: login rate limiting and lockout, a listed control not yet built

## 2026-11-16: The block editor's REST preloads

- Route descriptions (`OPTIONS`) are exported from the oracle (`rest-descriptions.json`) and served for every ported route, with `Allow` computed over the unchecked parameters as PHP computes it
- The themes route serves Twenty Twenty-Five from `registered.json`; a read whose links would create the global styles post answers 501 until that write is made as PHP makes it
- ADR 0013 now proposes three ways to serve the index from the same export, for a decision

## 2026-11-16: The switch-back suite (MVP 8.5) and the Permalinks refresh

- `reference/switch-back/run.sh` has HyScribe write a fresh default install through 53 real HTTP actions, then PHP WordPress, under `WP_DEBUG`, rechecks the data and serves 30 screens and pages; the release workflow runs it
- It found that Bulk Edit replaced a post's tags with tags named by their IDs (fixed, test first), and that a permalink structure change left PHP stale `rewrite_rules`
- Decision: the Permalinks screen refreshes `rewrite_rules` on each view, in primary mode, as `flush_rewrite_rules()` does; `.htaccess` is never written

## 2026-11-16: The setup wizard, terminal flow (MVP 8.4)

- `./hyscribe setup` copies an existing site's wp-config.php into `.env`: the capability constants only when literal, otherwise the safer value holds; salts it cannot copy are generated; no secret is echoed or printed
- First-run checks only read: both users connect, `db_version` in range, the read user holds only SELECT, a persistent object cache warns; MyISAM tables refuse primary mode and are a warning in shadow mode, which never writes (ADR 0012)

## 2026-11-16: REST read gaps closed but for the index (MVP 4.2)

- Fixed: an empty explicit-list order wrote `FIELD(col, )`, invalid SQL; the database layer now leaves it out as `parse_orderby()` does (posts fall back to the date, terms with an empty `include` or `slug__in` to the name)
- `oneOf` is ported, which serves the term limits of posts, patterns and menu items; revisions and autosaves of patterns, navigation and menu items; term search over several taxonomies (`TermQuery.taxonomies`)

## 2026-11-16: Lean round after phase 3, finished

- Deleted the write functions no handler calls (`wp_publish_post`, `add_role`, `remove_role`, the site transients and three `Write` methods) with their oracle actions; one in-transaction capability check, `hyscribe_write::caps::Caps`, serves hs-admin's saves and the GUID of a private post, now with the site's constants
- The write oracle's setups run without the admin hooks `update_option_new_admin_email()` and `WP_Internal_Pointers::dismiss_pointers_for_new_users()`: since the oracle loads the admin's code for every action, they had made `admin-options-adminhash` record nothing and shifted two cases' usermeta IDs; all 232 write goldens record their committed rows again

## 2026-11-16: The block editor's refusals (MVP 5.1, first part)

- `post.php?action=edit` and `post-new.php` answer PHP's refusals (403, the Trash's 409, an invalid post type's 500, the attachment redirect); the editor's page itself waits on its preloads (REST `OPTIONS` descriptions, the active theme route), `get_block_editor_settings()`, the server block definitions and its first view's writes
- By decision the first view's writes (the `wp_global_styles` post and `wp_theme` term, the auto-draft, the edit lock and the cache transients) are made as PHP makes them, in primary mode

## 2026-11-16: WordPress's emails as messages; ADR 0016 proposed (MVP 8.2)

- `hyscribe-mail` builds every email WordPress sends as a pure message, matched byte for byte against PHP's `wp_mail()` arguments captured by a `pre_wp_mail` must-use plugin (`reference/mail/golden.sh`), quirks included (`wp_update_user()`'s backslashes in an address, a doubly decoded site title)
- ADR 0016 proposes the transport: `.env` SMTP settings, `rustls` over the host's certificates, `lettre` without its C features, mail sent after the commit, rate limits, header injection refused, logs without bodies, subjects or keys; five decisions are listed

## 2026-11-16: ADR 0015 proposed: image decoding and resizing (MVP 6.1)

- A `hyscribe-image` crate with `#![forbid(unsafe_code)]` over the codec crates with SIMD off (not `image`, whose SIMD `unsafe` no imported audit covers), HyScribe's own resampler following Imagick's steps, limits checked from the header before any buffer, AVIF refused as PHP refuses it without editor support; goldens exact for names, dimensions and metadata, bounded for pixels. Six decisions are listed in the ADR

## 2026-11-16: Post saves and the uploader follow `unfiltered_html`

- Post saves skip kses, and the uploader's allowed types add htm, html and js, only for a user with `unfiltered_html`, read through the capability check; by default everything is filtered
- Fixed: post content and excerpts now go through `convert_invalid_entities()` as PHP's do
- The write oracle records `*-unfiltered-html` actions without `DISALLOW_UNFILTERED_HTML`, and `as-role.php` does the same for the `hyscribe_unfiltered_html` cookie

## 2026-11-16: Lean round after phase 3

- Fixed: the upload host in the posts' `wp_kses_post()` context is the site URL's, as `wp_upload_dir()` builds it; Quick Draft, the dashboard views and the users list's delete used the home URL's, so a PDF `<object>` lost its attributes on a site whose two URLs differ
- `reference/writes/helpers.php` holds the write oracle's shared code; every write golden recorded the same before and after
- 434 lines fewer: one admin-ajax dispatch table, one `WP_Ajax_Response` builder, one posts `Context` builder and one handler test harness

## 2026-11-16: Insecure home URLs follow WordPress's checks

- `wp_replace_insecure_home_url()` rewrote `http://` links to an HTTPS home on every HTTPS site; WordPress rewrites them only after its HTTPS migration, when home and siteurl both use HTTPS on the same host and `https_migration_required` is set. Post content, excerpts and text widgets now follow those checks
- Its oracle, `reference/insecure-home-url/golden.sh`, records the function over 24 home and siteurl pairs (case, whitespace, ports, paths, schemeless and relative URLs) with the option missing or stored 15 ways (384 lines)

## 2026-11-16: The comments' actions, finished (MVP 3.3)

- Reply, Quick Edit, the comment edit screen and its Update write in primary mode; admin requests carry the client address and user agent, from the same peer reading front end comment posting uses
- Reply and edits follow `unfiltered_html`, as `.env`'s `DISALLOW_UNFILTERED_HTML` allows; by default they go through kses
- The write oracle's comment post cases had replaced `$actions` and dropped every admin case; fixed
- Phase 3's admin actions are done but for the gaps listed in PROGRESS.md

## 2026-11-16: The capability constants come from `.env`

- `DISALLOW_UNFILTERED_HTML`, `ALLOW_UNFILTERED_UPLOADS` and `MULTISITE` take `true`, `false`, `1` or `0`; anything else stops the start, and a missing key takes the safer value
- `MULTISITE=true` stops the start: multisite is not in the MVP (docs/MVP.md) and comes with M9, and serving a network's tables as one site would be wrong
- Until the setup wizard exists, site owners copy the keys from `wp-config.php` by hand (DEPLOYMENT.md)

## 2026-11-16: The posts and pages lists' actions, finished

- Bulk Edit's missing Sticky view after the action was a test artefact: the test-only `router()` serves a fixed `Site`, while a served site reloads its options on every request
- Parallel mutation runs use one fixture database per worker: shared ones deadlock and leave the fixture changed

## 2026-11-16: The text widget's own filters

- A visual text widget runs the `widget_text_content` filters, no longer those of `the_content`: its images get their loading attributes in that context, so inside post content they wait for `the_content`'s pass and an image before the widget keeps the high priority, and an attachment's own content is not added to the widget's text
- Its oracle, `reference/text-widget/golden.sh`, records seven widgets, one inside a block widget, outside the loop, in a singular post's loop, through `the_content` of a post and of an attachment, over an attachment it inserts and rolls back (28 lines)

## 2026-11-16: An action stops at its first failed statement

- A deadlock rolls back the whole transaction on the server while sqlx still counts it open, so a later statement of the same action committed on its own, reproduced with a real deadlock. `Write` now gates every statement: after the first failure no statement runs and the commit fails, as ADR 0012's all or nothing requires; the connection returns to the pool with no open transaction
- No automatic retry, as PHP's `wpdb` does not retry: a deadlocked action fails its request

## 2026-11-16: The posts and pages lists' actions (MVP 3.1)

- `post.php`'s trash, restore and delete, the lists' bulk actions, Empty Trash, Bulk Edit and Quick Edit write PHP's rows; handlers that check capabilities post by post read them through their own write action
- Found while testing: two test processes on one database once committed rows into the fixture after a deadlock rolled a transaction back and later statements ran outside it

## 2026-11-15: hs-admin writes users (MVP 3.4)

- Add User, the profile and Edit User saves, deletion with reassignment, role changes, password reset keys, the color scheme and ending sessions; answers are PHP's once mail has gone out, and no mail is sent until phase 8
- `wp_fast_hash()` is BLAKE2b written by hand in `hyscribe-auth`, checked against PHP's output: RustCrypto's `blake2` matched too, but no imported `cargo vet` audit covers it; whether to certify the crate ourselves is open

## 2026-11-15: Comment moderation in hs-admin (MVP 3.3, first part)

- Admin handlers receive the site as their checks read it (`Acting`, `saves::read`), for per comment capability checks and links
- `_wp_ajax_delete_comment_response()` recounts at random in PHP; HyScribe always recounts, one of PHP's outcomes

## 2026-11-15: Each request reads the site's options

- Fixed a breach of ADR 0012: the served site loaded its autoloaded options once per process, so a Settings save or a change by PHP showed only after a restart. They are now read on every request in one query, about 0.8 ms; the rewrite rules are compiled again only when their permalink options change
- Table engines are still checked once at start, as ADR 0012 says; the active theme is always the bundled one (a missing feature, not a cache); the rules are compiled from the permalink options rather than read from `rewrite_rules`, which differ only when a plugin changed the stored rules

## 2026-11-15: Front end comment posting (MVP 3.7)

- The client address is the connection's peer, or under `BEHIND_PROXY` the last `X-Forwarded-For` hop, as DEPLOYMENT.md says forwarded headers are trusted only there
- The commenter cookies add `HttpOnly` and `SameSite=Lax` to PHP's, as SECURITY_MODEL.md requires of every cookie
- Shadow mode answers a comment post 403 with the read only message

## 2026-11-15: The block widget

- The legacy widget block renders `WP_Widget_Block`: its content through the `widget_block_content` filters, `do_blocks()` and `wp_filter_content_tags()` (neither `WP_Embed`'s autoembed nor shortcodes are run), in the wrapper whose class `get_dynamic_classname()` takes from the first block; a stored instance goes through `wp_parse_args()` as PHP's `widget()` takes it
- The loading attributes know which filter runs: while `the_content` or `widget_block_content` does, an image or iframe of another context gets none, as `wp_get_loading_optimization_attributes()` checks `doing_filter()`; `wp_filter_content_tags()` takes the context it runs in
- Its oracle, `reference/block-widget/golden.sh`, records the class name for each first block, and 15 widgets (two of them stored inactive widgets, saved as an object and as a query string) outside the loop, in a singular post's loop and through `the_content`, over an attachment and options it sets and rolls back (64 lines)

## 2026-11-15: The gallery widget

- The legacy widget block renders `WP_Widget_Media_Gallery` through a port of `gallery_shortcode()` under the `html5` gallery support of block themes: the image attachments of the instance's `ids`, in their order or the instance's `orderby` and `order` (random with `orderby_random`), each through `wp_get_attachment_link()` to its page or file, or as the image alone, with its caption, the item, icon and caption tags the instance names when WordPress allows them, and `gallery-N` IDs counted through the request. Nothing renders unless every ID is an attachment; an `ids` of arrays alone lists the image attachments up to `posts_per_page`, as `get_posts()` passes no `post__in` then
- Not ported: the shortcode's feed output. Where PHP throws a `TypeError` for an array item, icon or caption tag, the default tags are used
- `WP_Query`'s limit for `posts_per_page` is one function, which the latest posts block shares
- Its oracle, `reference/media-gallery-widget/golden.sh`, inserts thirteen attachments inside a transaction it rolls back and records 41 instances outside the loop and in a singular post's loop, and three more with `posts_per_page` changed (85 lines)

## 2026-11-15: The Settings saves (MVP 3.5)

- `sanitize_option()` is ported for the Settings options; the writer carries `WP_HOME` and `WP_SITEURL`, so `get_option()` returns them as PHP does
- In primary mode a Settings screen deletes the notices a save left once it shows them, as PHP does, with the page view's writes; shadow mode only reads
- Found: `Site` keeps the autoloaded options and rewrite rules for the life of the process, so a save shows only after a restart; ADR 0012 forbids caching writable state across requests, so this is a bug to fix

## 2026-11-15: GUIDs under pretty permalinks

- `wp_insert_post()` gives GUIDs under pretty permalinks as PHP does, matching 7 write goldens over the five Settings > Permalinks structures; scheduled posts, revisions and private posts the current user may not read keep plain GUIDs

## 2026-11-15: Categories and tags write through hs-admin (MVP 3.2)

- Ajax actions answer through `saves::Ajax` handlers, and the rows they print are rendered after the commit; in shadow mode they answer 403 with the read only message as plain text, PHP's Ajax `wp_die()` form
- `term.php` is ported

## 2026-11-15: The dashboard's and the admin's per user writes (MVP 3.6)

- Page view writes in primary mode run in one write action committed with the page; writing HTTP tests use the scratch database `wordpress_saves`, so the shared fixture never changes

## 2026-11-15: The dashboard's auto-draft and the admin's user settings

- Decision: the dashboard's Quick Draft auto-draft and the admin's user settings are written as PHP writes them, in primary mode, through the write layer; shadow mode writes nothing

## 2026-11-15: Phase 3 groundwork

- Admin writes go through `saves::act()`, one transaction per action; a `wp_die()` part way rolls back what PHP would have kept (ADR 0012); shadow mode answers saves with the login screen's read only message; the write oracle runs wp-admin's own switch cases from source; REST writes are wired with the first REST write handler (MVP 4.3)

## 2026-11-15: Phase 1 done but for pending decisions

- The last decision-free admin gaps are ported read only: media players and the pretty permalinks slug on the attachment edit screen, the personal data request lists, and a missing child or parent theme; what phase 1 still lacks waits on decisions

## 2026-11-15: Lean round over the write layer

- One first-error helper (`Steps`, generic over each area's error) serves terms, comments and users; options, meta and posts stop at the first error with `?`, and every area's error tests spend `fail_after` budgets, which now fail every read too
- One name filter, term count, time zone read and meta deletion of an object serve every area; seven unused write methods are gone from `hyscribe-db`

## 2026-11-15: Missing rows in the write tests

- Write tests and the write oracle name a missing row by `1 << 40` (`MISSING`, `HYSCRIBE_MISSING_ID`): rolled back inserts still move the auto increment counters, so a fixed 999 was reached in long runs and failed the coverage gate

## 2026-11-15: Post writes (MVP 2.4); phase 2 done

- The post write functions are ported over the terms, comments, options and meta ports; `EMPTY_TRASH_DAYS` is one crate constant; every write test shares one lock
- With posts, terms, comments, users, options and meta, the core write functions of MVP 2.4 are in place, and phase 2 (the write foundation) is done but for the listed gaps, of which GUIDs under pretty permalinks comes first

## 2026-11-15: The image widget

- The legacy widget block renders `WP_Widget_Media_Image`: an image attachment through `wp_get_attachment_image()` at a registered size or at a width and height, or an `<img>` for a URL alone with the `widget_media_image` loading attributes; linked to the file, the attachment page or a custom URL (the default link type), and captioned by `img_caption_shortcode()` under the `html5` caption support of block themes. An attachment that is no image, with no URL, prints the wrapper and title alone
- `image_downsize()` and `wp_get_attachment_image()` now take a width and height as well as a size name, through `image_get_intermediate_size()`'s choice of the stored size: an exact match, the smallest larger size in the same ratio (in the original's when a side is the integer 0), or the thumbnail
- Intended difference: PHP 8 throws a `TypeError` for a URL image with no width that renders in view (it multiplies `''` by `''` for its priority), which ends the page; here the image renders without a high priority. Width and height values PHP would compare as strings are read through `(int)`
- Its oracle, `reference/media-image-widget/golden.sh`, inserts four attachments inside a transaction it rolls back and records 50 instances outside the loop and in a singular post's loop (100 lines)

## 2026-11-15: Post locks, profile arguments, attachment states, site icon

- Ported read only; an attachment in the Trash answers PHP's 409

## 2026-11-15: The flaky notices replay

- Two one-off failures were a one-bit corruption ('e' to 'd', at an 8-byte boundary) on the side computed only from compiled-in data, not a dependence on the clock, order or randomness; no code change; a memory check of the build host is pending
- Coverage runs keep their full output in a log file, so a one-off failure can be diagnosed

## 2026-11-15: User, option and meta writes (MVP 2.4)

- Users, options, transients and meta are ported; the write oracle masks password hashes and transient expiries; `Write::fail_after` lets tests reach every error path
- Null meta values are stored as NULL and an update's `user_pass` as given, as PHP does (ADR 0003); the meta writes take an optional value

## 2026-11-15: REST media, revisions and autosaves

- Read routes ported; attachments are a type of the posts controller, `prepend_attachment()` and the `wpautop()` restore after block content are in hyscribe-render, and what PHP reads from disk (file sizes, a scaled original) answers 501

## 2026-11-15: Missing active theme and comment pages

- hs-admin shows an active theme the site does not have as PHP does, minus `switch_theme()`'s write; comment links name their comment page

## 2026-11-15: Comment writes (MVP 2.4)

- The comment write functions are ported; an action keeps its first database error and skips the rest, so the transaction is dropped
- `EMPTY_TRASH_DAYS` follows the capability constants' rule by extension (an `.env` key the wizard copies, default 30), open to correction
- The terms and comments ports each have a first-error helper (`Act`, `Steps`) and an author name filter; the lean round after 2.4 merges them

## 2026-11-15: Term writes (MVP 2.4)

- The term write functions are ported; each keeps its first database error and fails at the end, so the caller drops the action and nothing it wrote is committed (PHP can leave partial writes behind)
- The write oracle masks term ids in slugs and in `{taxonomy}_children`

## 2026-11-15: Nonce and capability checks (MVP 2.5)

- Admin actions and REST writes check nonces and capabilities as PHP 7.1.3 does before answering 501; PHP's answers are recorded through a must-use plugin that stops each request right before its first write, with table checksums compared before and after

## 2026-11-15: Login actions and dashboard arguments

- The login screen's other actions and the dashboard's arguments are ported read only; reset key checks wait on a `wp_verify_fast_hash()` port
- With a missing active theme, PHP's Themes screen switches theme (a write); by decision HyScribe will show that page without the switch, so its notice repeats on every view

## 2026-11-15: The write layer (MVP 2.2)

- `hyscribe-db` gains a gated `Writer` (primary mode only; one transaction per action, or a savepoint inside a caller's transaction for the write oracle), fixed statement write methods for every core table, and the optional read only `DB_READ_USER`; `db.sh up` creates a `wordpress_read` user
- Writes are refused outside the supported `db_version` range (above the newest supported and below the 7.0 floor) and on any table not on InnoDB; ADRs 0010 and 0012 say so
- An update that changes nothing reports one matched row, where PHP's mysqli reports none; the core write functions (2.4) must not rely on that count

## 2026-11-15: The write oracle (MVP 2.3)

- PHP's rows are recorded through core functions in rolled back transactions and compared with HyScribe's by table, column and key, with dates, Unix times and new ids masked; the writer's transactions must nest as savepoints inside the oracle's
- `db.sh up` waits for the database over TCP, so it no longer races the image's initialising server

## 2026-11-14: The public users endpoint applied, `_embed` and REST search

- The public users endpoint serves visitors as PHP does; `REST_PUBLIC_USERS=off` answers them as a site where no user has published posts
- `_embed` and `/wp/v2/search` ported; term lists ordered by name over the Theme Test Data are recorded by id where two terms share a name, since MariaDB breaks that tie differently between runs

## 2026-11-14: Update notices and missing plugins and themes

- WordPress's update notice is never shown; hs-admin shows HyScribe's own release notice once releases publish one
- plugins and themes a migrated site names but HyScribe lacks are shown as PHP shows missing ones, without deactivating them, and listed in the setup wizard's compatibility report

## 2026-11-14: ADRs 0010 and 0012 accepted

- The gated writer was accepted and the admin's writes, with four amendments: write SQL goes through ADR 0014's `Dialect`; an optional read only database user (`DB_READ_USER`) serves reads and shadow mode; writes to MyISAM tables are refused, since they cannot be all or nothing; the wizard warns when the PHP side uses a persistent object cache that HyScribe's writes would leave stale. Phase 2 starts

## 2026-11-14: The edit lock

- Decision: a view of the edit screen writes `_edit_lock` as PHP does, in primary mode, once the write layer exists (phase 2); until then those views answer 501

## 2026-11-14: The capability constants

- Decision: `.env` keys `MULTISITE`, `DISALLOW_UNFILTERED_HTML` and `ALLOW_UNFILTERED_UPLOADS`, copied once by the setup wizard from the old site's `wp-config.php`; no PHP file is read at run time, and a missing key takes the safer value

## 2026-11-14: REST synced patterns, navigation and menus; REST lean round

- Synced patterns, navigation menus and classic menus are served read only; no menu location is registered, as Twenty Twenty-Five registers none; the goldens' state removes PHP's fallback navigation post so they do not depend on the day they are recorded
- A lean round pruned the duplicated helpers the six REST branches had brought in

## 2026-11-14: The public users REST endpoint

- Decision: replicate WordPress; the endpoint is on by default and answers visitors as PHP does, with a setting to turn it off (then answering as a site with no published authors); docs/SECURITY_MODEL.md updated; `?author=N` and the users sitemap stay off by default

## 2026-11-14: Portable SQL (ADR 0014)

- `hyscribe-db`'s SQL is standard SQL; the MySQL and MariaDB only pieces (random and list orders, date functions, casts, `REGEXP`, the read only pool and row types) sit in one `Dialect` whose MySQL implementation keeps the SQL it wrote before, and a guard test fails on MySQL-only syntax outside it

## 2026-11-14: The HTTP client

- Decision: `hyper` with `rustls` over the host's certificate store for outbound fetches; every fetch follows the security model's rules, and which features fetch stays with their own decisions; nothing fetches until the client is built, with its own ADR

## 2026-11-14: Extension licensing

- The extension SDK and WIT interfaces are Apache-2.0 OR MIT, so extensions may carry any license; the official directory lists only GPL-compatible or OSI-approved extensions (ADR 0004)

## 2026-11-14: REST posts, pages and statuses

- Ported read only; REST content PHP would write caches for (oEmbed, feeds), or that render does not port yet, answers 501; `golden.sh` gained a `default` flag, and a bug that made it skip every default install list was fixed

## 2026-11-14: REST comments and users

- Ported read only; the public users endpoint stays closed to visitors (501) and `/users/me` answers PHP's 401; the admin's login reads now answer `comment_post` and `user_exists` from the database

## 2026-11-14: Decisions on the extension runtime, the porting pilot set, PHP proxy mode and database engines

- WebAssembly components are the extension runtime; ADR 0002 accepted
- the porting pilot set is what ships with WordPress: Akismet, Hello Dolly, Twenty Twenty-Three, Twenty Twenty-Four and Twenty Twenty-Five
- no PHP proxy mode; HyScribe is a drop-in replacement with zero dependency on PHP WordPress
- MySQL and MariaDB only for now, but SQL stays portable behind a database layer so other engines remain possible (ADR 0014); the existing MySQL-only queries move to portable forms before phase 2

## 2026-11-14: REST templates

- Templates, template parts and the global styles post ported read only; REST goldens are now recorded under `DISALLOW_UNFILTERED_HTML`, HyScribe's value for that constant, as the admin goldens are; theme file order follows the oracle's directory order

## 2026-11-14: REST taxonomies, terms and settings

- Ported read only; write endpoints carry only their permission checks so `Allow` and `targetHints` match PHP; `_embed` and requests where PHP writes `category_children` answer 501; the public users route's answer to visitors waits on a decision

## 2026-11-14: The editors' REST registry routes

- Block types, pattern categories and the theme's global styles served over registry data exported from the oracle (`rest-registered.json`, the theme's `registered.json`); GET requests on which PHP writes are recorded as `writes` rows and answer 501

## 2026-11-14: REST server started

- The REST API's read side starts ahead of phase 2, since reads write nothing (ADR 0013 proposed: a pure `hyscribe-rest` crate over a site trait); the route table and post types are exported from the oracle, and the `preg_match()` engine moved from the rewrite rules into `hyscribe-phpser`; writes answer 501 until phase 2

## 2026-11-14: Attachment edit screen, phase 1 done

- The attachment edit screen is ported read only; PHP writes `_edit_lock` on every view nobody else holds, so HyScribe answers 501 there until a decision is made
- Phase 1 of docs/MVP.md is done, but for screens that wait on decisions, the write layer or the REST API

## 2026-11-14: Lean round over the phase 1 admin

- One route shape in `screen()` (`served()`), query arguments through `page::query_strings`, `page::Request` for the Tools, Appearance, Users, Terms, Comments and Dashboard screens, `PostsSite::can_for` for `can_user` and `can_term`, one `text` helper; 277 lines removed, no golden changed

## 2026-11-14: Site editor refusals

- `site-editor.php`'s first view writes the `wp_global_styles` post and `wp_theme` term; by decision HyScribe makes that write with the write layer (phase 2), so the shell waits for it and for the REST API's preloads; only its refusals are served now
- Admin searches show password protected posts, as `WP_Query` does for logged in users

## 2026-11-14: Notices after actions

- Ported read only (MVP 1.6); requests PHP fails part way through, and settings transients PHP would delete on view, answer 501 rather than write (ADR 0003)

## 2026-11-14: Media library and Add New Media

- Ported read only; the Theme Test Data admin goldens are stored as hashes (`reference/media/mask.php`), since that content is not ours to commit; the attachment edit screen is left for later

## 2026-11-14: Themes, Plugins and the Tools subscreens

- Ported read only; Plugins and Site Health wait on decisions about PHP and server details, and plugins and themes a migrated site names on a decision; one `Answer::from_page()` now gives every screen the menu's refusal

## 2026-11-14: Phase 1's duplicates pruned

- An independent review of the three merged branches found two settings frameworks, four copies of the nonce and referer fields and duplicated dropdowns; they are one each now, and the referer field is escaped with `esc_url`, as `wp_referer_field()` does (a golden over a request with `&` and a quote)

## 2026-11-14: The General, Writing and Reading settings

- Ported read only; the admin page frame gained an admin head, late styles and scripts after the footer, and the site records whether `WP_HOME` and `WP_SITEURL` are set; the upload limit and the Reading screen's page view writes were left as pending decisions
- The three phase 1 branches merged with conflicts only where they added code side by side; every admin POST answers 501 in one place

## 2026-11-14: Editing another user and adding a user

- Ported read only, extending the profile rather than copying it; `wp_die()`'s 500 refusals are a new answer, and every POST to an admin screen answers 501 until actions are ported

## 2026-11-14: The Discussion, Media, Permalinks and Privacy settings

- Ported read only, the first of MVP phase 1's parallel work packages; the Permalinks screen is shown as PHP shows it on Apache with a writable `.htaccess` until a decision is made

## 2026-11-13: The MVP roadmap

- [docs/MVP.md](docs/MVP.md) defines the MVP as replacing PHP WordPress for a default install day to day, and plans it in nine phases of work packages, with what can be built side by side and the decisions each phase needs

## 2026-11-13: The profile

- `profile.php` is ported, tested against PHP for every role and with user meta set by filters; its Sessions row counts HyScribe's own sessions, and `wp_generate_password()` is ported for the password it offers
- The profile is served over HTTP only until application passwords are ported

## 2026-11-13: The users list

- `users.php` is ported for its default view, tested against PHP over the default install and over added users; the screen options' column list leaves out the columns WordPress never lets a user hide
- The users list is filtered by role, searched, sorted and paged as `WP_User_Query` does, the columns it may sort and search kept in lists the database layer checks too
- User names and emails are shown through the admin's display filters, as WordPress runs them for defence in depth, so stored markup never reaches the page raw

## 2026-11-13: The comments list

- The Comments screen lists a post's comments; the admin bar takes a screen's View link, as `wp_admin_bar_edit_menu()` adds it
- The Comments screen is searched, filtered by type and sorted as `WP_Comment_Query` does, the columns it may sort by kept in one list the database layer checks too
- The Comments screen takes its status views, a user and a page; its comment queries go through one filter of states and user, which the dashboard's latest comments share
- `edit-comments.php` is ported for its default view, tested against PHP over the default install and over comments in every state; the dashboard now shares its reply form, comment classes and status links
- `antispambot()` draws its character references at random, so the comment goldens are compared with them decoded, and the page's generation time is masked

## 2026-11-13: The categories and tags list

- `edit-tags.php` is ported for categories and tags, tested against PHP as whole pages over the default install and over added terms
- Admin recordings pin the update check to the running version, as hs-admin shows no update offer until a decision is made; WordPress 7.1.4's release had put the offer in every admin page PHP served
- `wp_kses_data()` is ported over the tags WordPress exports for it (`wordpress/wp-includes/kses-data.json`)
- The terms list is searched and sorted as `WP_Term_Query` does, its pages read from the database rather than from every term

## 2026-11-13: The pages list

- The posts list takes a post type, posts or pages, with each type's labels, columns, capabilities, help and Quick Edit fields; pages are listed as `WP_Posts_List_Table` lists a hierarchical type
- A page that is its own parent is shown at the top level without the write PHP makes to repair it (ADR 0003)
- Page templates come from the theme's `theme.json` custom templates, as rendering reads only the theme's files

## 2026-11-13: The pages widget, and the M3.8 widget work package done

- The legacy widget block renders `WP_Widget_Pages` over a new `wp_list_pages()` port: `get_pages()` for the published pages, sorted by the instance's columns through the `WP_Query` orderby parser (menu order and title by default), less the pages it excludes, walked as `Walker_Page` does. Pages whose parent is not listed follow, flat; without a top level page, those sharing the first's parent are the top level. The current page, its parent and its ancestors are marked on a page, an attachment or the page for posts, and the page for posts is marked as the parent elsewhere
- Its oracle, `reference/pages-widget/golden.sh`, inserts a tree of pages (untitled, protected, private, draft, a draft's child, an attachment) inside a transaction it rolls back, and records it on several requests, then with a static front page and a page for posts (136 lines)
- Every widget of the M3.8 work package is ported

## 2026-11-13: The tag cloud widget

- The legacy widget block renders `WP_Widget_Tag_Cloud`: `wp_tag_cloud()` with its defaults for the instance's taxonomy when WordPress registers it, else for tags, titled with the taxonomy's label unless the instance has a title; nothing without terms
- The tag cloud block and the widget share the `wp_tag_cloud()` port, which now takes all eight registered taxonomies, not only categories, tags and formats. `get_term_link()` gives `?taxonomy=X&term=slug` for a taxonomy without a query variable (navigation menus, link categories, themes and the like), where it gave a tag link before
- Its oracle, `reference/tag-cloud-widget/golden.sh`, feeds sets of terms through `terms_pre_query` as the block's does (64 lines); the block's oracle gained a navigation menu cloud

## 2026-11-13: The recent comments widget

- The legacy widget block renders `WP_Widget_Recent_Comments`: the newest approved comments on published posts, five unless the instance's `number` is a positive count, each as the author's link and the post's title linked to the comment. The list prints even when empty, and only the first of a request is `#recentcomments`, as PHP's `static $first_instance` has it
- `get_comment_author_link()` is ported for it: the author, linked to their site with `rel="ugc"`, plus `external nofollow` unless the URL has a scheme and the home URL's host (a protocol relative URL counts as external)
- Its oracle, `reference/recent-comments-widget/golden.sh`, inserts comments (pending, on a draft, a pingback, by a user, anonymous, with internal, external, relative, unsafe and bare `http://` URLs) inside a transaction it rolls back (11 lines)

## 2026-11-13: The recent posts widget

- The legacy widget block renders `WP_Widget_Recent_Posts`: the newest published posts, sticky ones in date order, five unless the instance's `number` is a positive count (a count past what the database takes lists them all), with `the_title()` (so "Protected:" and "(no title)"), the date in the site's format when asked, and `aria-current` on the queried post; nothing at all when no post is published
- Its oracle, `reference/recent-posts-widget/golden.sh`, inserts posts (protected, private, draft, sticky, untitled) inside a transaction it rolls back, and records them with each request (34 lines); the latest posts block and the widget share how a list shows a post's title

## 2026-11-13: The calendar widget

- The legacy widget block renders `WP_Widget_Calendar`: `get_calendar()` for the request's month, through the calendar block's port, with `get_calendar()`'s own check that prints nothing when neither the main query nor the site has a post. Only the first calendar widget of a request is `#calendar_wrap`, as PHP's `static $instance` has it
- Its cases sit in `reference/calendar/golden.sh` over that oracle's dates, requests, permalink structures and week starts, one with every post a draft. A structure with `%year%` alone joins them, which leaves the calendar block's own month and year unused as PHP does (267 new lines in all)

## 2026-11-13: The categories widget

- The legacy widget block renders `WP_Widget_Categories`: the list through the categories block's `wp_list_categories()`, or `wp_dropdown_categories()` by term ID in a form with its own script. PHP's `static $first_dropdown` is per request in the renderer: the first dropdown is `cat`, later ones are numbered. On a category archive the dropdown selects that category
- Its cases sit in `reference/categories/golden.sh`, over that oracle's category tree and its eight requests (56 new lines); the classic widgets share one dropdown script builder

## 2026-11-13: The archives widget

- The legacy widget block renders `WP_Widget_Archives`: the monthly list, or the dropdown with its own inline script, through the `wp_get_archives()` the archives block already ports. The dropdown's ID carries the widget's number, `-1` for an instance in the block, as `the_widget()` numbers it. 14 new cases in `reference/legacy-widget/golden.sh`
- `reference/default-install/php.sh` starts on macOS's bash 3.2 again, which treated the empty uploads mount as unset

## 2026-11-13: Search relevance and request slashes

- Searches are ordered by all of `WP_Query::parse_search_order()`, with the phrase and every term ranked, on the front end as in the posts list; before, only the single term rule was ported
- The admin reads the request URI slashed, as `wp_magic_quotes()` leaves `$_SERVER['REQUEST_URI']`, so URLs WordPress builds from it match

## 2026-11-13: The posts list

- The posts list is ported from `WP_Posts_List_Table` and `WP_List_Table`, tested against PHP as whole pages for every role that may edit posts and over added content in every status the list shows
- The admin's post queries select statuses as `WP_Query` does in the admin: the All list's statuses, with private posts only the user's own unless they may read others'
- `/hs-admin/edit` serves the unfiltered list; its query arguments answer 501 until each is ported with its own golden

## 2026-11-13: The dashboard

- The dashboard is ported widget by widget, each tested against PHP, then as whole pages for every role and for states the default install lacks (cached Site Health results, a private site, a user's own widget layout, scheduled posts, drafts and comments in each status)
- Its Quick Draft writes no auto-draft on a page view and it makes no calls to wordpress.org, until a decision is made

## 2026-11-13: The admin page frame

- Every hs-admin screen is printed inside one frame ported from `admin-header.php` and `admin-footer.php`; a screen supplies its title, help tabs and markup, as a `wp-admin` file does before including the header
- What PHP adds per request (the user's ID and color scheme, nonces, stored editor preferences, the command palette's menu) is set over the exported script registry, which keeps what every request shares
- Admin URLs filled into scripts drop `.php` like every other hs-admin link
- The Tools screen is the first whole page; the update notice is left out until a decision is made

## 2026-11-12: The admin menu

- The admin menu is `wp-admin`'s, built for a single site with a block theme and no PHP plugins; branches only plugins or classic themes reach are left out
- Its oracle is PHP with `DISALLOW_FILE_MODS` defined, which denies exactly the capabilities HyScribe never grants, so the menu PHP prints there is the one HyScribe must print
- Links drop `.php`; the Dashboard's is `index`, served as the dashboard

## 2026-11-12: Logging out

- Logging out works as `wp-login.php`'s logout action: the `log-out` nonce is checked for the user of the logged in cookie, the session file entry is removed and HyScribe's cookies are expired; a missing or stale nonce gets the same 403 page asking first. HyScribe expires only its own cookies, not the older WordPress cookie names PHP also clears

## 2026-11-12: Logging in

- `/hs-login` logs people in, in primary mode only: sessions in files (ADR 0011), cookies under HyScribe's own names with `SameSite=Lax`, and the redirect `wp-login.php` makes, kept on the site's host by a port of `wp_validate_redirect()`
- Login actions not ported yet answer `501 Not Implemented` rather than a screen that does not do what it says
- Capability checks take `wp-config.php`'s capability constants at their safer values until a decision is made

## 2026-11-12: The login screen

- `wp-login.php`'s login action is ported as a function of the request, the site and its users, so the page, its notices and the credential checks are tested against PHP without a running server; the HTTP layer will map its outcome to sessions and cookies
- A golden of failed logins only reads the database; successful logins, which make PHP write session tokens, are tested on their outcome instead

## 2026-11-12: The remaining review findings, and fuzz targets for the unfuzzed parsers

- The Interactivity API reads a directive's path in place and copies only the value it ends on, merges contexts through a key index, and leaves the markup unprocessed once it has copied 64 MiB of context; kses filters an attribute array's keys through an index. Each was quadratic in the input or copied a context once per nested element
- New fuzz targets: `render` (a post's content with Twenty Twenty-Five, the site's options, meta and comments taken from the input), `rewrite_pattern`, `http_date` and `text_fields` (`sanitize_text_field()`, permalink tags, PHP numeric strings, `urldecode()`, natural order comparison). The HTTP parsers are reached through a hidden `hyscribe_http::fuzzing` module
- `rewrite_pattern` found a stack overflow within a minute: a rewrite rule built from a hostile permalink option (`^{111222211}`, or deep groups) made the matcher recurse once per repetition, which aborts the whole process. The engine now refuses what PCRE refuses to compile and gives up with no match past 1,000 levels or 1,000,000 steps, as `preg_match()` returns false past its JIT stack and `pcre.backtrack_limit`; the exact points where each gives up differ, and no rule WordPress generates comes near them. A run of literal bytes matches in a loop, so long literal structures still match as in PHP
## 2026-11-12: The admin's style and script printing

- The new `hyscribe-admin` crate prints the admin's styles and scripts as `WP_Styles` and `WP_Scripts` do, from a registry exported from an admin request (`is_admin()` registers `postbox` and the other admin scripts a front end request lacks)
- Only what core's registry uses is ported: no core script with a loading strategy has dependents or inline scripts, no style is alternate or titled, every style prints with media `all`. A test pins these facts, so a new WordPress version that breaks one fails the build instead of printing wrongly

## 2026-11-12: Roles and capabilities

- The goal was set: the default WordPress install replicated in full, with its admin
- Capabilities are checked as `WP_User::has_cap()` does, over `map_meta_cap()` and the post type, taxonomy and status data exported from WordPress 7.1.3. The capabilities to change code or files are never granted (ADMIN.md)
- `DISALLOW_UNFILTERED_HTML` and `ALLOW_UNFILTERED_UPLOADS` are read from the site's constants, never assumed; a site with `MULTISITE` gets no capabilities until multisite's super admin rules are ported, so nothing fails open
- A loop of attachment parents, which PHP recurses into until the request dies, gives no status instead of taking the process down

## 2026-11-12: Request lines limited as Apache limits them

- Choice: rather than capping `cat`, `tag` and slug lists, which would serve something PHP WordPress does not, a request line longer than Apache's default `LimitRequestLine` (8,191 bytes with the method and protocol) gets Apache's 414. PHP WordPress behind Apache is never asked for more, and at that length no list comes near the 65,535 values one database query can bind
- The 414 page is Apache's without its server signature, which names Apache and the host (chosen so that HyScribe does not name itself as WordPress)
- The limit is fixed for now; making it configurable, for sites behind servers with other limits, comes later

## 2026-11-12: hs-admin brought forward

- hs-admin is started now; the remaining legacy widget slices wait. Its screens need authentication first (M4), starting with roles and capabilities
- Decision: HyScribe keeps its own login sessions and cookies, separate from PHP WordPress, so a user logs in to each separately
- Logging in needs primary mode; shadow mode stays strictly read only

## 2026-11-12: Denial of service fixes from the project review

- Rendering counts the blocks it is inside across every parse (synced patterns, post content, template parts) and renders nothing past 256, twice the parser's depth, which fits a 2 MiB worker stack. Intended difference: PHP WordPress has no such limit and runs out of memory or stack instead; a chain of synced patterns could abort the HyScribe process
- The comment template renders threads with a stack of open levels instead of recursing once per reply, finds replies through a map built once, and gives the reply link its depth from that walk, so a long reply chain takes linear time and no stack
- A `cat` list keeps each ID once before children are added, as `WP_Tax_Query::clean_query()`'s `array_unique()` does, and `tag` slugs are kept once through a set: both were quadratic on the async worker
- The server reads requests with hyper's HTTP/1 builder directly: a 30 second header read timeout (hyper's default, which `axum::serve` dropped by giving it no timer), at most 1,024 connections at once, and the listener closed as soon as shutdown starts, as `axum::serve` closed it
- Not done here: capping the length of `cat`, `tag` and slug lists, which past 65,535 bound values make the database refuse the query (a 500); it changes what PHP would serve, so it waits on a decision (since settled by the request line limit above)

## 2026-11-12: The legacy widget block

- The legacy widget block is ported now, in slices, rather than defer the classic widgets to M7.2 (classic themes), which will reuse them
- `wp_hash()` signs with `AUTH_KEY` and `AUTH_SALT` from the configuration, else the `auth_key` and `auth_salt` options. Where WordPress would generate and store a salt, HyScribe writes nothing and the signature fails, so a signed instance does not render
- A block theme registers no sidebars, so a stored widget renders only from the inactive widgets, without sidebar markup, as in WordPress
- Not ported: widget settings from before WordPress 2.8, which PHP converts and stores again when it reads them, and the `SECRET_KEY` and `SECRET_SALT` constants, which the configuration does not carry (a site setting them gets no signed instances)

## 2026-11-12: Block bindings

- Bound attributes are merged after `prepare_attributes_for_render()`, as PHP merges them, so they skip schema validation
- `replace_html()` seeks back to the first tag between selectors; HyScribe parses afresh instead, stopping where PHP's seek would keep a bail
- The post meta source returns only meta core registers for the REST API: `footnotes` on public posts and pages
- The post date block reads its bound `datetime` from the shared bindings pass, as WordPress 7.1 does, so any source can set it

## 2026-11-12: The HTML processor

- `WP_HTML_Processor` is ported for `<body>` fragments only, the one context WordPress's blocks parse in; the full document parser waits until something needs it
- Steps that cannot run in such a fragment are left out rather than ported untested: the head, frameset and after-body modes, clearing the stack back to a table context, and the adoption agency's early return for an unlisted formatting element (WordPress has no Noah's Ark clause)
- The token oracle walks 2,485 fragments through WordPress 7.1.3's processor, including inputs that exhaust the 10,000 bookmarks

## 2026-11-10: Decisions on time zones, fixtures, login links, ADRs, theme headers, CI and the RSS block

- Decision: named time zones use the `jiff-tzdb` crate, audited for `cargo vet`; goldens cover only zones and dates where its IANA release agrees with the oracle PHP's
- More fixtures: the WordPress Theme Test Data (formerly Theme Unit Test) is the next content fixture, for media and the blocks that show it. Its repository declares no licence, so it and its media are downloaded pinned and checksummed for tests and never committed
- The login/logout block links to `wp-login.php` as PHP does, and HyScribe redirects that path to `/hs-login`
- ADR 0001 accepted without minijinja: no template engine until the classic theme ADR (M7.2). ADR 0003 accepted
- The 128 `cargo vet` exemptions are accepted as the baseline; the list only shrinks from here
- Theme files get `Last-Modified` from each file's date in the pinned WordPress release and an `ETag` from its SHA-256
- CI: local gates are the check for now; the nightly GitHub run stays configured but is not relied on
- The RSS block may fetch feeds, only from an allowlist of hosts, with timeouts, size limits and non-public addresses refused (docs/SECURITY_MODEL.md)

## 2026-11-07: Decisions on embeds, core assets, password hashing and the next work

- Decision: other sites may frame embed pages, which go without `frame-ancestors`; every other response keeps `frame-ancestors 'self'`
- `wp-includes`: the pinned WordPress release is downloaded at build time and verified by its SHA-256. Whether unminified files are bundled is still open
- M2.8: the password hashing crates get `cargo vet` audits, not exemptions. To keep the audit small, bcrypt is ported from PHP's `crypt_blowfish` onto the `blowfish` crate, HMAC is written on `sha2` and phpass uses the existing MD5, which leaves three audits (`blowfish`, `cipher`, `inout`, about 4,800 lines) instead of about 60,000 lines. Refreshed imports also replace the `futures-io` and `futures-sink` exemptions
- Next: pretty permalinks, then named time zones

## 2026-11-06: Embed pages

- Embed pages are served as WordPress serves them; whether other sites may frame them is a new pending decision

## 2026-11-06: Theme files committed (in part)

- Every file of Twenty Twenty-Five but its PHP is committed, built into the binary and served at the theme's URL . Its fonts (SIL OFL 1.1) and images (CC0) are redistributed with it, as WordPress distributes them. Where `wp-includes` comes from is still open

## 2026-11-06: REST and oEmbed discovery 

- The page head prints the REST API and oEmbed discovery tags, and responses carry the REST and shortlink `Link` headers, as PHP sends them ; the RSD tag stays out while XML-RPC is off. The endpoints arrive with M5

## 2026-11-06: Footer credit

- The theme footer reads "Designed with HyScribe", linking to `https://github.com/hyscaler/HyScribe` 

## 2026-11-06: Trackbacks answered as closed

- A trackback POST gets WordPress's "Sorry, trackbacks are closed for this item." reply instead of 405, and `wp-trackback.php` is served 

## 2026-11-04: CI cost cut

- CI ran too often for its monthly budget. Pushes to `development` no longer start CI; the nightly run, skipped when `development` has not moved, is its gate, and every job has a timeout. A nightly run with changes costs about 34 minutes, most of it the 60 second fuzz run of each of the 21 targets

## 2026-11-04: HyScribe branding and asset URLs

- Output that names WordPress as the software names HyScribe instead, including the theme footer credit; database content is never modified, so the default post and commenter keep their WordPress text
- Public asset URLs match PHP WordPress exactly; the source of the files is still open

## 2026-10-30: Tag and post format archives

- The taxonomy query vars (`category_name`, `tag`, `post_format`, `taxonomy` and `term`) are routed as `WP_Query` reads them, with tag and post format archives
- The fixture database gains a sibling, `wordpress_terms`: the default install plus `reference/default-install/terms.sql`, which the head golden script also applies to the PHP oracle while it records term pages, so both sides see the same term IDs

## 2026-10-29: Posts in pages

- A post split by `<!--nextpage-->` shows its pages by the `page` var, with page links, as WordPress does

## 2026-10-29: Pages of a query of its own

- Query blocks that do not inherit the main query page by `query-<id>-page`, as WordPress pages them

## 2026-10-29: Query total block

- Ported; requests carry the main query's `found_posts`

## 2026-10-29: Read more and comment count blocks

- Ported the read more, post comments count and post comments link blocks

## 2026-10-29: Post excerpt and term description blocks

- Ported, with terms now carrying their description

## 2026-10-29: Paging and whole-page comparison

- List pages are paged as `WP_Query` pages them, and the query pagination blocks of inheriting queries are ported
- The HTTP tests now compare whole pages, status and HTML, with PHP's (41 default install pages, and 6 with one post a page); they replace the head and footer comparisons

## 2026-10-29: Elements block support

- Element colors on a block (`style.elements`) give it a `wp-elements-<n>` class and scoped rules, as `wp_render_elements_support_styles()` does; the oracles that switched it off now run with it

## 2026-10-29: Search and archive comment feeds; RSS 0.92

- A search's or archive's comment feed lists the comments on the posts it matches, and the posts of those comments stand in for the query's, as `WP_Query` does
- `?feed=comments-rss` is RSS 0.92 in WordPress, not a redirect; ported, its generator comment naming HyScribe like the other feeds

## 2026-10-29: RDF feed; RSS 0.92 redirects

- `?feed=rdf` ported, its generator agent naming HyScribe; `?feed=rss` redirects to `feed=rss2` as WordPress 7.1 does

## 2026-10-29: Fewer CI runs per push

- Request: fewer CI runs per push. A push to `development` now runs two jobs (checks and coverage) instead of seven; fuzzing and the release archive moved to pull requests, pushes to `main`, a nightly run against `development` and manual runs. Newer pushes cancel runs in progress, and documentation-only pushes start none
- The fuzz job had failed since 2026-10-28 on a wrong round-trip assertion in the `phpser_json_decode` target, now fixed

## 2026-10-29: Atom comment feeds

- The site's and each post's Atom comment feeds ported, with comment threading; replies in every comment feed now start with the "In reply to" link PHP gives them

## 2026-10-29: Atom posts feed

- `?feed=atom` ported; its `<generator>` names HyScribe as the RSS2 feed's does (`<generator uri="https://github.com/hyscaler/HyScribe" version="0.1.0">HyScribe</generator>`)

## 2026-10-29: Comment feeds

- The site's and each post's RSS2 comment feeds ported; with them every feed the default install's pages link to matches PHP apart from the generator

## 2026-10-29: The posts feed

- RSS2 posts feeds ported, byte for byte PHP's with the same caching headers and 304 replies
- Following the generator decision for the head, the feed's `<generator>` names HyScribe as well: `https://github.com/hyscaler/HyScribe?v=<version>`, in WordPress's form
- Opened: where the bundled `wp-includes` and theme assets come from, which decides how static files are served

## 2026-10-29: Comment pages

- Comment paging and the comments pagination blocks ported; with them the single post and its comment pages are byte for byte PHP's apart from the tags held back

## 2026-10-29: wp_footer(); whole pages match PHP

- `wp_footer()` ported, so 24 default install URLs are now byte for byte what PHP WordPress serves, apart from the tags held back. The emoji detection script joins the exported WordPress files

## 2026-10-29: Block style variations; every head matches

- Block style variations ported, so the single post's `<head>` matches PHP too: every default install page now matches apart from the tags held back
- Variation instances use `wp_unique_id()`, the counter other blocks share, and the instance is counted even when its stylesheet is empty, as in PHP

## 2026-10-29: Page head ported; the generator names HyScribe

- `wp_head()` ported for block themes: every default install page's `<head>` matches PHP apart from the single post (block style variations come next) and the tags held back
- Decision (part): the generator tag says `HyScribe <version>` instead of `WordPress 7.1.3`. The REST, RSD and oEmbed tags stay out of the head until a decision is made
- PHP's script module sorting recurses forever on a dependency cycle; HyScribe treats a cycle as a missing dependency

## 2026-10-28: Avatars; render uses sha2

- Avatars need `hash( 'sha256', $email )`. `hyscribe-render` now depends on `sha2` 0.11, which `sqlx-mysql` already brings into the binary under an existing `supply-chain` entry, so no crate was added to the tree
- Users are read with their login, email address and first and last names; the email address is only hashed for the Gravatar URL and never printed

## 2026-10-28: wp_kses_post() ported; block parser limits

- `wp_kses_post()` ported with both default `pre_kses` filters, which parse and serialize block markup, so `serialize_blocks()` came with it. 3,236 inputs (theme markup, the fixture's posts, hostile markup, random fragments) match PHP byte for byte
- WordPress filters comments and invalid declarations again until they stop changing; the port bounds that by depth and a work budget and drops a token past either. Limits of `$allowed_html` that no core list uses (`maxlen` and similar) are not ported and reject the attribute
- Stress testing found two exhaustion paths in the block parser that PHP shares. Every unclosed block copies the rest of the document (quadratic: PHP dies at its 128 MB memory limit at about 2,000 openers), and deep nesting overflowed a 2 MiB worker stack at 10,000 levels, which would abort the whole server. Decision under ADR 0006: the parser returns no blocks past 128 levels of nesting or 64 MiB of copied text. PHP still renders nesting that deep, so this is a deliberate difference on hostile input only (listed in COMPATIBILITY.md); real posts nest about ten deep

## 2026-10-28: Front end writes as PHP WordPress makes them; ADR 0009 accepted

- Decision: HyScribe emulates PHP WordPress exactly, including the writes it makes on page views: the navigation fallback's `wp_navigation` post, transients, theme mods and `category_children`. Each write is ported with the code that makes it and checked against PHP on the fixture database
- ADR 0009 accepted: rendering reads site data through render's `Site` trait, implemented over `hyscribe-db`
- Since the front end now writes, the `Site` trait will gain write methods where those writes are ported, and the database connection needs write access for them

## 2026-10-28: First dynamic blocks; rendering reads through a Site trait

- ADR 0009 (Proposed): dynamic blocks read the database while rendering, so `hyscribe-render` defines a synchronous `Site` trait and stays database free; `hyscribe-http` will implement it over `hyscribe-db` from a blocking task. Posts move to a dependency free `hyscribe-model` crate both sides share
- Block supports, the site title and tagline, and the post title and date blocks match PHP WordPress on their goldens
- Date formatting ports every PHP format character; http's partial date formatter and private permalink builder now use render's
- Known gaps recorded in PROGRESS.md: named time zones, pretty permalinks, the full `strtotime()` grammar

## 2026-10-28: kses and theme.json merged; lean review, second round

- Merged `hyscribe-kses` (WordPress escaping, entity handling, protocol and CSS filtering) and `hyscribe-theme-json` (global settings, styles and stylesheet), each with PHP goldens and no surviving mutants
- The review found ports duplicated across crates as they were written in parallel. Pruned: one PHP array key lookup (`Value::get_str`, `get_path`, `Key::from_php_string`), one `_wp_to_kebab_case` (theme-json), one `unhex` test helper (testkit), one run of the style engine golden, PHP's casts in one module (`hyscribe-phpser::cast`, with its own plain PHP golden), and the unused `blocks.json` export
- Two behaviour fixes fell out of it: `(string)` of a float in render now uses PHP's precision of 14, and the registry's numeric string check accepts vertical tab as whitespace, as PHP does

## 2026-10-28: Clean admin and login URLs; wp-admin and wp-login.php redirect

- Decision: HyScribe's URLs have no `.php`. The login screen is `/hs-login` and admin screens are `/hs-admin/<screen>` (`/hs-admin/edit?post_type=page`). The WordPress forms are kept for backwards compatibility only: `/wp-admin/<screen>.php` and `/wp-login.php` redirect to the clean URLs with the query kept, and HyScribe URLs written with `.php` get a permanent 308. Old-style front end URLs (`/?p=1`) stay as WordPress serves them when a site has clean URLs (pretty permalinks) turned off
- The redirects are 307 (temporary): browsers do not cache them, so switching a site back to PHP WordPress restores its own admin at once, and a POST to `wp-login.php` stays a POST
- Decision, confirmed against a `/hs-content/` alternative: `/wp-content/...` paths stay exactly as they are, with no redirect, so stored content, indexed image URLs and switching back keep working. `/wp-content/` and `/wp-includes/` are never redirected or rewritten; this also fixes the trailing-slash canonical redirect, which would have touched them

## 2026-10-27: Theme rendering groundwork, ported in parallel

- Matching Twenty Twenty-Five byte for byte needs about 25 dynamic blocks, block supports, the HTML tag processor, the theme.json engine and global stylesheet, the formatting functions and kses. The separable ports run in parallel, each on its own branch and crate with PHP goldens, and are reviewed before merging
- Merged: `wptexturize` and `convert_smilies` (1,414 PHP cases). In WordPress 7.1.3 smilies become emoji characters; only `:mrgreen:` is an image, so its base URL is a parameter
- Theme patterns are PHP; the theme port stores them run once into block markup, so HyScribe never runs PHP

## 2026-10-27: Lean code review, first round

- Direction: keep the code clean, with an independent review for unused and duplicated code after every round (CLAUDE.md working rule)
- First review found no dead code or unused dependencies; pruned the duplicates it found: one fixture database config in `hyscribe-testkit`, one `Prefix` type in `hyscribe-config` (the prefix rule is now enforced by the type at load time), test-only `Db::option` and `Db::prefix` removed, `encode_into` private, a duplicate `home` field removed, stale `.gitignore` lines removed
- Its other gap: request query parsing had no fuzz target. The parsers moved to `hyscribe-http::query` and gained the `http_query` target
- Not pruned: `cargo-vet` exemptions at older versions are used together with imported delta audits; kept, as `cargo vet prune` confirms

## 2026-10-27: Archives on the default install

- Search, category and date archives match PHP WordPress on status and title for every probed URL: 28 of 29 default install URLs now match or differ as intended
- List queries are built from fixed SQL fragments with bound values (`hyscribe-db::PostQuery`), using WordPress's own ordering without extra tiebreaks, so tied posts come back in the order MariaDB gives both

## 2026-10-27: Differential harness and canonical redirects

- PHP WordPress runs in Docker against the same fixture database (`reference/default-install/php.sh`), and `reference/diff/compare.sh` reports per URL whether status codes and titles match
- First report on the default install: 15 of 19 URLs match or differ as intended
- Intended difference: `?author=N` gives 404, because author archives give away login names (docs/SECURITY_MODEL.md)
- Plain permalink canonical redirects match PHP WordPress, including its quirk that any path gains a trailing slash and then shows the home page

## 2026-10-27: First drop-in run against the default install

- `hyscribe start` serves the default install fixture from MariaDB, read only: home page, `?p=1`, `?page_id=2`, 404 for drafts and unknown URLs. The static musl release archive does the same in the CI smoke test
- New crate `hyscribe-http` (axum), with security headers on every response and no error details shown to visitors
- `WP_HOME` in `.env` overrides the stored `home` option, as the WordPress constant does, so a copy of a site can be tried on another address
- An unreachable database is reported after 5 seconds instead of sqlx's default 30
- `cargo-vet` exemptions now 118, for the tokio and axum crates of the ADR 0001 stack, pending review

## 2026-10-27: Drop-in against the default install comes first

- Direction: the first goal is to point HyScribe at the default WordPress install's database and see it run. Work on further parsers pauses for this slice
- The slice uses the stack ADR 0001 proposes (tokio, axum, sqlx); ADR 0001 stays proposed until reviewed
- `hyscribe-db` reads options and posts read only. sqlx 0.9 refuses unaudited dynamic SQL, so the one place that inserts the validated table prefix is the only `AssertSqlSafe` in the code
- `cargo-vet` exemptions grew from 39 to 114 crates for the sqlx tree, recorded here and in the security model as pending review

## 2026-10-27: Block parser and json_decode

- `hyscribe-render::blocks::parse_blocks` ports `WP_Block_Parser`, including what it does with malformed input (unclosed blocks spill into the output, a stray closer turns the rest into HTML, PHP's `empty()` drops `"0"` between blocks)
- The delimiter regex is matched by hand. The `attrs` capture includes the whitespace after `}`, and PCRE's `\s` includes `\v` and `\f`, which JSON rejects, so such attributes are `null` in PHP and in HyScribe
- A failed attribute scan is remembered, so many unterminated delimiters cannot make parsing quadratic (PCRE would hit its backtrack limit instead)
- `hyscribe-phpser::json_decode` reproduces `json_decode($json, true)`, including integer-like keys, repeated keys, floats for large integers and the 511 level nesting limit
- Password hashing (M2.8) is paused: its crates have no imported `cargo-vet` audits, a supply chain decision

## 2026-10-27: wp-config.php reader

- `hyscribe-config::wpconfig` reads `define()` calls and `$table_prefix` from an existing site's `wp-config.php` without running PHP. Only single literals are read; computed values (`getenv`, concatenation, constants) are reported with their source text so the wizard can ask for them
- A `define()` inside a block or after `||` is marked conditional, since PHP may skip it
- `DB_HOST` is split exactly as `wpdb::parse_db_host` does, including its PHP `empty()` quirks (`db:0` has no port)
- PHP is the oracle: `reference/wpconfig/golden.sh` runs every corpus file in PHP and calls the real `wpdb` method. The corpus excludes GPLv2-only files (the official Docker image's config), per ADR 0004

## 2026-10-27: db_version floor

- The supported `db_version` floor is 61833, read from a WordPress 7.0.0 install; 7.0.0 joins the default install fixtures. 7.0.7 and 7.1.3 report the same value, so a `db_version` alone cannot tell 7.0 and 7.1 apart

## 2026-10-27: Service commands and release pipeline

- `hyscribe-service`: the running server holds an exclusive lock on `run/hyscribe.pid` for its whole life. Holding the lock is what "running" means, so a stale pid file never blocks a start and a recycled pid is never signalled. No `unsafe`: signals and the user id go through `rustix`, signal handling through `signal-hook`
- `start`, `service start` and `service restart` refuse to run as root; `hyscribe` with no command means `start`
- Release archives are built by `scripts/package.sh` and are byte for byte reproducible; signatures are GitHub build provenance attestations (Sigstore), verified with `gh attestation verify`
- Boot autostart stays the crontab approach until a decision is made

## 2026-10-27: Implementation plan and landscape review

- Password hashing moves forward to M2, because the new-site installer creates the admin user
- [docs/LANDSCAPE.md](docs/LANDSCAPE.md): no other project serves an existing WordPress database without PHP. Automattic's wordpress-rs is a REST client and becomes a conformance suite for M5; Cloudflare's EmDash is the closest competitor and its plugin capability model is input to the extension runtime decision

## 2026-10-27: M1 started, hs-admin emulates wp-admin

- [ADR 0008](docs/adr/0008-hs-admin-emulates-wp-admin.md): the dashboard is hs-admin and emulates `wp-admin` as closely as possible (screens, file names, markup, CSS classes) under `/hs-admin/`, with `/wp-admin/` redirecting there. Settles the admin question; new goal G7 and success criterion S8. See [docs/ADMIN.md](docs/ADMIN.md)
- The first test target for every layer is a default WordPress install. Fixtures for WordPress 7.0.7 and 7.1.3 built by `reference/default-install/build.sh`; both report `db_version` 61833
- M1 started: Cargo workspace, CI gates, `hyscribe-phpser`, `hyscribe-config`, `hyscribe-testkit` and the `hyscribe` binary (`version`, `config check`), all at 100% line and region coverage
- PHP float formatting matches PHP 8.4 for 4,000+ values, including round-half-even ties where Rust's own formatting differs
- Nesting depth for serialized data capped at 128 to bound recursion
- Default `BIND_HOST` is `127.0.0.1` and default `MODE` is `shadow`, so a fresh configuration exposes nothing and writes nothing

## 2026-10-26: Full test coverage

- [ADR 0007](docs/adr/0007-full-test-coverage.md): the entire application is held to full test coverage, 100% line and region coverage (branch coverage once the stable toolchain supports it), plus mutation testing so the tests are proven to catch defects
- Coverage can never decrease; exclusions are attributed, justified and reviewed
- Gate active from the first line of code in M1 and part of every milestone exit; also required for every port and listed as a release blocker

## 2026-10-26: Security contact

- Vulnerability reports go to GitHub Security Advisories or security@hyscribe.com

## 2026-10-26: Security is the first priority

- [ADR 0006](docs/adr/0006-security-first.md): security ranks above data compatibility, behaviour compatibility, performance and convenience
- Added `SECURITY.md` (reporting policy) and [docs/SECURITY_MODEL.md](docs/SECURITY_MODEL.md) (threat model, controls, secure development lifecycle, release blockers)
- New goal G0 and success criterion S0; security baseline moved into M1 and a security gate added to every milestone exit
- Attack-prone WordPress behaviours (XML-RPC, pingbacks, user enumeration) off by default with opt-in
- Ports must never reproduce the vulnerabilities of the plugins they replace

## 2026-10-26: Vision and rationale published

- [docs/VISION.md](docs/VISION.md) records why HyScribe exists: PHP runtime vulnerabilities (PHP2Shell, CVE-2026-109618 and CVE-2026-109619), plugin ecosystem vulnerability growth, agent-driven discovery and exploitation, and PHP performance and operations costs, each mapped to a HyScribe design answer
- Every factual claim was checked against primary sources on this date and is cited in the document
- ADR 0004 reduced to the accepted license decision; extension and contribution terms will be published when decided

## 2026-10-26: Self-contained installation folder

- Everything an installation uses lives in the `hyscribe/` folder in plainly named subfolders: `plugins/<slug>`, `themes/<slug>`, `languages/`, `uploads/` (new sites), `packages/`, `reports/`, `cache/`, `logs/`, `run/`
- Plugin and theme folders are named after the WordPress slug they replace, so `active_plugins`, `template` and `stylesheet` keep resolving the same way
- WordPress URLs (`/wp-content/plugins/...`, `/wp-content/themes/...`, `/wp-content/uploads/...`) are preserved and mapped onto those folders
- Exception: an existing site's media library stays in its original `wp-content/uploads`, to keep the switch back to PHP WordPress safe
- Source repository uses the same names: first-party ports under `plugins/` and `themes/`

## 2026-10-26: Single binary distribution

- Primary deployment is a `hyscribe-latest.tar.gz` archive with one static binary that runs itself as a service, the WordPress `latest.tar.gz` experience without PHP or a web server. See [ADR 0005](docs/adr/0005-single-binary-distribution.md) and [docs/DEPLOYMENT.md](docs/DEPLOYMENT.md)
- Docker kept as an optional distribution only
- Configuration moves from the draft `hyscribe.toml` to `.env` next to the binary
- `wp-includes` assets will be bundled in the binary 
- Wizard gains a new site path and a one-time access secret for the browser flow
- New pending decisions: boot autostart, self-update, later platforms

## 2026-10-26: Toolchain baseline

- Rust stable 1.99+ with clippy and rustfmt
- Targets `wasm32-wasip2` (extensions, ADR 0002) and `x86_64-unknown-linux-musl` (static release binaries, ADR 0001)

## 2026-10-26: License and WordPress version floor

- License decided: AGPL-3.0-or-later, official text added as `LICENSE`. Confirmed compatible with WordPress, which is GPL-2.0-or-later, and with WordPress databases (data and schema carry no license obligation). See [ADR 0004](docs/adr/0004-licensing.md)
- Consequence: plugins and themes licensed GPLv2-only cannot be ported without relicensing; the scanner reports them as "license blocked"
- Minimum supported WordPress version set to 7.0 for reading and writing
- New pending decisions: extension license boundary, contribution terms

## 2026-10-26: Project inception

- Repository created; work happens on the `development` branch, `main` stays empty until the first reviewed merge
- Goal set: a Rust drop-in replacement for WordPress with 100% backwards compatibility for databases and `wp-content` files
- Core requirements captured in [GOALS.md](GOALS.md):
  - single binary with a first-run setup wizard for database connection details
  - automatic detection of what works and which plugins or themes are missing
  - plugins and themes as Rust packages
  - WordPress plugins and themes ported to Rust by AI agents
- Planning documents drafted: goals, roadmap, architecture, compatibility contract, wizard, extension model, porting pipeline, testing, risks
- Proposed (not yet accepted) architecture decisions recorded in [docs/adr/](docs/adr/):
  - ADR 0001: Rust stack (tokio, axum, sqlx)
  - ADR 0002: WebAssembly components as the extension runtime
  - ADR 0003: Database compatibility contract (no core schema changes)

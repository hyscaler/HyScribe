# MVP acceptance run (9.1)

## The MVP is met, 2026-11-20, at commit 54a73e35 (development)

The fourth run's one gap, S8's site editor screen, was a capture race and is closed (see its verdict below); nothing else changed in the product. CI ran every gate on this commit and all six jobs passed (`checks`, `coverage` at 100%, `package` with the smoke test, `switch-back` over the default install and the Theme Test Data, `screenshots` at 55 of 58 with the 3 listed exceptions, and `fuzz`). With the fourth run's numbers for S1 (101 of 101 and 747 of 747), S2, S3, S4 (3.65 s), S7 (0 check failures on both fixtures), coverage, `cargo deny` and `cargo vet`, every criterion of GOALS.md passes, and passed in CI on this commit. MVP 9.1 is met.

Still open after the MVP, none of them a criterion: the release workflow's publishing steps (a draft release and the provenance attestation, proven only by the first `v*` tag), and the next phases of docs/MVP.md (themes, phase 10, and plugins, phase 11).

## Fourth run, 2026-11-20, at commit 513bf160 (development)

The criteria S1, S2, S3, S4, S7 and S8 of [GOALS.md](../GOALS.md) as later tightened, with Site Health's two screens and the media grid's address as met when listed in `reference/screenshots/expected.list`. Since the third run: the preview nonce check on every entry point, the block patterns route and theme URLs in pattern content, the navigation fallback REST route (it has no screenshot seed), the `TRUSTED_PROXIES` mapped address fix, and the CI jobs `switch-back` and `screenshots` after `coverage`. Nothing was fixed during the run.

### Verdict

The MVP is not met on this run, by one screen. S1, S2, S3, S4 and S7 pass with the numbers below, S1 now has zero differences on the default install as well as on the Theme Test Data, coverage, `cargo deny` and `cargo vet` pass, and S8's three listed differences count as met by decision. What is left:

1. S8, the site editor screen (`administrator-wp-admin-site-editor-php`): it differs by 12,722 pixels in the first screenshot run and by 12,692 in a rerun, 1280x800 against 1280x800, and it is not in `expected.list`. The report names the request that differs between PHP and HyScribe: `GET /index?rest_route=/wp-block-editor/v1/navigation-fallback&_embed=true&_locale=user` (the rerun lists `wp/v2/menus` as well). The diff picture shows the editor canvas drawn twice in one of the two renderings (the Blog heading, the empty query text and the pagination each appear twice). The third run's site editor matched, and the CI run on the branch (below) reported 55 of 58 with 0 differing. The cause is not established: it may be the new navigation fallback route's answer under the screenshot seed (which has no navigation fallback seed), or a timing difference between the two environments. It is a gap until a run in CI agrees with this one or the screen is listed or fixed. Listing it would be a decision (exactly three screens are listed).
   - Resolved after this run (2026-11-20): a capture race, not a product difference. Both sides' request sets varied between runs and the pictures showed the editor canvas still drawing, with spinners at different stages; the `wp_posts` dumps were identical, and HyScribe's navigation fallback answers match PHP's for `_embed=true&_locale=user` (two lines added to `navigation-fallback-existing.list`). `capture.py` now waits on the site editor until no spinner shows for three polls; with it the screen matched in three runs, locally and in CI (55 of 58, the 3 others listed)
2. CI: the `development` run of the push this run started from was cancelled by the next push, with `checks`, `coverage`, `package` and `switch-back` completed green and `screenshots` and `fuzz` cancelled; the run of the newer push was still running when this was written. So CI's verdict on this exact commit for `screenshots` is not recorded here. A green run on the branch `ci/slow-suites` has every job green, among them `switch-back` and `screenshots` (55 of 58 match, 3 differ as expected, 0 differ).

| Criterion | Result | Numbers |
| --- | --- | --- |
| S1 | Pass | Default install 101 of 101 (`pages-default-known.tsv` empty), Theme Test Data 747 of 747 (`pages-ttd-known.tsv` empty), edited site 12 of 12, commenter pages 95 of 95, pretty permalink pages green; all five page tests passed in the workspace run |
| S2 | Pass | Password goldens, the login golden and the capability cases green in the workspace run; the switch-back suite logs into PHP as `admin` and `switchback` |
| S3 | Pass (with the accepted differences of ADR 0015) | Upload and write oracle goldens green in the workspace run; the switch-back suite uploads in HyScribe and PHP serves the files, 0 failures on both fixtures |
| S4 | Pass, new site and existing site | Empty folder and empty database to a serving site in 3.65 s of wall time with the release binary (setup 1.34 s); `/` 200 with 69,636 bytes in 0.087 s; the timed test `the_new_site_wizard_reaches_a_home_page_within_s4_s_time` passes in the workspace run |
| S7 | Pass on both fixtures | 154 and 39 actions, 72 and 31 pages, 0 check failures, `debug.log` 0 bytes on both, 46 and 30 REST rows |
| S8 | Gap: 54 of 58 screens match, 3 differ as listed, 1 differs unlisted (site editor) | First run `54 of 58 screens match, 3 differ as expected, 1 differ, 0 expected but match`; rerun `53 of 58 screens match, 3 differ as expected, 2 differ` (the second was `administrator-wp-admin-options-general-php`, 426 pixels, which matched in the first run and in the third run: treated as a flake of that rerun, not reproduced). The media grid row reports 0 pixels differ and only its address differs, as listed. Byte equal goldens green in the workspace run |
| Coverage | Pass | Workspace 2,026 tests passed, 0 failed, 1 ignored (the switch-back test, run separately); lines 100.00% (186,636), regions 100.00% (13,403), functions 100.00% (102,224) |
| Supply chain | Pass | `cargo deny check` and `cargo vet` exit 0 |

### How it was run

Each step ran alone, with `HYSCRIBE_ORACLE_LOCK` for the shared oracle and each database from a fresh `db.sh up`, taken down after.

| Check | Command | Evidence |
| --- | --- | --- |
| Tests and coverage | The coverage gate (workspace tests under `cargo llvm-cov`, 100% line and region gate) | Exit 0; no failed or panicked test, as the log of that run shows |
| Release build, supply chain | `cargo build --release -p hyscribe` (3 m 35 s), `cargo deny check`, `cargo vet` | All exit 0 |
| Switch-back, default install | `reference/switch-back/run.sh` (own database and PHP 7.1.3 container) | "switch back: PHP WordPress reads everything HyScribe wrote"; `0 failures`; 72 pages; `debug.log` 0 bytes; 46 REST rows |
| Switch-back, Theme Test Data | `HYSCRIBE_SWITCH_BACK_FIXTURE=ttd reference/switch-back/run.sh` | Same message; `0 failures`; 31 pages; `debug.log` 0 bytes; 30 REST rows |
| Wizard, new site | pexpect driving `hyscribe setup` from the release binary in an empty folder over an empty database | 19 questions, 12 tables and first content installed, `.env` mode 600, `service start`, then `/` 200, `/hello-world/` 200, `/sample-page/` 200, `/wp-login.php` 200; 3.65 s in all |
| Screenshots | `reference/screenshots/golden.sh` (Playwright 1.49.1 image, PHP and HyScribe over one fixture database), run twice | Exit 1 both times for the site editor |

CI: the jobs `switch-back` and `screenshots` then ran on every push to `development` and `main`, after `coverage`. Results recorded by this run: the run of this run's base commit had `checks`, `coverage`, `package` and `switch-back` success, `screenshots` and `fuzz` cancelled by the next push; the next run was still running; the previous completed `development` run (the `TRUSTED_PROXIES` fix) succeeded before the two jobs existed. The branch run `ci/slow-suites`: every job success.

Run notes:

- `curl: (56)` is printed once by `php.sh` while polling for PHP in the switch-back and screenshot runs; it retries and the runs were clean.
- The third run's possible load flake (two upload tests answering 503) did not recur: the coverage gate passed on its first run, 2,026 tests.

### Per criterion changes since the third run

| Criterion | Third run | Fourth run |
| --- | --- | --- |
| S1 | Default 100 of 101, Theme Test Data 747 of 747 | Default 101 of 101, Theme Test Data 747 of 747 |
| S4 | New site in 3.92 s | New site in 3.65 s |
| S7 | 154 and 39 actions, 0 failures | Same lists, 0 failures |
| S8 | 51 of 58 match, 6 differ for listed reasons, 1 stale entry | 54 of 58 match, 3 differ as listed, site editor differs unlisted (new) |
| Coverage | 2,003 tests | 2,026 tests, still 100% |
| CI | Switch-back and screenshots in no push workflow | Both ran after coverage on every push at the time; branch run green; development run of this commit cancelled by the next push |

# Earlier runs

## Third run, 2026-11-19, at commit 805f5d07 (development)

The criteria S1, S2, S3, S4, S7 and S8 of [GOALS.md](../GOALS.md) as later tightened (the literal wording, not the MVP's own definition). Since the second run landed: all 747 Theme Test Data pages match, the RSS block and random galleries are compared, the S8 screenshot comparison, the new site path of the wizard with pretty permalinks and its timed test, the classic menu navigation fallback and its page list post, `POST users/me`, and CI on a self-hosted runner. Nothing was fixed during the run.

### Verdict

The MVP is not met against the literal wording of GOALS S1 and S8, and not yet "in CI" as 9.1's exit words it. It is met against every other measure: S2, S3, S4 and S7 pass on the evidence below, S1 passes on the Theme Test Data with zero differences, and S8's served screens are byte equal to wp-admin in goldens. What is left:

1. S1 on the default install: 100 of 101 requests match, 1 differs (the preview page with a bad nonce: PHP answers a `wp_die()` page, HyScribe answers the post). Zero differences is not reached by that one request.
2. S8 screenshots: 51 of 58 screens match pixel for pixel; 6 differ for listed reasons (the block patterns list answers 501 on three editor screens; Site Health's Status and Info list PHP-only sections HyScribe does not have; the grid's address keeps `upload?mode=grid` for the URL prefix of ADR 0008), and 1 entry of `reference/screenshots/expected.list` is stale: the site editor now matches, so the script exits 1 until that line is removed. No screen differs without a listed reason. The criterion as worded, "every screen matches", is therefore a gap of those 6 screens.
3. In CI: `ci.yml` runs the page goldens of S1, the S2 and S3 goldens, the S4 timed test and the 100% coverage gate in its coverage job, but the switch-back suite (S7) runs only in `release.yml` on a version tag and the screenshot comparison (S8) runs in no workflow.

| Criterion | Result | Numbers |
| --- | --- | --- |
| S1 | Theme Test Data: pass. Default install: 1 known difference (gap) | Theme Test Data 747 of 747, `pages-ttd-known.tsv` empty. Default install 100 of 101 (`pages-default-known.tsv`: the preview nonce page). Edited site 12 of 12, commenter pages 95 of 95, pretty permalink pages 727 |
| S2 | Pass | Password goldens, the 55 case login golden and 12,980 capability cases green in the workspace run; the switch-back suite logs into PHP as `admin` and `switchback` |
| S3 | Pass (with the accepted differences of ADR 0015) | Upload and write oracle goldens green in the workspace run; the switch-back suite uploads in HyScribe and PHP serves the files, 0 failures on both fixtures |
| S4 | Pass, new site and existing site | Empty folder and empty database to a serving site in 3.92 s of wall time with the release binary (setup 1.41 s); the timed test `the_new_site_wizard_reaches_a_home_page_within_s4_s_time` passes in the workspace run |
| S7 | Pass on both fixtures | 154 and 39 actions, 72 and 31 pages, 0 check failures, empty PHP debug log |
| S8 | Gap: 51 of 58 screens match pixel for pixel, 6 differ for listed reasons, 1 stale expectation | Byte equal goldens (39 files) green in the workspace run |
| Coverage | Pass | Workspace 2,003 tests passed, 0 failed, 1 ignored (the switch-back test, run separately); lines 100.00% (186,012), regions 100.00% (13,358), functions 100.00% (101,918) |
| Supply chain | Pass | `cargo deny check` and `cargo vet` exit 0 |

### How it was run

Each step ran with `HYSCRIBE_ORACLE_LOCK` for the shared oracle and each database from a fresh `db.sh up`, taken down after.

| Check | Command | Evidence |
| --- | --- | --- |
| Tests and coverage | The coverage gate (workspace tests under `cargo llvm-cov`, 100% line and region gate) | Exit 0, as the log of that run shows |
| Switch-back, default install | `reference/switch-back/run.sh` (own database and PHP 7.1.3 container) | "switch back: PHP WordPress reads everything HyScribe wrote"; `0 failures`; 72 pages; `debug.log` 0 bytes; 46 REST rows |
| Switch-back, Theme Test Data | `HYSCRIBE_SWITCH_BACK_FIXTURE=ttd reference/switch-back/run.sh` | Same message; `0 failures`; 31 pages; `debug.log` 0 bytes; 30 REST rows |
| Wizard, new site | pexpect driving `hyscribe setup` from the release binary in an empty folder over an empty database | 19 questions, 12 tables and first content installed, `.env` mode 600, `service start`, then `/` 200 (69,636 bytes, 0.087 s), `/hello-world/` 200, `/sample-page/` 200, `/wp-login.php` 200; 3.92 s in all |
| Screenshots | `reference/screenshots/golden.sh` (Playwright 1.49.1 image, PHP and HyScribe over one fixture database) | `51 of 58 screens match, 6 differ as expected, 0 differ, 1 expected but match`, exit 1 for the stale entry |
| Release build, supply chain | `cargo build --release -p hyscribe`, `cargo deny check`, `cargo vet` | All exit 0 |

CI was not rerun by this task; commit 805f5d07 was pushed and CI runs on it.

Run notes:

- A first coverage run at 956053aa failed two tests of `rest::tests::uploads` (`upload_routes_match_php` and `uploads_stop_short`, "database error: No such file or directory", answered 503 instead of 201 and 200) while the switch-back suite, the wizard run and the screenshot run were using the same machine and Docker. The coverage gate rerun at 805f5d07, with nothing else of this task running, passed all tests. The failure was not reproduced alone and its cause is not established: treat it as a possible flake under load and watch for it in CI.
- `curl: (56)` is printed once by `php.sh` while polling for PHP in the switch-back and screenshot runs; it retries and the runs were clean.
- The previous report's `pages-default.tsv` had 99 requests; it has 101 now (two `_fallback=1` lines of the navigation fallback work), 100 matching.

### Per criterion changes since the second run

| Criterion | Second run | Third run |
| --- | --- | --- |
| S1 | Default 98 of 99; Theme Test Data 696 of 747 (747 after the fixes) | Default 100 of 101, Theme Test Data 747 of 747 as a kept test |
| S4 | Pass for an existing site, no new site path | New site path built, timed in 3.92 s and by a test |
| S7 | 154 and 39 actions | Same lists, still pass |
| S8 | No screenshot comparison | 51 of 58 match, 6 differ for listed reasons |

The ranked gaps of the earlier report below still stand except the ones later work closed (the Theme Test Data pages, the wizard's new site path, the screenshot comparison being absent).

## First two runs, 2026-11-19

First run on 2026-11-19 at commit 2f563bf8; rerun the same day at commit 87ffe550 (development), for the criteria S1, S2, S3, S4, S7 and S8 of [GOALS.md](../GOALS.md) as [MVP.md](MVP.md) defines them. 147 commits landed between the two runs: the media library, comments edges, Profile and Edit User over HTTPS, shortcodes, duotone, attachment pages, block editor edges, registration, the embed allowlist, the Plugins screen and Site Health's Status tab, uploads of every type in PHP's default list, scaled and rotated uploads, the trash schedule, REST media writes and the security bounds (`TRUSTED_PROXIES`, `REQUEST_TIME_LIMIT`, the registration cap, redirect and global styles rules). Nothing was fixed during the run.

### Verdict

| Criterion | Result | One line |
| --- | --- | --- |
| S1 | Default install: pass. Theme Test Data: pass after the fixes | 98 of 99 default install requests match PHP byte for byte (the one difference is the preview nonce page); at the rerun 696 of 747 Theme Test Data requests matched (first run: 526), and after the fixes all 747 do, with no known difference left |
| S2 | Pass | PHP's password formats verify in HyScribe, and users HyScribe wrote log into PHP |
| S3 | Pass for the MVP's definition | Uploads, sizes (including `-scaled` and `-rotated`), and the media library match PHP in goldens, and the switch-back suite now uploads in HyScribe and has PHP serve the files |
| S4 | Pass for an existing site, with a gap | Wizard from an empty folder to a serving site in 0.8 s of machine time; the new site path is not built and no timed test is kept |
| S7 | Pass on both fixtures | 154 HyScribe actions on the default install and 39 on the Theme Test Data, then PHP reads everything: 0 check failures, 72 and 31 pages, empty debug log |
| S8 | Pass for every screen served; the 501 list is short | Every served screen is byte equal to wp-admin in goldens; no screenshot comparison exists |

The MVP's exit says "all met, in CI". CI still does not run this acceptance: the switch-back suite runs only in `release.yml` (on a version tag), the gates run locally before each merge, and no job runs the Theme Test Data comparison with a pass or fail on S1.

### How it was run

| Check | Where | Command or script | Evidence |
| --- | --- | --- | --- |
| Whole workspace tests | the coverage gate, fresh fixture databases | gate test over the whole workspace, `--no-fail-fast` | 1,916 passed, 0 failed, 1 ignored (the switch-back test, run below), as the log of that run shows (first run: 1,761) |
| Switch-back suite, default install | Own database and own PHP 7.1.3 container, under the oracle lock, debug test binary | `reference/switch-back/run.sh` | "switch back: PHP WordPress reads everything HyScribe wrote"; `writes_for_php_to_read` ok in 96.9 s; the logs hold `check.log`, `hyscribe.log`, `debug.log`, `rest.tsv` and 72 page captures |
| Switch-back suite, Theme Test Data | Same | `HYSCRIBE_SWITCH_BACK_FIXTURE=ttd reference/switch-back/run.sh` | Same message; test ok in 77.6 s |
| Setup wizard, end to end | Release binary (built in 3 m 41 s) in an empty folder, a pseudo terminal (pexpect) driving `hyscribe setup` | scripted answers, then `service start`, a request, `service stop` | 13 questions, `.env` mode 600, home page 200 with 69,381 bytes in 0.065 s; the whole setup took 0.8 s |
| Coverage and mutation | Not rerun | | Gated at each merge; the last gate runs are in HISTORY |

The database, PHP container and wizard service of this run were taken down afterwards; nothing of it keeps running.

### S1: the same pages as PHP

The whole page goldens (`reference/pages/`, recorded from PHP 7.1.3 over the same database, compared by `listed_pages_match_php`, `theme_test_data_pages_match_php`, `edited_site_pages_match_php` and `commenter_pages_match_php` in `crates/hyscribe-http/src/tests.rs`). All pass: every listed request matches or is in a `*-known.tsv` list and still differs (a listed request that starts to match fails the test).

| Fixture | Requests | Match | Known to differ | Known file |
| --- | --- | --- | --- | --- |
| Default install | 99 | 98 | 1 | `pages-default-known.tsv` |
| Edited site (templates, parts, global styles, navigation written by PHP) | 12 | 12 | 0 | none |
| Commenter pages (paged comments, held comments, cookies) | 95 | 95 | 0 | none |
| Theme Test Data, default options and option variants (after the fixes) | 747 | 747 | 0 | `pages-ttd-known.tsv`, now empty (the rerun: 696 and 51) |

Default install, the one difference: `/?p=1&preview=true&preview_id=1&preview_nonce=x` (PHP answers a `wp_die()` page for a bad preview nonce on every request, HyScribe answers the post).

Theme Test Data, the 51 listed requests of the rerun by cause (a request can carry several), and what became of each:

| Cause | Requests | What became of it |
| --- | --- | --- |
| `rss` | 34 | The RSS block is compared against a feed of the test site (`reference/url-details`), fetched as `RSS_ALLOWED_HOSTS` allows |
| `rand` | 16 | A gallery with `orderby="rand"` is compared by its items in any order, as PHP's `RAND()` answers differently on every request |
| `loading` | 11 | Image loading priority attributes: no difference was left, these requests differed in the RSS block's error text only |
| `fallback` | 3 | Ported: the navigation fallback built from a classic menu, with the `wp_navigation` post PHP writes for it in primary mode (write oracle `navigation-fallback-*`). A menu with items of a post, a term or an archive, or a link to the page asked for, is not ported and keeps the page list |
| `duotone` | 1 | No difference was left either |
| `attachment-pages` | 1 | Ported: the audio attachment page's player enqueues MediaElement as `wp_audio_shortcode()` does, and the footer prints scripts in the order they were enqueued |

Result: S1 is met on the default install, the MVP's target, and on the Theme Test Data: every one of its 747 requests matches PHP.

Not ported: `[playlist]` and `[embed]` (`WP_Embed::run_shortcode()`) stay as written, and so does a shortcode inside an HTML attribute (`do_shortcodes_in_html_tags()`).

### S2: logins both ways

| Direction | Evidence |
| --- | --- |
| PHP made the user, HyScribe logs them in | Password goldens recorded from PHP 8.4: 1,813 cases (`hyscribe-auth`, `reference/auth/golden.php`) covering `$wp`, phpass `$P$`, bcrypt variants and MD5; fast hash and salt goldens; the login tests and the 55 case login golden (`reference/login/golden.sh`) pass in the workspace run |
| HyScribe made the user, PHP logs them in | The switch-back suite adds the user `switchback` through the users screen with the password `Switch back 1!` and then logs into PHP's `wp-login.php` as that user and as the administrator: both answered 302 in this run, then the profile screen showed the user |
| Roles | Capability checks match PHP for 12,980 cases (`map_meta_cap()` and `has_cap()`); the action checks golden has 1,480 rows across roles |

Result: pass. Known edges: hashes in Argon2 and SHA-crypt formats never verify (WordPress itself does not write them); the session expired dialog inside hs-admin answers 501.

### S3: a post and an upload in HyScribe, correct in PHP

| Part | Evidence | Result |
| --- | --- | --- |
| Post written in HyScribe, shown by PHP | Switch-back suite: posts, pages, REST posts with revisions, sticky and scheduled posts, categories and tags read by PHP with 0 failures | Pass |
| Upload rows | Write oracle actions row for row (43 for the media library, 15 for non-image types, 7 for scaled and rotated, 9 for REST media updates and deletes), 107 upload route cases for 6 identities, 228 REST media cases | Pass |
| Generated sizes | `makes_the_sizes_and_metadata_php_makes` (42 cases), `sizes_are_php_s`, `matches_wp_read_image_metadata`; scaled and rotated photos (corpus of 7 more files, 49 route cases); pixel data is within the bounds ADR 0015 sets, not byte equal | Pass with the ADR's bounds |
| Both in one run, upload then PHP serves it | Switch-back suite: REST and async uploads (`wide.jpg`, `large.png`, `orient-6` and others) with a featured image, then PHP's library list, the attachment's edit screen and page, and the files and their sizes, 0 failures, on both fixtures | Pass (first run: missing) |
| Media library | `query-attachments` and the other media Ajax: 66 Ajax goldens match PHP; attachment edit saves, attach, detach, deletes | Pass |
| Non-image uploads | Every type in PHP's default list: 196 of 222 PHP type checks match, none of 1,068 libmagic samples disagree | Pass, with the 501 edges below |

Differences from PHP accepted in ADR 0015 or by decision: AVIF gets no sizes, WebP sizes are lossless, CMYK JPEG and 16 bit PNG come out as 8 bit RGB, BMP gets no sizes, GPS data is dropped from sizes, the palette PNG similarity bound of 0.94 for one scaled test file. Still 501: PDF previews (Ghostscript in PHP), the getID3 metadata of MP3, MP4 and MOV, Ogg, FLAC, ASF, Matroska, AVI and FLV, Word 97 and other OLE files, bytes libmagic calls `data`, a custom `upload_path`, the async uploader's `fetch`. Media trash and restore, and deleting a logo, site icon or header attachment, are also 501.

Result: S3 is met, with the edges above.

### S4: the wizard

| Check | Result |
| --- | --- |
| Tests | Setup and wizard tests green in the workspace run: terminal and browser flows, `wp-config.php` reading, first run checks, MyISAM refusal of primary mode, secret and token handling |
| End to end | A pseudo terminal ran `hyscribe setup` in an empty folder with the release binary: 13 questions (WordPress folder: none, database name, user, password, host, port, prefix, mode, read user, proxy, address, port, uploads folder), keys and salts generated (8), `db_version 61833` found, `.env` written with mode 600. `service start` then served the home page with status 200 and 69,381 bytes in 0.065 s. Setup took 0.8 s of machine time, so the 5 minute budget is in the human's typing |
| Release archive | `scripts/smoke-test.sh` runs in `ci.yml` and `release.yml` (not rerun here) |

Gaps: the setup is for an existing site only. The "new site" path of docs/SETUP_WIZARD.md (empty database, schema, first administrator) is not built, and the compatibility scan is out of the MVP (the scan is to list missing plugins and themes; it is M2). There is no timed acceptance test, so "under 5 minutes" is shown by this run, not kept in CI.

Result: pass for the MVP's own definition (a default install's database); partial against the literal wording "empty directory to running site" if an empty database is meant.

### S7: handing the database back to PHP

The suite (`reference/switch-back/run.sh`, MVP 8.5) writes with the binary over HTTP using HyScribe's own login, then starts PHP WordPress with `WP_DEBUG` and `WP_DEBUG_LOG` on the same database.

| Measure | Default install | Theme Test Data |
| --- | --- | --- |
| HyScribe's actions | 154 requests of `actions.list` (dashboard writes, terms, posts and pages, comments, users and profile, every Settings screen, REST posts, pages, patterns, navigation, revisions, a visitor's comment, image uploads on the REST and async routes with sizes, template, template part and global styles saves, a cron published scheduled post and a cron emptied trash), all answered their expected status | 39 requests of `actions-ttd.list` |
| PHP's own checks (`check.php`) | "0 failures"; PHP's REST read of the 46 objects HyScribe's REST wrote matches HyScribe's answers | "0 failures"; 30 REST rows |
| Pages and screens | 72 requests of `pages.list`, each answered its status, showed its text, no PHP or database error | 31 requests of `pages-ttd.list` |
| PHP debug log | Empty (0 bytes) | Empty (0 bytes) |
| Logins | `admin` and `switchback` both 302 | `admin` 302 |

Left out, so S7 is not shown for them: a database that has had PHP write and HyScribe write in turns, a second pass with PHP writing back and HyScribe reading again, and the plugin and update screens' effects (none write). The suite's cron step runs only by hand (`cd02f281`), not in a plain run of the list.

Result: pass on both fixtures for what the suite covers.

### S8: hs-admin screens against wp-admin

Method: 39 golden files (`crates/hyscribe-admin/tests/golden/`, recorded from PHP 7.1.3 per role and per state) compared byte for byte, which is stricter than a normalized DOM comparison. They all pass in the workspace run. They cover: dashboard, admin menu and bar, posts, pages, terms, comments, users, user edit, profile (also over HTTPS), every Settings screen, the block editor page, the site editor page, revisions, notices, privacy requests, Site Health Info and Status, Plugins, Themes, Tools, Import, Export, Media library and attachment edit, login, lost password and registration screens, action checks and saves.

Not shown: a screenshot comparison (GOALS and ADMIN.md ask for one; none exists), and behaviour that runs in the browser (the editors' and media screens' JavaScript against HyScribe's REST answers, Quick Edit, the grid) beyond the byte equality of the markup and the REST and Ajax answers.

Entries of the default install's administrator menu and what they do:

| Menu entry | State |
| --- | --- |
| Dashboard, Posts, Pages, Media (library, Add New), Categories, Tags, Comments, Users, Add User, Profile, Tools, Import, Export, Site Health (Status and Info), Settings (all seven), Themes, Plugins, Site Editor | Served, goldens match |
| Updates under Dashboard | Refused as PHP refuses it, since `update_core` is never granted |
| Profile and Edit User on an HTTPS site | Served, with the application passwords list |

Result: every screen of the default install's menu is served and matches. The GOALS wording "every hs-admin screen matches on the default install fixture" holds for the byte comparison; the screenshot half of it is missing.

### Remaining gaps

Ranked by how often a default install user is likely to hit them. Status: Decided means it has been answered; Deliberate means a difference kept on purpose (for security or by design); To do means no decision is needed, the work is not done.

| Rank | Gap | Who hits it | Status |
| --- | --- | --- | --- |
| 1 | Mail: nothing is sent without `SMTP_HOST`, so users cannot reset their own passwords; the personal data request mails (confirmation request, export ready, erasure done) are not sent | Every site until SMTP is set | Decided (ADR 0016, SMTP in `.env`); the wizard does not ask for it yet: to do. The personal data request actions answer 501: to do |
| 2 | Uploads PHP handles that HyScribe answers 501: the getID3 metadata of Ogg, FLAC, ASF, Matroska, AVI and FLV files, MP3, MP4 and old Office files of unusual structure, bytes libmagic calls `data` | Sites that attach such audio or video | Mostly done since this run (ADR 0020 and its amendments): PDFs upload without previews, and MP3, MP4, QuickTime and Word, Excel and PowerPoint 97 to 2003 files upload; the remainder is to do |
| 3 | Media trash and restore, `featured_media` or a negative `post` on a media update, deleting a logo, site icon or header attachment | Sites with `MEDIA_TRASH` on, theme logo changers | To do (`MEDIA_TRASH` is an `.env` key the wizard copies from `wp-config.php`, decided on 2026-11-19) |
| 4 | Outbound fetches are off by default: link previews in the editor (`url-details`), uncached embeds, the RSS block; with them on, embeds and the RSS block reach only listed hosts | Authors who paste links, sites with the RSS block | Decided (ADR 0019) |
| 5 | The block editor's edges: new terms created inline, a custom template made from the editor, `meta` other than footnotes, image crop and rotate in the editor (media post processing), fonts routes, `_fields` on templates, batch requests including a 501 route | Power users of the block editor | To do; each is a documented 501 that writes nothing |
| 6 | Themes and plugins: other themes than Twenty Twenty-Five are not served, no plugin runs, Themes takes no arguments, plugin and theme install and activation answer 501 | Sites with a different theme or any plugin | Decided (plugin work starts now); phases 10 and 11, after the MVP |
| 7 | Front end: `[playlist]` and `[embed]` in classic content, a classic menu with items of a post, a term or an archive for the navigation fallback, and the video attachment page's player; the Theme Test Data pages all match | Visitors of sites with older content | To do; default install pages are not affected |
| 8 | Login: the session expired dialog inside hs-admin, a registration form with an array as `redirect_to`, other POST actions of `/hs-login` | Rare | To do |
| 9 | Stricter than PHP on purpose: a global styles value with `{` or `}` is dropped, `X-Forwarded-For` only from `TRUSTED_PROXIES`, a request over `REQUEST_TIME_LIMIT` answers 503, registrations capped at 3 an hour per address, redirect hops of embeds and RSS must stay on the allowed hosts, XML-RPC, pingbacks and user enumeration off (author archives turn on with `AUTHOR_ARCHIVES=on`) | Rare, or by design | Deliberate (ADR 0006) |
| 10 | Image pixel differences: AVIF without sizes, lossless WebP sizes, CMYK and 16 bit images come out as 8 bit RGB, no BMP sizes, GPS data dropped from sizes | Sites with those images | Decided (ADR 0015) |
| 11 | Multisite refused, WordPress's own `wp-cron.php` and XML-RPC not served, database TLS unsupported | Few default installs | Decided (out of the MVP) |
| 12 | Wizard: no new site path, no compatibility scan, no timed test | New installs | To do (the scan is M2) |

Also of note:

| Item | Detail |
| --- | --- |
| Stale documents | `docs/operators/not-supported.md` still lists Plugins, Site Health's Status tab, and Profile and Edit User over HTTPS as 501, which they no longer are and lists uploads of other file types as 501 that a decision ported; `PROGRESS.md` quotes the first run's verdict and `docs/MVP.md`'s "Where it stands" table is dated 2026-11-13 |
| Acceptance in CI | No workflow runs S1 on the Theme Test Data, S3, S4, S7 or S8 as a gate; the switch-back suite is release only. This is what keeps 9.1's exit ("in CI") unmet |
| Outbound fetches | `HTTP_OUTBOUND` is off by default (ADR 0019) |
| Run notes | `php.sh up` printed one `curl: (56)` while polling for PHP; it retried and the run was clean |

### Is the MVP met

The functional MVP as `docs/MVP.md` defines it is met on the default install: S1, S2, S3, S7 and S8 pass, and S4 passes for an existing site. Two things stand between this and "met" as phase 9.1 words its exit.

1. The criteria run "in CI". Nothing gates S1 on the Theme Test Data, S4, S7 or S8 in CI, and the gates run locally; a scheduled job on a self-hosted runner for the switch-back suite and the Theme Test Data pages would close it.
2. Literal wording in GOALS: S1 asks for zero differences on the Theme Test Data (met after the fixes), S4 asks for a new site path, and S8 asks for a screenshot comparison. These can be reworded to the MVP's definitions, or the work done.

Practical gaps a day to day user meets are rank 1 to 3 above: mail needs SMTP set, a few rare upload types answer 501, and media trash is not ported.

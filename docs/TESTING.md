# Testing strategy

Compatibility and security claims are only as good as the tests behind them. PHP WordPress is the oracle, and the whole application is held to full test coverage ([ADR 0007](adr/0007-full-test-coverage.md)).

## Coverage policy

Full coverage applies to everything that ships: every crate in the workspace, the binary, the wizard, the admin UI, the SDK, every first-party plugin and theme port, and the porting tools.

| Measure | Requirement | Tool |
| --- | --- | --- |
| Line coverage | 100% | `cargo-llvm-cov` |
| Region coverage | 100% | `cargo-llvm-cov` |
| Branch coverage | Tracked and reported; 100% target, enforced as soon as the stable toolchain supports it | `cargo-llvm-cov` (nightly job until then) |
| Mutation score | No surviving mutants in changed code; 100% target on security-critical crates | `cargo-mutants` |
| Admin UI (TypeScript) | 100% lines, branches, functions, statements | Vitest coverage, Playwright for flows |
| Every public HTTP route | At least one test per route, method and auth state | Integration suite |
| Every core hook HyScribe fires | At least one test asserting it fires with the WordPress arguments | Hook catalogue tests |
| Every parser | Unit, golden and fuzz tests | `cargo-fuzz` |

Coverage is a floor, not the goal. Mutation testing is what proves the tests would catch a real bug.

### Rules

- **CI gate.** A pull request that lowers any coverage figure, or leaves a surviving mutant in the code it changes, fails
- **Tests with the change.** Every change, including bug fixes, comes with tests in the same pull request; a bug fix starts with a failing test that reproduces it
- **Exclusions are rare and reviewed.** Code can be excluded from coverage only with `#[cfg_attr(coverage_nightly, coverage(off))]` plus a one-line reason, and only for: generated bindings, code that is unreachable by construction, and process-level glue covered by end-to-end tests instead. Every exclusion is listed in `coverage-exclusions.md` and reviewed in the pull request that adds it
- **No test-only behaviour in production code.** Testability comes from dependency injection and traits, not `cfg(test)` branches in shipped logic
- **Deterministic.** No sleeps, no network to the internet, fixed seeds and clocks; flaky tests are fixed or removed within a day, never retried into passing

### Extensions and WebAssembly

Coverage tools cannot instrument code running inside the WebAssembly sandbox well. Ports are therefore built twice in CI:

- **Natively**, against a test host provided by `hyscribe-sdk`, where coverage is measured and must be 100%
- **As components**, loaded by the real runtime in the differential and integration suites, to prove the shipped artifact behaves the same

### How coverage is measured

- Command: `cargo llvm-cov --workspace --ignore-filename-regex '/tests/' --fail-under-lines 100 --fail-under-regions 100`
- Measured: everything under `crates/*/src/`, including the binary's `main.rs` (covered by running the real binary from integration tests)
- Not measured: `crates/*/tests/` (integration tests, whose only unexecuted code is failure reporting) and `fuzz/`
- Each crate's own unit tests must cover that crate fully. llvm-cov rates a function by its best compiled copy rather than merging copies, so coverage reached only through another crate's tests does not count
- Child processes started by tests keep `LLVM_PROFILE_FILE`, or their coverage is lost
- Property tests use a fixed seed (`RngSeed::Fixed`), so coverage is the same on every run

### Reporting

- Coverage and mutation reports are published as CI artifacts for every pull request
- `PROGRESS.md` shows the current coverage per crate once code exists

## First test target: default install

The first fixture, and the first thing every layer is tested against, is a plain WordPress install with nothing changed: what `wp-admin/install.php` produces for a site title, an admin user and an email, with the bundled default theme, the sample post, the sample page, the sample comment and the bundled plugins left inactive.

- Built by `reference/default-install/build.sh` for each pinned WordPress version (7.0.x and the latest 7.x), dumped to `fixtures/default-install/<version>/`
- **Read:** HyScribe reads every row of the dump; every serialized option and meta value round trips byte for byte (M1 exit)
- **Install:** HyScribe's new-site install, given the same answers, produces the same database as WordPress's installer, ignoring only values WordPress randomizes (salts, password hashes, timestamps, the cron schedule) (M2 exit)
- **Serve:** the front page, the sample post, the sample page, archives, feeds and `wp-login.php` match PHP WordPress (M3 exit)
- **Admin:** every hs-admin screen matches `wp-admin` on this database ([ADMIN.md](ADMIN.md)) (M5 exit)

Further fixtures are added only after the default install passes at the current milestone.

## Reference environment

- Containers: PHP WordPress 7.0 and later (pinned versions), MariaDB and MySQL, WP-CLI
- Fixture sites built by scripts, never by hand:
  - default install
  - Theme Unit Test data imported
  - non-default table prefix, latin1 legacy charset, multisite subdomain and subdirectory
  - sites with each pilot plugin active
- Database snapshots versioned so tests are reproducible
- The `wasm32-wasip2` Rust target on every machine that runs the tests (`rustup target add wasm32-wasip2`; `scripts/ci-setup.sh` adds it in CI): `hyscribe-ext`'s tests build their test extension (`crates/hyscribe-ext/guest`, on `hyscribe-sdk`) and test theme (`guest/theme`, ADR 0023) from source with `crates/hyscribe-ext/guest/build.sh` (`build.sh theme` for the theme); a port's own code is tested natively against the SDK's test host (`hyscribe_sdk::test`, ADR 0023), at 100% coverage with no surviving mutants like the host's, in a clean environment so coverage and mutation flags stay the host's, and no compiled `.wasm` is committed (ADR 0022); a mutant of `hyscribe-sdk` is caught by `hyscribe-ext`'s tests, which build the extension from the mutated source, so mutation runs over the SDK take `--test-package hyscribe-ext`

## Test layers

| Layer | What it checks |
| --- | --- |
| Unit | Every function and module, starting with parsers and codecs: PHP serialize, block grammar, shortcodes, wpautop, `wp-config.php` |
| Property | Invariants with `proptest`: round trips, parser totality, escaping never emits raw input |
| Integration | Every HTTP route, CLI command, service verb and wizard step against a real database |
| End to end | Extract the release archive, `service start`, complete the browser wizard, serve the fixture site (Playwright) |
| Mutation | `cargo-mutants` on changed code per pull request, full run nightly |
| Security | Regression test per fixed vulnerability, attack payloads per route, capability denial tests per extension API |
| Golden | Rust implementations of WordPress functions against outputs captured from PHP for thousands of inputs (wpautop, wptexturize, sanitize_title, kses) |
| Round trip | Read every value from fixture DBs, re-serialize, assert byte equality |
| Differential HTTP | Same request to PHP WordPress and HyScribe on the same DB snapshot; compare status, key headers, and normalized HTML or JSON |
| Write compatibility | Perform an action in HyScribe, then verify with PHP WordPress (WP-CLI and HTTP) that the result is correct; and the reverse |
| Write layer | Every `hyscribe-db` write method against the fixture database inside a transaction that is rolled back, so read tests always see the fixture as built; write tests take one lock so no two wait on each other's rows; a row a method inserts is compared with the fixture row PHP wrote for the same values |
| Auth | Cross-login, cookie reuse between both systems |
| Fuzz | cargo-fuzz targets for every parser |
| Performance | Benchmarks of key pages against PHP WordPress |

### Running it on the default install

```
reference/default-install/db.sh up      # MariaDB with the fixture
reference/default-install/php.sh up     # PHP WordPress on http://127.0.0.1:8081, same database
# start HyScribe on 8080 with WP_HOME and WP_SITEURL set to http://127.0.0.1:8081, as the PHP side has them, then:
reference/diff/compare.sh http://127.0.0.1:8081 http://127.0.0.1:8080 /tmp/diff
```

`reference/diff/default-install.urls` lists the URLs; an intended difference carries the status HyScribe gives instead. The report compares status codes and titles today; body comparison follows the HTML normalizer.

## Write oracle

Every write HyScribe makes is checked against the rows PHP WordPress writes for the same action on the same fixture ([ADR 0012](adr/0012-admin-writes.md), MVP 2.3).

### Recording

- `reference/writes/actions.php` lists the actions: each has an `action` closure and an optional `setup` that makes the state it starts from and returns what the action needs (the ID of the post to publish, for example)
- `reference/writes/golden.sh [action ...]` runs each action with `reference/writes/run.php` in the PHP container, one PHP process per action, as user 1, with `DISABLE_WP_CRON` and `DISALLOW_UNFILTERED_HTML` (HyScribe's default, the safer value of the capability constants' `.env` key); an action whose name ends in `-unfiltered-html` runs without that constant, as a site whose owner allows unfiltered HTML. Admin screen goldens get the same through the `hyscribe_unfiltered_html` cookie of `reference/admin-menu/as-role.php`
- The runner calls the core functions (`wp_insert_post()`, `wp_set_comment_status()` and so on) inside a transaction it rolls back. The actions are the functions MVP 2.4 ports; an admin action (phase 3) is the screen's own `switch` case, run from WordPress's source by `reference/writes/actions-admin.php` (docs/ADMIN.md, "Actions")
- It takes every row of the twelve core tables before the setup, before the action and after it, and writes the rows the action added (`+`), changed (`~`, only the changed columns) and deleted (`-`) to `crates/hyscribe-testkit/tests/golden/writes/<action>.tsv`
- Record from a fresh `db.sh up` with `php.sh up`, and take both down after: the rollback leaves the rows as they were, but not the auto increment counters

### The golden format

- One line per column: `table<TAB>key<TAB>op<TAB>column<TAB>value`, with a second value after the first for `~` (before, after). PHP writes the lines sorted bytewise; the comparison does not depend on their order
- The key is the masked primary key (`option_name` for `wp_options`, `object_id,term_taxonomy_id` for `wp_term_relationships`)
- Values are escaped so a line stays a line: `\N` for NULL, and `\\`, `\t`, `\n`, `\r`
- Lines starting with `#` are comments: what each token stood for in the recorded run, so a golden can be read back to the real values

### What is masked and why

The masks hide what differs between two correct runs and nothing else. They are applied the same way on both sides, before rows are compared, so a column whose only change is a masked value is not listed.

- `%now%`: a whole value shaped `YYYY-MM-DD HH:MM:SS` between the clock before the setup and after the action (PHP's clock in PHP, the database server's in Rust, both the host's; UTC; the fixture's `gmt_offset` is 0, so local dates are the same). Post, comment and user dates
- `%unix%`: any run of exactly ten digits whose value is a Unix time in that span, inside any value. `_wp_trash_meta_time`, `_edit_lock`'s time, the `cron` option's event keys
- `%<table>:<n>%`: the n-th new auto increment ID of a table, in the order of the IDs, counting rows the setup added. New means absent before the setup, so the masks do not depend on the counters (which the rolled back runs advance) or on another test's rows. The ID is masked in its own column and in the columns known to hold it: `post_parent`, `post_id`, `object_id`, `comment_post_ID`, `term_id`, `term_taxonomy_id` and the other links between the core tables, as a whole term `slug` (a name of punctuation only gets its term's ID), as a digit run in `guid` (`?p=12`) and `post_name` (`12-revision-v1`), and as the parents and children of a `{$taxonomy}_children` option (`a:1:{i:12;a:1:{i:0;i:13;}}`, an `option_value` of that shape only, so its list positions are kept)

- `%password%`: a whole value shaped as WordPress 7's password hash (`$wp$2y$12$` and 53 characters of salt and hash), whose bcrypt salt is random: `user_pass` of new users and new passwords. HyScribe's tests check the stored hash against the password instead
- `%fasthash%`: `$generic$` and the 40 characters after it, `wp_fast_hash()` of a random password reset key, in `user_activation_key` after its time (`%unix%`)
- `%expiry%`: the `option_value` of a transient's timeout (`_transient_timeout_*` and `_site_transient_timeout_*` options), the time plus the transient's lifetime, which falls after the run. HyScribe's tests check it against the clock instead
- `%hash%`: a serialized 32 digit hexadecimal string in the `adminhash` option or a user's `_new_email` meta, the random key `update_option_new_admin_email()` or `send_confirmation_on_profile_email()` makes for confirming a new email
- `wp_page_for_privacy_policy` set to a new post's ID gets that post's `%wp_posts:<n>%` mask as its whole value

Values that are generated but have no mask yet (session tokens, salted hashes, GUIDs of uploads) get one, documented here, with the first action that writes them.

### Checking HyScribe

`hyscribe_testkit::writes::check_writes(&fixture_database(), "<action>", setup, action)` runs the setup and the action, as async closures over a `&mut MySqlConnection`, inside a transaction on the fixture database that is never committed, takes the same diff with the same masks, and returns the differences by table, column and key. HyScribe's write functions run on that connection and must open their transaction with sqlx's `begin()` on it, which nests as a savepoint (a raw `START TRANSACTION` or `COMMIT` would commit the test's rows). The setup must leave the same rows as PHP's setup: their IDs count towards the masks, and the action's before values are theirs.

Error paths: `Write::fail_after(n)` lets `n` more statements run and fails each one after, so a test runs an action under every budget from 0 up until it succeeds and each `?` on its way returns the database's error once (`fails_at_each_step` in hyscribe-write's tests).

### Adding an action

1. Add it to `reference/writes/actions.php`
2. Record it with `reference/writes/golden.sh <action>` over a fresh `db.sh up` and `php.sh up`, then take both down and start a fresh `db.sh up` (the masks in `reference/writes/run.php` and `crates/hyscribe-testkit/src/writes.rs` must stay alike)
3. Read the golden: every line should be a write PHP makes for that action
4. Add the test that runs HyScribe's port through `check_writes`

## Switching back

The switch-back suite (MVP 8.5) checks S7: a database HyScribe has written is served by PHP WordPress with nothing lost or broken. It is slow and writes to the fixture, so it is not part of `cargo test`; the release workflow runs it, and it runs locally with Docker:

```
reference/switch-back/run.sh [log folder]      # default target/switch-back
```

- **HyScribe writes.** Over a fresh `db.sh up`, the ignored test `crates/hyscribe/tests/switch_back.rs` starts the release binary in primary mode and sends it the requests of `reference/switch-back/actions.list` over HTTP, logged in through `/hs-login` with HyScribe's sessions: the dashboard's Quick Draft and screen state, Quick Edit and Bulk Edit, trash and the emptied trash, categories and tags, comments posted from the front end and moderated, replied to and edited, users added, edited, promoted and deleted, the profile and every Settings screen with the permalinks. Each answer must have its status and no error
- **PHP checks the data.** `php.sh up` on the same database, with WP_DEBUG and WP_DEBUG_LOG on (set in the container's `wp-config.php`, since WordPress defines WP_DEBUG before plugins load) and `reference/switch-back/plugin.php` as a must-use plugin (no cron, update checks or outbound requests). `reference/switch-back/check.php` runs in the container: every option and meta value unserializes, `wp_load_alloptions()`, `get_userdata()` and a known role for every user, `wp_count_posts()` and `wp_count_comments()`, every post's `comment_count` and every term's `count` recomputed by WordPress (in a transaction rolled back) against what is stored, what `sticky_posts`, the privacy page and `default_category` name exists, and the `rewrite_rules` HyScribe left either empty or what PHP generates
- **PHP serves the screens and pages.** It logs in through `wp-login.php` as the administrator and as the user HyScribe added (so HyScribe's password hash is checked by PHP), and asks for each screen and page of `reference/switch-back/pages.list`: each must have its status and show its text
- **Fails** on any check, any page showing a PHP error or a database error, and any line in PHP's `debug.log`. Logs, PHP's pages and a dump of the database HyScribe wrote stay in the log folder
- The database is reset with a fresh `db.sh up` after. `HYSCRIBE_ORACLE_LOCK` names a lock file the PHP part waits for (`flock`) when the oracle container is shared; the `HYSCRIBE_DB_*` variables pick the fixture as for `db.sh`
- A new write action joins the suite: its request in `actions.list`, and what it shows in PHP in `pages.list`

## Screenshot comparison

GOALS S8 asks for rendered pixels as well as markup: CSS, scripts and assets matter too. `reference/screenshots/golden.sh` photographs each screen of `reference/screenshots/screens.list` (as administrator, editor, subscriber and visitor) with a headless Chromium in Docker (Playwright's image, pinned by digest in the `Dockerfile`), first from PHP WordPress and then from HyScribe's release binary, each on a fresh `db.sh up` with `seed.sql` added, both answering as `http://wp.test` on one Docker network so every URL on the page is the same and only the prefix differs (`/wp-admin/<file>.php`, `/hs-admin/<file>`).

- **Fixed conditions:** viewport 1280 pixels wide, full page, UTC, en-US, light colour scheme, animations and carets off, one Chromium and its fonts. PHP runs with HyScribe's constants (`DISALLOW_FILE_MODS`, `DISALLOW_UNFILTERED_HTML`), no cron, no outbound requests, and `mu-plugin.php` for what hs-admin shows differently on purpose (no update offers, no plugins, one theme, no Events and News)
- **Masks:** what legitimately differs is painted over on both sides, named per screen in `screens.list` (the maximum upload size, the generated password, the Post via email strings, the server's time)
- **Tolerance:** a pixel differs when a colour channel is more than 16 of 255 apart; a screen matches at the same size with at most 0.01 percent of its pixels different and the same requests (status, method, path and query, the prefix and nonces normalized)
- **Report:** `target/screenshots/report.tsv` and `report.html`, a `diff-<screen>.png` for each difference (red over the dimmed PHP picture). Exit 0 when every screen matches or differs as listed in `expected.list` (screen and reason); a listed screen that matches is stale and fails
- **In CI:** the self-hosted runner needs Docker and the release binary (`HYSCRIBE_BIN`); one run takes about 15 minutes. It is not a workspace test: the lists are the contract

## HTML normalization

Differential tests normalize before comparing: attribute order, whitespace between tags, nonce values, timestamps, asset version query strings. The normalizer is itself tested so that it cannot hide real differences.

## CI gates

- `cargo fmt --check`, `cargo clippy -D warnings`, unit, property, golden and integration tests before every merge, and in CI for every release tag and when run by hand
- Coverage gate (100% lines and regions, no decrease anywhere) and mutation testing of changed code on every pull request
- Differential and write-compatibility suites on every pull request to `development`
- Full matrix (all WordPress versions in range, MySQL and MariaDB) nightly

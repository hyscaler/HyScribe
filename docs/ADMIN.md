# hs-admin

The HyScribe dashboard. It emulates `wp-admin` as closely as possible ([ADR 0008](adr/0008-hs-admin-emulates-wp-admin.md)), so anyone who can use WordPress can use HyScribe without learning anything new.

## Rules

- Same menus, screens, titles, columns, actions, notices and color schemes as `wp-admin` for the WordPress version the database reports
- Its own look, so nobody mistakes it for `wp-admin` (ADR 0008 amendment, 2026-11-20): users without a scheme, with `modern` or an unknown one see the HyScribe colour scheme, WordPress's colour Sass in the brand's colours (since 2026-11-23, Coffee before; nothing is written for it), core's own screen strings name HyScribe in place of WordPress, and since 2026-11-23 the brand's mark from `brand/` stands where WordPress's logo was: the admin bar has a HyScribe item with the small mark, linked to the Dashboard, `/hs-login` shows the mark, and admin and login pages of a site without a Site Icon take the small mark as their icon. The post editor's publish panel shows the mark over the bundle's WordPress logo fallback (a style rule appended to the admin bar's, on the block editors' pages). Since 2026-11-21 the footer shows `HyScribe x.y.z`, and since 2026-11-24 its sentence credits HyScaler with a link (the attribution `NOTICE` asks for, admin footer only), and since 2026-11-25 offers porting help with a `mailto:` link (subject naming the site's domain), as the dashboard's welcome banner does, whose link goes to the release notes of HyScribe's version and whose colours and mark are the brand's, the block editor's and site editor's bundles get HyScribe wording as script translations (`<handle>-js-translations`, `hyscribe_admin::look`), the Classic, Shortcode and Comment Edit Link block descriptions are reworded in the editor's block definitions, and the setup wizard names WordPress only as the install a site comes from. Markup, ids, classes, handles, URLs and hook names stay as they are; the PHP oracle runs `reference/admin-look/hyscribe-look.php` so goldens stay differential
- Same screens and query arguments under `/hs-admin/`, named without `.php`: `/hs-admin/edit?post_type=page` is `wp-admin/edit.php?post_type=page`. The login screen is `/hs-login`
- The WordPress URLs are kept for backwards compatibility only: `/wp-admin/<screen>.php` redirects to `/hs-admin/<screen>` (`index.php` to `/hs-admin/`) and `/wp-login.php` to `/hs-login`, with the query kept. These are `307 Temporary Redirect`: browsers do not cache them, so a site switched back to PHP WordPress gets its own `wp-admin` back at once, and a form posted to `wp-login.php` stays a POST
- HyScribe URLs written with `.php` lead to the clean form, query kept: a GET or HEAD of `/hs-admin/<screen>.php` gets a `301`, any other method on it is served in place (a redirect would drop its body), and `/hs-login.php` gets a permanent `308`
- HyScribe prints the clean form everywhere: `admin_url()` maps `<file>.php` to `<file>` (`hyscribe_admin::menu::link`), and every screen's page, hs-admin's redirects and the login's redirect pass through `hyscribe_admin::menu::clean_links`, which drops `.php` after `/hs-admin/` (plain, JSON-escaped or URL-encoded) and from relative `href` and `action` links. Goldens map `wp-admin/<file>.php` to `hs-admin/<file>` in `hyscribe_testkit::hs_admin_page`
- Links inside hs-admin are relative and drop `.php`, as `wp-admin`'s are relative: the admin menu's `edit.php?post_type=page` is `edit?post_type=page`, and its Dashboard link `index`, which serves the dashboard as `/hs-admin/` does
- `/wp-content/...` and `/wp-includes/...` URLs are never redirected or rewritten
- Same HTML structure, IDs and CSS classes, so WordPress admin CSS, scripts and plugin admin pages work unchanged
- Capabilities checked on the server for every screen and action, exactly as WordPress does
- Every state-changing action needs a valid nonce

## URLs

| WordPress | HyScribe |
| --- | --- |
| `/wp-admin/` | `/hs-admin/` |
| `/wp-admin/edit.php?post_type=page` | `/hs-admin/edit?post_type=page` |
| `/wp-admin/admin.php?page=wpcf7` | `/hs-admin/admin?page=wpcf7` |
| `/wp-admin/admin-ajax.php` | `/hs-admin/admin-ajax` |
| `/wp-login.php?action=lostpassword` | `/hs-login?action=lostpassword` (same form) |
| `/wp-admin/install.php` | `/hs-admin/install` for new sites, same steps as WordPress |

## First screens

In the order they are built, driven by the default install fixture ([TESTING.md](TESTING.md#first-test-target-default-install)):

1. `install` (new site: language, site title, username, password, email, search engine visibility)
2. `hs-login`
3. Dashboard (`index.php`) with the default widgets
4. Posts and Pages lists and the block editor
5. Media library
6. Comments
7. Appearance (themes, editor for block themes)
8. Plugins (installed ports and missing plugins from the compatibility report)
9. Users and profile
10. Settings (General, Writing, Reading, Discussion, Media, Permalinks, Privacy)
11. Tools

## Intended differences

Only for security (ADR 0006), and the look of the amendment above. Each difference is listed here and shown in the compatibility report.

| `wp-admin` feature | hs-admin | Reason |
| --- | --- | --- |
| Theme and plugin file editors | Not available | Editing code from the browser is a common takeover path |
| Upload a plugin or theme `.zip` with PHP | Replaced by signed `.hyx` packages | HyScribe never runs PHP or files from disk |
| Core, plugin and theme auto updates from wordpress.org | Replaced by HyScribe releases and the package registry | Updates must be signed HyScribe artifacts |
| Login cookies | HyScribe's own names (`hyscribe_<hash>`, `hyscribe_logged_in_<hash>`, `hyscribe_test_cookie`), `SameSite=Lax`, with sessions in files (ADR 0011) | Separate from a PHP WordPress login on the same domain |
| Secure login cookies | Set when the site URL is https, as served through a TLS proxy | HyScribe serves plain HTTP, so `is_ssl()` has nothing to read; an https site's sessions must never travel over HTTP |
| Compressed loader links (`load-styles.php?c=1` once `can_compress_scripts` is set) | Always `c=0` | The loaders never gzip themselves; compressing responses is the web server's or proxy's job |
| Logging in in shadow mode | The login screen says the site is read only, and nobody logs in | Shadow mode never writes |
| What a HyScribe extension asks for (its manifest's grants and limits) | Listed in its Plugins screen row until its activation approves it (ADR 0022) | An extension runs only with grants an administrator approved |
| Plugin updates (`update_plugins`, never granted) | A newer version in the kept registry index gets PHP's update row with `hyscribe plugin update <id>` in place of an update link, a revoked version an error row; no menu count and no Update Available view (ADR 0024) | Files never change from the browser; updates come only from the registry, verified |
| The capabilities to edit, install, upload, update or delete code (`edit_files`, `edit_plugins`, `edit_themes`, `install_plugins`, `upload_plugins`, `update_plugins`, `delete_plugins`, the same for themes, `update_core`, `update_php`, `install_languages`, `update_languages`) | Never granted, whatever the role | The server's code and files never change from the browser |

## Actions

An action's request passes PHP's checks first (`hyscribe-admin`'s `actions`: `check_admin_referer()`, `check_ajax_referer()` and the capability checks), then goes to its write's handler, which runs as one action of the writer ([ADR 0012](adr/0012-admin-writes.md)): committed when it answers, rolled back when it stops. Without a handler the action answers `501`. In shadow mode there is no writer, and an action with a handler answers `wp_die()`'s page with a `403` saying the site is read only, the login screen's message. REST writes take the same path ([ADR 0013](adr/0013-rest-server.md)'s amendment): the posts' and pages' create, update and delete (MVP 4.3) write their plan in one writer action in `hyscribe-http`'s `rest/writes.rs`, and answer 403 in shadow mode; the other REST writes still answer `501` after their permission check.

### Adding a handler

1. Port what PHP does once the checks pass as a handler in `crates/hyscribe-http/src/admin/saves/<screen family>.rs`: `fn(&mut Write, &Asked, &dyn Acting) -> Saving` (see `users::promote`), reading the request with `Asked::request` and reading and writing the database only through its `Write`, which sees the action's own writes, with hyscribe-write's ported functions. The `Acting` site answers what the checks read (`current_user_can()`, `wp_validate_redirect()`, `admin_url()`, a comment's link) as it was before the action, each call made through `saves::read()`, which lets it block; a test of a handler that reads none of it passes `saves::tests::unread::Unread`
2. Register it with one arm in `saves::handler()`, by the screen (hs-admin's path without `/hs-admin/`) and the action `hyscribe_admin::actions::action()` names: the users and terms lists' `current_action()`, else `$_REQUEST['action']`; a screen that names its actions otherwise gets its arm there. An `admin-ajax.php` action registers in `saves::ajax_handler()` and answers a `saves::Ajax`: text, JSON or XML, or rows the answer prints once committed (`saves::ajax_served()`, see `saves/terms.rs`)
3. Answer as PHP: `Ok(Answer::Redirect(..))` for its `wp_redirect()`, committed; `Err(Stop::Died(..))` for a `wp_die()` part way, rolled back, so nothing is half written; `hyscribe_write::Error::Unported` for what PHP cannot go on with (a fatal error), rolled back and answered `501`. A database error rolls back and shows the database error page
4. Rows: add the case to `reference/writes/actions-admin.php`, which runs the screen's own `switch` case from WordPress's source (`$admin_case`) over the request, and record it with `reference/writes/golden.sh <case>`. The Rust test runs the handler through `saves::act()` inside `check_writes`, with each statement budget short of success failing once (`saves/tests.rs`)
5. Answer: add the request to `reference/admin-saves/cases.list` and record `reference/admin-saves/golden.sh` (PHP really writes: from a fresh `db.sh up` and `php.sh up`, both reset after) into `crates/hyscribe-admin/tests/golden/admin-saves.tsv`. `actions_answer_as_php_once_checked` sends each case to hs-admin with a writer as the fixture's `SELECT` only user, so no test changes the fixture; a case that needs rows the fixture lacks is compared in the oracle test instead. The cases after `# Restored` each start from the whole database as it was, run the SQL of their eighth column first (a fresh edit lock), and with `->` as their screen after record the screen their redirect leads to in `admin-saves-after.tsv`; `the_lists_actions_answer_as_php` sends them to hs-admin with a real writer on `wordpress_saves`, loaded afresh for each, and compares their answers and screens after (`saves/posts/tests.rs`)
6. The checks' tests that expected `501` for the action now expect the handler's answer, or shadow mode's `403` on a site without a writer

A handler that needs `current_user_can()` after part of its action wrote, as the posts lists' bulk actions do post by post, reads the capabilities through its own `Write` (`saves/posts.rs`, `Caps`); what a check can refuse before anything is written (an edit lock's `409`, Quick Edit's lock message) is refused there.

`admin-ajax.php`'s actions are sent as `wp_die()` sends text in Ajax, or as JSON for `wp_send_json()`'s (`saves::Ajax::Json`), and without a writer the request gets `wp_die()`'s message alone with a `403`. `destroy-sessions` writes no row: it ends HyScribe's own session files (ADR 0011) in the HTTP route. When an action changed the current user's password, the route sets the auth cookies again on the same session, as `wp_update_user()` does. A page an action shows before it writes, as the deletion's confirmation (`users.php` `delete`), is a screen once its checks pass. PHP's mails are not sent (MVP phase 8): the handlers answer as PHP does once they are, and `reference/admin-saves/golden.sh` takes them as sent. The Screen Options form (`set_screen_options()`) is the action `screen-options` on every screen, checked before the screen's own. The heartbeat (`heartbeat`) is an `admin-ajax.php` handler too: it refreshes nonces, reports the posts list's locks and renews the edit lock of the post being edited, writing `_edit_lock` as PHP does; a visitor's heartbeat writes nothing and is answered in both modes. The revisions screen's restore (`revision.php?action=restore`) is a handler of `revision`.

### Page view writes

In primary mode a page view writes what PHP writes while showing it (`admin/views.rs`): `wp_user_settings()` syncs the user's settings with their `wp-settings-{ID}` cookies, a list's `?mode=` saves the view mode (`set_user_setting()`), the dashboard's `?welcome=` saves the welcome panel's state, the Quick Draft form writes its auto-draft when the user has none to reuse, and the Permalinks screen refreshes the `rewrite_rules` option as its `flush_rewrite_rules()` does (`.htaccess` is never written). They run in one action, committed with the page (and `?mode=`'s with the redirect past a list's last page), and the cookies are set as PHP sets them. Shadow mode writes nothing, sets no cookie, and answers `?welcome=` with `501`.

`reference/admin-views/golden.sh` records PHP really writing, request after request, and `page_views_and_screen_state_write_as_php` sends the same requests to hs-admin over `wordpress_saves`, a scratch copy of the default install that `db.sh up` creates and the test loads afresh.

## Site Health

The Info tab (`site-health?tab=debug`) shows `WP_Debug_Data::debug_data()`'s sections that hold for HyScribe in WordPress's markup, for the bundled theme only (another answers `501`): WordPress (without the wordpress.org reachability row, as the server makes no calls), the active theme (without its folder: the theme is built into the binary), media (the maximum upload size, `UPLOAD_MAX_SIZE`) and the database through HyScribe's driver; the drop-ins, must-use and other plugins and inactive themes, of which HyScribe has none, print nothing. In place of the paths and sizes, server, constants and filesystem sections, a HyScribe section shows its version, mode, binary, build target and the `.env` limits. Private fields stay out of the copied text, as in WordPress, and no secret is printed.

The Status tab (`site-health`) is PHP's page: the same frame, text, tabs and script data (`SiteHealth`), byte for byte in the goldens of `reference/site-health/golden.sh`. PHP's browser script sorts the `direct` results the page's data carries into the critical, recommended and good lists; HyScribe sends its own six checks in place of PHP's 22, and no `async` test, so no check calls another site. Each result has WordPress's shape (label, status, badge, description, actions, test). The tab is served for the bundled theme only, as the Info tab is, and answers `501` for any other argument.

| Check (`test`) | Counterpart | States |
| --- | --- | --- |
| `hyscribe_version`: "Your version of HyScribe (x.y.z)" | `wordpress_version`, without its update check | Always good; it names HyScribe's version and the WordPress version it stands in for, and says updating means replacing the binary |
| `sql_server`: "SQL server is up to date" | `get_test_sql_server()`, its labels and words | Good; recommended ("Outdated SQL server") under MySQL 8.0 or MariaDB 10.11; critical ("Severely outdated SQL server") under 5.5 |
| `https_status`: "Your website is using an active HTTPS connection" | `get_test_https_status()`, its labels and words | Good when both addresses are https; else recommended ("Your website does not use HTTPS") |
| `hyscribe_env_permissions`: "The .env file can only be accessed by its owner" | None | Good when no bit of the group or others is set (or no `.env` exists, as in a container); recommended when the group has access or its mode cannot be read; critical when others do |
| `available_updates_disk_space`: "Disk space available to safely perform updates" | `get_test_available_updates_disk_space()`, its labels and words | Good from 100 MB free; recommended below 100 MB or unknown; critical below 20 MB |
| `hyscribe_service`: "HyScribe runs as a service" | None | Good when `run/hyscribe.pid` names this process; else recommended, since `./hyscribe service stop` and `restart` cannot reach it |

Differences from PHP's checks of the same name:

- `sql_server` compares the leading numbers of `SELECT VERSION()` (`11.8.9-MariaDB-ubu2404` is 11.8.9), where PHP's `version_compare()` also weighs a suffix; it has no `db.php` drop-in sentence, as HyScribe has no drop-ins.
- `https_status` knows the two addresses and no connection, so it has no "You are accessing this website using HTTPS, but" variants (`is_ssl()` has nothing to read, as above). It cannot ask whether HTTPS is supported, as that would call the site, so it has no "Update your site to use HTTPS" button and no "Talk to your web host" sentence; it says HyScribe serves plain HTTP and to put a proxy that terminates HTTPS in front of it, with `BEHIND_PROXY` set.
- `available_updates_disk_space` measures the disk the `hyscribe/` folder is on, in place of `WP_CONTENT_DIR`.
- The binary version, the `.env` permissions and the service have no counterpart in WordPress; they use the same markup and the grouping the script gives them.
- HyScribe's `SiteHealth.site_status.issues` counts come from the transient the page's script posts back, as in PHP.

The Plugins screen (`plugins`) is PHP's with no plugin installed, byte for byte in the goldens but for one sentence: its Troubleshooting help names the plugins folder as `hyscribe/plugins/`, relative to the HyScribe folder, where PHP prints the absolute `WP_PLUGIN_DIR`. Arguments and actions answer `501`.

## Testing

- Each screen has a fidelity test against PHP `wp-admin` on the same fixture database: normalized DOM comparison and a screenshot comparison with a small tolerance
- Each action has an integration test for allowed and denied capabilities and for missing or invalid nonces
- Coverage rules from [ADR 0007](adr/0007-full-test-coverage.md) apply

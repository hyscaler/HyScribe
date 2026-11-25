# Switching a site over, and back

HyScribe reads and writes the WordPress database as PHP WordPress does. It never changes the core tables' schema and writes only rows PHP WordPress could have written. So the same database works with either, and switching back is stopping one and starting the other.

Read [not-supported.md](not-supported.md) first: what a site needs and HyScribe answers 501 decides how far you can switch.

## Before you start

- Back up the database and the uploads folder (see [maintenance.md](maintenance.md))
- The site must be WordPress 7.0 or 7.1 (`db_version` 61833), single site, not multisite
- Leave the WordPress folder in place. It is what you switch back to, and its `wp-content/uploads` stays where it is
- Set up mail: HyScribe sends through an SMTP relay (`SMTP_HOST` and the keys beside it, [configuration.md](configuration.md#mail)), never through a local `sendmail`. Without one, comment notices and password reset links stop while it is primary

## Rehearse on a copy

The safest first step is a whole switch over and back on a copy of the site:

1. Load the backup into a second database, and copy the WordPress folder
2. Run HyScribe in primary mode against the copy (steps below), with `WP_HOME` and `WP_SITEURL` set to its own address
3. Do the site's usual work in hs-admin: write and edit posts, moderate comments, change settings
4. Stop HyScribe and serve the copy with PHP WordPress: check the same screens, with `WP_DEBUG` on, and read its `debug.log`

## 1. Shadow mode beside PHP WordPress

Shadow mode is read only. HyScribe opens no writer at all, so it cannot change the database whatever a request asks.

1. Install and run the setup wizard against the site's `wp-config.php`, answering yes to `Start in shadow mode (read only) first?` ([install.md](install.md))
2. Optionally give a database user granted only `SELECT`, so even a fault cannot write
3. Start it on its own address: `./hyscribe service start`. PHP WordPress keeps serving the public site
4. Browse HyScribe on `127.0.0.1:8080` (through an SSH tunnel if remote). If the stored site address sends you to the public site, set `WP_HOME` and `WP_SITEURL` to HyScribe's address in `.env` while trying it
5. Compare pages, feeds and the REST API with the live site

In shadow mode logins are off (the login screen says the site is read only), so hs-admin cannot be used, comment posts and other writes are refused with 403, and no scheduled job runs.

## 2. Primary mode

Primary mode serves the site and writes to the database.

1. Check the gaps once more: themes, plugins, uploads and mail ([not-supported.md](not-supported.md))
2. Set `MODE=primary` in `.env`, remove any `WP_HOME` and `WP_SITEURL` used for trying, and set `BEHIND_PROXY=true` if a proxy is in front
3. `./hyscribe config check`, then `./hyscribe service restart`
4. Read the log (`./hyscribe service log`) for `writes refused:` lines: HyScribe does not write to a table whose engine cannot roll back (MyISAM, for example; the line gives the `ALTER TABLE ... ENGINE=InnoDB` that converts it), nor to a database whose `db_version` it does not support
5. Read it for `cron:` lines too: the scheduled core jobs run in the service unless `DISABLE_WP_CRON` is set or an `object-cache.php` was found
6. Check that `UPLOADS_DIR` names the site's `wp-content/uploads` and that the service user can write it: HyScribe serves `/wp-content/uploads/` from there and writes new images there, where PHP WordPress finds them after a switch back. Then point the reverse proxy at HyScribe
7. Log in at `/hs-login` (old `/wp-login.php` links redirect there)

Users log in again: HyScribe keeps its own sessions under `sessions/`, so a PHP WordPress login cookie does not carry over. Passwords do, through the same hashes and the copied keys and salts.

### PHP WordPress beside it

PHP WordPress may keep running on the same database, for example for WP-CLI or a screen HyScribe answers 501, and the two are as safe together as two PHP servers. Two things to know:

- A persistent object cache (an `object-cache.php` drop-in with Redis or Memcached) is not cleared by HyScribe's writes, so PHP WordPress can show stale data until it expires. Flush it after HyScribe has written, and before switching back
- HyScribe runs only WordPress's core jobs. Plugins' events and the update checks wait in the `cron` option until PHP WordPress runs its cron. To keep them running, have the system's cron call PHP's `wp-cron.php` as before; it takes the same `doing_cron` lock as HyScribe, so the two never run events at once

## Switching back to PHP WordPress

1. `./hyscribe service stop`
2. If the site uses a persistent object cache, flush it
3. Point the reverse proxy back at PHP WordPress, including `/wp-content/uploads/`
4. Users log in again through `wp-login.php`
5. Visit a page or run PHP's cron once, so the events HyScribe left due (plugins' events, update checks) run

Everything HyScribe wrote is in the WordPress tables, in WordPress's format, and the `cron` option keeps every event. Uploaded images are in `UPLOADS_DIR`, the folder PHP WordPress already uses. HyScribe keeps only its login sessions and logs outside the database, in `hyscribe/`, which PHP WordPress does not need. Keep the `hyscribe/` folder until you are sure; to remove HyScribe, delete it (for a site whose uploads live in `hyscribe/uploads/`, move them out first).

## What the switch-back suite guarantees

Every release runs the switch-back suite (`reference/switch-back/run.sh`) on a fresh default WordPress install. The release binary makes 124 changes over HTTP, as an administrator and as visitors, among them:

- The dashboard: a Quick Draft, the welcome panel, box order and screen options
- Posts and pages: Quick Edit, bulk edit, trash, restore, delete, emptying the trash, restoring a revision, the edit lock
- Categories and tags: added, edited, renamed and deleted
- Comments: posted from the front end, moderated every way, replied to, edited and deleted
- Users: added, edited, promoted, given a reset link and deleted with their posts reassigned; the profile and color scheme
- Every Settings screen, two permalink structures and a static front page
- REST writes of posts, pages, navigation menus and synced patterns: created, updated, trashed and deleted
- A restart with `EMPTY_TRASH_DAYS=0`, after which trashing deletes for good

Then PHP WordPress 7.1.3, on the same database with `WP_DEBUG` on, checks:

- Every option and meta value unserializes, and every user loads with a role WordPress knows
- The stored comment and term counts equal the ones WordPress recomputes
- What the options point at (sticky posts, front page, posts page, privacy page, default category) exists
- The rewrite rules HyScribe stored equal the ones WordPress generates, and work
- PHP's REST read of each object HyScribe's REST API wrote equals HyScribe's answer
- 50 admin screens and front end pages, as the administrator, the user HyScribe added and a visitor, answer their status and show their text, with no PHP or database error
- PHP's debug log stays empty

It does not yet cover the REST writes of terms, comments, users and settings, nor anything HyScribe does not do, and it runs on the default install, not on your site's data. Rehearse on a copy, and keep the backup.

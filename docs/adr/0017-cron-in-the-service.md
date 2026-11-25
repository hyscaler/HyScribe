# 0017: WP-Cron in the service

- Status: Accepted (2026-11-16)
- Date: 2026-11-16

## Context

- WordPress keeps its scheduled events in the `cron` option: a serialized array of timestamps, hooks and events, with `version` 2. PHP runs the due ones from `spawn_cron()`, which a page load starts (a loopback request to `wp-cron.php`), or from a system cron that calls `wp-cron.php`
- `wp-cron.php` takes the `doing_cron` transient as a lock, then for each due event: reschedules it when recurring (`wp_reschedule_event()`), unschedules it (`wp_unschedule_event()`), and fires its hook. It stops when another process took the lock, and deletes the lock at the end
- Some core jobs change what visitors see: a scheduled post is only published by its `publish_future_post` event, the trash is only emptied by `wp_scheduled_delete`, expired transients and old auto-drafts stay until their jobs run. A site served by HyScribe alone gets no page loads into PHP, so nothing runs them
- ADR 0012: HyScribe and a PHP WordPress (or WP-CLI) may write the same database at the same time, and must be as safe together as two PHP servers. ADR 0003: HyScribe writes only what PHP would. ADR 0005: one binary running as a service
- The jobs are ported in `hyscribe-write`'s `cron` module, each matched row for row with the write oracle (MVP 8.1); nothing runs them yet

## Decision

Accepted on 2026-11-16 as proposed, with the setup wizard copying both constants from `wp-config.php` and WordPress's scheduling functions kept under their names.

### When events run

- Only in primary mode. Shadow mode has no writer (ADR 0010), so it never runs or changes an event
- A task in the service checks once a minute (`WP_CRON_LOCK_TIMEOUT`) whether any event of a hook HyScribe runs is due, reading only the `cron` option. No page load, loopback request or extra process is needed
- `hyscribe cron run` runs the due events once and exits, for owners who turn the task off and call it from the system's cron, as PHP sites call `wp-cron.php`

### The lock, shared with PHP

- HyScribe takes and gives back the same `doing_cron` transient `wp-cron.php` uses, written as PHP writes it (`_transient_doing_cron`, the time as `sprintf( '%.22F', microtime( true ) )`)
- Before a run: a lock younger than 60 seconds (or more than ten minutes ahead, which `spawn_cron()` treats as none) means another process is running, and HyScribe waits for the next minute. Otherwise HyScribe sets the lock to its own value and commits it
- After each event, HyScribe reads the lock again and stops if it is no longer its own, as `wp-cron.php` does; at the end it deletes the lock only if it is still its own
- So a PHP WordPress on the same database (page loads, a system cron, WP-CLI) and HyScribe never both run the event list at once, with the same gap PHP leaves between two of its own servers (the check and the set are not atomic). No locking beyond PHP's (ADR 0012)
- A persistent object cache on the PHP side (Redis, Memcached) keeps PHP's transients out of the database, so HyScribe cannot see PHP's lock. When the setup wizard or the start finds one (ADR 0012's warning), the service does not run cron, and says so in its log

### Which hooks run

- HyScribe runs these core hooks, each a ported job matched with the write oracle:
  - `publish_future_post`: `check_and_publish_future_post()`, with `wp_publish_post()`, or the event moved to the post's time if early
  - `wp_scheduled_delete`: posts and comments in the trash longer than `EMPTY_TRASH_DAYS`
  - `delete_expired_transients`
  - `wp_scheduled_auto_draft_delete`: `wp_delete_auto_drafts()`
  - `wp_privacy_delete_old_export_files`: export files older than three days under the uploads folder's `wp-personal-data-exports` (the uploads folder of DEPLOYMENT.md), never following a symbolic link
  - `wp_privacy_personal_data_cleanup_requests`: pending privacy requests older than a day marked failed
- Each event is one action (ADR 0012): reschedule, unschedule and the job commit together. A job that fails (a database error, or a row HyScribe does not port, such as an attachment in the trash) rolls back, so its event stays as it was, is logged, and is not retried by the same process until it restarts
- Every other event is left in the `cron` option exactly as it is, never run, rescheduled or dropped, so a PHP WordPress on the database still finds it:
  - The update checks `wp_version_check`, `wp_update_plugins`, `wp_update_themes` and `wp_maybe_auto_update` call wordpress.org; they are not run, as such calls stay out until a decision is made
  - `wp_site_health_scheduled_check`, `do_pings` (outbound HTTP, waiting for the HTTP client), `recovery_mode_clean_expired_keys` (HyScribe has no recovery mode), the upgrade batches (`wp_split_shared_term_batch`, `wp_update_comment_type_batch`), PHP's installer clean ups and `wp_update_user_counts`
  - Plugins' hooks, and any event of a recurrence a plugin adds (`cron_schedules`), which HyScribe cannot reschedule
- HyScribe adds a hook to the list only with its job ported and matched with the write oracle

### Hooks and scheduling functions

- Events stay keyed by hook name, as in the `cron` option. When the extension runtime exists, an extension that registers a handler for a hook gets that hook's events run, with their stored arguments, without any change to the stored `cron` option
- `wp_schedule_event`, `wp_next_scheduled`, `wp_unschedule_event` and the `cron_schedules` filter exist under their names, with WordPress's arguments, writing the option as PHP writes it; until the extension runtime they are plain internal calls (docs/EXTENSIONS.md)

### Configuration (`.env`)

- `DISABLE_WP_CRON`: `true` turns the service's task off (run `hyscribe cron run` from the system's cron instead). Same values as the capability constants (`true`, `false`, `1`, `0`); missing or empty is `false`. The setup wizard copies it from a literal `define()` in `wp-config.php`
- `ALTERNATE_WP_CRON`: accepted and ignored, with a line in the log. It replaces PHP's loopback request with a redirect, and HyScribe makes no such request. The setup wizard copies it from a literal `define()` in `wp-config.php`, as it copies `DISABLE_WP_CRON`
- `EMPTY_TRASH_DAYS` stays WordPress's 30 until it becomes a key of its own

## Consequences

- Scheduled posts publish, the trash empties and expired transients go on a site HyScribe serves alone, at most a minute late, without visitors' page loads
- The `cron` option stays one PHP WordPress reads: HyScribe writes it with PHP's serialization, keys and order, so switching back to PHP, or running both, keeps every event
- Events HyScribe leaves stay due. On a site without PHP they are never run; each recurring one stays a single overdue entry, so the option does not grow. Plugins' cron work waits for the extension runtime (ADR 0002)
- A failing job keeps its event, so it never loses work, but its hook does not run until the cause is fixed and the service restarts
- A site whose PHP side uses a persistent object cache gets no cron from HyScribe, to keep the one at a time guarantee

## Amendment (2026-11-19): the events a page view schedules

- Found by the switch-back suite: PHP's `wp-admin/admin.php` schedules `wp_scheduled_delete` and `delete_expired_transients` (daily, from `time()`) on any request of a logged in user that finds them missing, so a site only HyScribe served never had them, and the cron above, which runs only events that exist, never emptied its trash
- In primary mode, each request of a logged in user to an hs-admin screen (the page, a save or an upload, whatever it answers; not `admin-ajax`, which PHP's `admin-ajax.php` runs without `admin.php`) first schedules, in an action of its own, what is missing of: `wp_privacy_delete_old_export_files` (hourly) and `wp_privacy_personal_data_cleanup_requests` (daily), which PHP's `init` schedules on every request, then `wp_scheduled_delete` and `delete_expired_transients` (daily). The rows match the write oracle's `cron-schedule-events*` actions, and an event already scheduled is left as it is
- As in PHP, where a failed `update_option()` goes on, a database error here is printed and the screen is served; a `cron` option older than version 2, which PHP upgrades, is left as it is. Shadow mode writes nothing
- Not scheduled: `wp_update_user_counts` (`admin_init`) and the update checks, which the cron does not run (they stay for PHP to schedule); `wp_scheduled_auto_draft_delete` is scheduled where PHP does, with the first auto-draft

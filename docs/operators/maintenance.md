# Backups and upgrades

## What to back up

| What | Where | Why |
| --- | --- | --- |
| The database | MySQL or MariaDB | All content, users and settings. HyScribe keeps none of these anywhere else |
| `.env` | `hyscribe/.env` | Database credentials, keys and salts. Losing the keys and salts logs everyone out. Keep the copy as private as the file (mode `0600`) |
| Uploads | the site's `wp-content/uploads` (`UPLOADS_DIR`), or `hyscribe/uploads/` when that is not set | Media files, as for any WordPress site. Scheduled jobs delete personal data exports older than three days from its `wp-personal-data-exports` folder |
| `sessions/` | `hyscribe/sessions/` | Optional: without it users log in again |
| Extensions | `hyscribe/plugins/`, `hyscribe/packages/` and `hyscribe/registry/` | Only when extensions are installed: their folders with their approvals and fault counts, the kept packages, and the registry's verified key list and index. Reinstalling the packages also brings them back |
| `logs/` | `hyscribe/logs/` | Optional |

Not needed: the binary, `LICENSE`, `THIRD-PARTY-NOTICES`, `README.txt`, `.env.example` and `docs/` (all in the archive), and `run/`.

## Database

HyScribe writes each action in one InnoDB transaction, so a consistent dump needs no downtime:

```
mysqldump --single-transaction --default-character-set=utf8mb4 -u USER -p DBNAME > site.sql
```

Use `mariadb-dump` on MariaDB. A dump taken while HyScribe runs in primary mode is as valid for PHP WordPress as one taken under PHP WordPress.

## The `hyscribe/` folder

From the folder holding `hyscribe/`:

```
tar -czf hyscribe-backup.tar.gz hyscribe/.env hyscribe/sessions hyscribe/logs
```

Add `hyscribe/uploads` when the site's uploads live there, and `hyscribe/plugins hyscribe/packages` (with `hyscribe/registry` when it exists) when extensions are installed. Back up an `UPLOADS_DIR` outside `hyscribe/` as you did under PHP WordPress.

## Restore

1. `./hyscribe service stop`
2. Restore the database: `mysql -u USER -p DBNAME < site.sql` (`mariadb` on MariaDB)
3. Restore the files from the folder holding `hyscribe/`: `tar -xzf hyscribe-backup.tar.gz`, and the uploads folder
4. `chmod 600 hyscribe/.env` if the restore changed its mode: HyScribe refuses to start otherwise
5. `./hyscribe config check`, then `./hyscribe service start`

A database restored from a dump taken under PHP WordPress works the same way. Restoring `sessions/` from an older backup brings back sessions that have since been logged out; leave it out to make everyone log in again.

## Upgrades

An upgrade replaces the files the archive ships and keeps everything else (`.env`, `sessions/`, `logs/`, `uploads/`, and the extensions' `plugins/`, `packages/` and `registry/`).

1. Download and verify the new archive ([install.md](install.md))
2. Back up as above
3. From the folder holding `hyscribe/`:

```
hyscribe/hyscribe service stop
tar -xzf hyscribe_<version>_linux_amd64.tar.gz
hyscribe/hyscribe version
hyscribe/hyscribe config check
hyscribe/hyscribe service start
```

4. Read the log (`./hyscribe service log`) for the new version and any `writes refused:` or `cron:` line

HyScribe never alters WordPress tables on start, so there is no migration to run, and rolling back is extracting the old archive the same way.

A built in `hyscribe update` command does not exist yet.

## When WordPress itself changes

- Each HyScribe release supports a fixed set of WordPress database versions (`db_version`), today WordPress 7.0 and 7.1
- If the database is upgraded by a newer PHP WordPress, HyScribe refuses every write until a release that supports it, and logs `writes refused:` at start. Reads go on
- Upgrade HyScribe before or together with PHP WordPress, never after a schema change it does not know

## Scheduled jobs

In primary mode the service runs WordPress's scheduled core jobs once a minute, taking the same `doing_cron` lock PHP WordPress takes, so the two never run them at once. Events of plugins and of the update checks stay in the `cron` option for PHP WordPress. To run them from the system's cron instead, set `DISABLE_WP_CRON=true` and add a line such as:

```
* * * * * /srv/hyscribe/hyscribe cron run
```

Cron does not run in shadow mode, nor when an `object-cache.php` sits in the `wp-content` folder above `UPLOADS_DIR`, since a persistent object cache keeps PHP's lock out of the database. `cron run` refuses to run in either case.

A failing job is logged (`cron: <hook> failed and waits for a restart`), keeps its event, and its hook is not tried again until the service restarts.

To keep plugins' events and the update checks running, call PHP's `wp-cron.php` from the system's cron as well, as before (see [switching.md](switching.md)).

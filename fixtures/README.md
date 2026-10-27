# Fixtures

Generated test data. Never edit by hand; rebuild with the scripts in `reference/`.

## default-install

The first test target ([docs/TESTING.md](../docs/TESTING.md#first-test-target-default-install)): a WordPress install with nothing changed, one folder per pinned WordPress version.

| File | Contents |
| --- | --- |
| `database.sql` | `mysqldump` of the database, one row per `INSERT` |
| `install.log` | WP-CLI output with the WordPress version and `db_version` |

- Built by `reference/default-install/build.sh [version ...]` (needs Docker or Podman and network access)
- Install answers: site title `HyScribe Default Install`, URL `http://default-install.test`, user `admin`, email `admin@default-install.test`, password `reference-only-password`
- The password is published on purpose so login tests can use it; it protects nothing
- Salts live in `wp-config.php`, not in the database, so the dumps hold no secrets

| WordPress | `db_version` |
| --- | --- |
| 7.0.0 | 61833 |
| 7.0.7 | 61833 |
| 7.1.3 | 61833 |

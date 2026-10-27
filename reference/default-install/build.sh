#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Builds the default install fixture: a plain WordPress install with nothing
# changed, dumped to fixtures/default-install/<version>/database.sql.
#
# Usage: reference/default-install/build.sh [wordpress-version ...]
# Needs docker (or podman with DOCKER=podman) and network access.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
MARIADB_IMAGE="${MARIADB_IMAGE:-mariadb:11.8}"
CLI_IMAGE="${CLI_IMAGE:-wordpress:cli-php8.4}"
VERSIONS=("$@")
if [ ${#VERSIONS[@]} -eq 0 ]; then
  VERSIONS=(7.0.7 7.1.3)
fi

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
NAME="hyscribe-ref-$$"
DB_PASSWORD="reference-only"

cleanup() {
  "$DOCKER" rm -f "$NAME-db" >/dev/null 2>&1 || true
  "$DOCKER" network rm "$NAME" >/dev/null 2>&1 || true
}
trap cleanup EXIT

"$DOCKER" network create "$NAME" >/dev/null
"$DOCKER" run -d --name "$NAME-db" --network "$NAME" \
  -e MARIADB_ROOT_PASSWORD="$DB_PASSWORD" "$MARIADB_IMAGE" >/dev/null

until "$DOCKER" exec "$NAME-db" mariadb -uroot -p"$DB_PASSWORD" -e 'SELECT 1' >/dev/null 2>&1; do
  sleep 1
done

for version in "${VERSIONS[@]}"; do
  db="wp_${version//./_}"
  out="$ROOT/fixtures/default-install/$version"
  mkdir -p "$out"
  "$DOCKER" exec "$NAME-db" mariadb -uroot -p"$DB_PASSWORD" -e "DROP DATABASE IF EXISTS $db; CREATE DATABASE $db CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_520_ci"

  # Same answers a person gives wp-admin/install.php on a fresh site.
  "$DOCKER" run --rm --network "$NAME" --user 33:33 -e HOME=/tmp "$CLI_IMAGE" sh -euc "
    cd /tmp
    wp() { php -d memory_limit=1G /usr/local/bin/wp \"\$@\"; }
    wp core download --version=$version --locale=en_US --path=/tmp/wp --quiet
    wp config create --path=/tmp/wp --dbname=$db --dbuser=root --dbpass=$DB_PASSWORD --dbhost=$NAME-db --skip-check --quiet
    wp core install --path=/tmp/wp --url=http://default-install.test --title='HyScribe Default Install' \
      --admin_user=admin --admin_password=reference-only-password --admin_email=admin@default-install.test --skip-email
    wp core version --path=/tmp/wp --extra
    wp option get db_version --path=/tmp/wp
  " | tee "$out/install.log"

  "$DOCKER" exec "$NAME-db" mariadb-dump -uroot -p"$DB_PASSWORD" \
    --skip-dump-date --skip-extended-insert --order-by-primary --hex-blob=false \
    --skip-comments "$db" > "$out/database.sql"
  echo "wrote $out/database.sql"
done

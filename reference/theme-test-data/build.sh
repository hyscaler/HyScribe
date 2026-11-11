#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Builds the Theme Test Data fixture: WordPress's theme test content
# (github.com/WordPress/theme-test-data, formerly the Theme Unit Test)
# imported into a fresh install, with its media. The data has no licence,
# so nothing here is committed: the dump and uploads go to
#   target/fixtures/theme-test-data/<version>/database.sql
#   target/fixtures/theme-test-data/<version>/uploads/
# The data file and the importer plugin are pinned by SHA-256; the
# importer downloads the media from wordpress.com.
#
# Usage: reference/theme-test-data/build.sh [wordpress-version]
# Needs docker (or podman with DOCKER=podman) and network access.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
MARIADB_IMAGE="${MARIADB_IMAGE:-mariadb:11.8}"
CLI_IMAGE="${CLI_IMAGE:-wordpress:cli-php8.4}"
VERSION="${1:-7.1.3}"
case "$VERSION" in
  *[!0-9.]* | "") echo "not a WordPress version: $VERSION" >&2; exit 2 ;;
esac
DATA_COMMIT="b47acf980696897936265182cb684dca648476c7"
DATA_SHA256="457aace6ec93cf77369bbcc6158996e52da8798bd5e39c83d58dfab9b50d64fa"
IMPORTER_VERSION="0.9.7"
IMPORTER_SHA256="d1b4d5c516351a5433fd98a58e1ff965c7822048d63de38840b6c7c506f422a6"

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/target/fixtures/theme-test-data/$VERSION"
NAME="hyscribe-ttd-$$"
DB_PASSWORD="reference-only"
WORK=$(mktemp -d)

cleanup() {
  "$DOCKER" rm -f "$NAME-db" "$NAME-cli" >/dev/null 2>&1 || true
  "$DOCKER" network rm "$NAME" >/dev/null 2>&1 || true
  rm -rf "$WORK"
}
trap cleanup EXIT

fetch() {
  curl -fsSL --proto '=https' --tlsv1.2 -o "$WORK/$1" "$2"
  echo "$3  $WORK/$1" | sha256sum -c --quiet -
}
fetch data.xml "https://raw.githubusercontent.com/WordPress/theme-test-data/$DATA_COMMIT/themeunittestdata.wordpress.xml" "$DATA_SHA256"
fetch importer.zip "https://downloads.wordpress.org/plugin/wordpress-importer.$IMPORTER_VERSION.zip" "$IMPORTER_SHA256"
chmod a+rx "$WORK" && chmod a+r "$WORK"/*

"$DOCKER" network create "$NAME" >/dev/null
"$DOCKER" run -d --name "$NAME-db" --network "$NAME" \
  -e MARIADB_ROOT_PASSWORD="$DB_PASSWORD" "$MARIADB_IMAGE" >/dev/null
until "$DOCKER" exec "$NAME-db" mariadb -uroot -p"$DB_PASSWORD" -e 'SELECT 1' >/dev/null 2>&1; do
  sleep 1
done
"$DOCKER" exec "$NAME-db" mariadb -uroot -p"$DB_PASSWORD" -e "CREATE DATABASE wordpress CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_520_ci"

# The default install's answers, then the import, as a person would run it.
"$DOCKER" run --name "$NAME-cli" --network "$NAME" --user 33:33 -e HOME=/tmp \
  -v "$WORK:/data:ro" "$CLI_IMAGE" sh -euc "
    cd /tmp
    wp() { php -d memory_limit=1G /usr/local/bin/wp \"\$@\"; }
    wp core download --version=$VERSION --locale=en_US --path=/tmp/wp --quiet
    wp config create --path=/tmp/wp --dbname=wordpress --dbuser=root --dbpass=$DB_PASSWORD --dbhost=$NAME-db --skip-check --quiet
    wp core install --path=/tmp/wp --url=http://theme-test-data.test --title='HyScribe Theme Test Data' \
      --admin_user=admin --admin_password=reference-only-password --admin_email=admin@theme-test-data.test --skip-email
    wp plugin install /data/importer.zip --activate --path=/tmp/wp
    wp import /data/data.xml --authors=create --path=/tmp/wp --quiet
    wp plugin deactivate wordpress-importer --uninstall --path=/tmp/wp
    wp core version --path=/tmp/wp --extra
  " | tee "$WORK/install.log"

rm -rf "$OUT"
mkdir -p "$OUT"
cp "$WORK/install.log" "$OUT/"
"$DOCKER" cp "$NAME-cli:/tmp/wp/wp-content/uploads" "$OUT/uploads"
"$DOCKER" exec "$NAME-db" mariadb-dump -uroot -p"$DB_PASSWORD" \
  --skip-dump-date --skip-extended-insert --order-by-primary --hex-blob=false \
  --skip-comments wordpress > "$OUT/database.sql"
echo "wrote $OUT"

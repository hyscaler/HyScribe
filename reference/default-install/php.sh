#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Runs PHP WordPress (the official image) on 127.0.0.1 against the database
# from db.sh, as the oracle for differential tests:
#   reference/default-install/db.sh up
#   reference/default-install/php.sh up      (http://127.0.0.1:8081)
#   reference/default-install/php.sh down
# WP_HOME and WP_SITEURL point at the local address, so WordPress does not
# redirect to the fixture's http://default-install.test. Read only use: the
# differential tests only send GET requests. HYSCRIBE_PHP_DB=wordpress_ttd
# runs it on the Theme Test Data fixture instead, with its uploads.
# HYSCRIBE_PHP_UPLOADS=<folder> serves that folder as the uploads.
# HYSCRIBE_PHP_HOME=<url> sets WP_HOME and WP_SITEURL to that address, and
# HYSCRIBE_PHP_ALIAS=<name> gives the container that network alias, so
# another container of the network (the screenshot browser) reaches the
# site by name. HYSCRIBE_PHP_EXTRA is PHP added to wp-config.php. The themes
# pinned in themes/pinned.tsv that reference/themes/pin.sh unpacked
# are mounted read only into wp-content/themes, and so are the fixture themes
# of reference/themes/fixtures (10.3), which HyScribe embeds only in its tests.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
IMAGE="${WORDPRESS_IMAGE:-wordpress:7.1.3-php8.4-apache}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
PORT="${HYSCRIBE_PHP_PORT:-8081}"
NETWORK="${HYSCRIBE_REF_NETWORK:-hyscribe-ref}"
DB_NAME="${HYSCRIBE_PHP_DB:-wordpress}"
HOME_URL="${HYSCRIBE_PHP_HOME:-http://127.0.0.1:$PORT}"
ALIAS=()
[ -z "${HYSCRIBE_PHP_ALIAS:-}" ] || ALIAS=(--network-alias "$HYSCRIBE_PHP_ALIAS")
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
UPLOADS=()
if [ "$DB_NAME" = wordpress_ttd ]; then
  UPLOADS=(-v "$ROOT/target/fixtures/theme-test-data/${HYSCRIBE_WP_VERSION:-7.1.3}/uploads:/var/www/html/wp-content/uploads:ro")
fi
# HYSCRIBE_PHP_UPLOADS: a folder of uploads to serve, writable (the switch-back suite's).
if [ -n "${HYSCRIBE_PHP_UPLOADS:-}" ]; then
  UPLOADS=(-v "$HYSCRIBE_PHP_UPLOADS:/var/www/html/wp-content/uploads")
fi
for pinned in "$ROOT"/target/pinned-themes/*/; do
  [ -d "$pinned" ] || continue
  UPLOADS+=(-v "${pinned%/}:/var/www/html/wp-content/themes/$(basename "$pinned"):ro")
done

for fixture in "$ROOT"/reference/themes/fixtures/*/; do
  [ -d "$fixture" ] || continue
  UPLOADS+=(-v "${fixture%/}:/var/www/html/wp-content/themes/$(basename "$fixture"):ro")
done

case "${1:-}" in
  up)
    "$DOCKER" rm -f "$NAME" >/dev/null 2>&1 || true
    "$DOCKER" run -d --name "$NAME" --network "$NETWORK" ${ALIAS[@]+"${ALIAS[@]}"} -p "127.0.0.1:$PORT:80" ${UPLOADS[@]+"${UPLOADS[@]}"} \
      -e WORDPRESS_DB_HOST=db -e WORDPRESS_DB_NAME="$DB_NAME" \
      -e WORDPRESS_DB_USER=wordpress -e WORDPRESS_DB_PASSWORD=reference-only \
      -e WORDPRESS_CONFIG_EXTRA="define( 'WP_HOME', '$HOME_URL' ); define( 'WP_SITEURL', '$HOME_URL' ); ${HYSCRIBE_PHP_EXTRA:-}" \
      "$IMAGE" >/dev/null
    until curl -fsS -o /dev/null "http://127.0.0.1:$PORT/"; do
      sleep 1
    done
    echo "PHP WordPress on http://127.0.0.1:$PORT"
    ;;
  down)
    "$DOCKER" rm -f "$NAME" >/dev/null
    ;;
  *)
    echo "usage: $0 up | down" >&2
    exit 2
    ;;
esac

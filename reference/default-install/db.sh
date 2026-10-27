#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Runs MariaDB on 127.0.0.1 with a default install fixture loaded, for
# trying HyScribe against it and for the database tests:
#   reference/default-install/db.sh up [wordpress-version]   (default 7.1.3)
#   reference/default-install/db.sh down
# Then: DB_HOST=127.0.0.1 DB_PORT=33061 DB_NAME=wordpress DB_USER=wordpress
#       DB_PASSWORD=reference-only
# Needs docker (or podman with DOCKER=podman).
set -euo pipefail

DOCKER="${DOCKER:-docker}"
IMAGE="${MARIADB_IMAGE:-mariadb:11.8}"
NAME="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
PORT="${HYSCRIBE_DB_PORT:-33061}"
# php.sh joins this network to reach the database as "db".
NETWORK="${HYSCRIBE_REF_NETWORK:-hyscribe-ref}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

case "${1:-}" in
  up)
    version="${2:-7.1.3}"
    dump="$ROOT/fixtures/default-install/$version/database.sql"
    test -f "$dump" || { echo "no fixture for $version" >&2; exit 1; }
    "$DOCKER" rm -f "$NAME" >/dev/null 2>&1 || true
    "$DOCKER" network inspect "$NETWORK" >/dev/null 2>&1 || "$DOCKER" network create "$NETWORK" >/dev/null
    "$DOCKER" run -d --name "$NAME" --network "$NETWORK" --network-alias db -p "127.0.0.1:$PORT:3306" \
      -e MARIADB_ROOT_PASSWORD=reference-only -e MARIADB_DATABASE=wordpress \
      -e MARIADB_USER=wordpress -e MARIADB_PASSWORD=reference-only "$IMAGE" >/dev/null
    until "$DOCKER" exec "$NAME" mariadb -uroot -preference-only -e 'SELECT 1' >/dev/null 2>&1; do
      sleep 1
    done
    "$DOCKER" exec -i "$NAME" mariadb -uroot -preference-only wordpress < "$dump"
    echo "default install $version on 127.0.0.1:$PORT (database wordpress, user wordpress)"
    ;;
  down)
    "$DOCKER" rm -f "$NAME" >/dev/null
    "$DOCKER" network rm "$NETWORK" >/dev/null 2>&1 || true
    ;;
  *)
    echo "usage: $0 up [wordpress-version] | down" >&2
    exit 2
    ;;
esac

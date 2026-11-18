#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Runs MariaDB on 127.0.0.1 with a default install fixture loaded, for
# trying HyScribe against it and for the database tests:
#   reference/default-install/db.sh up [wordpress-version]   (default 7.1.3)
#   reference/default-install/db.sh down
# Then: DB_HOST=127.0.0.1 DB_PORT=33061 DB_NAME=wordpress DB_USER=wordpress
#       DB_PASSWORD=reference-only
# or DB_HOST=localhost:${TMPDIR:-/tmp}/hyscribe-db-33061/mysqld.sock, its socket.
# The wordpress_terms database holds the same install with terms.sql added,
# wordpress_saves nothing until a test loads the install into it, and
# wordpress_ttd the Theme Test Data fixture when it has been built
# (reference/theme-test-data/build.sh).
# Needs docker (or podman with DOCKER=podman).
set -euo pipefail

DOCKER="${DOCKER:-docker}"
IMAGE="${MARIADB_IMAGE:-mariadb:11.8}"
NAME="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
PORT="${HYSCRIBE_DB_PORT:-33061}"
# php.sh joins this network to reach the database as "db".
NETWORK="${HYSCRIBE_REF_NETWORK:-hyscribe-ref}"
SOCKETS="${TMPDIR:-/tmp}/hyscribe-db-$PORT"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

case "${1:-}" in
  up)
    version="${2:-7.1.3}"
    dump="$ROOT/fixtures/default-install/$version/database.sql"
    test -f "$dump" || { echo "no fixture for $version" >&2; exit 1; }
    "$DOCKER" rm -f "$NAME" >/dev/null 2>&1 || true
    "$DOCKER" network inspect "$NETWORK" >/dev/null 2>&1 || "$DOCKER" network create "$NETWORK" >/dev/null
    # The server's Unix socket, for the tests of a socket DB_HOST.
    mkdir -p "$SOCKETS"
    # A killed container's socket and lock stop the next server; the image's
    # user owns them, so the image removes them.
    if [ -n "$(ls -A "$SOCKETS")" ]; then
      "$DOCKER" run --rm -v "$SOCKETS:/run/mysqld" --entrypoint rm "$IMAGE" -f \
        /run/mysqld/mysqld.sock /run/mysqld/mysqld.sock.lock /run/mysqld/mysqld.pid
    fi
    # The data lives in memory (HYSCRIBE_DB_TMPFS, a size or "off"): every test
    # run reloads the fixture, so nothing is lost, and writing tests no longer
    # wait on the disk.
    data=()
    [ "${HYSCRIBE_DB_TMPFS:-4g}" = off ] || data=(--tmpfs "/var/lib/mysql:rw,size=${HYSCRIBE_DB_TMPFS:-4g}")
    "$DOCKER" run -d --name "$NAME" --network "$NETWORK" --network-alias db -p "127.0.0.1:$PORT:3306" \
      -v "$SOCKETS:/run/mysqld" "${data[@]}" \
      -e MARIADB_ROOT_PASSWORD=reference-only -e MARIADB_DATABASE=wordpress \
      -e MARIADB_USER=wordpress -e MARIADB_PASSWORD=reference-only "$IMAGE" >/dev/null
    # Over TCP: the image's first, initialising server listens on the socket only.
    # Five minutes at most, then the server's log says why it did not start.
    tries=0
    until "$DOCKER" exec "$NAME" mariadb --protocol=tcp -h127.0.0.1 -uroot -preference-only -e 'SELECT 1' >/dev/null 2>&1; do
      tries=$((tries + 1))
      if [ "$tries" -ge 300 ]; then
        "$DOCKER" logs --tail 20 "$NAME" >&2 || true
        echo "$NAME did not start within 300 seconds" >&2
        exit 1
      fi
      sleep 1
    done
    "$DOCKER" exec -i "$NAME" mariadb -uroot -preference-only wordpress < "$dump"
    # A read only user, as DB_READ_USER names one (ADR 0010).
    "$DOCKER" exec "$NAME" mariadb -uroot -preference-only \
      -e "CREATE USER 'wordpress_read'@'%' IDENTIFIED BY 'reference-only'; GRANT SELECT ON wordpress.* TO 'wordpress_read'@'%'"
    "$DOCKER" exec "$NAME" mariadb -uroot -preference-only \
      -e "CREATE DATABASE wordpress_terms; GRANT ALL ON wordpress_terms.* TO 'wordpress'@'%'"
    cat "$dump" "$ROOT/reference/default-install/terms.sql" |
      "$DOCKER" exec -i "$NAME" mariadb -uroot -preference-only wordpress_terms
    # A scratch copy for the tests whose requests really write, which load it afresh.
    "$DOCKER" exec "$NAME" mariadb -uroot -preference-only \
      -e "CREATE DATABASE wordpress_saves; GRANT ALL ON wordpress_saves.* TO 'wordpress'@'%'"
    ttd="$ROOT/target/fixtures/theme-test-data/$version/database.sql"
    if [ -f "$ttd" ]; then
      "$DOCKER" exec "$NAME" mariadb -uroot -preference-only \
        -e "CREATE DATABASE wordpress_ttd; GRANT ALL ON wordpress_ttd.* TO 'wordpress'@'%'"
      "$DOCKER" exec -i "$NAME" mariadb -uroot -preference-only wordpress_ttd < "$ttd"
    fi
    echo "default install $version on 127.0.0.1:$PORT (databases wordpress and wordpress_terms$([ -f "$ttd" ] && echo ' and wordpress_ttd'), user wordpress)"
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

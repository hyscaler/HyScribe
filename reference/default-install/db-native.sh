#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# The default install fixture in a MariaDB installed on this machine, for
# machines without Docker (the macOS and Windows release jobs' smoke tests):
#   reference/default-install/db-native.sh up [wordpress-version]   (default 7.1.3)
#   reference/default-install/db-native.sh down
# Same address and login as db.sh gives: 127.0.0.1, HYSCRIBE_DB_PORT (default
# 33061), database wordpress, user wordpress, password reference-only. Only
# that database: the tests' other databases need db.sh. The server and its
# data live in a folder of the temp folder, removed by down.
# Needs mariadb-install-db, mariadbd and mariadb on PATH (Homebrew's mariadb,
# or on Windows the bin folder of MariaDB's zip, run from Git for Windows's
# bash). Windows has no Unix socket: there root logs in over TCP with the
# fixture's password, which mariadb-install-db sets.
set -euo pipefail

PORT="${HYSCRIBE_DB_PORT:-33061}"
tmp="${TMPDIR:-/tmp}"
DIR="${tmp%/}/hyscribe-db-native-$PORT"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*) windows=1 ;;
  *) windows= ;;
esac
root_sql() {
  if [ -n "$windows" ]; then
    mariadb --no-defaults --protocol=tcp -h127.0.0.1 -P"$PORT" -uroot -preference-only "$@"
  else
    mariadb --no-defaults --socket="$DIR/mysqld.sock" -uroot "$@"
  fi
}

down() {
  if [ -n "$windows" ] && [ -d "$DIR" ]; then
    mariadb-admin --no-defaults --protocol=tcp -h127.0.0.1 -P"$PORT" -uroot -preference-only \
      shutdown > /dev/null 2>&1 || true
    sleep 2
  elif [ -f "$DIR/mysqld.pid" ]; then
    pid="$(cat "$DIR/mysqld.pid")"
    kill "$pid" 2> /dev/null || true
    tries=0
    while kill -0 "$pid" 2> /dev/null && [ "$tries" -lt 30 ]; do
      tries=$((tries + 1))
      sleep 1
    done
  fi
  rm -rf "$DIR"
}

case "${1:-}" in
  up)
    version="${2:-7.1.3}"
    dump="$ROOT/fixtures/default-install/$version/database.sql"
    test -f "$dump" || { echo "no fixture for $version" >&2; exit 1; }
    down
    mkdir -p "$DIR"
    if [ -n "$windows" ]; then
      # Windows's own installer of a data folder, which writes its my.ini.
      data="$(cygpath -w "$DIR/data")"
      mariadb-install-db --datadir="$data" --port="$PORT" --password=reference-only \
        > "$DIR/install.log" 2>&1 || { cat "$DIR/install.log" >&2; exit 1; }
      nohup mariadbd --defaults-file="$data\\my.ini" --bind-address=127.0.0.1 \
        > "$DIR/server.log" 2>&1 &
    else
      mariadb-install-db --no-defaults --datadir="$DIR/data" \
        --auth-root-authentication-method=normal > "$DIR/install.log" 2>&1 ||
        { cat "$DIR/install.log" >&2; exit 1; }
      nohup mariadbd --no-defaults --datadir="$DIR/data" --bind-address=127.0.0.1 --port="$PORT" \
        --socket="$DIR/mysqld.sock" --pid-file="$DIR/mysqld.pid" > "$DIR/server.log" 2>&1 &
    fi
    server=$!
    tries=0
    until root_sql -e 'SELECT 1' > /dev/null 2>&1; do
      tries=$((tries + 1))
      if [ "$tries" -ge 60 ] || ! kill -0 "$server" 2> /dev/null; then
        tail -n 20 "$DIR/server.log" >&2 || true
        echo "MariaDB did not start" >&2
        exit 1
      fi
      sleep 1
    done
    # Both hosts: a local connection matches 'localhost' before '%'.
    root_sql -e "CREATE DATABASE wordpress;
      CREATE USER 'wordpress'@'%' IDENTIFIED BY 'reference-only';
      CREATE USER 'wordpress'@'localhost' IDENTIFIED BY 'reference-only';
      GRANT ALL ON wordpress.* TO 'wordpress'@'%';
      GRANT ALL ON wordpress.* TO 'wordpress'@'localhost'"
    root_sql wordpress < "$dump"
    echo "default install $version on 127.0.0.1:$PORT (database wordpress, user wordpress)"
    ;;
  down)
    down
    ;;
  *)
    echo "usage: $0 up [wordpress-version] | down" >&2
    exit 2
    ;;
esac

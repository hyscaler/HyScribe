#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# The switch-back suite (MVP 8.5, GOALS S7): HyScribe writes, then PHP
# WordPress serves the same database and finds nothing lost or broken.
#   reference/switch-back/run.sh [log folder]      (default target/switch-back)
# 1. A fresh default install (db.sh up) is written by the release binary,
#    the requests of actions.list sent over HTTP with HyScribe's own login
#    (crates/hyscribe/tests/switch_back.rs, an ignored test), which saves
#    the answers of its REST writes in rest.tsv.
# 2. PHP WordPress (php.sh up, on the same database) runs check.php with
#    WordPress's own functions: options and meta unserialize, users load,
#    stored counts match what PHP recomputes, the rewrite rules work, and
#    PHP's REST read of each object HyScribe's REST API wrote matches the
#    answer HyScribe gave.
# 3. It logs in as the administrator and as the user HyScribe added, and
#    asks for every screen and page of pages.list: each must answer its
#    status, show its text and no PHP error or database error.
# WP_DEBUG and WP_DEBUG_LOG are on for the run (set in the container's
# wp-config.php, as WordPress defines WP_DEBUG before plugins load) with
# plugin.php as a must-use plugin; any line in debug.log fails the run.
# The database is reset with a fresh db.sh up after. With
# HYSCRIBE_ORACLE_LOCK set, the PHP part waits for that lock (flock), as
# the oracle container is shared. Takes the same HYSCRIBE_DB_CONTAINER,
# HYSCRIBE_REF_NETWORK and HYSCRIBE_DB_PORT as db.sh and php.sh, and
# HYSCRIBE_TEST_DB_PORT for the test (the same port); HYSCRIBE_PHP_CONTAINER
# and HYSCRIBE_PHP_PORT as php.sh, the port also the site's address that
# HyScribe writes.
# HYSCRIBE_SWITCH_BACK_FIXTURE=ttd runs the suite over the Theme Test Data
# database (wordpress_ttd, built by reference/theme-test-data/build.sh)
# with actions-ttd.list and pages-ttd.list. HyScribe's uploads folder is
# $OUT/uploads (a copy of the fixture's for ttd), which PHP serves from.
# A "%case%" in pages.list is the ID that case's answer gave (ids.tsv), and
# "%ym%" the year and month folder of today's uploads; the text "-" asks for
# no text.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
CARGO="${CARGO:-cargo}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
export HYSCRIBE_PHP_PORT="${HYSCRIBE_PHP_PORT:-8081}"
BASE="http://127.0.0.1:$HYSCRIBE_PHP_PORT"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
HERE="$ROOT/reference/switch-back"
OUT="${1:-$ROOT/target/switch-back}"
export HYSCRIBE_TEST_DB_PORT="${HYSCRIBE_TEST_DB_PORT:-${HYSCRIBE_DB_PORT:-33061}}"
FIXTURE="${HYSCRIBE_SWITCH_BACK_FIXTURE:-default}"
case "$FIXTURE" in
  default) DBNAME=wordpress; SUFFIX="" ;;
  ttd) DBNAME=wordpress_ttd; SUFFIX="-ttd" ;;
  *) echo "HYSCRIBE_SWITCH_BACK_FIXTURE is default or ttd" >&2; exit 2 ;;
esac
export HYSCRIBE_TEST_DB_NAME="$DBNAME" HYSCRIBE_PHP_DB="$DBNAME" HYSCRIBE_DB_CONTAINER="$DB"
export HYSCRIBE_SWITCH_BACK_LIST="$HERE/actions$SUFFIX.list"
WP=/var/www/html
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
export HYSCRIBE_SWITCH_BACK_OUT="$OUT"
rm -rf "$OUT"/*.log "$OUT"/*.html "$OUT"/*.jar "$OUT"/rest.tsv "$OUT"/ids.tsv "$OUT/uploads"
mkdir -p "$OUT/uploads"
if [ "$FIXTURE" = ttd ]; then
  cp -r "$ROOT/target/fixtures/theme-test-data/${HYSCRIBE_WP_VERSION:-7.1.3}/uploads/." "$OUT/uploads/"
fi
# PHP reads the uploads HyScribe wrote: its container mounts the folder.
chmod -R a+rX "$OUT/uploads"
export HYSCRIBE_PHP_UPLOADS="$OUT/uploads"

"$ROOT/reference/default-install/db.sh" up
reset() { "$ROOT/reference/default-install/db.sh" down; "$ROOT/reference/default-install/db.sh" up; }
trap reset EXIT

echo "HyScribe writes (log: $OUT/hyscribe.log)"
if ! "$CARGO" test -p hyscribe --test switch_back -- --ignored --nocapture > "$OUT/hyscribe.log" 2>&1; then
  tail -n 40 "$OUT/hyscribe.log" >&2
  echo "HyScribe's side failed" >&2
  exit 1
fi
# The rules as HyScribe left them, which PHP's first request regenerates if empty.
"$DOCKER" exec "$DB" mariadb -uroot -preference-only -N -B -r "$DBNAME" \
  -e "SELECT option_value FROM wp_options WHERE option_name = 'rewrite_rules'" | head -c -1 > "$OUT/rewrite_rules.txt"
"$DOCKER" exec "$DB" mariadb-dump -uroot -preference-only "$DBNAME" > "$OUT/hyscribe.sql"
chmod -R a+rX "$OUT/uploads"

# PHP WordPress up on the database, checked, and down again.
php_side() {
  local status=0
  "$ROOT/reference/default-install/php.sh" up || status=1
  [ "$status" = 1 ] || php_checks || status=1
  "$ROOT/reference/default-install/php.sh" down
  return "$status"
}

# Called where errexit is off, so each step's failure is returned.
php_checks() {
  local failed=0
  "$DOCKER" exec "$NAME" sed -i \
    "s/^define( 'WP_DEBUG', .*/define( 'WP_DEBUG', true ); define( 'WP_DEBUG_LOG', true ); define( 'WP_DEBUG_DISPLAY', false );/" \
    "$WP/wp-config.php" || return 1
  "$DOCKER" exec "$NAME" grep -q "define( 'WP_DEBUG_LOG', true )" "$WP/wp-config.php" || return 1
  "$DOCKER" exec -i "$NAME" sh -c "mkdir -p $WP/wp-content/mu-plugins && cat > $WP/wp-content/mu-plugins/hyscribe-switch-back.php" < "$HERE/plugin.php" || return 1
  "$DOCKER" exec "$NAME" rm -f "$WP/wp-content/debug.log" || return 1
  "$DOCKER" exec -i "$NAME" sh -c 'cat > /tmp/hyscribe-rewrite-rules' < "$OUT/rewrite_rules.txt" || return 1
  "$DOCKER" exec -i "$NAME" sh -c 'cat > /tmp/hyscribe-rest.tsv' < "$OUT/rest.tsv" || return 1

  echo "WordPress's checks (log: $OUT/check.log)"
  "$DOCKER" exec -i -e HYSCRIBE_SB_FIXTURE="$FIXTURE" "$NAME" php < "$HERE/check.php" > "$OUT/check.log" 2>&1 || failed=1
  grep '^FAIL' "$OUT/check.log" >&2 || true

  # Logins through wp-login.php, the second with the password HyScribe hashed.
  log_in() {
    local status
    status=$(curl -s -o /dev/null -w '%{http_code}' -c "$OUT/$1.jar" -b 'wordpress_test_cookie=WP%20Cookie%20check' \
      --data-urlencode "log=$1" --data-urlencode "pwd=$2" -d testcookie=1 "$BASE/wp-login.php")
    [ "$status" = 302 ] || { echo "FAIL login as $1: $status" >&2; return 1; }
  }
  log_in admin reference-only-password || failed=1
  [ "$FIXTURE" != default ] || log_in switchback 'Switch back 1!' || failed=1

  echo "Screens and pages (pages in $OUT)"
  local n=0 user path status text jar got id ym
  ym=$(date +%Y/%m)
  while IFS=$'\t' read -r user path status text; do
    case "$user" in '#'*|'') continue ;; esac
    n=$((n + 1))
    path="${path//\%ym\%/$ym}"
    text="${text//\%ym\%/$ym}"
    while IFS=$'\t' read -r case_name id; do
      path="${path//\%$case_name\%/$id}"
      text="${text//\%$case_name\%/$id}"
    done < "$OUT/ids.tsv"
    jar=()
    [ "$user" = visitor ] || jar=(-b "$OUT/$user.jar")
    got=$(curl -g -s -o "$OUT/page-$n.html" -w '%{http_code}' "${jar[@]}" "$BASE$path")
    if [ "$got" != "$status" ]; then
      echo "FAIL $user $path: $got, not $status (page-$n.html)" >&2; failed=1
    fi
    if [ "$text" != - ] && ! grep -aqF -- "$text" "$OUT/page-$n.html"; then
      echo "FAIL $user $path does not show: $text (page-$n.html)" >&2; failed=1
    fi
    if grep -qiE 'database error|Fatal error|<b>(Warning|Notice|Deprecated)</b>' "$OUT/page-$n.html"; then
      echo "FAIL $user $path shows an error (page-$n.html)" >&2; failed=1
    fi
  done < "$HERE/pages$SUFFIX.list"
  echo "$n pages"

  "$DOCKER" exec "$NAME" sh -c "cat $WP/wp-content/debug.log 2>/dev/null || true" > "$OUT/debug.log"
  if [ -s "$OUT/debug.log" ]; then
    echo "FAIL PHP's debug log ($OUT/debug.log):" >&2
    head -n 20 "$OUT/debug.log" >&2
    failed=1
  fi
  return "$failed"
}

status=0
if [ -n "${HYSCRIBE_ORACLE_LOCK:-}" ]; then
  ( flock 9; php_side ) 9> "$HYSCRIBE_ORACLE_LOCK" || status=1
else
  php_side || status=1
fi
if [ "$status" = 0 ]; then echo "switch back: PHP WordPress reads everything HyScribe wrote"; else echo "switch back: FAILED" >&2; fi
exit "$status"

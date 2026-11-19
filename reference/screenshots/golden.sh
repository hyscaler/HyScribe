#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# The screenshot comparison (GOALS S8, an open question): every screen of screens.list,
# as each role, rendered by a headless Chromium (Playwright's image, pinned
# by digest, see Dockerfile) from PHP WordPress and from HyScribe over the same fixture
# database, then compared pixel by pixel (compare.py states the tolerance).
#   reference/screenshots/golden.sh [report folder]   (default target/screenshots)
# Both servers are containers of one network and answer to http://wp.test, so
# every URL on the page is the same on both sides; only the prefix differs
# (/wp-admin/<file>.php, /hs-admin/<file>, ADR 0008). Each side starts from
# a fresh db.sh up with seed.sql added (page views write), PHP first; the
# database is reset after. The report folder gets php/ and hyscribe/ (the
# pictures), report.tsv, report.html and diff-<screen>.png for each screen
# that differs. Exit 0 when every screen matches, 1 when one differs.
# HYSCRIBE_BIN is the release binary to serve (default target/release/hyscribe,
# built with cargo when absent; it runs in the Playwright image, Ubuntu 24.04,
# so a musl build runs as well). Takes the same HYSCRIBE_DB_CONTAINER,
# HYSCRIBE_REF_NETWORK and HYSCRIBE_DB_PORT as db.sh. With
# HYSCRIBE_ORACLE_LOCK set it waits for that lock (flock), as the oracle's
# containers are shared. Needs docker, and network to pull the images once.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
HERE="$ROOT/reference/screenshots"
if [ -n "${HYSCRIBE_ORACLE_LOCK:-}" ] && [ -z "${HYSCRIBE_SHOTS_LOCKED:-}" ]; then
  export HYSCRIBE_SHOTS_LOCKED=1
  exec flock "$HYSCRIBE_ORACLE_LOCK" "$0" "$@"
fi
OUT="${1:-$ROOT/target/screenshots}"
IMAGE="${HYSCRIBE_SHOTS_IMAGE:-hyscribe-shots:1.49.1}"
BIN="${HYSCRIBE_BIN:-$ROOT/target/release/hyscribe}"
CARGO="${CARGO:-cargo}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
NETWORK="${HYSCRIBE_REF_NETWORK:-hyscribe-ref}"
PORT="${HYSCRIBE_DB_PORT:-33061}"
PHP_NAME="hs-shots-php-$PORT"
HS_NAME="hs-shots-hyscribe-$PORT"
SITE=wp.test
URL="http://$SITE"
DB_SH="$ROOT/reference/default-install/db.sh"

mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
# HYSCRIBE_SHOTS_KEEP_PHP=1 reuses the PHP pictures of an earlier run, for fixing HyScribe.
KEEP_PHP=
if [ -n "${HYSCRIBE_SHOTS_KEEP_PHP:-}" ] && [ -n "$(ls -A "$OUT/php" 2>/dev/null)" ]; then KEEP_PHP=1; fi
[ -n "$KEEP_PHP" ] || rm -rf "$OUT/php"
rm -rf "$OUT/hyscribe" "$OUT"/diff-*.png "$OUT"/report.* "$OUT/hyscribe-home"
mkdir -p "$OUT/php" "$OUT/hyscribe"
"$DOCKER" image inspect "$IMAGE" >/dev/null 2>&1 || "$DOCKER" build -t "$IMAGE" "$HERE"
if [ ! -x "$BIN" ]; then
  (cd "$ROOT" && "$CARGO" build --release -p hyscribe)
fi

fresh() {
  "$DB_SH" down >/dev/null 2>&1 || true
  "$DB_SH" up >/dev/null
  "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress < "$HERE/seed.sql"
}
cleanup() {
  "$DOCKER" rm -f "$PHP_NAME" "$HS_NAME" >/dev/null 2>&1 || true
  "$DB_SH" down >/dev/null 2>&1 || true
  "$DB_SH" up >/dev/null
}
trap cleanup EXIT

# Rootless Docker maps the container's root to the caller, so the browser
# runs as root there and writes files the caller owns; rootful Docker runs it as the caller.
AS_USER=(--user "$(id -u):$(id -g)")
if "$DOCKER" info --format '{{.SecurityOptions}}' 2>/dev/null | grep -q rootless; then AS_USER=(); fi

# The browser's container: a script of this folder, the output folder, the network.
browser() {
  "$DOCKER" run --rm --init --ipc=host --network "$NETWORK" ${AS_USER[@]+"${AS_USER[@]}"} -e HOME=/tmp \
    -v "$HERE:/shots:ro" -v "$OUT:/out" "$IMAGE" python3 "$@"
}

wait_for() {
  local tries=0
  until "$DOCKER" run --rm --network "$NETWORK" "$IMAGE" python3 -c \
    "import urllib.request as u; u.urlopen('$URL/', timeout=5)" >/dev/null 2>&1; do
    tries=$((tries + 1))
    [ "$tries" -lt 60 ] || { echo "$1 did not answer at $URL" >&2; exit 1; }
    sleep 1
  done
}

if [ -z "$KEEP_PHP" ]; then
  echo "PHP WordPress"
  fresh
  HYSCRIBE_PHP_CONTAINER="$PHP_NAME" HYSCRIBE_PHP_PORT="$((PORT + 1))" HYSCRIBE_PHP_HOME="$URL" HYSCRIBE_PHP_ALIAS="$SITE" \
    HYSCRIBE_PHP_EXTRA="define( 'DISABLE_WP_CRON', true ); define( 'AUTOMATIC_UPDATER_DISABLED', true ); define( 'WP_HTTP_BLOCK_EXTERNAL', true ); define( 'DISALLOW_FILE_MODS', true ); define( 'DISALLOW_UNFILTERED_HTML', true );" \
    "$ROOT/reference/default-install/php.sh" up >/dev/null
  MU=/var/www/html/wp-content/mu-plugins
  "$DOCKER" exec -i "$PHP_NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-screenshots.php" < "$HERE/mu-plugin.php"
  browser /shots/capture.py php "$URL" /shots/screens.list /out/php
  "$DOCKER" rm -f "$PHP_NAME" >/dev/null
fi

echo "HyScribe"
fresh
HOME_DIR="$OUT/hyscribe-home/hyscribe"
mkdir -p "$HOME_DIR"
cp "$BIN" "$HOME_DIR/hyscribe"
{
  printf 'DB_HOST=db\nDB_PORT=3306\nDB_NAME=wordpress\nDB_USER=wordpress\nDB_PASSWORD=reference-only\n'
  printf 'BIND_HOST=0.0.0.0\nPORT=80\nMODE=primary\nWP_HOME=%s\nWP_SITEURL=%s\n' "$URL" "$URL"
  for key in AUTH_KEY SECURE_AUTH_KEY LOGGED_IN_KEY NONCE_KEY AUTH_SALT SECURE_AUTH_SALT LOGGED_IN_SALT NONCE_SALT; do
    printf '%s=screenshots-%s\n' "$key" "$key"
  done
} > "$HOME_DIR/.env"
chmod 600 "$HOME_DIR/.env"
# HyScribe refuses root: the container copies the folder, gives it to user 1000 and drops to it.
"$DOCKER" run -d --name "$HS_NAME" --network "$NETWORK" --network-alias "$SITE" \
  --sysctl net.ipv4.ip_unprivileged_port_start=0 -v "$HOME_DIR:/seed:ro" "$IMAGE" sh -c \
  'cp -r /seed /run/hyscribe && chown -R 1000:1000 /run/hyscribe && cd /run/hyscribe && exec setpriv --reuid=1000 --regid=1000 --clear-groups ./hyscribe start' >/dev/null
wait_for HyScribe
browser /shots/capture.py hyscribe "$URL" /shots/screens.list /out/hyscribe
"$DOCKER" logs "$HS_NAME" > "$OUT/hyscribe.log" 2>&1 || true
"$DOCKER" exec "$HS_NAME" sh -c 'cat /run/hyscribe/logs/hyscribe.log 2>/dev/null' >> "$OUT/hyscribe.log" || true
"$DOCKER" rm -f "$HS_NAME" >/dev/null
rm -rf "$OUT/hyscribe-home"

echo "Comparison (report: $OUT/report.html)"
status=0
browser /shots/compare.py /out/php /out/hyscribe /out /shots/expected.list || status=$?
exit "$status"

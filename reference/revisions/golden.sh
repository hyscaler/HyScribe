#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's revisions screen (revision.php) for each role,
# over reference/revisions/content.sql, which it adds and deletes after, as
# crates/hyscribe-admin/tests/golden/revisions.tsv, one row each:
#   screen?query<TAB>role<TAB>hex(page)
# a redirect kept as 302<TAB>its Location and a refusal as 403<TAB>page.
# Nonces are masked as reference/media/golden.sh masks them, and the time
# since each revision as %time_ago%. The last row is a restore, which
# writes (reference/writes/golden.sh admin-revision-restore has its rows).
# Start from a fresh db.sh up, then php.sh up, and take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
MU=/var/www/html/wp-content/mu-plugins
OUT="$ROOT/crates/hyscribe-admin/tests/golden/revisions.tsv"
TMP=$(mktemp)
NONCES=$(mktemp)
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress; }
cleanup() {
  rm -f "$TMP" "$TMP.head" "$NONCES"
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php"
  echo "DELETE FROM wp_postmeta WHERE post_id IN (SELECT ID FROM wp_posts WHERE ID BETWEEN 1301 AND 1335 OR post_parent BETWEEN 1301 AND 1335);
    DELETE FROM wp_posts WHERE post_parent BETWEEN 1301 AND 1335;
    DELETE FROM wp_users WHERE ID = 2; DELETE FROM wp_usermeta WHERE umeta_id BETWEEN 1301 AND 1303;
    DELETE FROM wp_posts WHERE ID BETWEEN 1301 AND 1335;" | sql
}
trap cleanup EXIT
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
sql < "$ROOT/reference/revisions/content.sql"
actions="log-out heartbeat-nonce wp_rest update_can_compress_scripts revisions-ajax-nonce"
for id in $(seq 1301 1335); do
  actions="$actions restore-post_$id"
done
# shellcheck disable=SC2086
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 1 ); foreach ( array_slice( $argv, 1 ) as $action ) { echo wp_create_nonce( $action ), "\t", $action, "\n"; }' \
  $actions > "$NONCES"
record() {
  status=$(curl -s -o "$TMP" -D "$TMP.head" -w '%{http_code}' -b "hyscribe_role=$2" "$BASE/wp-admin/$1")
  case "$status" in
    200) ;;
    302) location=$(grep -i '^location:' "$TMP.head" | cut -d' ' -f2- | tr -d '\r') && printf '302\t%s' "$location" > "$TMP" ;;
    403) { printf '%s\t' "$status"; cat "$TMP"; } > "$TMP.die" && mv "$TMP.die" "$TMP" ;;
    *) echo "PHP answered $status for $1 as $2" >&2; exit 1 ;;
  esac
  sed -i -E 's/"timeAgo":"[^"]*"/"timeAgo":"%time_ago%"/g' "$TMP"
  printf '%s\t%s\t%s\n' "$1" "$2" "$(php "$ROOT/reference/media/mask.php" "$TMP" "$NONCES")" >> "$OUT"
}
: > "$OUT"
for id in 1302 1303 1304 1305 1307 1308 1310 1312 1313 1315 1316 1318 1320 1321; do
  record "revision.php?revision=$id" administrator
done
for query in 'from=1302&to=1304' 'to=1303' 'revision=1304&from=1302' 'from=1318&to=1304' 'from=1304&to=1302' \
  'action=view&revision=1303' 'action=edit&revision=1305' 'revision=1303&from=abc' 'revision=1303abc' 'revision=1301' \
  'revision=9999' '' 'revision=abc' 'revision=1303&action=nope' 'from=1303' 'revision=0&to=1304' \
  'action=restore&revision=1303' 'action=restore&revision=1321' 'action=restore&revision=9999' \
  'action=restore&revision=1301' 'action=restore&to=1303' 'revision=1322' 'revision=1324' 'revision=1326' \
  'revision=1328' 'revision=1331' 'revision=1335' 'revision[]=1303' 'action=restore&revision=1322' \
  'action=restore&revision=1324'; do
  record "revision.php?$query" administrator
done
for role in editor author contributor subscriber; do
  for id in 1303 1307 1318; do
    record "revision.php?revision=$id" "$role"
  done
  record 'revision.php?action=restore&revision=1303' "$role"
done
# A restore past its checks, which writes: the redirect alone is kept, its
# nonce masked in the row's query too.
nonce=$(grep -P '\trestore-post_1303$' "$NONCES" | cut -f1)
record "revision.php?action=restore&revision=1303&_wpnonce=$nonce" administrator
sed -i "s/_wpnonce=$nonce\t/_wpnonce=%nonce%\t/" "$OUT"
wc -l "$OUT"

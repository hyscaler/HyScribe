#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's admin-ajax.php heartbeat answers over
# reference/revisions/content.sql (which it adds and deletes after), each
# screen's heartbeat data sent by user 1 in a role (noavatars: the
# administrator without avatars), and two by a visitor,
# as crates/hyscribe-http/tests/golden/heartbeat.tsv, one row each:
#   role (- for a visitor)<TAB>body<TAB>status<TAB>answer
# The body's nonces are named %nonce% (this tick's heartbeat nonce),
# %old_nonce% (the tick before's) or bad; in the answer nonces are masked as
# reference/media/golden.sh masks them, and the time as %time%. The lock
# refresh of post 1309 writes its _edit_lock. Start from a fresh db.sh up,
# then php.sh up, and take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
MU=/var/www/html/wp-content/mu-plugins
OUT="$ROOT/crates/hyscribe-http/tests/golden/heartbeat.tsv"
TMP=$(mktemp)
NONCES=$(mktemp)
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress; }
cleanup() {
  rm -f "$TMP" "$NONCES"
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php"
  echo "DELETE FROM wp_postmeta WHERE post_id BETWEEN 1301 AND 1335;
    DELETE FROM wp_users WHERE ID = 2; DELETE FROM wp_usermeta WHERE umeta_id BETWEEN 1301 AND 1303;
    DELETE FROM wp_posts WHERE ID BETWEEN 1301 AND 1335;" | sql
}
trap cleanup EXIT
sql < "$ROOT/reference/revisions/content.sql"
actions="heartbeat-nonce wp_rest getpermalink samplepermalink closedpostboxes internal-linking meta-box-loader
  update-post_1301 update-post_1319"
# shellcheck disable=SC2086
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 1 ); foreach ( array_slice( $argv, 1 ) as $action ) { echo wp_create_nonce( $action ), "\t", $action, "\n"; }' \
  $actions > "$NONCES"
nonce=$(grep -P '\theartbeat-nonce$' "$NONCES" | cut -f1)
old=$("$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; echo substr( wp_hash( ( wp_nonce_tick() - 1 ) . "|heartbeat-nonce|1|", "nonce" ), -12, 10 );')
record() {
  body=${2//%nonce%/$nonce}
  body=${body//%old_nonce%/$old}
  role=$1
  [ "$role" = noavatars ] && role=administrator
  status=$(curl -s -o "$TMP" -w '%{http_code}' -b "hyscribe_role=$role" --data "$body" "$BASE/wp-admin/admin-ajax.php")
  sed -i -E 's/"server_time":[0-9]+/"server_time":%time%/; s/"new_lock":"[0-9]+:/"new_lock":"%time%:/' "$TMP"
  printf '%s\t%s\t%s\t%s\n' "$1" "$2" "$status" "$(php -r '$page = file_get_contents( $argv[1] ); foreach ( file( $argv[2], FILE_IGNORE_NEW_LINES ) as $line ) { list( $n, $a ) = explode( "\t", $line ); $page = str_replace( $n, "%nonce:$a%", $page ); } echo $page;' "$TMP" "$NONCES")" >> "$OUT"
}
: > "$OUT"
# A visitor, before user 1 is logged in by the role plugin.
record - 'action=heartbeat'
record - 'action=heartbeat&screen_id=front&data[wp-refresh-post-lock][post_id]=1301'
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
lock='data[wp-refresh-post-lock][post_id]'
nonces='data[wp-refresh-post-nonces][post_id]'
loader='data[wp-refresh-metabox-loader-nonces][post_id]'
locked='data[wp-check-locked-posts][]'
for body in 'action=heartbeat' 'action=heartbeat&_nonce=' 'action=heartbeat&_nonce=0' \
  "action=heartbeat&_nonce=bad&screen_id=post&$nonces=1301&$loader=1301" 'action=heartbeat&_nonce=bad' \
  "action=heartbeat&_nonce=%old_nonce%&screen_id=post&$nonces=1301&$lock=1319" \
  "action=heartbeat&_nonce=%old_nonce%&$nonces=9999&$loader=-5" "action=heartbeat&_nonce=%old_nonce%&$nonces=0&$loader=0" \
  "action=heartbeat&_nonce=%old_nonce%&data[wp-refresh-post-nonces]=&data[wp-refresh-metabox-loader-nonces]=" \
  "action=heartbeat&_nonce=%nonce%&screen_id=edit-post&$locked=post-1319&$locked=post-1301&$locked=post-1&$locked=pos&$locked=post-0" \
  "action=heartbeat&_nonce=%nonce%&screen_id=post&$lock=1319" "action=heartbeat&_nonce=%nonce%&screen_id=post&$lock=1309" \
  "action=heartbeat&_nonce=%nonce%&$lock=0" "action=heartbeat&_nonce=%nonce%&$lock=9999" \
  "action=heartbeat&_nonce=%nonce%&data[wp-refresh-post-lock][other]=1" "action=heartbeat&_nonce=%nonce%&data[wp-check-locked-posts]=post-1319" \
  'action=heartbeat&_nonce=%nonce%&data=x' 'action=heartbeat&_nonce=%nonce%&data=' 'action=heartbeat&_nonce=%nonce%&screen_id=dashboard' \
  "action=heartbeat&_nonce=%nonce%&data[wp-check-locked-posts][0][]=post-1319" \
  "action=heartbeat&_nonce=%nonce%&data[wp-refresh-post-lock]=1319"; do
  record administrator "$body"
done
for role in author contributor; do
  record "$role" "action=heartbeat&_nonce=%nonce%&screen_id=edit-post&$locked=post-1319&$lock=1301&$nonces=1301"
done
# Without avatars (the noavatars rows), as the Discussion settings turn them off.
echo "UPDATE wp_options SET option_value = '' WHERE option_name = 'show_avatars';" | sql
record noavatars "action=heartbeat&_nonce=%nonce%&screen_id=edit-post&$locked=post-1319&$lock=1319"
echo "UPDATE wp_options SET option_value = '1' WHERE option_name = 'show_avatars';" | sql
wc -l "$OUT"

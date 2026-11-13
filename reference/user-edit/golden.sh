#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's Edit User and Add User screens over the users of
# reference/users-list/content.sql, which it adds and deletes after, with
# filters from reference/user-edit/variants.php, as
# crates/hyscribe-admin/tests/golden/user-edit.tsv, the generated password
# masked as %password%:
#   variant<TAB>file?query<TAB>role<TAB>hex(page)
# A refusal is kept as 403<TAB>page or 500<TAB>page, as wp_die() answers.
# Masks as reference/admin-page/golden.sh. Start from a fresh db.sh up, then
# php.sh up, and take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-admin/tests/golden/user-edit.tsv"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp)
NONCES=$(mktemp)
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress; }
cleanup() {
  rm -f "$TMP" "$TMP.head" "$NONCES"
  echo "DELETE FROM wp_users WHERE ID BETWEEN 601 AND 607; DELETE FROM wp_usermeta WHERE umeta_id BETWEEN 601 AND 629;
    DELETE FROM wp_posts WHERE ID BETWEEN 601 AND 604;" | sql
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php" "$MU/hyscribe-variants.php"
}
trap cleanup EXIT
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-variants.php" < "$ROOT/reference/user-edit/variants.php"
actions="log-out heartbeat-nonce wp_rest update_can_compress_scripts save-color-scheme create-user"
for id in 0 1 601 602 603 604 605 606 607 -601; do
  actions="$actions update-user_$id reset-password-for-$id"
done
# shellcheck disable=SC2086
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 1 ); foreach ( array_slice( $argv, 1 ) as $action ) { echo wp_create_nonce( $action ), "\t", $action, "\n"; }' \
  $actions > "$NONCES"
record() {
  : > "$TMP"
  status=$(curl -s -o "$TMP" -D "$TMP.head" -w '%{http_code}' -b "hyscribe_role=$2; hyscribe_variant=$1" "$BASE/wp-admin/$3")
  case "$status" in
    200) ;;
    302) printf '302\t%s' "$(grep -i '^location:' "$TMP.head" | cut -d' ' -f2 | tr -d '\r')" > "$TMP" ;;
    403 | 500) { printf '%s\t' "$status"; cat "$TMP"; } > "$TMP.die" && mv "$TMP.die" "$TMP" ;;
    *) echo "PHP answered $status for $3" >&2; exit 1 ;;
  esac
  page=$(php -r '
    $p = file_get_contents( $argv[1] );
    foreach ( file( $argv[2], FILE_IGNORE_NEW_LINES ) as $line ) {
      list( $nonce, $action ) = explode( "\t", $line );
      $p = str_replace( $nonce, "%nonce:$action%", $p );
    }
    $p = preg_replace( "/\"time\":\"[0-9]+\"/", "\"time\":\"%time%\"", $p );
    $p = preg_replace( "/data-pw=\"[^\"]*\"/", "data-pw=\"%password%\"", $p );
    echo bin2hex( $p );' "$TMP" "$NONCES")
  printf '%s\t%s\t%s\t%s\n' "$1" "$3" "$2" "$page" >> "$OUT"
}
: > "$OUT"
sql < "$ROOT/reference/users-list/content.sql"
for role in administrator editor author contributor subscriber; do
  for id in 601 602 603 604 605 606 607; do
    record u3 "$role" "user-edit.php?user_id=$id"
  done
  record u3 "$role" "user-edit.php?user_id=1"
  record u3 "$role" user-new.php
done
for query in '' '?user_id=999' '?user_id=0' '?user_id=x' '?user_id=-601' '?user_id=1x' '?user_id=%20601'; do
  record u3 administrator "user-edit.php$query"
done
for query in 'user_id=601&wp_http_referer=%2Fwp-admin%2Fusers.php%3Frole%3Deditor%26update%3Dx%26user_id%3D3%26paged%3D2' \
  'wp_http_referer=http%3A%2F%2Fexample.test%2Fa%20b%22%3C%27&user_id=602' 'user_id=601&wp_http_referer=javascript%3Aalert(1)' \
  'user_id=601&wp_http_referer=' 'user_id=601&wp_http_referer=0' 'user_id=1&wp_http_referer=%2Fwp-admin%2Fusers.php'; do
  record u3 administrator "user-edit.php?$query"
done
for variant in u3options u3sessions u3caps u3noavatars; do
  record "$variant" administrator "user-edit.php?user_id=601"
done
for id in 604 606; do
  record u3roles administrator "user-edit.php?user_id=$id"
  record u3roles editor "user-edit.php?user_id=$id"
done
record u3default administrator user-new.php
for variant in u3nocreate u3nopromote; do
  record "$variant" administrator "user-edit.php?user_id=601"
  record "$variant" administrator user-new.php
done
wc -l "$OUT"

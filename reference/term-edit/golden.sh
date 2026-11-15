#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's term edit screen, term.php, over the default
# install, then over reference/terms-list/content.sql, which it adds and
# deletes after, as crates/hyscribe-admin/tests/golden/term-edit.tsv:
#   variant?query<TAB>role<TAB>hex(page)
# A refusal is kept as <status><TAB>page and a redirect as 302<TAB>location.
# Masks as reference/terms-list/golden.sh. Start from a fresh db.sh up, then
# php.sh up, and take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-admin/tests/golden/term-edit.tsv"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp)
NONCES=$(mktemp)
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress; }
cleanup() {
  rm -f "$TMP" "$TMP.head" "$NONCES"
  echo "DELETE FROM wp_terms WHERE term_id BETWEEN 401 AND 416;
    DELETE FROM wp_term_taxonomy WHERE term_taxonomy_id BETWEEN 401 AND 416;
    DELETE FROM wp_options WHERE option_name = 'category_children';" | sql
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php"
}
trap cleanup EXIT
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
actions="log-out heartbeat-nonce wp_rest update_can_compress_scripts"
for id in 1 402 405 406 407 412 416; do
  actions="$actions update-tag_$id delete-tag_$id"
done
# shellcheck disable=SC2086
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 1 ); foreach ( array_slice( $argv, 1 ) as $action ) { echo wp_create_nonce( $action ), "\t", $action, "\n"; }' \
  $actions > "$NONCES"
record() {
  : > "$TMP"
  status=$(curl -s -o "$TMP" -D "$TMP.head" -w '%{http_code}' ${4:+-A "$4"} -b "hyscribe_role=$2" "$BASE/wp-admin/term.php$3")
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
    echo bin2hex( $p );' "$TMP" "$NONCES")
  printf '%s%s\t%s\t%s\n' "$1" "$3" "$2" "$page" >> "$OUT"
}
: > "$OUT"
referer='wp_http_referer=%2Fwp-admin%2Fedit-tags.php%3Ftaxonomy%3Dcategory'
for role in administrator editor author; do
  record default "$role" "?taxonomy=category&tag_ID=1&post_type=post&$referer"
done
for query in '?taxonomy=category&tag_ID=1' '?taxonomy=category' '?taxonomy=post_tag' \
  '?taxonomy=category&tag_ID=999' '?taxonomy=post_tag&tag_ID=1' '?taxonomy=category&tag_ID=1x' \
  "?taxonomy=category&tag_ID=1&message=3&$referer" '?taxonomy=category&tag_ID=1&message=3' \
  '?taxonomy=category&tag_ID=1&message=5&error=1' '?taxonomy=category&tag_ID=1&message=1' \
  '?taxonomy=category&tag_ID=1&message=x' '?taxonomy=category&tag_ID=1&error=' \
  '?taxonomy=category&tag_ID=1&message=3&wp_http_referer=http%3A%2F%2Felsewhere.test%2F' \
  '?taxonomy=category&tag_ID=1&message=3&wp_http_referer=%2Fwp-admin%2Fedit-tags.php%3Ftaxonomy%3Dcategory%26action%3Dedit%26tag_ID%3D1%26message%3D2%26s%3Da%2Bb' \
  '?taxonomy=category&tag_ID=1&wp_http_referer=edit-tags.php%3Fx%3D%3Cy%3E'; do
  record default administrator "$query"
done
record mobile administrator '?taxonomy=category&tag_ID=1' 'Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) Mobile'
sql < "$ROOT/reference/terms-list/content.sql"
for query in '?taxonomy=category&tag_ID=402' '?taxonomy=category&tag_ID=405' '?taxonomy=category&tag_ID=406' \
  '?taxonomy=category&tag_ID=407' '?taxonomy=category&tag_ID=1' '?taxonomy=post_tag&tag_ID=412' \
  '?taxonomy=post_tag&tag_ID=416&message=3'; do
  record terms administrator "$query"
done
record terms editor '?taxonomy=post_tag&tag_ID=412'
wc -l "$OUT"

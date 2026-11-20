#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's Categories and Tags screens, then over
# reference/terms-list/content.sql, which it adds and deletes after, with
# filters from reference/terms-list/variants.php, as
# crates/hyscribe-admin/tests/golden/terms-list.tsv:
#   variant?query<TAB>role<TAB>hex(page)
# A refusal is kept as 403<TAB>page and a redirect as 302<TAB>location. Masks
# as reference/admin-page/golden.sh.
# Start from a fresh db.sh up, then php.sh up, and take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-admin/tests/golden/terms-list.tsv"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp)
NONCES=$(mktemp)
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress; }
cleanup() {
  rm -f "$TMP" "$NONCES"
  echo "DELETE FROM wp_terms WHERE term_id BETWEEN 401 AND 416;
    DELETE FROM wp_term_taxonomy WHERE term_taxonomy_id BETWEEN 401 AND 416;
    DELETE FROM wp_options WHERE option_name = 'category_children';" | sql
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php" "$MU/hyscribe-variants.php"
  "$ROOT/reference/admin-look/look.sh" remove || true
}
trap cleanup EXIT
"$ROOT/reference/admin-look/look.sh" add
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-variants.php" < "$ROOT/reference/terms-list/variants.php"
actions="log-out heartbeat-nonce wp_rest update_can_compress_scripts screen-options-nonce add-tag taxinlineeditnonce bulk-tags"
for id in 1 401 402 403 404 405 406 407 411 412 413 414 415 416; do
  actions="$actions delete-tag_$id"
done
# shellcheck disable=SC2086
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 1 ); foreach ( array_slice( $argv, 1 ) as $action ) { echo wp_create_nonce( $action ), "\t", $action, "\n"; }' \
  $actions > "$NONCES"
record() {
  : > "$TMP"
  status=$(curl -s -o "$TMP" -D "$TMP.head" -w '%{http_code}' -b "hyscribe_role=$2; hyscribe_variant=${1%%\?*}" "$BASE/wp-admin/edit-tags.php$3")
  case "$status" in
    200) ;;
    302) printf '302\t%s' "$(grep -i '^location:' "$TMP.head" | cut -d' ' -f2 | tr -d '\r')" > "$TMP" ;;
    403) { printf '403\t'; cat "$TMP"; } > "$TMP.403" && mv "$TMP.403" "$TMP" ;;
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
for taxonomy in category post_tag; do
  for role in administrator editor author; do
    record default "$role" "?taxonomy=$taxonomy"
  done
done
sql < "$ROOT/reference/terms-list/content.sql"
for taxonomy in category post_tag; do
  for role in administrator editor; do
    record content "$role" "?taxonomy=$taxonomy"
  done
done
for query in '?taxonomy=category' '?taxonomy=category&paged=2' '?taxonomy=category&paged=3' \
  '?taxonomy=category&paged=9' '?taxonomy=post_tag&paged=2'; do
  record paged administrator "$query"
done
# Searched and sorted: a search lists what it finds flat, an order flat by page.
for query in '?taxonomy=category&s=a' '?taxonomy=category&s=LO' '?taxonomy=category&s=nothing' \
  '?taxonomy=category&s=0' "?taxonomy=category&s=a%27b" '?taxonomy=category&orderby=name&order=desc' \
  '?taxonomy=category&orderby=slug' '?taxonomy=category&orderby=term_order' \
  '?taxonomy=category&orderby=Term_Order' '?taxonomy=category&orderby=' '?taxonomy=category&order=xyz' \
  '?taxonomy=post_tag&orderby=slug&order=desc' '?taxonomy=post_tag&s=e&orderby=name&order=asc'; do
  record content administrator "$query"
done
for query in '?taxonomy=category&s=a&paged=2' '?taxonomy=post_tag&orderby=name&paged=2'; do
  record paged administrator "$query"
done
rm -f "$TMP.head"
wc -l "$OUT"

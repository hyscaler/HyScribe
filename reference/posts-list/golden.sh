#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's posts list with post 1 trashed, then over
# reference/posts-list/content.sql, which it adds and deletes after, as
# crates/hyscribe-admin/tests/golden/posts-list.tsv:
#   variant[?query]<TAB>role<TAB>hex(page)
# Posts 304 and 306 are sticky and the hidden variant's columns and page size come from
# filters (reference/posts-list/variants.php). Masks as
# reference/admin-page/golden.sh. Start from a fresh db.sh up, then php.sh up,
# and take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-admin/tests/golden/posts-list.tsv"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp)
NONCES=$(mktemp)
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress; }
cleanup() {
  rm -f "$TMP" "$NONCES"
  echo "DELETE FROM wp_posts WHERE ID BETWEEN 301 AND 315; DELETE FROM wp_comments WHERE comment_ID BETWEEN 301 AND 304;
    DELETE FROM wp_postmeta WHERE post_id IN (306, 307, 312); UPDATE wp_posts SET post_status = 'publish' WHERE ID = 1;
    DELETE FROM wp_term_relationships WHERE object_id BETWEEN 301 AND 315; DELETE FROM wp_terms WHERE term_id BETWEEN 301 AND 307;
    DELETE FROM wp_term_taxonomy WHERE term_taxonomy_id BETWEEN 301 AND 307; UPDATE wp_term_taxonomy SET count = 1 WHERE term_taxonomy_id = 1;
    DELETE FROM wp_usermeta WHERE user_id = 2; DELETE FROM wp_users WHERE ID = 2;" | sql
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php" "$MU/hyscribe-variants.php"
}
trap cleanup EXIT
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-variants.php" < "$ROOT/reference/posts-list/variants.php"
actions="log-out heartbeat-nonce wp_rest update_can_compress_scripts screen-options-nonce inlineeditnonce bulk-posts untrash-post_307 delete-post_307"
for id in 1 301 302 303 304 305 306 308 309 310 311 312 313 314 315; do
  actions="$actions trash-post_$id"
done
# shellcheck disable=SC2086
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 1 ); foreach ( array_slice( $argv, 1 ) as $action ) { echo wp_create_nonce( $action ), "\t", $action, "\n"; }' \
  $actions > "$NONCES"
record() {
  : > "$TMP"
  curl -sf -o "$TMP" -b "hyscribe_role=$2; hyscribe_variant=${1%%\?*}" "$BASE/wp-admin/edit.php${3:-}"
  page=$(php -r '
    $p = file_get_contents( $argv[1] );
    foreach ( file( $argv[2], FILE_IGNORE_NEW_LINES ) as $line ) {
      list( $nonce, $action ) = explode( "\t", $line );
      $p = str_replace( $nonce, "%nonce:$action%", $p );
    }
    $p = preg_replace( "/\"time\":\"[0-9]+\"/", "\"time\":\"%time%\"", $p );
    echo bin2hex( $p );' "$TMP" "$NONCES")
  printf '%s%s\t%s\t%s\n' "$1" "${3:-}" "$2" "$page" >> "$OUT"
}
: > "$OUT"
record default administrator '?post_status=trash'
# With post 1 in the Trash the list is empty.
echo "UPDATE wp_posts SET post_status = 'trash' WHERE ID = 1;" | sql
record empty administrator
record empty author
echo "UPDATE wp_posts SET post_status = 'publish' WHERE ID = 1;" | sql
sql < "$ROOT/reference/posts-list/content.sql"
for role in administrator editor author contributor; do
  record content "$role"
done
record hidden administrator
for status in publish future draft pending private trash; do
  record content administrator "?post_status=$status"
done
record content author '?post_status=private'
wc -l "$OUT"

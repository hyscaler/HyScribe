#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's posts and pages lists with post 1 trashed, then over
# reference/posts-list/content.sql, which it adds and deletes after, as
# crates/hyscribe-admin/tests/golden/posts-list.tsv:
#   variant[?query]<TAB>role<TAB>hex(page)
# Posts 304 and 306 are sticky and the hidden variant's columns and page size come from
# filters (reference/posts-list/variants.php). The h9locks variant adds edit
# locks and h9host asks with another Host header. Masks as
# reference/admin-page/golden.sh. With PHP on the Theme Test Data
# (HYSCRIBE_PHP_DB=wordpress_ttd reference/default-install/php.sh up) it
# records searches of that content instead, as
# crates/hyscribe-admin/tests/golden/posts-list-ttd.tsv, each page as the
# hashes of reference/media/mask.php. Start from a fresh db.sh up, then
# php.sh up, and take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TTD=$("$DOCKER" exec "$NAME" sh -c 'echo "$WORDPRESS_DB_NAME"')
if [ "$TTD" = wordpress_ttd ]; then
  OUT="$ROOT/crates/hyscribe-admin/tests/golden/posts-list-ttd.tsv"
else
  OUT="$ROOT/crates/hyscribe-admin/tests/golden/posts-list.tsv"
fi
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp)
NONCES=$(mktemp)
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress; }
cleanup() {
  rm -f "$TMP" "$NONCES"
  echo "DELETE FROM wp_posts WHERE ID BETWEEN 301 AND 321; DELETE FROM wp_comments WHERE comment_ID BETWEEN 301 AND 306;
    DELETE FROM wp_postmeta WHERE post_id IN (301, 302, 303, 304, 305, 306, 307, 312); UPDATE wp_posts SET post_status = 'publish' WHERE ID = 1;
    DELETE FROM wp_term_relationships WHERE object_id BETWEEN 301 AND 315; DELETE FROM wp_terms WHERE term_id BETWEEN 301 AND 307;
    DELETE FROM wp_term_taxonomy WHERE term_taxonomy_id BETWEEN 301 AND 307; UPDATE wp_term_taxonomy SET count = 1 WHERE term_taxonomy_id = 1;
    DELETE FROM wp_usermeta WHERE user_id = 2; DELETE FROM wp_users WHERE ID = 2;" | sql
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php" "$MU/hyscribe-variants.php"
}
trap cleanup EXIT
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
[ "$TTD" = wordpress_ttd ] || "$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-variants.php" < "$ROOT/reference/posts-list/variants.php"
actions="log-out heartbeat-nonce wp_rest update_can_compress_scripts screen-options-nonce inlineeditnonce bulk-posts untrash-post_307 delete-post_307"
ids="1 2 3 301 302 303 304 305 306 308 309 310 311 312 313 314 315 316 317 318 319 320 321"
if [ "$TTD" = wordpress_ttd ]; then
  ids=$("$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; echo implode( " ", get_posts( array( "post_type" => "any", "post_status" => "any", "numberposts" => -1, "fields" => "ids" ) ) );')
fi
for id in $ids; do
  actions="$actions trash-post_$id"
done
# shellcheck disable=SC2086
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 1 ); foreach ( array_slice( $argv, 1 ) as $action ) { echo wp_create_nonce( $action ), "\t", $action, "\n"; }' \
  $actions > "$NONCES"
record() {
  : > "$TMP"
  # A redirect is kept as its status and location.
  status=$(curl -s -o "$TMP" -D "$TMP.head" -w '%{http_code}' ${HOST_HEADER:+-H "Host: $HOST_HEADER"} -b "hyscribe_role=$2; hyscribe_variant=${1%%\?*}${4:+; $4}" "$BASE/wp-admin/edit.php${3:-}")
  [ "$status" = 200 ] || [ "$status" = 302 ] || { echo "PHP answered $status for ${3:-}" >&2; exit 1; }
  if [ "$status" = 302 ]; then
    printf '302\t%s' "$(grep -i '^location:' "$TMP.head" | cut -d' ' -f2 | tr -d '\r')" > "$TMP"
  fi
  rm -f "$TMP.head"
  hashed=
  [ "$TTD" = wordpress_ttd ] && hashed=1
  page=$(php "$ROOT/reference/media/mask.php" "$TMP" "$NONCES" "$hashed" "${3:-}")
  printf '%s%s\t%s\t%s\n' "$1" "${3:-}" "$2" "$page" >> "$OUT"
}
: > "$OUT"
# A logged in user's search finds password protected posts too.
if [ "$TTD" = wordpress_ttd ]; then
  for query in '?s=Template:+Password' '?s=enter&post_status=publish'; do
    record default administrator "$query"
  done
  wc -l "$OUT"
  exit
fi
record default administrator '?post_status=trash'
# Pages, for those who may edit them.
for role in administrator editor; do
  record default "$role" '?post_type=page'
done
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
for query in '?paged=2' '?paged=5' '?paged=9' '?orderby=title&order=desc&paged=2'; do
  record hidden administrator "$query"
done
for query in '?author=2' '?author=1' '?all_posts=1' '?show_sticky=1' '?m=202611' '?cat=301' \
  '?category_name=local' '?tag=php' '?post_format=aside' '?author=1,-2' '?author=00' '?author=a-1' \
  '?all_posts=' '?cat=0' '?cat=00'; do
  record content administrator "$query"
done
record content author '?all_posts=1'
for query in '?s=edit' '?s=draft&post_status=draft' '?s=-later+e' '?s=edit&orderby=title&order=asc' \
  '?s=new+edit' '?s=nothing+here' "?s=a'b%22c" '?s=Edited&exact=1' '?s=edited+lately&sentence=1'; do
  record content administrator "$query"
done
for query in '?orderby=title&order=asc' '?orderby=title' '?post_status=publish&orderby=date&order=asc' '?order=desc'; do
  record content administrator "$query"
done
# Edit locks: fresh ones by user 2 (one from `_edit_last`), the viewer's
# own, a stale one and one by a missing user.
echo "INSERT INTO wp_postmeta (post_id, meta_key, meta_value) VALUES (301, '_edit_lock', '9999999999:2'),
  (302, '_edit_lock', '9999999999:1'), (303, '_edit_lock', '1000:2'), (304, '_edit_lock', '9999999999'),
  (304, '_edit_last', '2'), (305, '_edit_lock', '9999999999:99'), (307, '_edit_lock', '9999999999:2');" | sql
record h9locks administrator
record h9locks editor
record h9locks administrator '?post_status=trash'
echo "DELETE FROM wp_postmeta WHERE meta_key IN ('_edit_lock', '_edit_last') AND post_id BETWEEN 301 AND 307;" | sql
# Preview links take the admin's scheme, sort and page links the request's host.
record h9home administrator
HOST_HEADER=admin.test:8081 record h9host administrator
HOST_HEADER=admin.test:8081 record h9host administrator '?orderby=title&order=asc'
# Pages, as a tree unless ordered otherwise or searched.
for role in administrator editor; do
  record content "$role" '?post_type=page'
done
record front administrator '?post_type=page'
for query in '?post_type=page&post_status=publish' '?post_type=page&post_status=draft' \
  '?post_type=page&orderby=title&order=asc' '?post_type=page&s=a' '?post_type=page&orderby=menu_order+title'; do
  record content administrator "$query"
done
for query in '?post_type=page&paged=2' '?post_type=page&paged=3' '?post_type=page&paged=4'; do
  record hidden administrator "$query"
done
# The extended view, from the settings cookie or the request; last, as the
# request's mode is saved in the user's settings.
record excerpt administrator '' 'wp-settings-1=posts_list_mode%3Dexcerpt'
record excerpt contributor '' 'wp-settings-1=posts_list_mode%3Dexcerpt'
record excerpt administrator '?mode=list' 'wp-settings-1=posts_list_mode%3Dexcerpt'
record content administrator '?mode=excerpt'
wc -l "$OUT"

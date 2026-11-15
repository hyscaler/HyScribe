#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's Comments screen, then over
# reference/comments-list/content.sql, which it adds and deletes after, with
# filters from reference/comments-list/variants.php, and, for the g9attach
# variant, over reference/attachment-edit/content.sql with comments on its
# attachments; the page's time masked
# as %pagegen%, as
# crates/hyscribe-admin/tests/golden/comments-list.tsv:
#   variant?query<TAB>role<TAB>hex(page)
# A refusal is kept as 403<TAB>page and a redirect as 302<TAB>location. Masks
# as reference/admin-page/golden.sh. Start from a fresh db.sh up, then
# php.sh up, and take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-admin/tests/golden/comments-list.tsv"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp)
NONCES=$(mktemp)
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress; }
cleanup() {
  rm -f "$TMP" "$NONCES"
  echo "DELETE FROM wp_comments WHERE comment_ID BETWEEN 501 AND 508 OR comment_ID BETWEEN 801 AND 803;
    DELETE FROM wp_posts WHERE ID = 501 OR ID BETWEEN 801 AND 809; DELETE FROM wp_postmeta WHERE meta_id BETWEEN 801 AND 813;
    DELETE FROM wp_users WHERE ID IN (2, 3); DELETE FROM wp_usermeta WHERE umeta_id BETWEEN 801 AND 806;
    DELETE FROM wp_commentmeta WHERE meta_id BETWEEN 501 AND 504;
    UPDATE wp_posts SET comment_count = 1 WHERE ID = 1; UPDATE wp_posts SET comment_count = 0 WHERE ID = 2;
    UPDATE wp_comments SET comment_approved = '1' WHERE comment_ID = 1;" | sql
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php" "$MU/hyscribe-variants.php"
}
trap cleanup EXIT
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-variants.php" < "$ROOT/reference/comments-list/variants.php"
actions="log-out heartbeat-nonce wp_rest update_can_compress_scripts screen-options-nonce bulk-comments replyto-comment fetch-list-WP_Comments_List_Table internal-linking bulk-destroy"
for id in 1 501 502 503 504 505 506 507 508 801 802 803; do
  actions="$actions approve-comment_$id delete-comment_$id"
done
# shellcheck disable=SC2086
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 1 ); foreach ( array_slice( $argv, 1 ) as $action ) { echo wp_create_nonce( $action ), "\t", $action, "\n"; }' \
  $actions > "$NONCES"
record() {
  : > "$TMP"
  status=$(curl -s -o "$TMP" -D "$TMP.head" -w '%{http_code}' -b "hyscribe_role=$2; hyscribe_variant=${1%%\?*}" "$BASE/wp-admin/edit-comments.php$3")
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
    $p = preg_replace( "/(name=\"pagegen_timestamp\" value=\")[^\"]*/", "\\1%pagegen%", $p );
    echo bin2hex( $p );' "$TMP" "$NONCES")
  printf '%s%s\t%s\t%s\n' "$1" "$3" "$2" "$page" >> "$OUT"
}
: > "$OUT"
for role in administrator editor author contributor subscriber; do
  record default "$role" ''
done
record noavatars administrator ''
for view in moderated trash; do
  record default administrator "?comment_status=$view"
done
record default administrator '?p=1'
record g9keys administrator ''
record g9pages administrator ''
# With its only comment in the Trash the list is empty.
echo "UPDATE wp_comments SET comment_approved = 'trash' WHERE comment_ID = 1;" | sql
record empty administrator ''
record empty administrator '?comment_status=trash'
record empty editor '?comment_status=trash'
echo "UPDATE wp_comments SET comment_approved = '1' WHERE comment_ID = 1;" | sql
sql < "$ROOT/reference/comments-list/content.sql"
for role in administrator editor author contributor; do
  record content "$role" ''
done
for view in moderated approved spam trash 'mine&user_id=1' mine bogus 'all&user_id=x'; do
  record content administrator "?comment_status=$view"
done
record content editor '?comment_status=spam'
for query in p=1 p=2 p=501 p=999 p=abc 'p=1&comment_status=moderated' 'p=1&comment_type=comment&comment_status=all'; do
  record content administrator "?$query"
done
record content editor '?p=1'
record content author '?p=1'
for query in s=example 's=127.0.0.1&mode=detail' s=zzz s=0 s=100%25 s=Pat%27s comment_type=comment comment_type=comments \
  comment_type=pings comment_type=pingback comment_type=note comment_type=all 'comment_type=pings&comment_status=approved' \
  'orderby=comment_author&order=asc' orderby=comment_author 'orderby=comment_post_ID&order=desc' \
  'orderby=comment_date&order=asc' orderby=bogus 'orderby=comment_ID,comment_author&order=asc' orderby=0 \
  'orderby=comment_karma&order=asc' mode=excerpt mode=list 's=e&orderby=comment_author&order=asc'; do
  record content administrator "?$query"
done
record content author '?comment_status=trash'
record paged administrator ''
for page in 2 4 -2 0 9; do
  record paged administrator "?paged=$page"
done
record paged administrator '?comment_status=approved&paged=2'
record hidecols administrator ''
record g9cpages administrator ''
record g9coldest administrator ''
# Comments on an image, a PDF and a tiny image.
sql < "$ROOT/reference/attachment-edit/content.sql"
echo "INSERT INTO wp_comments (comment_ID, comment_post_ID, comment_author, comment_date, comment_date_gmt, comment_content,
  comment_approved, comment_type) VALUES (802, 802, 'Reader', '2026-11-08 10:00:00', '2026-11-08 10:00:00', 'On the report', '1', 'comment'),
  (803, 805, 'Reader', '2026-11-09 10:00:00', '2026-11-09 10:00:00', 'Tiny', '0', 'comment');" | sql
record g9attach administrator ''
record g9attach administrator '?p=801'
record g9attach editor ''
rm -f "$TMP.head"
wc -l "$OUT"

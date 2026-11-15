#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's attachment edit screen (post.php?action=edit for
# an attachment) for each role and with the variants of
# reference/attachment-edit/variants.php, which write nothing, as
#   crates/hyscribe-admin/tests/golden/attachment-edit.tsv      (default
#     install, over reference/attachment-edit/content.sql, which it adds
#     and deletes after)
#   crates/hyscribe-admin/tests/golden/attachment-edit-ttd.tsv  (Theme Test
#     Data, with HYSCRIBE_PHP_DB=wordpress_ttd reference/default-install/php.sh up)
# one row each:
#   variant<TAB>screen?query<TAB>role<TAB>hex(page)
# A refusal is kept as 403<TAB>page, a missing post as 404<TAB>page and one in
# the Trash as 409<TAB>page. Masks
# and hashes as reference/media/golden.sh. Only locked views are recorded: an
# unlocked one writes the user's edit lock. Start from a fresh db.sh up, then
# php.sh up, and take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
MU=/var/www/html/wp-content/mu-plugins
TTD=$("$DOCKER" exec "$NAME" sh -c 'echo "$WORDPRESS_DB_NAME"')
if [ "$TTD" = wordpress_ttd ]; then
  OUT="$ROOT/crates/hyscribe-admin/tests/golden/attachment-edit-ttd.tsv"
else
  OUT="$ROOT/crates/hyscribe-admin/tests/golden/attachment-edit.tsv"
fi
TMP=$(mktemp)
NONCES=$(mktemp)
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress; }
cleanup() {
  rm -f "$TMP" "$TMP.head" "$NONCES"
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php" "$MU/hyscribe-variants.php"
  if [ "$TTD" != wordpress_ttd ]; then
    echo "DELETE FROM wp_users WHERE ID IN (2, 3); DELETE FROM wp_usermeta WHERE umeta_id BETWEEN 801 AND 806;
      DELETE FROM wp_posts WHERE ID BETWEEN 801 AND 823; DELETE FROM wp_postmeta WHERE meta_id BETWEEN 801 AND 841;
      DELETE FROM wp_comments WHERE comment_ID = 801;" | sql
  fi
}
trap cleanup EXIT
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-variants.php" < "$ROOT/reference/attachment-edit/variants.php"
if [ "$TTD" = wordpress_ttd ]; then
  ids="611 1692 827 1045 1686"
else
  sql < "$ROOT/reference/attachment-edit/content.sql"
  ids="801 802 803 805 807 808 810 811 812 813 815 817 818 819 820 821 822 823"
fi
actions="log-out heartbeat-nonce wp_rest update_can_compress_scripts screen-options-nonce meta-box-order closedpostboxes
  media-form media-send-to-editor set-attachment-thumbnail
  samplepermalink get-comments replyto-comment internal-linking fetch-list-WP_Post_Comments_List_Table"
for id in $ids; do
  actions="$actions update-post_$id image_editor-$id delete-post_$id lock-post_$id post_preview_$id"
done
# shellcheck disable=SC2086
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 1 ); foreach ( array_slice( $argv, 1 ) as $action ) { echo wp_create_nonce( $action ), "\t", $action, "\n"; }' \
  $actions > "$NONCES"
# The browser of a variant: the visual editor works in some, which changes
# how the description is escaped.
agent() {
  case "$1" in
    a8lockchrome | a8locknorich | j9lockchrome) echo 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36' ;;
    a8lockfirefox) echo 'Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0' ;;
    a8locksafari) echo 'Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15' ;;
    a8lockiphone | j9lockiphone) echo 'Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148 Safari/604.1' ;;
    a8lockoldiphone) echo 'Mozilla/5.0 (iPhone; U; CPU iPhone OS 4_0 like Mac OS X) AppleWebKit/532.9 (KHTML, like Gecko) Mobile/8A293 Safari/6531.22.7' ;;
    a8lockedge) echo 'Mozilla/5.0 (Windows NT 10.0) Edg/140.0' ;;
    a8lockopera) echo 'Opera/9.80 (X11; Linux x86_64) Presto/2.12.388 Version/12.16' ;;
    a8lockoperamini) echo 'Opera/9.80 (J2ME/MIDP; Opera Mini/9.80) Presto/2.5.25 Version/10.54' ;;
    a8lockie11) echo 'Mozilla/5.0 (Windows NT 10.0; Trident/7.0; rv:11.0)' ;;
    a8lockie10) echo 'Mozilla/5.0 (compatible; MSIE 10.0; Windows NT 6.1; Trident/6.0)' ;;
    a8lockmacie) echo 'Mozilla/4.0 (compatible; MSIE 5.23; Mac_PowerPC) Gecko' ;;
    a8lockie) echo 'Mozilla/4.0 (compatible; MSIE 5.0; Gecko)' ;;
    a8locklynx) echo 'Lynx/2.8.9rel.1 libwww-FM/2.14 Gecko' ;;
    *) echo curl ;;
  esac
}
record() {
  : > "$TMP"
  agent=$(agent "$1")
  referer=
  case "$1" in
    a8lockref) referer="$BASE/wp-admin/upload.php?mode=list" ;;
    a8lockrefpost) referer="$BASE/wp-admin/post.php?post=1&action=edit" ;;
  esac
  status=$(curl -s -A "$agent" -e "$referer" -o "$TMP" -D "$TMP.head" -w '%{http_code}' -b "hyscribe_role=$3; hyscribe_variant=$1" "$BASE/wp-admin/$2")
  case "$status" in
    200) ;;
    403 | 404 | 409) { printf '%s\t' "$status"; cat "$TMP"; } > "$TMP.die" && mv "$TMP.die" "$TMP" ;;
    *) echo "PHP answered $status for $2" >&2; exit 1 ;;
  esac
  hashed=
  [ "$TTD" = wordpress_ttd ] && hashed=1
  page=$(php "$ROOT/reference/media/mask.php" "$TMP" "$NONCES" "$hashed" "$2")
  printf '%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$page" >> "$OUT"
}
: > "$OUT"
record default 'post.php?post=9999&action=edit' administrator
if [ "$TTD" = wordpress_ttd ]; then
  for id in $ids; do
    record a8lock "post.php?post=$id&action=edit" administrator
  done
  record a8lock3 'post.php?post=611&action=edit' administrator
  record a8lock 'post.php?post=611&action=edit' author
  record a8locklayout 'post.php?post=611&action=edit' administrator
else
  for id in $ids; do
    [ "$id" -gt 802 ] && continue
    for role in administrator editor author contributor subscriber; do
      record a8lock "post.php?post=$id&action=edit" "$role"
    done
  done
  for id in 803 805 807 808 810 811 812 813 815; do
    record a8lock "post.php?post=$id&action=edit" administrator
  done
  record a8lock 'post.php?post=811&action=edit' contributor
  record a8lock 'post.php?post=803&action=edit' editor
  record a8lock 'post.php?post=808&action=edit' contributor
  record a8lockcount 'post.php?post=802&action=edit' administrator
  for variant in a8lockcount a8lockref a8lockrefpost a8locklayout a8lockchrome a8locknorich a8lockfirefox \
    a8locksafari a8lockiphone a8lockoldiphone a8lockedge a8lockopera a8lockoperamini a8lockie11 a8lockie10 \
    a8lockmacie a8lockie a8locklynx a8locknodelete a8locknoread; do
    record "$variant" 'post.php?post=801&action=edit' administrator
  done
  for query in 'message=1' 'message=10' 'message=11' 'message=x'; do
    record a8lock "post.php?post=801&action=edit&$query" administrator
  done
  record a8lock 'post.php?action=edit&post=802' administrator
  for id in 817 818 819 820 821; do
    record j9lock "post.php?post=$id&action=edit" administrator
  done
  for role in editor author; do
    record j9lock 'post.php?post=817&action=edit' "$role"
  done
  record j9lock 'post.php?post=818&action=edit' editor
  record j9lockchrome 'post.php?post=818&action=edit' administrator
  record j9lockiphone 'post.php?post=817&action=edit' administrator
  record j9noupload 'post.php?post=818&action=edit' administrator
  for id in 801 802 803 805 807 808 811 812 813 817 818 822 823; do
    record j9pretty "post.php?post=$id&action=edit" administrator
  done
  for role in editor author contributor; do
    record j9pretty 'post.php?post=801&action=edit' "$role"
  done
  record j9pretty 'post.php?post=811&action=edit' contributor
fi
wc -l "$OUT"

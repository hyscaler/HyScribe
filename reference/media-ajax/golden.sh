#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's admin-ajax.php answers of the media library
# (query-attachments, get-attachment, save-attachment,
# save-attachment-compat, delete-post) over
# reference/attachment-edit/content.sql, which it adds and deletes after,
# each case of reference/media-ajax/cases.list sent by user 1 in a role, as
# crates/hyscribe-http/tests/golden/media-ajax.tsv, one row each:
#   role<TAB>flags<TAB>body<TAB>status<TAB>X-WP-Total,X-WP-TotalPages<TAB>answer
# A body's %nonce:<action>% is user 1's nonce of the action; in the answer
# nonces are masked as reference/media/golden.sh masks them. The flag
# `trash` defines MEDIA_TRASH for the request (wp-config.php reads the
# hyscribe_media_trash cookie for the run); `restore` cases write, and the
# database is put back from a dump after each, the times they write masked
# as %now%. Start from a fresh db.sh up, then php.sh up, and take both down
# after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
MU=/var/www/html/wp-content/mu-plugins
CONFIG=/var/www/html/wp-config.php
OUT="$ROOT/crates/hyscribe-http/tests/golden/media-ajax.tsv"
TMP=$(mktemp)
NONCES=$(mktemp)
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress; }
cleanup() {
  rm -f "$TMP" "$TMP.head" "$NONCES"
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php"
  "$DOCKER" exec "$NAME" sh -c "[ ! -f $CONFIG.hyscribe ] || mv $CONFIG.hyscribe $CONFIG"
  "$DOCKER" exec "$DB" rm -f /tmp/media-ajax.sql
  echo "DELETE FROM wp_users WHERE ID IN (2, 3); DELETE FROM wp_usermeta WHERE umeta_id BETWEEN 801 AND 806;
    DELETE FROM wp_posts WHERE ID BETWEEN 801 AND 831; DELETE FROM wp_postmeta WHERE meta_id BETWEEN 801 AND 854;
    DELETE FROM wp_comments WHERE comment_ID = 801;" | sql
  "$ROOT/reference/admin-look/look.sh" remove || true
}
trap cleanup EXIT
"$ROOT/reference/admin-look/look.sh" add
sql < "$ROOT/reference/attachment-edit/content.sql"
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
# MEDIA_TRASH is defined before must-use plugins load, so in wp-config.php.
"$DOCKER" exec -i "$NAME" sh -c "cp $CONFIG $CONFIG.hyscribe && php /dev/stdin $CONFIG" <<'PHP'
<?php
$config = file_get_contents( $argv[1] );
$define = "<?php if ( isset( \$_COOKIE['hyscribe_media_trash'] ) ) { define( 'MEDIA_TRASH', true ); }";
file_put_contents( $argv[1], preg_replace( '/^<\?php/', $define, $config, 1 ) );
PHP
actions=""
for id in 1 2 801 802 803 805 807 808 810 811 812 813 815 817 818 819 820 821 822 823 824 825 826 828 829 830 99999; do
  actions="$actions update-post_$id image_editor-$id delete-post_$id"
done
# shellcheck disable=SC2086
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 1 ); foreach ( array_slice( $argv, 1 ) as $action ) { echo wp_create_nonce( $action ), "\t", $action, "\n"; }' \
  $actions > "$NONCES"
"$DOCKER" exec "$DB" sh -c "mariadb-dump -uroot -preference-only wordpress > /tmp/media-ajax.sql"
: > "$OUT"
while IFS=$'\t' read -r role flags body; do
  case "$role" in '' | '#'*) continue ;; esac
  sent=$(php -r '$body = $argv[1]; foreach ( file( $argv[2], FILE_IGNORE_NEW_LINES ) as $line ) { list( $n, $a ) = explode( "\t", $line ); $body = str_replace( "%nonce:$a%", $n, $body ); } echo $body;' "$body" "$NONCES")
  cookies="hyscribe_role=$role"
  [[ "$flags" == *trash* ]] && cookies="$cookies; hyscribe_media_trash=1"
  status=$(curl -s -o "$TMP" -D "$TMP.head" -w '%{http_code}' -b "$cookies" --data "$sent" "$BASE/wp-admin/admin-ajax.php")
  total=$(grep -i '^x-wp-total:' "$TMP.head" | cut -d' ' -f2 | tr -d '\r' || true)
  pages=$(grep -i '^x-wp-totalpages:' "$TMP.head" | cut -d' ' -f2 | tr -d '\r' || true)
  if [[ "$flags" == *restore* ]]; then
    sed -i -E 's/"modified":[0-9]+/"modified":%now%/g' "$TMP"
    "$DOCKER" exec "$DB" sh -c "mariadb -uroot -preference-only wordpress < /tmp/media-ajax.sql"
  fi
  answer=$(php -r '$page = file_get_contents( $argv[1] ); foreach ( file( $argv[2], FILE_IGNORE_NEW_LINES ) as $line ) { list( $n, $a ) = explode( "\t", $line ); $page = str_replace( $n, "%nonce:$a%", $page ); } echo $page;' "$TMP" "$NONCES")
  printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$role" "$flags" "$body" "$status" "$total,$pages" "$answer" >> "$OUT"
done < "$ROOT/reference/media-ajax/cases.list"
wc -l "$OUT"

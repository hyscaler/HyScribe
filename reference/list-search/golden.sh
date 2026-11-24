#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's answers to the list screens' search forms as a
# browser submits them (every field the form renders, no action chosen), as
# crates/hyscribe-http/tests/golden/list-search.tsv:
#   path?query<TAB>role<TAB>status<TAB>location, hex(page) or -
# The Posts, Pages, Comments, Users and Media lists send the nonce and the
# referer back and are redirected without them; each redirect's target is
# recorded too. The Categories and Tags lists answer at once (the Plugins
# search form sends only `s`, whose search is not ported). A
# page is kept for the administrator only (other roles' lists are in the
# lists' own goldens: theirs is `-`), a refusal for every role as its page.
# Masks as reference/admin-page/golden.sh. Start from a fresh db.sh up, then
# php.sh up, and take both down after; it only reads.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-http/tests/golden/list-search.tsv"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp)
NONCES=$(mktemp)
cleanup() {
  rm -f "$TMP" "$TMP.head" "$NONCES"
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php"
  "$ROOT/reference/admin-look/look.sh" remove || true
}
trap cleanup EXIT
"$ROOT/reference/admin-look/look.sh" add
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
actions="log-out heartbeat-nonce wp_rest update_can_compress_scripts screen-options-nonce inlineeditnonce bulk-posts
  add-tag taxinlineeditnonce bulk-tags bulk-comments replyto-comment fetch-list-WP_Comments_List_Table internal-linking
  bulk-destroy bulk-users bulk-media find-posts media-form media-send-to-editor set-attachment-thumbnail
  approve-comment_1 delete-comment_1 unapprove-comment_1 spam-comment_1 trash-comment_1 delete-tag_1 delete-tag_2
  delete-tag_3 trash-post_1 trash-post_2 delete-post_1 delete-post_2 untrash-post_1 untrash-post_2 update-user_1
  reset-password-for-1 delete-user_1 plugin-upload trash-post_3 untrash-post_3 delete-post_3 trash-post_4 untrash-post_4
  delete-post_4"
# shellcheck disable=SC2086
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 1 ); foreach ( array_slice( $argv, 1 ) as $action ) { echo wp_create_nonce( $action ), "\t", $action, "\n"; }' \
  $actions > "$NONCES"
# Records `$1`, a path and query under wp-admin, as role `$2`, and the target
# of a redirect.
record() {
  : > "$TMP"
  status=$(curl -s -o "$TMP" -D "$TMP.head" -w '%{http_code}' -b "hyscribe_role=$2" "$BASE/wp-admin/$1")
  case "$status" in
    200 | 403)
      if [ "$status" = 200 ] && [ "$2" != administrator ]; then
        body=-
      else
        body=$(php -r '
          $p = file_get_contents( $argv[1] );
          foreach ( file( $argv[2], FILE_IGNORE_NEW_LINES ) as $line ) {
            list( $nonce, $action ) = explode( "\t", $line );
            $p = str_replace( $nonce, "%nonce:$action%", $p );
          }
          $p = preg_replace( "/\"time\":\"[0-9]+\"/", "\"time\":\"%time%\"", $p );
          $p = preg_replace( "/(name=\"pagegen_timestamp\" value=\")[^\"]*/", "\\1%pagegen%", $p );
          echo bin2hex( $p );' "$TMP" "$NONCES")
      fi ;;
    302) body=$(grep -i '^location:' "$TMP.head" | cut -d' ' -f2 | tr -d '\r') ;;
    *) echo "PHP answered $status for $1" >&2; exit 1 ;;
  esac
  printf '%s\t%s\t%s\t%s\n' "$1" "$2" "$status" "${body#"$BASE"}" >> "$OUT"
  if [ "$status" = 302 ]; then
    target=${body#"$BASE"}
    record "${target#/wp-admin/}" "$2"
  fi
}
: > "$OUT"
N=nonce
for role in administrator editor author contributor subscriber; do
  for typed in Hello ''; do
    record "edit.php?s=$typed&post_status=all&post_type=post&_wpnonce=$N&_wp_http_referer=%2Fwp-admin%2Fedit.php&action=-1&m=0&cat=0&paged=1&action2=-1" "$role"
    record "edit.php?s=$typed&post_status=all&post_type=page&_wpnonce=$N&_wp_http_referer=%2Fwp-admin%2Fedit.php%3Fpost_type%3Dpage&action=-1&m=0&paged=1&action2=-1" "$role"
    record "edit-comments.php?s=$typed&comment_status=all&pagegen_timestamp=2026-11-24+10%3A00%3A00&_total=1&_per_page=20&_page=1&_ajax_fetch_list_nonce=$N&_wp_http_referer=%2Fwp-admin%2Fedit-comments.php&_wpnonce=$N&_wp_http_referer=%2Fwp-admin%2Fedit-comments.php&action=-1&comment_type=&paged=1&action2=-1" "$role"
    record "users.php?s=$typed&_wpnonce=$N&_wp_http_referer=%2Fwp-admin%2Fusers.php&action=-1&new_role=&paged=1&action2=-1&new_role2=" "$role"
    record "upload.php?mode=list&attachment-filter=&m=0&s=$typed&_wpnonce=$N&_wp_http_referer=%2Fwp-admin%2Fupload.php%3Fmode%3Dlist&action=-1&affected=&_ajax_nonce=$N&ps=&paged=1" "$role"
    for taxonomy in category post_tag; do
      record "edit-tags.php?taxonomy=$taxonomy&post_type=post&s=$typed" "$role"
    done
  done
  # The Filter buttons of the posts and comments lists send the same fields.
  record "edit.php?s=&post_status=all&post_type=post&_wpnonce=$N&_wp_http_referer=%2Fwp-admin%2Fedit.php&action=-1&m=202611&cat=0&filter_action=Filter&paged=1&action2=-1" "$role"
  record "edit-comments.php?s=&comment_status=all&pagegen_timestamp=2026-11-24+10%3A00%3A00&_total=1&_per_page=20&_page=1&_ajax_fetch_list_nonce=$N&_wp_http_referer=%2Fwp-admin%2Fedit-comments.php&_wpnonce=$N&_wp_http_referer=%2Fwp-admin%2Fedit-comments.php&action=-1&comment_type=comment&filter_action=Filter&paged=1&action2=-1" "$role"
done
wc -l "$OUT"

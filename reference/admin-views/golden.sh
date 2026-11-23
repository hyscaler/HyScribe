#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records what the PHP oracle answers the requests of
# reference/admin-views/cases.list, in order, as they really write (the
# dashboard's and the admin's per user writes, MVP 3.6), as
# crates/hyscribe-admin/tests/golden/admin-views.tsv, or those of
# reference/admin-views/<list>.list as admin-views-<list>.tsv:
#   reference/admin-views/golden.sh [list]
#   case<TAB>status<TAB>Location<TAB>Set-Cookie headers (joined by " | ")<TAB>hex(body)
# Each is user 1's, an administrator (reference/admin-menu/as-role.php),
# with user 1's nonce. Nonces become %nonce:<action>%, times %time%, the
# cookies' times %now% and %expires%, the drafts' dates %date% (and in other
# lists the comments' times), and the address requests come from %ip%. The
# wp_navigation post php.sh up wrote is deleted first, so the first
# auto-draft is post 4. Start from a fresh db.sh up and php.sh up, and take
# both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
LIST="${1:-cases}"
CASES="$ROOT/reference/admin-views/$LIST.list"
OUT="$ROOT/crates/hyscribe-admin/tests/golden/admin-views.tsv"
[ "$LIST" = cases ] || OUT="$ROOT/crates/hyscribe-admin/tests/golden/admin-views-$LIST.tsv"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp -d)
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress "$@"; }
cleanup() {
  rm -rf "$TMP"
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php"
  "$ROOT/reference/admin-look/look.sh" remove || true
}
trap cleanup EXIT
"$ROOT/reference/admin-look/look.sh" add
echo "DELETE FROM wp_posts WHERE post_type = 'wp_navigation'; ALTER TABLE wp_posts AUTO_INCREMENT = 4;" | sql
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
# User 1's nonces, without a session as the requests have none.
actions=(log-out heartbeat-nonce wp_rest update_can_compress_scripts welcome-panel-nonce screen-options-nonce
  replyto-comment add-post closedpostboxes meta-box-order internal-linking community_events
  health-check-site-status health-check-site-status-result inlineeditnonce bulk-posts bulk-comments
  approve-comment_1 delete-comment_1 update-user_1 profile-update-user_1 destroy_sessions_1 save-color-scheme
  user-profile_1 fetch-list-WP_Comments_List_Table unfiltered-html-comment discussion-options)
for id in 1 2 3 4 5 6; do actions+=("trash-post_$id" "untrash-post_$id" "delete-post_$id"); done
for id in 1 2 3 4 5 6; do actions+=("approve-comment_$id" "delete-comment_$id" "update-comment_$id"); done
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 1 ); foreach ( array_slice( $argv, 1 ) as $action ) { echo wp_create_nonce( $action ), "\t", $action, "\n"; }' \
  "${actions[@]}" > "$TMP/nonces"
# Requests from the host reach PHP from the container network's gateway.
ip=$("$DOCKER" inspect -f '{{range .NetworkSettings.Networks}}{{.Gateway}}{{end}}' "$NAME")
: > "$OUT"
while IFS=$'\t' read -r case method target body action field cookie; do
  url="$BASE/wp-admin/$target"
  nonce=bad
  [ "$action" = - ] || [ "$action" = none ] || nonce=$(grep -P "\t$action\$" "$TMP/nonces" | cut -f1)
  jar="hyscribe_role=administrator"
  [ "$cookie" = - ] || jar="$jar; $cookie"
  args=(-g -s -o "$TMP/body" -D "$TMP/head" -w '%{http_code}' -b "$jar")
  if [ "$method" = POST ]; then
    data=$body; [ "$data" != - ] || data=""
    args+=(-H 'Content-Type: application/x-www-form-urlencoded' --data-raw "$data&$field=$nonce")
  fi
  status=$(curl "${args[@]}" "$url")
  location=$(grep -i '^location:' "$TMP/head" | cut -d' ' -f2- | tr -d '\r' || true)
  cookies=$(grep -i '^set-cookie:' "$TMP/head" | cut -d' ' -f2- | tr -d '\r' |
    sed -E 's/(wp-settings-time-1=)[0-9]+/\1%now%/; s/expires=[^;]+; Max-Age=31536000/expires=%expires%; Max-Age=31536000/' |
    paste -sd'|' | sed 's/|/ | /g' || true)
  page=$(php -r '
    $p = file_get_contents( $argv[1] );
    foreach ( file( $argv[2], FILE_IGNORE_NEW_LINES ) as $line ) {
      list( $nonce, $action ) = explode( "\t", $line );
      $p = str_replace( $nonce, "%nonce:$action%", $p );
    }
    $p = preg_replace( "/\"time\":\"[0-9]+\"/", "\"time\":\"%time%\"", $p );
    $p = preg_replace( "/<time datetime=\"[^\"]*\">[^<]*<\/time>/", "<time datetime=\"%date%\">%date%</time>", $p );
    if ( "cases" !== $argv[4] ) {
      $p = preg_replace( "#\\d{4}/\\d{2}/\\d{2} at \\d{1,2}:\\d{2} [ap]m#", "%date%", $p );
    }
    $p = str_replace( $argv[3], "%ip%", $p );
    echo bin2hex( $p );' "$TMP/body" "$TMP/nonces" "$ip" "$LIST")
  printf '%s\t%s\t%s\t%s\t%s\n' "$case" "$status" "$location" "$cookies" "$page" >> "$OUT"
  # Comments saved a second apart list in one order: the list does not sort
  # comments of the same second by ID.
  [ "$LIST" = cases ] || sleep 1
done < <(grep -v '^#' "$CASES")
# What PHP wrote, for a review.
sql -N -e "SELECT ID, post_status, post_title FROM wp_posts WHERE ID > 3; SELECT meta_key, meta_value FROM wp_usermeta WHERE user_id = 1 AND meta_key NOT IN ('session_tokens')" < /dev/null >&2
wc -l "$OUT"

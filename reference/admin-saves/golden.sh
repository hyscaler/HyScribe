#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records what the PHP oracle answers the admin actions of
# reference/admin-saves/cases.list once their checks pass, as they really
# write, as crates/hyscribe-admin/tests/golden/admin-saves.tsv:
#   case<TAB>status<TAB>Location<TAB>hex(body)
# and, for a case whose seventh field is `follow`, the page its Location
# shows after (`body`: the page it answers), dated at the test clock
# (clock.php), with nonces and times masked as reference/settings/golden.sh
# and reference/settings-more/golden.sh mask them, as
# crates/hyscribe-admin/tests/golden/admin-saves-after.tsv:
#   case<TAB>hex(page)
# Each case is sent as user 1, an administrator
# (reference/admin-menu/as-role.php), with user 1's nonce, over the default
# install and reference/action-checks/content.sql (added, and deleted
# after). The actions write, so start from a fresh db.sh up and php.sh up,
# and take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
HERE="$ROOT/reference/admin-saves"
CONTENT="$ROOT/reference/action-checks/content.sql"
OUT="$ROOT/crates/hyscribe-admin/tests/golden/admin-saves.tsv"
AFTER="$ROOT/crates/hyscribe-admin/tests/golden/admin-saves-after.tsv"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp -d)
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress "$@"; }
cleanup() {
  rm -rf "$TMP"
  sed -n '/^-- cleanup$/,$p' "$CONTENT" | sql
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php" "$MU/hyscribe-clock.php"
}
trap cleanup EXIT
sed '/^-- cleanup$/,$d' "$CONTENT" | sql
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-clock.php" < "$HERE/clock.php"
: > "$OUT"
: > "$AFTER"
# The nonces the pages after print, masked as %nonce:action%.
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 1 ); foreach ( array_slice( $argv, 1 ) as $action ) { echo wp_create_nonce( $action ), "\t", $action, "\n"; }' \
  log-out heartbeat-nonce wp_rest update_can_compress_scripts reset-password-for-0 general-options writing-options reading-options \
  discussion-options media-options update-permalink create-privacy-page set-privacy-page dismiss-1-new_admin_email \
  media-form media-send-to-editor set-attachment-thumbnail > "$TMP/nonces"
# The page in the file $1, masked, in hexadecimal.
masked() {
  php -r '
    $p = file_get_contents( $argv[1] );
    foreach ( file( $argv[2], FILE_IGNORE_NEW_LINES ) as $line ) {
      list( $nonce, $action ) = explode( "\t", $line );
      $p = str_replace( $nonce, "%nonce:$action%", $p );
    }
    $p = preg_replace( "/\"time\":\"[0-9]+\"/", "\"time\":\"%time%\"", $p );
    $p = preg_replace( "/<kbd>[A-Za-z0-9]{8}<\/kbd>/", "<kbd>%password%</kbd>", $p );
    $p = str_replace( "/" . gmdate( "Y/m/d" ) . "/", "/%ymd%/", $p );
    $p = str_replace( "/" . gmdate( "Y/m" ) . "/", "/%ym%/", $p );
    echo bin2hex( $p );' "$1" "$TMP/nonces"
}
while IFS=$'\t' read -r case method target body action field after; do
  # User 1's nonce, without a session as the requests have none.
  nonce=$("$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 1 ); echo wp_create_nonce( $argv[1] );' "$action")
  url="$BASE/wp-admin/$target"
  args=(-g -s -o "$TMP/body" -D "$TMP/head" -w '%{http_code}' -b hyscribe_role=administrator)
  if [ "$method" = POST ]; then
    data=$body; [ "$data" != - ] || data=""
    args+=(-H 'Content-Type: application/x-www-form-urlencoded' --data-raw "${data:+$data&}$field=$nonce")
  else
    url="$url&$field=$nonce"
  fi
  status=$(curl "${args[@]}" "$url")
  location=$(grep -i '^location:' "$TMP/head" | cut -d' ' -f2- | tr -d '\r' || true)
  printf '%s\t%s\t%s\t%s\n' "$case" "$status" "$location" "$(xxd -p < "$TMP/body" | tr -d '\n')" >> "$OUT"
  case "${after:--}" in
    follow)
      case "$location" in /*) location="$BASE$location" ;; esac
      curl -s -o "$TMP/after" -b hyscribe_role=administrator "$location"
      printf '%s\t%s\n' "$case" "$(masked "$TMP/after")" >> "$AFTER" ;;
    body) printf '%s\t%s\n' "$case" "$(masked "$TMP/body")" >> "$AFTER" ;;
  esac
done < <(grep -v '^#' "$HERE/cases.list")
# What PHP wrote, for a review: users-promote made user 801 an editor.
echo "user 801: $(sql -N -e "SELECT meta_value FROM wp_usermeta WHERE user_id = 801 AND meta_key = 'wp_capabilities'" < /dev/null)" >&2
wc -l "$OUT"

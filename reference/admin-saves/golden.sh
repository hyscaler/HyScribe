#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records what the PHP oracle answers the admin actions of
# reference/admin-saves/cases.list once their checks pass, as they really
# write, as crates/hyscribe-admin/tests/golden/admin-saves.tsv:
#   case<TAB>status<TAB>Location<TAB>hex(body)
# and the screen named in a case's seventh column (`->` for the one its
# redirect leads to), fetched after it, as admin-saves-after.tsv:
# case<TAB>status<TAB>hex(page), dated at the test clock (clock.php). Nonces in bodies and pages are masked as %nonce:action%,
# and the Settings screens' times and paths of today as reference/settings/
# and reference/settings-more/golden.sh mask them. Each case is sent as user 1, an administrator
# (reference/admin-menu/as-role.php), with user 1's nonce, over the default
# install and reference/action-checks/content.sql (added, and deleted
# after), the cases after cases.list's line "# Fresh" each from the content
# and user 1 as they were, those after "# Restored" from the whole database
# as it was, each after the SQL of its eighth column, if any. Mail is taken as
# sent, as hs-admin answers (MVP phase 8 sends it). The actions write, so
# start from a fresh db.sh up and php.sh up, and take both down after.
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
# The content and user 1 as they were, and no user the cases added.
reset() {
  sed -n '/^-- cleanup$/,$p' "$CONTENT" | sql
  echo "DELETE FROM wp_users WHERE ID > 801 OR ID = 1; DELETE FROM wp_usermeta WHERE user_id > 801 OR user_id = 1;
    INSERT INTO wp_users SELECT * FROM hyscribe_user; INSERT INTO wp_usermeta SELECT * FROM hyscribe_usermeta;" | sql
  sed '/^-- cleanup$/,$d' "$CONTENT" | sql
}
cleanup() {
  rm -rf "$TMP"
  "$DOCKER" exec "$DB" sh -c 'mariadb -uroot -preference-only wordpress < /tmp/admin-saves.sql; rm /tmp/admin-saves.sql' || true
  reset
  sed -n '/^-- cleanup$/,$p' "$CONTENT" | sql
  echo "DROP TABLE hyscribe_user, hyscribe_usermeta;" | sql
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php" "$MU/hyscribe-mail.php" "$MU/hyscribe-clock.php"
  "$ROOT/reference/admin-look/look.sh" remove || true
}
echo "CREATE TABLE hyscribe_user AS SELECT * FROM wp_users WHERE ID = 1;
  CREATE TABLE hyscribe_usermeta AS SELECT * FROM wp_usermeta WHERE user_id = 1;" | sql
trap cleanup EXIT
"$ROOT/reference/admin-look/look.sh" add
sed '/^-- cleanup$/,$d' "$CONTENT" | sql
"$DOCKER" exec "$DB" sh -c 'mariadb-dump -uroot -preference-only wordpress > /tmp/admin-saves.sql'
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-clock.php" < "$HERE/clock.php"
echo "<?php add_filter( 'pre_wp_mail', '__return_true' );" | "$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-mail.php"
# The nonces masked: the cases', the screens' and those of terms 801 to 809.
masked="log-out heartbeat-nonce wp_rest update_can_compress_scripts switch-theme_twentytwentytwo switch-theme_hyscribe-fixture-child bulk-tags add-tag taxinlineeditnonce screen-options-nonce"
masked="$masked trash-post_1 trash-post_2 trash-post_3 reset-password-for-0 general-options writing-options reading-options discussion-options media-options"
masked="$masked update-permalink create-privacy-page set-privacy-page dismiss-1-new_admin_email media-form media-send-to-editor set-attachment-thumbnail"
masked="$masked $(grep -v '^#' "$HERE/cases.list" | cut -f5 | sort -u | tr '\n' ' ')"
for id in 1 801 802 803 804 805 806 807 808 809; do masked="$masked delete-tag_$id update-tag_$id"; done
# shellcheck disable=SC2086
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 1 ); foreach ( array_slice( $argv, 1 ) as $action ) { echo wp_create_nonce( $action ), "\t", $action, "\n"; }' \
  $masked > "$TMP/nonces"
hex() {
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
: > "$OUT"
: > "$AFTER"
fresh=
restored=
while IFS=$'\t' read -r case method target body action field after setup; do
  case "$case" in
    '# Fresh'*) fresh=1; continue ;;
    '# Restored'*) restored=1; continue ;;
    '#'*) continue ;;
  esac
  [ -z "$fresh" ] || reset
  [ -z "$restored" ] || "$DOCKER" exec "$DB" sh -c 'mariadb -uroot -preference-only wordpress < /tmp/admin-saves.sql'
  [ -z "${setup:-}" ] || sql -e "$setup" < /dev/null
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
  printf '%s\t%s\t%s\t%s\n' "$case" "$status" "$location" "$(hex "$TMP/body")" >> "$OUT"
  if [ "${after:-}" = "->" ]; then
    case "$location" in http*) after=$location ;; /*) after=$BASE$location ;; *) after=$BASE/wp-admin/$location ;; esac
    [ "$status" = 302 ] || after=
  elif [ -n "${after:-}" ]; then
    after=$BASE/wp-admin/$after
  fi
  if [ -n "${after:-}" ]; then
    status=$(curl -g -s -o "$TMP/after" -w '%{http_code}' -b hyscribe_role=administrator "$after")
    printf '%s\t%s\t%s\n' "$case" "$status" "$(hex "$TMP/after")" >> "$AFTER"
  fi
done < "$HERE/cases.list"
wc -l "$OUT"

#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's Themes, Plugins, Import, Export, Site Health,
# Export Personal Data and Erase Personal Data screens for each role, and
# some screens in the variants of reference/appearance/variants.php,
# which also gives the site the themes and plugins HyScribe has, as
# crates/hyscribe-admin/tests/golden/appearance.tsv:
#   variant<TAB>screen<TAB>role<TAB>hex(page)
# A refusal is kept as 403<TAB>page. Masks as reference/profile/golden.sh;
# the dashboard's Quick Draft post as %auto-draft%, and the Themes screen's
# automatic update message as %auto-update%, which a last line keeps with
# the time it was made:
#   auto-update<TAB>time<TAB>message
# The g9 and j9 variants record the Themes screen, the dashboard and Tools
# with an active theme the site does not have, or its parent missing: each sets stylesheet and template in the
# database, and the options table is put back from a dump after every view,
# since the Themes screen then switches to the default theme.
# Start from a fresh db.sh up, then php.sh up, and take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-admin/tests/golden/appearance.tsv"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp)
NONCES=$(mktemp)
OPTIONS=$(mktemp)
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress; }
"$DOCKER" exec "$DB" mariadb-dump -uroot -preference-only wordpress wp_options > "$OPTIONS"
cleanup() {
  sql < "$OPTIONS"
  rm -f "$TMP" "$TMP.403" "$NONCES" "$OPTIONS"
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php" "$MU/hyscribe-variants.php"
  "$ROOT/reference/admin-look/look.sh" remove || true
}
trap cleanup EXIT
"$ROOT/reference/admin-look/look.sh" add
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-variants.php" < "$ROOT/reference/appearance/variants.php"
actions="log-out heartbeat-nonce wp_rest update_can_compress_scripts updates switch-theme_twentytwentyfive personal-data-request bulk-privacy_requests bulk-plugins screen-options-nonce health-check-site-status health-check-site-status-result welcome-panel-nonce replyto-comment add-post closedpostboxes meta-box-order internal-linking community_events approve-comment_1 delete-comment_1"
# shellcheck disable=SC2086
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 1 ); foreach ( array_slice( $argv, 1 ) as $action ) { echo wp_create_nonce( $action ), "\t", $action, "\n"; }' \
  $actions > "$NONCES"
message() {
  "$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; require ABSPATH . "wp-admin/includes/update.php"; echo time(), "\t", wp_get_auto_update_message();'
}
before=$(message)
record() {
  : > "$TMP"
  status=$(curl -s -o "$TMP" -w '%{http_code}' -b "hyscribe_role=$3; hyscribe_variant=$1" "$BASE/wp-admin/$2${4:-}")
  case "$status" in
    200) ;;
    403) { printf '403\t'; cat "$TMP"; } > "$TMP.403" && mv "$TMP.403" "$TMP" ;;
    *) echo "PHP answered $status for $2" >&2; exit 1 ;;
  esac
  page=$(php -r '
    $p = file_get_contents( $argv[1] );
    foreach ( file( $argv[2], FILE_IGNORE_NEW_LINES ) as $line ) {
      list( $nonce, $action ) = explode( "\t", $line );
      $p = str_replace( $nonce, "%nonce:$action%", $p );
    }
    $p = preg_replace( "/\"time\":\"[0-9]+\"/", "\"time\":\"%time%\"", $p );
    $p = preg_replace( "/(name=\"post_ID\" value=\")[0-9]+/", "\\1%auto-draft%", $p );
    $p = str_replace( explode( "\t", $argv[3] )[1], "%auto-update%", $p );
    echo bin2hex( $p );' "$TMP" "$NONCES" "$before")
  printf '%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$page" >> "$OUT"
}
: > "$OUT"
for screen in themes.php plugins.php update-core.php import.php export.php site-health.php export-personal-data.php erase-personal-data.php; do
  for role in administrator editor author contributor subscriber; do
    record default "$screen" "$role"
  done
done
for variant in a5auto a5nooptions a5noswitch; do
  record "$variant" themes.php administrator
done
for screen in plugins.php export-personal-data.php; do
  record a5hidden "$screen" administrator
done
record g9broken themes.php administrator '?broken=1'
record default update-core.php administrator '?action=upgrade-core'
# Plugins the site names and HyScribe does not have: PHP deactivates them
# as the screen opens, so the options table is put back after each view.
record_active() {
  printf "UPDATE wp_options SET option_value = '%s' WHERE option_name = 'active_plugins';\n" "$2" | sql
  record "$1" plugins.php administrator
  sql < "$OPTIONS"
}
record_active o36one 'a:1:{i:0;s:19:"missing/missing.php";}'
record_active o36two 'a:2:{i:0;s:11:"one/one.php";i:1;s:7:"two.php";}'
record_active o36path 'a:1:{i:0;s:11:"../evil.php";}'
record_active o36inner 'a:1:{i:0;s:12:"dir/../x.php";}'
record_active o36drive 'a:1:{i:0;s:8:"c:/x.php";}'
record_active o36escape 'a:1:{i:0;s:12:"a b/<i>&.php";}'
record_active o36dup 'a:2:{i:0;s:7:"x/x.php";i:1;s:7:"x/x.php";}'
# An active theme the site does not have: stylesheet, template, then the
# screens and roles.
missing() {
  printf "UPDATE wp_options SET option_value = '%s' WHERE option_name = 'stylesheet';\n" "$2" | sql
  printf "UPDATE wp_options SET option_value = '%s' WHERE option_name = 'template';\n" "$3" | sql
  record "$1" "$4" "$5"
  sql < "$OPTIONS"
}
for role in administrator editor; do
  missing g9gone gone gone themes.php "$role"
  missing g9gone gone gone index.php "$role"
done
missing g9noswitch gone gone themes.php administrator
missing g9nooptions gone gone themes.php administrator
missing g9kid kid gone themes.php administrator
# A missing child of Twenty Twenty-Five, and Twenty Twenty-Five with a
# missing parent.
for screen in themes.php index.php tools.php; do
  for role in administrator editor; do
    missing j9child kid twentytwentyfive "$screen" "$role"
    missing j9parent twentytwentyfive gone "$screen" "$role"
  done
done
after=$(message)
if [ "${before#*	}" != "${after#*	}" ]; then
  echo "The automatic update message changed while recording; run again" >&2
  exit 1
fi
printf 'auto-update\t%s\n' "$before" >> "$OUT"
wc -l "$OUT"

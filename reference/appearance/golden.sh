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
# the Themes screen's automatic update message is masked as %auto-update%,
# and a last line keeps it with the time it was made:
#   auto-update<TAB>time<TAB>message
# Start from a fresh db.sh up, then php.sh up, and take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-admin/tests/golden/appearance.tsv"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp)
NONCES=$(mktemp)
cleanup() {
  rm -f "$TMP" "$TMP.403" "$NONCES"
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php" "$MU/hyscribe-variants.php"
}
trap cleanup EXIT
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-variants.php" < "$ROOT/reference/appearance/variants.php"
actions="log-out heartbeat-nonce wp_rest update_can_compress_scripts updates switch-theme_twentytwentyfive personal-data-request bulk-privacy_requests bulk-plugins screen-options-nonce health-check-site-status health-check-site-status-result"
# shellcheck disable=SC2086
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 1 ); foreach ( array_slice( $argv, 1 ) as $action ) { echo wp_create_nonce( $action ), "\t", $action, "\n"; }' \
  $actions > "$NONCES"
message() {
  "$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; require ABSPATH . "wp-admin/includes/update.php"; echo time(), "\t", wp_get_auto_update_message();'
}
before=$(message)
record() {
  : > "$TMP"
  status=$(curl -s -o "$TMP" -w '%{http_code}' -b "hyscribe_role=$3; hyscribe_variant=$1" "$BASE/wp-admin/$2")
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
    $p = str_replace( explode( "\t", $argv[3] )[1], "%auto-update%", $p );
    echo bin2hex( $p );' "$TMP" "$NONCES" "$before")
  printf '%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$page" >> "$OUT"
}
: > "$OUT"
for screen in themes.php plugins.php import.php export.php site-health.php export-personal-data.php erase-personal-data.php; do
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
after=$(message)
if [ "${before#*	}" != "${after#*	}" ]; then
  echo "The automatic update message changed while recording; run again" >&2
  exit 1
fi
printf 'auto-update\t%s\n' "$before" >> "$OUT"
wc -l "$OUT"

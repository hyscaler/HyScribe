#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's Discussion, Media, Permalinks and Privacy settings
# screens for each role and in the variants of
# reference/settings-more/variants.php as
# crates/hyscribe-admin/tests/golden/settings-more.tsv, today's date in the
# permalink examples masked as %ymd% and %ym%:
#   variant<TAB>screen<TAB>role<TAB>hex(page)
# A refusal is kept as 403<TAB>page. Masks as reference/profile/golden.sh.
# Start from a fresh db.sh up, then php.sh up, and take both down after: the
# Permalinks screen rewrites the rewrite_rules option.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-admin/tests/golden/settings-more.tsv"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp)
NONCES=$(mktemp)
cleanup() {
  rm -f "$TMP" "$TMP.head" "$NONCES"
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php" "$MU/hyscribe-variants.php"
  "$ROOT/reference/admin-look/look.sh" remove || true
}
trap cleanup EXIT
"$ROOT/reference/admin-look/look.sh" add
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-variants.php" < "$ROOT/reference/settings-more/variants.php"
actions="log-out heartbeat-nonce wp_rest update_can_compress_scripts discussion-options media-options update-permalink create-privacy-page set-privacy-page"
# shellcheck disable=SC2086
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 1 ); foreach ( array_slice( $argv, 1 ) as $action ) { echo wp_create_nonce( $action ), "\t", $action, "\n"; }' \
  $actions > "$NONCES"
record() {
  : > "$TMP"
  status=$(curl -s -o "$TMP" -D "$TMP.head" -w '%{http_code}' -b "hyscribe_role=$3; hyscribe_variant=$1" "$BASE/wp-admin/$2")
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
    $p = str_replace( "/" . gmdate( "Y/m/d" ) . "/", "/%ymd%/", $p );
    $p = str_replace( "/" . gmdate( "Y/m" ) . "/", "/%ym%/", $p );
    echo bin2hex( $p );' "$TMP" "$NONCES")
  printf '%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$page" >> "$OUT"
}
: > "$OUT"
for screen in options-discussion.php options-media.php options-permalink.php options-privacy.php; do
  for role in administrator editor author contributor subscriber; do
    record default "$screen" "$role"
  done
done
for variant in s2discussion s2noavatars; do
  record "$variant" options-discussion.php administrator
done
for variant in s2media s2urlpath s2uploads; do
  record "$variant" options-media.php administrator
done
for variant in s2day s2month s2numeric s2postname s2custom; do
  record "$variant" options-permalink.php administrator
done
for variant in s2published s2unset s2missing s2trashed s2nopages s2orphan; do
  record "$variant" options-privacy.php administrator
done
wc -l "$OUT"

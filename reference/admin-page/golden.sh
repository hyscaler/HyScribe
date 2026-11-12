#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's whole admin pages, for each role that may open
# them, as crates/hyscribe-admin/tests/golden/pages.tsv:
#   role<TAB>screen<TAB>hex(page)
# Nonces and the request's time are replaced by %nonce:<action>% and %time%,
# and the Quick Draft's auto-draft ID by %auto-draft%.
# It uses reference/admin-menu/as-role.php; admin screens write to the
# database, so start from a fresh db.sh up (then php.sh up) and take it down
# after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-admin/tests/golden/pages.tsv"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp)
NONCES=$(mktemp)
cleanup() {
  rm -f "$TMP" "$NONCES"
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php"
}
trap cleanup EXIT
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
# User 1's nonces, without a session as the requests have none.
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 1 ); foreach ( array_slice( $argv, 1 ) as $action ) { echo wp_create_nonce( $action ), "\t", $action, "\n"; }' \
  log-out heartbeat-nonce wp_rest update_can_compress_scripts welcome-panel-nonce screen-options-nonce replyto-comment \
  add-post closedpostboxes meta-box-order approve-comment_1 delete-comment_1 internal-linking community_events \
  health-check-site-status health-check-site-status-result > "$NONCES"
: > "$OUT"
for role in administrator editor author contributor subscriber; do
  for screen in 'tools.php' 'index.php'; do
    [ "$role" = subscriber ] && [ "$screen" = tools.php ] && continue
    : > "$TMP"
    curl -sf -o "$TMP" -b "hyscribe_role=$role" "$BASE/wp-admin/$screen"
    page=$(php -r '
      $p = file_get_contents( $argv[1] );
      foreach ( file( $argv[2], FILE_IGNORE_NEW_LINES ) as $line ) {
        list( $nonce, $action ) = explode( "\t", $line );
        $p = str_replace( $nonce, "%nonce:$action%", $p );
      }
      $p = preg_replace( "/\"time\":\"[0-9]+\"/", "\"time\":\"%time%\"", $p );
      $p = preg_replace( "/(name=\"post_ID\" value=\")[0-9]+/", "\\1%auto-draft%", $p );
      echo bin2hex( $p );' "$TMP" "$NONCES")
    printf '%s\t%s\t%s\n' "$role" "$screen" "$page" >> "$OUT"
  done
done
wc -l "$OUT"

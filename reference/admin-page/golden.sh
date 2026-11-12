#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's whole admin pages, for each role that may open
# them, as crates/hyscribe-admin/tests/golden/pages.tsv:
#   role<TAB>screen<TAB>hex(page)
# Nonces and the request's time are replaced by %nonce:<action>% and %time%.
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
cleanup() {
  rm -f "$TMP"
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php"
}
trap cleanup EXIT
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
: > "$OUT"
for role in administrator editor author contributor; do
  for screen in 'tools.php'; do
    : > "$TMP"
    curl -sf -o "$TMP" -b "hyscribe_role=$role" "$BASE/wp-admin/$screen"
    page=$(php -r '
      $p = file_get_contents( $argv[1] );
      $p = preg_replace( "/_wpnonce=[0-9a-f]{10}/", "_wpnonce=%nonce:log-out%", $p );
      $p = preg_replace( "/(heartbeatSettings = \\{\"nonce\":\")[0-9a-f]{10}/", "\\1%nonce:heartbeat-nonce%", $p );
      $p = preg_replace( "/(createNonceMiddleware\\( \")[0-9a-f]{10}/", "\\1%nonce:wp_rest%", $p );
      $p = preg_replace( "/(compressionNonce = \")[0-9a-f]{10}/", "\\1%nonce:update_can_compress_scripts%", $p );
      $p = preg_replace( "/\"time\":\"[0-9]+\"/", "\"time\":\"%time%\"", $p );
      echo bin2hex( $p );' "$TMP")
    printf '%s\t%s\t%s\n' "$role" "$screen" "$page" >> "$OUT"
  done
done
wc -l "$OUT"

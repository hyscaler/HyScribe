#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's admin bar on wp-admin screens, #wpadminbar up to
# #wpbody, for each role, as crates/hyscribe-admin/tests/golden/admin-bar.tsv:
#   role<TAB>screen<TAB>hex(admin bar)
# The logout link's nonce is replaced by %nonce:log-out%. It uses
# reference/admin-menu/as-role.php; admin screens write to the database, so
# start from a fresh db.sh up (then php.sh up) and take it down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-admin/tests/golden/admin-bar.tsv"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp)
cleanup() {
  rm -f "$TMP"
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php"
  "$ROOT/reference/admin-look/look.sh" remove || true
}
trap cleanup EXIT
"$ROOT/reference/admin-look/look.sh" add
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
: > "$OUT"
for role in administrator editor author contributor subscriber; do
  for screen in 'index.php' 'profile.php' 'edit.php'; do
    : > "$TMP"
    curl -s -o "$TMP" -b "hyscribe_role=$role" "$BASE/wp-admin/$screen"
    bar=$(php -r '$p = file_get_contents( $argv[1] ); $a = strpos( $p, "<div id=\"wpadminbar\"" ); $b = strpos( $p, "<div id=\"wpbody\"" ); if ( false === $a ) { exit; } $bar = preg_replace( "/_wpnonce=[0-9a-f]{10}/", "_wpnonce=%nonce:log-out%", substr( $p, $a, $b - $a ) ); echo bin2hex( $bar );' "$TMP")
    printf '%s\t%s\t%s\n' "$role" "$screen" "$bar" >> "$OUT"
  done
done
wc -l "$OUT"

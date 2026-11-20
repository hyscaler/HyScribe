#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's login screen and Tools screen with a site icon,
# from reference/site-icon/content.sql, which it adds and deletes after,
# chosen by reference/site-icon/variants.php, as
# crates/hyscribe-admin/tests/golden/site-icon.tsv:
#   variant<TAB>file<TAB>role<TAB>hex(page)
# the login screen's role is -, its nonce masked as reference/login/golden.sh
# masks it, the Tools screen's nonces and time as reference/admin-page/golden.sh
# masks them. Start from a fresh db.sh up,
# then php.sh up, and take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-admin/tests/golden/site-icon.tsv"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp)
NONCES=$(mktemp)
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress; }
cleanup() {
  rm -f "$TMP" "$NONCES"
  echo "DELETE FROM wp_posts WHERE ID BETWEEN 951 AND 954; DELETE FROM wp_postmeta WHERE meta_id BETWEEN 951 AND 958;" | sql
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php" "$MU/hyscribe-variants.php"
  "$ROOT/reference/admin-look/look.sh" remove || true
}
trap cleanup EXIT
"$ROOT/reference/admin-look/look.sh" add
sql < "$ROOT/reference/site-icon/content.sql"
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-variants.php" < "$ROOT/reference/site-icon/variants.php"
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 1 ); foreach ( array_slice( $argv, 1 ) as $action ) { echo wp_create_nonce( $action ), "\t", $action, "\n"; }' \
  log-out heartbeat-nonce wp_rest update_can_compress_scripts > "$NONCES"
record() {
  : > "$TMP"
  curl -sf -o "$TMP" -b "hyscribe_role=$3; hyscribe_variant=$1" "$BASE/$2"
  page=$(php -r '
    $p = file_get_contents( $argv[1] );
    foreach ( file( $argv[2], FILE_IGNORE_NEW_LINES ) as $line ) {
      list( $nonce, $action ) = explode( "\t", $line );
      $p = str_replace( $nonce, "%nonce:$action%", $p );
    }
    $p = preg_replace( "/\"time\":\"[0-9]+\"/", "\"time\":\"%time%\"", $p );
    // A visitor'"'"'s user-profile nonce, as reference/login/golden.sh masks it.
    echo bin2hex( preg_replace( "/(\"nonce\":\")[0-9a-f]{10}\"/", "\\1%nonce:reset-password-for-user%\"", $p ) );' "$TMP" "$NONCES")
  printf '%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$page" >> "$OUT"
}
: > "$OUT"
# The login screen for a visitor, then the admin as user 1 in a role.
for variant in h9icon h9iconbare; do
  record "$variant" wp-login.php -
done
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
record h9icon wp-admin/tools.php administrator
record h9icon wp-admin/tools.php editor
for variant in h9iconbare h9iconpdf h9iconnone; do
  record "$variant" wp-admin/tools.php administrator
done
wc -l "$OUT"

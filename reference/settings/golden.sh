#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's General, Writing and Reading Settings screens for
# each role and in the variants of reference/settings/variants.php, General
# also over reference/settings/content.sql, which it adds and deletes after,
# for mobile browsers, and Reading asked with a query its referer field
# escapes, as crates/hyscribe-admin/tests/golden/settings.tsv,
# the Writing screen's random strings masked as %password%:
#   variant<TAB>role<TAB>screen<TAB>hex(page)
# A refusal is kept as 403<TAB>page. Masks as reference/admin-page/golden.sh.
# Start from a fresh db.sh up, then php.sh up, and take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-admin/tests/golden/settings.tsv"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp)
NONCES=$(mktemp)
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress; }
cleanup() {
  rm -f "$TMP" "$TMP.head" "$NONCES"
  echo "DELETE FROM wp_posts WHERE ID IN (701, 702);" | sql
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php" "$MU/hyscribe-variants.php"
}
trap cleanup EXIT
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-variants.php" < "$ROOT/reference/settings/variants.php"
actions="log-out heartbeat-nonce wp_rest update_can_compress_scripts reset-password-for-0 general-options writing-options reading-options dismiss-1-new_admin_email media-form media-send-to-editor set-attachment-thumbnail"
# shellcheck disable=SC2086
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 1 ); foreach ( array_slice( $argv, 1 ) as $action ) { echo wp_create_nonce( $action ), "\t", $action, "\n"; }' \
  $actions > "$NONCES"
record() {
  : > "$TMP"
  status=$(curl -s -o "$TMP" -D "$TMP.head" -w '%{http_code}' -A "${4:-curl}" -b "hyscribe_role=$2; hyscribe_variant=$1" \
    "$BASE/wp-admin/options-$3.php${5:-}")
  case "$status" in
    200) ;;
    403) { printf '403\t'; cat "$TMP"; } > "$TMP.403" && mv "$TMP.403" "$TMP" ;;
    *) echo "PHP answered $status for $3" >&2; exit 1 ;;
  esac
  page=$(php -r '
    $p = file_get_contents( $argv[1] );
    foreach ( file( $argv[2], FILE_IGNORE_NEW_LINES ) as $line ) {
      list( $nonce, $action ) = explode( "\t", $line );
      $p = str_replace( $nonce, "%nonce:$action%", $p );
    }
    $p = preg_replace( "/\"time\":\"[0-9]+\"/", "\"time\":\"%time%\"", $p );
    $p = preg_replace( "/<kbd>[A-Za-z0-9]{8}<\/kbd>/", "<kbd>%password%</kbd>", $p );
    echo bin2hex( $p );' "$TMP" "$NONCES")
  printf '%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$page" >> "$OUT"
}
: > "$OUT"
for screen in general writing reading; do
  for role in administrator editor author contributor subscriber; do
    record default "$role" "$screen"
  done
done
for variant in s1general s1offset s1etc s1kolkata s1calcutta s1utc s1plus; do
  record "$variant" administrator general
done
for variant in s1writing s1private; do
  record "$variant" administrator writing
done
for variant in s1static s1same s1privacy s1private s1nopages; do
  record "$variant" administrator reading
done
record s1android administrator general 'Mozilla/5.0 (Linux; Android 14) Mobile'
record s1ios7 administrator general 'Mozilla/5.0 (iPhone; CPU iPhone OS 7_0 like Mac OS X) Mobile'
record s1ios5 administrator general 'Mozilla/5.0 (iPad; CPU OS 5_1 like Mac OS X) Mobile'
record s1referer administrator reading curl "?x=1&y='q'"
sql < "$ROOT/reference/settings/content.sql"
record s1media administrator general
wc -l "$OUT"

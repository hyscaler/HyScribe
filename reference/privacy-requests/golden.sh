#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's Export Personal Data and Erase Personal Data
# screens over reference/privacy-requests/content.sql, which it adds and
# deletes after, with filters from reference/privacy-requests/variants.php,
# as crates/hyscribe-admin/tests/golden/privacy-requests.tsv:
#   variant<TAB>screen?query<TAB>role<TAB>hex(page)
# A refusal is kept as 403<TAB>page and a redirect as 302<TAB>location. Masks
# as reference/users-list/golden.sh. The content's pending requests are
# fresh, so no view marks one failed. Start from a fresh db.sh up, then
# php.sh up, and take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-admin/tests/golden/privacy-requests.tsv"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp)
NONCES=$(mktemp)
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress; }
cleanup() {
  rm -f "$TMP" "$TMP.head" "$NONCES"
  echo "DELETE FROM wp_posts WHERE ID BETWEEN 861 AND 874; DELETE FROM wp_postmeta WHERE meta_id BETWEEN 861 AND 866;" | sql
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php" "$MU/hyscribe-variants.php"
  "$ROOT/reference/admin-look/look.sh" remove || true
}
trap cleanup EXIT
"$ROOT/reference/admin-look/look.sh" add
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-variants.php" < "$ROOT/reference/privacy-requests/variants.php"
sql < "$ROOT/reference/privacy-requests/content.sql"
actions="log-out heartbeat-nonce wp_rest update_can_compress_scripts screen-options-nonce personal-data-request
  bulk-privacy_requests"
for id in 861 862 863 864 865; do
  actions="$actions wp-privacy-export-personal-data-$id"
done
for id in 871 872 873 874; do
  actions="$actions wp-privacy-erase-personal-data-$id"
done
# shellcheck disable=SC2086
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 1 ); foreach ( array_slice( $argv, 1 ) as $action ) { echo wp_create_nonce( $action ), "\t", $action, "\n"; }' \
  $actions > "$NONCES"
record() {
  : > "$TMP"
  status=$(curl -s -o "$TMP" -D "$TMP.head" -w '%{http_code}' -b "hyscribe_role=$3; hyscribe_variant=$1" "$BASE/wp-admin/$2")
  case "$status" in
    200) ;;
    302) printf '302\t%s' "$(grep -i '^location:' "$TMP.head" | cut -d' ' -f2 | tr -d '\r')" > "$TMP" ;;
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
    echo bin2hex( $p );' "$TMP" "$NONCES")
  printf '%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$page" >> "$OUT"
}
: > "$OUT"
for screen in export-personal-data.php erase-personal-data.php; do
  record default "$screen" administrator
  record default "$screen" editor
  for status in request-pending request-confirmed request-completed request-failed unknown; do
    record default "$screen?filter-status=$status" administrator
  done
  record j9hidden "$screen" administrator
  for page in 1 2 3 9; do
    record j9pp "$screen?paged=$page" administrator
  done
done
for query in 'orderby=requester' 'orderby=requester&order=desc' 'orderby=requested' 'orderby=requested&order=asc' \
  'order=asc' 'orderby=other' 's=pending' 's=nothing' 's=example&filter-status=request-pending' \
  'orderby=requester&filter-status=request-pending&paged=1'; do
  record default "export-personal-data.php?$query" administrator
done
wc -l "$OUT"

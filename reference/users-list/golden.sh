#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's Users screen, then over
# reference/users-list/content.sql, which it adds and deletes after, with
# filters from reference/users-list/variants.php, as
# crates/hyscribe-admin/tests/golden/users-list.tsv:
#   variant?query<TAB>role<TAB>hex(page)
# A refusal is kept as 403<TAB>page and a redirect as 302<TAB>location. Masks
# as reference/admin-page/golden.sh. Start from a fresh db.sh up, then
# php.sh up, and take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-admin/tests/golden/users-list.tsv"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp)
NONCES=$(mktemp)
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress; }
cleanup() {
  rm -f "$TMP" "$NONCES"
  echo "DELETE FROM wp_users WHERE ID BETWEEN 601 AND 607; DELETE FROM wp_usermeta WHERE umeta_id BETWEEN 601 AND 629;
    DELETE FROM wp_posts WHERE ID BETWEEN 601 AND 604;" | sql
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php" "$MU/hyscribe-variants.php"
}
trap cleanup EXIT
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-variants.php" < "$ROOT/reference/users-list/variants.php"
actions="log-out heartbeat-nonce wp_rest update_can_compress_scripts screen-options-nonce bulk-users"
# shellcheck disable=SC2086
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 1 ); foreach ( array_slice( $argv, 1 ) as $action ) { echo wp_create_nonce( $action ), "\t", $action, "\n"; }' \
  $actions > "$NONCES"
record() {
  : > "$TMP"
  status=$(curl -s -o "$TMP" -D "$TMP.head" -w '%{http_code}' -b "hyscribe_role=$2; hyscribe_variant=${1%%\?*}" "$BASE/wp-admin/users.php$3")
  case "$status" in
    200) ;;
    302) printf '302\t%s' "$(grep -i '^location:' "$TMP.head" | cut -d' ' -f2 | tr -d '\r')" > "$TMP" ;;
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
    echo bin2hex( $p );' "$TMP" "$NONCES")
  printf '%s%s\t%s\t%s\n' "$1" "$3" "$2" "$page" >> "$OUT"
}
: > "$OUT"
for role in administrator editor author contributor subscriber; do
  record default "$role" ''
done
record noavatars administrator ''
sql < "$ROOT/reference/users-list/content.sql"
for variant in users paged hidecols; do
  record "$variant" administrator ''
done
for query in role=editor role=author role=subscriber role=administrator role=none role=bogus 'role=editor,author' \
  'role=%20author%20,editor' role=0 s=ed s=example.test s=@example s=601 s=1 s=http://ann s=zzz 's=e*' 's=*' s=0 s=%00 s=a%00 \
  'orderby=email&order=asc' 'orderby=email&order=desc' 'orderby=login&order=desc' orderby=name 'orderby=post_count,login&order=desc' \
  'orderby=registered,login' 'orderby=ID&order=desc' orderby=bogus 'orderby=nicename%20url' order=xyz \
  'role=editor&s=e&orderby=email&order=desc'; do
  record users administrator "?$query"
done
for query in paged=2 paged=4 paged=9 paged=-2 'role=editor&paged=2'; do
  record paged administrator "?$query"
done
rm -f "$TMP.head"
wc -l "$OUT"

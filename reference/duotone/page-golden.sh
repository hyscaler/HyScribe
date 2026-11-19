#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the pages PHP answers for the posts of reference/duotone/page.sql,
# which it adds to the default install and deletes after, as
#   crates/hyscribe-http/tests/golden/duotone-pages.tsv:
#   options<TAB>hex(path and query)<TAB>status<TAB>location<TAB>hex(body)
# Page views write: the wp_navigation post php.sh up wrote is
# deleted first, so start from a fresh db.sh up with php.sh up, and take
# both down after.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
BASE="${HYSCRIBE_PHP_BASE:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-http/tests/golden/duotone-pages.tsv"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp)
HEADERS=$(mktemp)
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress; }
cleanup() {
  rm -f "$TMP" "$HEADERS"
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-options.php"
  echo "DELETE FROM wp_posts WHERE ID BETWEEN 9101 AND 9102;" | sql
}
trap cleanup EXIT
"$DOCKER" exec "$NAME" mkdir -p "$MU"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-options.php" < "$ROOT/reference/pages/options.php"
sql < "$ROOT/reference/duotone/page.sql"
echo "DELETE FROM wp_posts WHERE post_type = 'wp_navigation'" | sql
curl -s -o /dev/null "$BASE/"
: > "$OUT"
for path in '/?p=9101' '/?p=9102'; do
  curl -g -s -D "$HEADERS" -o "$TMP" "$BASE$path" > /dev/null
  status=$(head -n 1 "$HEADERS" | cut -d ' ' -f 2)
  location=$(grep -i '^location:' "$HEADERS" | cut -d ' ' -f 2 | tr -d '\r' || true)
  printf '\t%s\t%s\t%s\t%s\n' "$(printf '%s' "$path" | xxd -p | tr -d '\n')" "$status" "$location" \
    "$(xxd -p "$TMP" | tr -d '\n')" >> "$OUT"
done
wc -l "$OUT"

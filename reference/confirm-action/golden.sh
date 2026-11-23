#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's confirmation links of personal data requests, the
# requests of reference/confirm-action/cases.list over the requests of
# reference/confirm-action/content.sql (added, and deleted after), as
# crates/hyscribe-admin/tests/golden/confirm-action.tsv:
#   query<TAB>status<TAB>hex(page)
# Mail is taken as sent, as HyScribe answers until its transport sends it
# (ADR 0016), by the must-use plugin variants.php, removed after. A link that
# confirms a request writes: start from a fresh db.sh up and reset it after.
# The rows it writes are the write oracle's login-confirm-request.
set -euo pipefail
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-admin/tests/golden/confirm-action.tsv"
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp)
HEADERS=$(mktemp)
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress; }
cleanup() {
  rm -f "$TMP" "$HEADERS"
  echo "DELETE FROM wp_posts WHERE ID BETWEEN 951 AND 960; DELETE FROM wp_postmeta WHERE post_id BETWEEN 951 AND 960;" | sql
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-confirm-action.php"
  "$ROOT/reference/admin-look/look.sh" remove || true
}
trap cleanup EXIT
"$ROOT/reference/admin-look/look.sh" add
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-confirm-action.php" < "$ROOT/reference/confirm-action/variants.php"
sql < "$ROOT/reference/confirm-action/content.sql"
: > "$OUT"
grep -v '^#' "$ROOT/reference/confirm-action/cases.list" | while IFS= read -r query; do
  : > "$TMP"
  curl -s -D "$HEADERS" -o "$TMP" "$BASE/wp-login.php?$query" > /dev/null
  status=$(head -n 1 "$HEADERS" | cut -d ' ' -f 2)
  printf '%s\t%s\t%s\n' "$query" "$status" "$(xxd -p "$TMP" | tr -d '\n')" >> "$OUT"
done
wc -l "$OUT"

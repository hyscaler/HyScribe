#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's site editor (site-editor.php) for the roles that
# may not open it, with and without the arguments it takes, as
# crates/hyscribe-http/tests/golden/site-editor.tsv:
#   query<TAB>role<TAB>403<TAB>hex(page)
# The administrator's page is left out: its first view writes the theme's
# wp_global_styles post, and it preloads REST responses HyScribe does not
# serve yet. Writes nothing. Start from a fresh db.sh up, then php.sh up,
# and take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-http/tests/golden/site-editor.tsv"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp)
cleanup() {
  rm -f "$TMP"
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php"
}
trap cleanup EXIT
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
: > "$OUT"
for query in '' '?p=%2Fpage' '?postType=page&postId=2&canvas=edit'; do
  for role in editor author contributor subscriber; do
    status=$(curl -s -o "$TMP" -w '%{http_code}' -b "hyscribe_role=$role" "$BASE/wp-admin/site-editor.php$query")
    [ "$status" = 403 ] || { echo "PHP answered $status for $role$query" >&2; exit 1; }
    printf '%s\t%s\t%s\t%s\n' "$query" "$role" "$status" "$(od -An -v -tx1 "$TMP" | tr -d ' \n')" >> "$OUT"
  done
done
wc -l "$OUT"

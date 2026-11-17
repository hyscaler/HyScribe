#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's site editor (site-editor.php) as
# crates/hyscribe-http/tests/golden/site-editor.tsv:
#   query<TAB>role<TAB>status<TAB>location<TAB>hex(page)
# for the roles that may not open it, with and without the arguments it
# takes, and for the administrator, the redirects from older addresses and
# the refusal of an unknown post type. Writes nothing. Start from a fresh
# db.sh up, then php.sh up, and take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-http/tests/golden/site-editor.tsv"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp -d)
cleanup() {
  rm -rf "$TMP"
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php"
}
trap cleanup EXIT
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
: > "$OUT"
record() {
  local query=$1 role=$2 expected=$3
  status=$(curl -s -D "$TMP/headers" -o "$TMP/page" -w '%{http_code}' -b "hyscribe_role=$role" "$BASE/wp-admin/site-editor.php$query")
  [ "$status" = "$expected" ] || { echo "PHP answered $status for $role$query" >&2; exit 1; }
  location=$(sed -n 's/^Location: \(.*\)\r$/\1/p' "$TMP/headers")
  printf '%s\t%s\t%s\t%s\t%s\n' "$query" "$role" "$status" "$location" "$(od -An -v -tx1 "$TMP/page" | tr -d ' \n')" >> "$OUT"
}
for query in '' '?p=%2Fpage' '?postType=page&postId=2&canvas=edit'; do
  for role in editor author contributor subscriber; do
    record "$query" "$role" 403
  done
done
for query in '?postType=wp_navigation&postId=7&x=1' '?postType=wp_navigation' '?postType=wp_navigation&postId=0' \
  '?postType=wp_navigation&path=/wp_global_styles' '?path=/wp_global_styles&x=1' '?postType=page&path=/wp_global_styles' \
  '?postType=page' '?postType=page&postId=2' '?postType=page&postId=2&canvas=edit' '?canvas=0&postType=page&postId=2' \
  '?postType=page&postId=%27a%20b&canvas=1' '?postType=wp_template&path=/page' \
  '?postType=wp_template&postId=twentytwentyfive%2F%2Findex&canvas=edit' '?postType=wp_block&canvas=edit' \
  '?postType=wp_block&postId=5&canvas=1' '?postType=wp_template_part&x=y' '?postType=wp_template_part&postId=7' \
  '?postType=wp_template_part&postId=twentytwentyfive%2F%2Fheader&canvas=edit' '?path=/wp_template_part/all' \
  '?postType=x&path=/wp_template_part/all&q=1' '?path=/page' '?path=/wp_template' '?path=/patterns' \
  '?path=/navigation' '?path=/other' '?postId=2' '?postType=nope' '?POSTTYPE=page' '?canvas=edit'; do
  record "$query" administrator 302
done
for query in '?p=/&postType=nope' '?p=&postType=nope&canvas=edit' '?p=/page&postType=Page'; do
  record "$query" administrator 500
done
wc -l "$OUT"

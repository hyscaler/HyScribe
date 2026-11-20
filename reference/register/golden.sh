#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's registration form posted, the requests of
# reference/register/cases.list, as crates/hyscribe-admin/tests/golden/register.tsv:
#   query<TAB>state<TAB>form<TAB>status<TAB>location<TAB>cookies<TAB>hex(page)
# where state is register for a site anyone may register on (the must-use
# plugin reference/register/variants.php, removed after, which also takes
# every mail as sent) or 0, and cookies are the Set-Cookie headers but the test
# cookie's, joined by a space. The requests make users: start from a fresh
# db.sh up and reset it after. The rows they write are the write oracle's
# login-register.
set -euo pipefail
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-admin/tests/golden/register.tsv"
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp)
HEADERS=$(mktemp)
cleanup() {
  rm -f "$TMP" "$HEADERS"
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-register.php"
  "$ROOT/reference/admin-look/look.sh" remove || true
}
trap cleanup EXIT
"$ROOT/reference/admin-look/look.sh" add
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-register.php" < "$ROOT/reference/register/variants.php"
: > "$OUT"
grep -v '^#' "$ROOT/reference/register/cases.list" | while IFS= read -r line; do
  # cut, as read would merge the empty fields between tabs.
  query=$(cut -f 1 <<< "$line") state=$(cut -f 2 <<< "$line") form=$(cut -f 3 <<< "$line")
  : > "$TMP"
  args=(-s -D "$HEADERS" -o "$TMP" --data-raw "$form")
  [ "$state" = register ] && args+=(-b 'hyscribe_variant=register')
  curl "${args[@]}" "$BASE/wp-login.php${query:+?$query}" > /dev/null
  status=$(head -n 1 "$HEADERS" | cut -d ' ' -f 2)
  location=$(grep -i '^location:' "$HEADERS" | cut -d ' ' -f 2- | tr -d '\r' || true)
  cookies=$(grep -i '^set-cookie:' "$HEADERS" | grep -v wordpress_test_cookie | cut -d ' ' -f 2- | tr -d '\r' \
    | paste -sd ' ' || true)
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$query" "$state" "$form" "$status" "$location" "$cookies" \
    "$(xxd -p "$TMP" | tr -d '\n')" >> "$OUT"
done
wc -l "$OUT"

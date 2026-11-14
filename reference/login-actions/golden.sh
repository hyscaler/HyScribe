#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's login screen actions other than logging in and out,
# for GET requests, as crates/hyscribe-admin/tests/golden/login-actions.tsv:
#   query<TAB>state<TAB>status<TAB>location<TAB>cookies<TAB>hex(page)
# where state is 0 for no cookie, register for a site anyone may register on
# (reference/login-actions/variants.php, a must-use plugin removed after),
# referer for a Referer header, or rp:<value> for a reset password cookie of
# that value. cookies are the Set-Cookie headers but the test cookie's,
# joined by a space, with an expiry date replaced by %date%. Needs the default
# install oracle running (db.sh up, then php.sh up); it only reads.
set -euo pipefail
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-admin/tests/golden/login-actions.tsv"
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp)
HEADERS=$(mktemp)
cleanup() {
  rm -f "$TMP" "$HEADERS"
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-login-variants.php"
}
trap cleanup EXIT
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-login-variants.php" < "$ROOT/reference/login-actions/variants.php"
HASH=$(printf %s "$BASE" | md5sum | cut -d ' ' -f 1)
: > "$OUT"
record() {
  local query=$1 state=$2
  : > "$TMP"
  local args=(-s -D "$HEADERS" -o "$TMP")
  case "$state" in
    register) args+=(-b 'hyscribe_variant=register') ;;
    referer) args+=(-e "$BASE/sample-page/") ;;
    rp:*) args+=(-b "wp-resetpass-$HASH=${state#rp:}") ;;
  esac
  curl "${args[@]}" "$BASE/wp-login.php${query:+?$query}" > /dev/null
  local status location cookies
  status=$(head -n 1 "$HEADERS" | cut -d ' ' -f 2)
  location=$(grep -i '^location:' "$HEADERS" | cut -d ' ' -f 2- | tr -d '\r' || true)
  cookies=$(grep -i '^set-cookie:' "$HEADERS" | grep -v wordpress_test_cookie | cut -d ' ' -f 2- | tr -d '\r' \
    | sed -E 's/expires=[^;]*/expires=%date%/; s/Max-Age=[0-9]+/Max-Age=%age%/' | paste -sd ' ' || true)
  printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$query" "$state" "$status" "$location" "$cookies" "$(xxd -p "$TMP" | tr -d '\n')" >> "$OUT"
}
for query in 'action=lostpassword' 'action=retrievepassword' 'action=lostpassword&error=invalidkey' \
  'action=lostpassword&error=expiredkey' 'action=lostpassword&error=x' 'action=lostpassword&redirect_to=%2Fx%22' \
  'action=lostpassword&redirect_to%5B%5D=x' 'action=lostpassword&interim-login=1' 'action=register' \
  'action=rp' 'action=resetpass' 'key=abc&login=admin' 'action=rp&key=abc&login=a%20b%3Cc' 'key=x' \
  'checkemail=confirm' 'checkemail=registered' 'checkemail=x' 'checkemail=confirm&interim-login=1' 'action=checkemail' \
  'action=confirmaction' 'action=confirmaction&request_id=1' 'action=confirmaction&confirm_key=x' \
  'action=confirmaction&request_id=1&confirm_key=x' 'action=confirmaction&request_id=x&confirm_key=' 'action=postpass'; do
  record "$query" 0
done
for query in 'action=register' 'action=register&redirect_to=%2Fx%22' 'action=lostpassword' 'action=rp' \
  'action=register&interim-login=1'; do
  record "$query" register
done
record 'action=postpass' referer
for value in 'admin:abc' 'nobody:abc' ':abc' 'admin' 'admin:' 'admin:%21%21' 'a%20b:x:y'; do
  record 'action=rp' "rp:$value"
done
wc -l "$OUT"

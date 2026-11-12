#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's login screen, wp-login.php, for requests that do
# not log anyone in, as crates/hyscribe-admin/tests/golden/login.tsv:
#   method<TAB>query<TAB>body<TAB>state<TAB>status<TAB>hex(page)
# where state is 1 for the test cookie, 0 for no cookie, expired for an auth
# cookie that does not validate, resetpass for the test cookie and a reset
# password cookie, or use_ssl for the test cookie with every user's use_ssl
# option on (a must-use plugin filters it, and is removed after).
# user-profile's nonce is replaced by %nonce:reset-password-for-user%. Needs
# the default install oracle running (db.sh up, then php.sh up). Failed logins
# write nothing, so it only reads.
set -euo pipefail
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-admin/tests/golden/login.tsv"
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp)
HEADERS=$(mktemp)
cleanup() {
  rm -f "$TMP" "$HEADERS"
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-use-ssl.php"
}
trap cleanup EXIT
HASH=$(printf %s "$BASE" | md5sum | cut -d ' ' -f 1)
: > "$OUT"
record() {
  local method=$1 query=$2 body=$3 state=$4
  : > "$TMP"
  local args=(-s -D "$HEADERS" -o "$TMP")
  case "$state" in
    1 | use_ssl) args+=(-b 'wordpress_test_cookie=WP%20Cookie%20check') ;;
    expired) args+=(-b "wordpress_$HASH=admin|1|x|y") ;;
    resetpass) args+=(-b "wordpress_test_cookie=WP%20Cookie%20check; wp-resetpass-$HASH=%3A%3A%3Cb%3EAd%20%20min%3Akey") ;;
  esac
  [ "$method" = POST ] && args+=(--data-raw "$body")
  curl "${args[@]}" "$BASE/wp-login.php${query:+?$query}" > /dev/null
  local status
  status=$(head -n 1 "$HEADERS" | cut -d ' ' -f 2)
  sed -i -E 's/("nonce":")[0-9a-f]{10}"/\1%nonce:reset-password-for-user%"/' "$TMP"
  printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$method" "$query" "$body" "$state" "$status" "$(xxd -p "$TMP" | tr -d '\n')" >> "$OUT"
}
ADMIN="$BASE/wp-admin"
for query in '' 'action=login' 'action=bogus' 'loggedout=true' 'loggedout=0' 'reauth=1' 'interim-login=1' \
  'interim-login=1&loggedout=true' 'customize-login=1' 'registration=disabled' \
  "redirect_to=$ADMIN/edit.php%3Fpost_type%3Dpage%26x%3D%22%3E" 'redirect_to%5B%5D=x' \
  "redirect_to=$ADMIN/about.php%3Fupdated" "redirect_to=$ADMIN/authorize-application.php" \
  "redirect_to=$ADMIN/authorize-application.php%3Fapp_name%3DApp%253Cb%253E" \
  "redirect_to=$ADMIN/authorize-application.php%3Fapp_name%3D" 'loggedout=true&registration=disabled' "redirect_to=$ADMIN/a%00'b%5C"; do
  record GET "$query" '' 0
done
for body in 'log=&pwd=&testcookie=1' 'log=admin&pwd=&testcookie=1' 'log=nobody&pwd=x&testcookie=1' \
  'log=admin&pwd=wrong&testcookie=1' 'log=admin&pwd=wrong&rememberme=forever&testcookie=1' \
  'log=nobody%40default-install.test&pwd=x&testcookie=1' 'log=admin%40default-install.test&pwd=wrong&testcookie=1' \
  'log=%3Cb%3Eadmin%22&pwd=wrong&testcookie=1' 'log=admin&pwd=wrong&interim-login=1' \
  'log=admin&pwd=wrong&redirect_to=%2Fx&testcookie=1' 'log=&pwd=x' 'pwd=x'; do
  record POST '' "$body" 1
done
record POST '' 'log=admin&pwd=wrong&testcookie=1' 0
record POST '' 'log=&pwd=&testcookie=1' 0
record POST 'reauth=1' 'log=admin&pwd=wrong' 1
record GET 'action=entered_recovery_mode' '' 0
record GET '' '' expired
record POST '' 'log=&pwd=' expired
record POST 'loggedout=0' 'log=admin&pwd=wrong&testcookie=1' 1
record POST 'loggedout=1' 'log=admin&pwd=wrong&testcookie=1' 1
record GET '' '' resetpass
record POST '' 'log=admin&pwd=wrong&testcookie=1' resetpass
record POST '' 'pwd=x&testcookie=1' resetpass
"$DOCKER" exec "$NAME" sh -c "mkdir -p $MU && printf '%s\n' '<?php' \"add_filter( 'get_user_option_use_ssl', '__return_true' );\" > $MU/hyscribe-use-ssl.php"
for body in 'log=admin&pwd=wrong&testcookie=1' 'log=admin%40default-install.test&pwd=wrong&testcookie=1' \
  'log=nobody&pwd=x&testcookie=1' 'log=%40admin&pwd=x&testcookie=1' \
  "log=admin&pwd=wrong&redirect_to=$ADMIN/x&testcookie=1" 'log=admin&pwd=wrong&redirect_to=http://example.test/x&testcookie=1'; do
  record POST '' "$body" use_ssl
done
record GET '' '' use_ssl
wc -l "$OUT"

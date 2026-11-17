#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's lost password and reset password flows, the
# requests of reference/lost-password/cases.list over the users of
# reference/lost-password/content.sql (added, and deleted after), as
# crates/hyscribe-admin/tests/golden/lost-password.tsv:
#   method<TAB>query<TAB>cookie<TAB>form<TAB>status<TAB>location<TAB>cookies<TAB>hex(page)
# where cookie is the reset password cookie's value and cookies the
# Set-Cookie headers but the test cookie's, joined by a space, with an expiry
# date replaced by %date%. The reset form's suggested password is replaced
# by %pw%, the time of utils' settings by %time% and user-profile's nonce by
# %nonce:reset-password-for-user%. Mail is
# taken as sent, as HyScribe answers until its transport sends it (ADR 0016),
# by the must-use plugin variants.php, removed after. The requests write
# reset keys and a password: start from a fresh db.sh up and reset it after.
set -euo pipefail
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-admin/tests/golden/lost-password.tsv"
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp)
HEADERS=$(mktemp)
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress; }
cleanup() {
  rm -f "$TMP" "$HEADERS"
  echo "DELETE FROM wp_users WHERE ID BETWEEN 701 AND 708; DELETE FROM wp_usermeta WHERE user_id BETWEEN 701 AND 708;" | sql
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-lost-password.php"
}
trap cleanup EXIT
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-lost-password.php" < "$ROOT/reference/lost-password/variants.php"
sql < "$ROOT/reference/lost-password/content.sql"
HASH=$(printf %s "$BASE" | md5sum | cut -d ' ' -f 1)
: > "$OUT"
grep -v '^#' "$ROOT/reference/lost-password/cases.list" | while IFS= read -r line; do
  # cut, as read would merge the empty fields between tabs.
  method=$(cut -f 1 <<< "$line") query=$(cut -f 2 <<< "$line")
  cookie=$(cut -f 3 <<< "$line") form=$(cut -f 4 <<< "$line")
  : > "$TMP"
  args=(-s -D "$HEADERS" -o "$TMP")
  [ -n "$cookie" ] && args+=(-b "wp-resetpass-$HASH=$cookie")
  [ "$method" = POST ] && args+=(--data-raw "$form")
  curl "${args[@]}" "$BASE/wp-login.php${query:+?$query}" > /dev/null
  status=$(head -n 1 "$HEADERS" | cut -d ' ' -f 2)
  location=$(grep -i '^location:' "$HEADERS" | cut -d ' ' -f 2- | tr -d '\r' || true)
  cookies=$(grep -i '^set-cookie:' "$HEADERS" | grep -v wordpress_test_cookie | cut -d ' ' -f 2- | tr -d '\r' \
    | sed -E 's/expires=[^;]*/expires=%date%/; s/Max-Age=[0-9]+/Max-Age=%age%/' | paste -sd ' ' || true)
  sed -i -E -e 's/data-pw="[^"]*"/data-pw="%pw%"/' -e 's/("time":")[0-9]+"/\1%time%"/' -e 's/("nonce":")[0-9a-f]{10}"/\1%nonce:reset-password-for-user%"/' "$TMP"
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$method" "$query" "$cookie" "$form" "$status" "$location" "$cookies" \
    "$(xxd -p "$TMP" | tr -d '\n')" >> "$OUT"
done
wc -l "$OUT"

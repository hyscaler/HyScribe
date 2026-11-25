#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the front end as a visitor and as each role logged in, over the
# posts of reference/private-views/content.sql in every status, for each
# request of reference/private-views/views.list, as
# crates/hyscribe-http/tests/golden/private-views.tsv:
#   options<TAB>hex(path and query)<TAB>status<TAB>location<TAB>Cache-Control<TAB>Expires<TAB>time<TAB>sha256(body without its RSD link)
# where options are as reference/pages/golden.sh takes them, `_user` names
# the role logged in (none for a visitor), time is the answer's Date in Unix
# seconds (a draft's preview is dated then), and the pages themselves are
# kept in target/pages/private-views.tsv for reading differences. A path's
# %nonce<id>% is the user's own preview nonce for post <id>.
# Needs reference/default-install/php.sh up over a fresh db.sh up; it adds
# reference/rest/users.sql's users and content.sql and logs in, which
# writes sessions, so reset the database after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
BASE="${HYSCRIBE_PHP_BASE:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-http/tests/golden/private-views.tsv"
RAW="$ROOT/target/pages/private-views.tsv"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp -d)
cleanup() {
  rm -rf "$TMP"
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-options.php" "$MU/hyscribe-nonce.php"
}
trap cleanup EXIT
"$DOCKER" exec "$NAME" mkdir -p "$MU"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-options.php" < "$ROOT/reference/pages/options.php"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-nonce.php" < "$ROOT/reference/private-views/nonce.php"
"$DOCKER" exec "$DB" mariadb -uroot -preference-only wordpress -e "DELETE FROM wp_posts WHERE post_type = 'wp_navigation'"
cat "$ROOT/reference/rest/users.sql" "$ROOT/reference/private-views/content.sql" |
  "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress
# The first page view writes caches the later ones read.
curl -s -o /dev/null "$BASE/"
for who in administrator editor author contributor subscriber; do
  login=$who
  [ "$who" = administrator ] && login=admin
  curl -s -o /dev/null -c "$TMP/$who.jar" -b 'wordpress_test_cookie=WP%20Cookie%20check' \
    --data-urlencode "log=$login" --data-urlencode 'pwd=reference-only-password' -d 'testcookie=1' "$BASE/wp-login.php"
done
mkdir -p "$(dirname "$RAW")"
: > "$OUT"
: > "$RAW"
while IFS= read -r line; do
  # Not by `read`, which drops a leading tab, and so empty options.
  options=${line%%$'\t'*}
  path=${line#*$'\t'}
  who=$(python3 -I -c 'import sys, urllib.parse as u; print(u.parse_qs(sys.argv[1]).get("_user", [""])[0], end="")' "$options")
  jar=()
  [ -z "$who" ] || jar=(-b "$TMP/$who.jar")
  asked=$path
  while [[ "$asked" =~ %nonce([0-9]+)% ]]; do
    id=${BASH_REMATCH[1]}
    nonce=$(curl -s "${jar[@]}" "$BASE/?hyscribe_preview_nonce=$id")
    asked=${asked//%nonce$id%/$nonce}
  done
  curl -g -s -D "$TMP/headers" -o "$TMP/body" -H "X-HyScribe-Options: $options" "${jar[@]}" "$BASE$asked" > /dev/null
  header() { grep -i "^$1:" "$TMP/headers" | cut -d ' ' -f 2- | tr -d '\r' || true; }
  status=$(head -n 1 "$TMP/headers" | cut -d ' ' -f 2)
  date=$(date -u -d "$(header date)" +%s)
  key=$(printf '%s' "$path" | xxd -p | tr -d '\n')
  # The page without its RSD link, which HyScribe does not print, and its hash.
  { read -r page; read -r hash; } < <(python3 -I "$ROOT/reference/pages/ttd-body.py" "$TMP/body")
  row=$(printf '%s\t%s\t%s\t%s\t%s\t%s\t%s' "$options" "$key" "$status" "$(header location)" "$(header cache-control)" "$(header expires)" "$date")
  printf '%s\t%s\n' "$row" "$hash" >> "$OUT"
  printf '%s\t%s\n' "$row" "$page" >> "$RAW"
done < "$ROOT/reference/private-views/views.list"
wc -l "$OUT"

#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records whole pages for each request of reference/pages/<list>.list
# (default or ttd) as crates/hyscribe-http/tests/golden/pages-<list>.tsv:
#   options<TAB>hex(path and query)<TAB>status<TAB>location<TAB>body
# where options are the list's (reference/pages/options.php, installed as a
# must-use plugin for the run and removed after) and the body is hex, or
# over the Theme Test Data, whose content is not ours to commit, the
# SHA-256 of the page without its RSD link, with the page itself in
# target/pages/ttd.tsv for reading differences (reference/pages/ttd-body.py
# says how a page with a gallery in random order is hashed, an open question).
#   default: reference/default-install/php.sh up
#   ttd:     HYSCRIBE_PHP_DB=wordpress_saves HYSCRIBE_PHP_UPLOADS=$PWD/target/fixtures/theme-test-data/7.1.3/uploads reference/default-install/php.sh up
#   over the Theme Test Data fixture loaded into wordpress_saves with
#   reference/pages/ttd-ties.sql, which tells apart the terms of one name
#   that MariaDB would order differently from run to run (the Rust test
#   loads the same, see hyscribe_testkit::writes::load_theme_test_data)
#   site-edits: reference/default-install/php.sh up, over the site editor's
#   saves in reference/site-edits/content.sql, which it adds
#   comments: reference/default-install/php.sh up, over the comments of
#   reference/pages/comments.sql, which it adds, with the commenter cookies
#   an option named _cookie sends (urlencoded, `name=value; name=value`)
# The ttd run also writes the day it ran as pages-ttd.clock (noon UTC, Unix
# seconds): the hashes hold the calendar's today, so the Rust test answers
# them at that time.
# Page views write: the wp_navigation post php.sh up wrote is
# deleted first (site-edits: replaced), so start from a fresh db.sh up and
# reset it after.
set -euo pipefail

LIST="${1:?usage: $0 default|ttd|site-edits|comments}"
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
DATABASE=wordpress
[ "$LIST" = ttd ] && DATABASE=wordpress_saves
BASE="${HYSCRIBE_PHP_BASE:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-http/tests/golden/pages-$LIST.tsv"
RAW="$ROOT/target/pages/$LIST.tsv"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp)
HEADERS=$(mktemp)
cleanup() {
  rm -f "$TMP" "$HEADERS"
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-options.php"
}
trap cleanup EXIT
if [ "$LIST" = ttd ]; then
  # The RSS blocks read the test site, as HyScribe's test does.
  . "$ROOT/reference/url-details/serve.sh"
  trap 'cleanup; serve_cleanup' EXIT
fi
"$DOCKER" exec "$NAME" mkdir -p "$MU"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-options.php" < "$ROOT/reference/pages/options.php"
if [ "$LIST" = ttd ]; then
  cat "$ROOT/target/fixtures/theme-test-data/7.1.3/database.sql" "$ROOT/reference/pages/ttd-ties.sql" "$ROOT/reference/pages/ttd-feeds.sql" |
    "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only "$DATABASE"
fi
if [ "$LIST" = site-edits ]; then
  "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only "$DATABASE" < "$ROOT/reference/site-edits/content.sql"
elif [ "$LIST" = comments ]; then
  "$DOCKER" exec "$DB" mariadb -uroot -preference-only "$DATABASE" -e "DELETE FROM wp_posts WHERE post_type = 'wp_navigation'"
  "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only "$DATABASE" < "$ROOT/reference/pages/comments.sql"
else
  "$DOCKER" exec "$DB" mariadb -uroot -preference-only "$DATABASE" -e "DELETE FROM wp_posts WHERE post_type = 'wp_navigation'"
fi
# The first page view writes caches the later ones read.
curl -s -o /dev/null "$BASE/"
mkdir -p "$(dirname "$RAW")"
if [ "$LIST" = ttd ]; then
  date -u -d "$(date -u +%F) 12:00:00" +%s > "${OUT%.tsv}.clock"
fi
: > "$OUT"
: > "$RAW"
while IFS= read -r line; do
  # Not by `read`, which drops a leading tab, and so empty options.
  options=${line%%$'\t'*}
  path=${line#*$'\t'}
  cookie=$(python3 -I -c 'import sys, urllib.parse as u; print(u.parse_qs(sys.argv[1], keep_blank_values=True).get("_cookie", [""])[0], end="")' "$options")
  curl -g -s -D "$HEADERS" -o "$TMP" -H "X-HyScribe-Options: $options" ${cookie:+-b "$cookie"} "$BASE$path" > /dev/null
  status=$(head -n 1 "$HEADERS" | cut -d ' ' -f 2)
  location=$(grep -i '^location:' "$HEADERS" | cut -d ' ' -f 2 | tr -d '\r' || true)
  key=$(printf '%s' "$path" | xxd -p | tr -d '\n')
  if [ "$LIST" = ttd ]; then
    { read -r page; read -r body; } < <(python3 -I "$ROOT/reference/pages/ttd-body.py" "$TMP")
    printf '%s\t%s\t%s\t%s\t%s\n' "$options" "$key" "$status" "$location" "$page" >> "$RAW"
  else
    body=$(xxd -p "$TMP" | tr -d '\n')
  fi
  printf '%s\t%s\t%s\t%s\t%s\n' "$options" "$key" "$status" "$location" "$body" >> "$OUT"
done < "$ROOT/reference/pages/$LIST.list"
wc -l "$OUT"

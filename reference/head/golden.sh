#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the <head> PHP WordPress prints for each page of the default
# install as crates/hyscribe-render/tests/golden/head.tsv, and what
# wp_footer() prints after the theme's footer as footer.tsv: one line per
# URL,
#   hex(path and query)<TAB>hex(head or footer)
# and the whole page as pages.tsv,
#   hex(path and query)<TAB>status<TAB>hex(page)
# then paged.tsv and singular-pages.tsv, described where they are made.
# Needs the default install oracle running (reference/default-install/db.sh
# up, then php.sh up). Page views write to the database, so start
# from a fresh db.sh up.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
BASE="${HYSCRIBE_PHP_BASE:-http://127.0.0.1:8081}"
OUT="$ROOT/crates/hyscribe-render/tests/golden/head.tsv"
FOOTER="$ROOT/crates/hyscribe-render/tests/golden/footer.tsv"
PAGES="$ROOT/crates/hyscribe-render/tests/golden/pages.tsv"

mkdir -p "$(dirname "$OUT")"
: > "$OUT"
: > "$FOOTER"
: > "$PAGES"
TMP=$(mktemp)
trap 'rm -f "$TMP"' EXIT
for path in / '/?p=1' '/?page_id=2' '/?p=1&replytocom=1' '/?p=1&cpage=2' '/?p=1&cpage=abc' '/?s=hello' '/?s=' '/?cat=1' '/?m=2026' '/?m=202610' \
  '/?m=20261027' '/?cat=1&m=2026' '/?p=999' '/?cat=999' '/no/such/path/' \
  '/?paged=2' '/?paged=1' '/?paged=0' '/?paged=-1' '/?paged=1x' '/?page=2' '/page/2/' '/?s=hello&paged=1' '/?s=a&paged=2' \
  '/?m=2026&paged=2' '/?cat=1&paged=1' '/?cat=1&paged=2' '/?p=1&paged=2' '/?page_id=2&page=2' \
  '/?p=1&page=1' '/?p=1&page=abc' '/?p=1&page=0' '/?p=1&cpage=2&page=2' '/?page=2&paged=3' '/?s=hello&page=2' '/?cat=1&page=3' \
  '/?s=nothingmatches&paged=1' '/?m=2026&s=nothingmatches' '/?paged=9223372036854775807' '/?paged=99999999999999999999' \
  '/?cat=5,1' '/?cat=,1' '/?cat=-5,1' '/?cat=1,-1' '/?cat=-1' '/?cat=5,6' '/?cat=1%2C5' '/?cat=abc' '/?cat=-1&paged=2'; do
  status=$(curl -s -o "$TMP" -w '%{http_code}' "$BASE$path")
  page=$(xxd -p "$TMP" | tr -d '\n')
  head=$(printf '%s' "$page" | python3 -I -c 'import sys; s=bytes.fromhex(sys.stdin.read()); i=s.find(b"<head>"); j=s.find(b"</head>"); sys.stdout.write((s[i:j+7]).hex())')
  # The theme's footer part closes .wp-site-blocks; wp_footer() follows.
  footer=$(printf '%s' "$page" | python3 -I -c 'import sys; s=bytes.fromhex(sys.stdin.read()); m=b"</footer>\n</div>\n"; i=s.rfind(m)+len(m); j=s.rfind(b"</body>"); sys.stdout.write((s[i:j]).hex())')
  key=$(printf '%s' "$path" | xxd -p | tr -d '\n')
  printf '%s\t%s\n' "$key" "$head" >> "$OUT"
  printf '%s\t%s\n' "$key" "$footer" >> "$FOOTER"
  printf '%s\t%s\t%s\n' "$key" "$status" "$page" >> "$PAGES"
done

# Pages past the first, with one post a page; the option goes back after.
PAGED="$ROOT/crates/hyscribe-render/tests/golden/paged.tsv"
DOCKER="${DOCKER:-docker}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
per_page() {
  "$DOCKER" exec "$DB" mariadb -uroot -preference-only wordpress \
    -e "UPDATE wp_options SET option_value = '$1' WHERE option_name = 'posts_per_page'"
}
per_page 1
trap 'per_page 10; rm -f "$TMP"' EXIT
: > "$PAGED"
for path in '/?s=a' '/?s=a&paged=2' '/?s=a&paged=3' '/?s=a&page=2' '/?s=a&paged=2&x[]=1' '/?paged=1'; do
  status=$(curl -s -o "$TMP" -w '%{http_code}' "$BASE$path")
  key=$(printf '%s' "$path" | xxd -p | tr -d '\n')
  printf '%s\t%s\t%s\n' "$key" "$status" "$(xxd -p "$TMP" | tr -d '\n')" >> "$PAGED"
done

# A post in pages, by the page asked for, inserted for these requests and
# deleted after: hex(query)<TAB>status<TAB>location<TAB>hex(title), the
# post's ID and content first.
SINGULAR="$ROOT/crates/hyscribe-render/tests/golden/singular-pages.tsv"
CONTENT='<!-- wp:paragraph --><p>One</p><!-- /wp:paragraph --><!-- wp:nextpage --><!--nextpage--><!-- /wp:nextpage --><!-- wp:paragraph --><p>Two</p><!-- /wp:paragraph -->'
php_run() { "$DOCKER" exec -i "${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}" php; }
# The content has no single quotes, so it goes in a PHP string as it is.
id=$(printf '<?php require "/var/www/html/wp-load.php"; echo wp_insert_post( array( "post_title" => "Paged", "post_status" => "publish", "post_content" => '"'"'%s'"'"' ) );' "$CONTENT" | php_run)
delete_post() { printf '<?php require "/var/www/html/wp-load.php"; wp_delete_post( %s, true );' "$id" | php_run >/dev/null; }
trap 'per_page 10; delete_post; rm -f "$TMP"' EXIT
printf '%s\t%s\n' "$id" "$(printf '%s' "$CONTENT" | xxd -p | tr -d '\n')" > "$SINGULAR"
for q in '' '&page=1' '&page=2' '&page=3' '&page=abc' '&page=0' '&page=2/' '&page=02' '&page=-2' '&paged=2' '&page=3&x=1'; do
  query="p=$id$q"
  out=$(curl -s -o "$TMP" -w '%{http_code}\t%{redirect_url}' "$BASE/?$query")
  title=$( (grep -o '<title>[^<]*</title>' "$TMP" || true) | head -n 1 | sed 's/<[^>]*>//g' | tr -d '\n' | xxd -p | tr -d '\n')
  printf '%s\t%s\t%s\n' "$(printf '%s' "$query" | xxd -p | tr -d '\n')" "$out" "$title" >> "$SINGULAR"
done
wc -l "$OUT" "$FOOTER" "$PAGES" "$PAGED" "$SINGULAR"

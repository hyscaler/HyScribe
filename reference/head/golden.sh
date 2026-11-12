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
# then terms.tsv, weeks.tsv, paged.tsv and singular-pages.tsv, described
# where they are made.
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
  '/?cat=5,1' '/?cat=,1' '/?cat=-5,1' '/?cat=1,-1' '/?cat=-1' '/?cat=5,6' '/?cat=1%2C5' '/?cat=abc' '/?cat=-1&paged=2' \
  '/?year=2026' '/?monthnum=10' '/?year=2026&monthnum=10&day=27' '/?day=27' '/?year=2025' '/?year=2025&m=2026' '/?monthnum=13' \
  '/?m=202610&day=27' '/?year=abc' '/?day=40' '/?year=2024&monthnum=2&day=29' '/?year=2026&monthnum=10&day=27&m=2026' '/?year=2026&paged=2' \
  '/?p=-1' '/?p=-1&cat=1' '/?p=0' '/?p=abc' '/?page_id=0' '/?p=-1&page_id=3' '/?year=2026&day=27' '/?monthnum=10&day=27' \
  '/?monthnum=12' '/?m=2026&monthnum=11' '/?m=202610&monthnum=11' '/?m=20261027&day=26' '/?m=202610&monthnum=10' '/?year=2000&monthnum=2&day=29' \
  '/?name=hello-world' '/?name=Hello-World' '/?name=hello%20world' '/?name=nope' '/?pagename=sample-page' '/?pagename=Sample-Page' \
  '/?pagename=/sample-page/' '/?pagename=nope' '/?name=hello-world&cpage=2' '/?name=%20' '/?name=hello-world&post_type=page' \
  '/?name=sample&page=2' '/?name=sam&post_type=attachment' '/?name=hello-world&post_type=attachment' \
  '/?category_name=uncategorized' '/?taxonomy=category&term=uncategorized' '/?category_name=nope' '/?category_name=uncategorized,nope' \
  '/?tag=foo' '/?post_format=aside' '/?taxonomy=foo&term=x' '/?category_name=/' '/?taxonomy=category&term=nope&cat=1,,2' '/?feed=comments-rss2&cat=-1' \
  '/?hour=8' '/?hour=9' '/?minute=8' '/?second=15' '/?hour=8&minute=8' '/?hour=8&second=99' '/?hour=8&minute=9' '/?minute=8&hour=100' \
  '/?hour=8&minute=8&second=15' '/?minute=08&second=15' '/?w=44' '/?w=43' '/?w=0' '/?w=abc' '/?hour=8&day=40' '/?hour=8&year=2026' \
  '/?hour=8&m=2026' '/?w=44&m=2026' '/?m=2026102708' '/?m=202610270808' '/?m=20261027080815' '/?m=2026102709' '/?hour=24' '/?hour=abc' \
  '/?hour=' '/?hour=8&paged=2' '/?hour=8&year=2026&monthnum=2&day=30' '/?hour=8&year=2026&monthnum=13' '/?hour=8&feed=rss2' '/?w=44&feed=rss2' \
  '/?hour=8&minute=8&second=14999' '/?minute=8&second=14999' '/?hour=8&year=2026&monthnum=2&day=30&s=hello' \
  '/?hour=8&year=2026&monthnum=2&day=30&category_name=uncategorized' '/?hour=9&year=2026&monthnum=13' \
  '/?exact=1&s=hello' '/?exact=1&s=Hello+world!' '/?exact=0&s=hello' '/?sentence=1&s=hello+world' '/?sentence=1&s=world+hello' \
  '/?sentence=1&exact=1&s=Hello+world!' '/?exact=1&s=%22Hello+world!%22' '/?sentence=1&s=a' '/?exact=1&s=hello&feed=rss2' \
  '/?sentence=1&s=hello%0Aworld' '/?sentence=1&s=sample' '/?sentence=1&exact=1&s=' \
  '/?attachment_id=999' '/?attachment_id=0' '/?attachment_id=abc' '/?attachment_id=3' '/?subpost_id=999' '/?subpost=foo' '/?attachment=foo' \
  '/?name=!!' '/?attachment=%20' '/?attachment=!!' '/?subpost_id=0&attachment_id=1' '/?subpost_id=abc&attachment_id=1' '/?attachment_id=1' \
  '/?sitemap=index' '/?sitemap=posts&sitemap-subtype=post&paged=1' '/?sitemap=posts&sitemap-subtype=page&paged=1' \
  '/?sitemap=taxonomies&sitemap-subtype=category&paged=1' '/?sitemap-stylesheet=index' '/?sitemap-stylesheet=sitemap' \
  '/?sitemap=posts&sitemap-subtype=post' '/?sitemap=posts&sitemap-subtype=post&paged=2' '/?sitemap=posts&sitemap-subtype=page&paged=2' \
  '/?sitemap=nope' '/?sitemap=posts&sitemap-subtype=attachment' '/?sitemap=posts' '/?sitemap=taxonomies&sitemap-subtype=post_tag&paged=1' \
  '/?sitemap-stylesheet=nope' '/?sitemap=%20' '/?sitemap=index&sitemap-stylesheet=sitemap' '/?sitemap=posts&sitemap-subtype=post&paged=0' \
  '/?sitemap=wp_block' '/?sitemap=' '/?robots=1' '/?robots=abc' '/?robots=0' '/?robots=' '/?robots=1&feed=rss2' '/?robots=1&sitemap=index' \
  '/?robots=1&p=1' '/?robots=1&p=2' '/?robots=1&p=1.' '/?sitemap=0' '/?sitemap-stylesheet=0' '/?sitemap=index&sitemap-stylesheet=+0+' \
  '/?sitemap=%3Cb%3Eindex%3C/b%3E' '/?sitemap=ind%2525ex' '/?sitemap=index&p=999' '/?sitemap=posts&sitemap-subtype=%20post%20'; do
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

# Term archives, with reference/default-install/terms.sql added for these
# requests and taken out after, as terms.tsv in the format of pages.tsv.
TERMS="$ROOT/crates/hyscribe-render/tests/golden/terms.tsv"
DOCKER="${DOCKER:-docker}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress; }
remove_terms() {
  echo 'DELETE FROM wp_term_relationships WHERE term_taxonomy_id IN (2, 3, 4, 5);
    DELETE FROM wp_term_taxonomy WHERE term_taxonomy_id IN (2, 3, 4, 5); DELETE FROM wp_terms WHERE term_id IN (2, 3, 4, 5);
    DELETE FROM wp_posts WHERE ID IN (101, 102); DELETE FROM wp_usermeta WHERE umeta_id = 101;' | sql
}
sql < "$ROOT/reference/default-install/terms.sql"
trap 'remove_terms; rm -f "$TMP"' EXIT
: > "$TERMS"
for path in '/?tag=foo' '/?tag=Foo' '/?tag=empty' '/?tag=nope' '/?tag=foo&paged=2' '/?tag=empty&paged=2' '/?tag=foo&paged=1' \
  '/?tag=foo,nope' '/?tag=foo+nope' '/?tag=%20' '/?tag=foo&cat=1' '/?s=hello&tag=foo' '/?m=2026&tag=foo' '/?tag=foo&feed=rss2' \
  '/?post_format=aside' '/?post_format=post-format-aside' '/?post_format=chat' '/?post_format=,post-format-aside' \
  '/?post_format=aside&feed=rss2' '/?taxonomy=post_tag&term=foo' '/?taxonomy=foo&term=x' '/?s=hello&taxonomy=foo&term=x' \
  '/?category_name=uncategorized,nope' '/?p=1' '/?feed=rss2' '/' '/?post_format=aside&page=2' '/?tag=foo&page=2' \
  '/?sitemap=index' '/?sitemap=taxonomies&sitemap-subtype=post_tag&paged=1' '/?sitemap=taxonomies&sitemap-subtype=post_format&paged=1' \
  '/?sitemap=posts&sitemap-subtype=post&paged=1' '/?sitemap=posts&sitemap-subtype=wp_block&paged=1' \
  '/?sitemap=taxonomies&sitemap-subtype=link_category&paged=1' '/?post_format=aside&post_type=attachment' '/?post_format=aside&s=hello&post_type=attachment'; do
  status=$(curl -s -o "$TMP" -w '%{http_code}' "$BASE$path")
  key=$(printf '%s' "$path" | xxd -p | tr -d '\n')
  printf '%s\t%s\t%s\n' "$key" "$status" "$(xxd -p "$TMP" | tr -d '\n')" >> "$TERMS"
done
remove_terms
trap 'rm -f "$TMP"' EXIT

# Week archives under other first days of the week, as weeks.tsv:
# start_of_week<TAB>hex(path)<TAB>status<TAB>hex(page); the option goes
# back to 1 after.
WEEKS="$ROOT/crates/hyscribe-render/tests/golden/weeks.tsv"
start_of_week() {
  "$DOCKER" exec "$DB" mariadb -uroot -preference-only wordpress \
    -e "UPDATE wp_options SET option_value = '$1' WHERE option_name = 'start_of_week'"
}
trap 'start_of_week 1; rm -f "$TMP"' EXIT
: > "$WEEKS"
for day in 0 3; do
  start_of_week "$day"
  for path in '/?w=42' '/?w=43' '/?w=44'; do
    status=$(curl -s -o "$TMP" -w '%{http_code}' "$BASE$path")
    key=$(printf '%s' "$path" | xxd -p | tr -d '\n')
    printf '%s\t%s\t%s\t%s\n' "$day" "$key" "$status" "$(xxd -p "$TMP" | tr -d '\n')" >> "$WEEKS"
  done
done
start_of_week 1
trap 'rm -f "$TMP"' EXIT

# Pages past the first, with one post a page; the option goes back after.
PAGED="$ROOT/crates/hyscribe-render/tests/golden/paged.tsv"
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
wc -l "$OUT" "$FOOTER" "$PAGES" "$TERMS" "$WEEKS" "$PAGED" "$SINGULAR"

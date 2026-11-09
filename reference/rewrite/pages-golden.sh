#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records whole pages and redirects under pretty permalinks, and under
# plain ones with a page on the front or for posts, as
# crates/hyscribe-http/tests/golden/pretty-pages.tsv:
#   structure|category base|tag base|page on front|page for posts<TAB>hex(path and query)<TAB>status<TAB>location<TAB>hex(body)
# reference/rewrite/permalinks.php is installed as a must-use plugin for the
# run and removed after; it sets the permalink options by filter. Page views
# write to the database, so start from a fresh db.sh up, then
# php.sh up.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-http/tests/golden/pretty-pages.tsv"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp)
HEADERS=$(mktemp)
cleanup() {
  rm -f "$TMP" "$HEADERS"
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-permalinks.php"
}
trap cleanup EXIT
"$DOCKER" exec "$NAME" mkdir -p "$MU"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-permalinks.php" < "$ROOT/reference/rewrite/permalinks.php"

PATHS=(
  / /hello-world/ /2026/10/27/hello-world/ /sample-page/ /category/uncategorized/ /2026/ /2026/10/ /2026/10/27/
  /page/2/ /feed/ /feed/atom/ /comments/feed/ /hello-world/feed/ /2026/10/27/hello-world/feed/ /category/uncategorized/feed/
  /search/hello/ /archives/1 /index.php/hello-world/ /uncategorized/hello-world/ /nope/
  '/?p=1' '/?page_id=2' '/?cat=1' '/?m=2026' '/?m=202610' '/?m=20261027' '/?feed=rss2' '/?feed=comments-rss2' '/?s=hello'
  '/?paged=2' '/?p=1&cpage=2' '/?p=1&page=2' '/?p=1&feed=rss2' '/?cat=1&feed=rss2' '/?p=1&embed=true' '/?p=1&tb=1'
  /hello-world /2026/10/27/hello-world /sample-page /wp-sitemap.xml /wp-sitemap-posts-post-1.xml '/?sitemap=index' /robots.txt
  /hello-world/comment-page-2/ /topics/uncategorized/ '/?year=2026&monthnum=10' '/?year=2026' '/?name=hello-world'
  '/?pagename=sample-page' /sample-page/2/ /hello-world/2/ '/?cat=1&paged=2' /page/1/ '/hello-world/?replytocom=1' '/?p=999'
  /wp-sitemap-taxonomies-category-1.xml /feed/rss/ /hello-world/feed/rss2/ '/?feed=rss' '/?p=1&preview=true' '/?p=1.'
  '/hello-world/?x=1' '/?page_id=2&x=y' '/?page=2&paged=0' '/?page=2' '/?pagename=' '/?x=1' '/?cat=' '/?year=' /sample-page/feed/ '/?page_id=2&feed=rss2'
)
# Under a page for posts only: elsewhere `paged` on a page is redirected
# by a URL rebuilt without the port, which the chained-redirect guard then
# turns down, so the oracle's port decides it.
POSTS_PATHS=(/sample-page/page/2/ '/?page_id=2&paged=2' '/?page_id=2&page=2' '/?pagename=sample-page&page=2' '/?page_id=2&cpage=2' '/?page_id=x')
# The draft privacy policy page as the page for posts.
DRAFT_PATHS=('/?pagename=privacy-policy' '/?page_id=3' /privacy-policy/ '/?pagename=privacy-policy&year=2025' '/?pagename=privacy-policy&s=zzz')
SETUPS=('/%year%/%monthnum%/%day%/%postname%/|||' '/%postname%/|||' '/archives/%post_id%|||' '/index.php/%postname%/|||' '/%category%/%postname%/|||'
  '/%postname%/|topics|labels|2' '|||2' '/%postname%/|||0|2' '||||2' '/%postname%/|||0|3')
: > "$OUT"
for setup in "${SETUPS[@]}"; do
  paths=("${PATHS[@]}")
  case $setup in
    *'|'*'|'*'|'*'|2') paths+=("${POSTS_PATHS[@]}") ;;
    *'|'*'|'*'|'*'|3') paths+=("${DRAFT_PATHS[@]}") ;;
  esac
  for path in "${paths[@]}"; do
    curl -s -D "$HEADERS" -o "$TMP" -H "X-HyScribe-Permalinks: $setup" "$BASE$path" > /dev/null
    status=$(head -n 1 "$HEADERS" | cut -d ' ' -f 2)
    location=$(grep -i '^location:' "$HEADERS" | cut -d ' ' -f 2 | tr -d '\r' || true)
    printf '%s\t%s\t%s\t%s\t%s\n' "$setup" "$(printf '%s' "$path" | xxd -p | tr -d '\n')" "$status" "$location" "$(xxd -p "$TMP" | tr -d '\n')" >> "$OUT"
  done
done
wc -l "$OUT"

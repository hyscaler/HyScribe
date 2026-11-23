#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records what WP::parse_request() makes of requests under a range of
# permalink settings, as crates/hyscribe-http/tests/golden/parse-request.tsv:
#   structure|category base|tag base|page on front<TAB>hex(path and query)<TAB>hex(JSON of the matched rule, matched query and query vars)
# reference/rewrite/permalinks.php is installed as a must-use plugin for the
# run and removed after. Needs the default install oracle running (db.sh up,
# then php.sh up). Only reads from it.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-http/tests/golden/parse-request.tsv"
MU=/var/www/html/wp-content/mu-plugins
cleanup() { "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-permalinks.php"; }
trap cleanup EXIT
"$DOCKER" exec "$NAME" mkdir -p "$MU"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-permalinks.php" < "$ROOT/reference/rewrite/permalinks.php"

PATHS=(
  / /index.php /index.php/ /hello-world/ /hello-world /hello-world/2/ /hello-world/feed/ /hello-world/trackback/ /hello-world/embed/
  /hello-world/comment-page-2/ /hello-world/attachment/foo/ /hello-world/foo/ /hello-world/foo/bar/
  /2026/10/27/hello-world/ /2026/10/27/hello-world/2/ /2026/10/27/hello-world/feed/ /2026/10/27/hello-world/feed/atom/
  /2026/10/27/hello-world/trackback/ /2026/10/27/hello-world/embed/ /2026/10/27/hello-world/comment-page-2/
  /2026/10/27/hello-world/attachment/foo/ /2026/10/27/hello-world/foo/ /2026/10/hello-world/ /2026/hello-world.html
  /2026/ /2026/10/ /2026/10/27/ /2026/10/page/2/ /2026/10/feed/ /2026/comment-page-3/ /page/2/ /page2 /feed/ /rss2/ /feed/rdf/
  /comments/feed/ /comments/atom/ /comments/embed/ /comment-page-2/ /embed/ /search/hello/ /search/hello+world/
  /search/hello%20world/page/2/ /search/a/b/feed/ /author/admin/ /author/admin/page/2/ /category/uncategorized/
  /category/uncategorized/feed/atom/ /category/a/b/ /category/a/b/page/3/ /tag/foo/ /tag/foo/bar/ /type/aside/ /sample-page/ /sample-page
  /sample-page/2/ /sample-page/feed/ /sample-page/embed/ /sample-page/trackback/ /sample-page/comment-page-2/
  /sample-page/attachment/x/ /privacy-policy/ /nope/ /nope/deeper/ /a/b/c/d/ /wp-sitemap.xml /wp-sitemap-posts-post-1.xml
  /wp-sitemap-taxonomies-category-1.xml /wp-sitemap-users-1.xml /wp-sitemap.xsl /robots.txt /favicon.ico /sitemap.xml
  /sitemap.xml.gz /wp-json/ /wp-json/wp/v2/posts /index.php/wp-json/wp/v2 /wp-atom.php /wp-rss2.php /x/wp-feed.php
  /wp-app.php/x /wp-register.php /index.php/2026/10/27/hello-world/ /index.php/hello-world/ /index.php/category/uncategorized/
  /index.php/feed/ /%E2%82%AC/ /caf%C3%A9/ '/hello%20world/' /hello-world/?p=2 '/?p=1' '/hello-world/?s=x' /archives/1 /archives/1/
  /archives/1/2/ /archives/1/feed/ /archives/date/2026/10/ /1/ /1/2/ /date/2026/10/ /blog/hello-world/ /blog/2026/hello-world.html
  /blog/2026/ /blog/author/admin/ /blog/type/aside/ /topics/uncategorized/ /sections/uncategorized/ /labels/foo/ /tags/foo/
  /uncategorized/hello-world/ /uncategorized/hello-world/feed/ /uncategorized/ /admin/hello-world/ /admin/ /27/10/2026/hello-world
  /27/10/2026/ /10/27/2026/1/ /10/27/2026/ /2026/10/27/08/08/15/ /2026/10/27/08/08/15/2/ /2026/10/27/08/ /HELLO-WORLD/
  '/hello-world/?error=403' '/nope/deeper/?error=500' '/?error=403' '/hello-world//' '//hello-world/' /wp-content/nope/ /hello-world.html /0/ /00/ /2026/13/ '/hello-world/%0A' '/x%0A/'
)
SETUPS=(
  '/%year%/%monthnum%/%day%/%postname%/|||' '/%postname%/|||' '/%postname%/|||2' '/archives/%post_id%|||'
  '/index.php/%year%/%monthnum%/%day%/%postname%/|||' '/%category%/%postname%/|||' '/%author%/%postname%/|||'
  '/blog/%postname%/|topics|/labels|' '/blog/%year%/%postname%.html|/sections/|tags|2' '/%post_id%/|||'
  '/%day%/%monthnum%/%year%/%postname%|||' '/%monthnum%/%day%/%year%/%post_id%/|||'
  '/%year%/%monthnum%/%day%/%hour%/%minute%/%second%/|||' '/%pagename%/|||'
)
: > "$OUT"
for setup in "${SETUPS[@]}"; do
  for path in "${PATHS[@]}"; do
    body=$(curl -s -H "X-HyScribe-Permalinks: $setup" -H 'X-HyScribe-Parse: 1' "$BASE$path")
    printf '%s\t%s\t%s\n' "$setup" "$(printf '%s' "$path" | xxd -p | tr -d '\n')" "$(printf '%s' "$body" | xxd -p | tr -d '\n')" >> "$OUT"
  done
done
wc -l "$OUT"

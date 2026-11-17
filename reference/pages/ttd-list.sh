#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Builds reference/pages/ttd.list, the requests reference/pages/golden.sh
# records over the Theme Test Data fixture, from its database (db.sh up
# with the fixture built): one line per request,
#   options<TAB>path and query
# where options, a query string, are the option values the request is
# served with (reference/pages/options.php), empty for the fixture's own.
# Every post, page and attachment, every term, author and date archive
# with its pages and one past them, searches, feeds, comment pages, 404s,
# the front page with show_on_front both ways, and the navigation fallback
# PHP makes of a classic menu. Only reads.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/reference/pages/ttd.list"
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only -N -B wordpress_ttd -e "$1"; }
PER_PAGE=$(sql "SELECT option_value FROM wp_options WHERE option_name = 'posts_per_page'")
# A request, then its pages past the first and one more, for $2 posts.
paged() {
  printf '\t%s\n' "$1"
  local page
  for ((page = 2; page <= ($2 + PER_PAGE - 1) / PER_PAGE + 1; page++)); do
    printf '\t%s&paged=%s\n' "$1" "$page"
  done
}
POSTS="post_type = 'post' AND post_status = 'publish'"
{
  printf '\t/\n'
  printf '\t/?paged=%s\n' 2 3 6 7
  sql "SELECT ID FROM wp_posts WHERE post_type = 'post' ORDER BY ID" | sed 's|^|\t/?p=|'
  sql "SELECT ID FROM wp_posts WHERE post_type = 'page' ORDER BY ID" | sed 's|^|\t/?page_id=|'
  # Attachment pages are off in the fixture, so their requests redirect to
  # the file; then on.
  sql "SELECT ID FROM wp_posts WHERE post_type = 'attachment' ORDER BY ID" | sed 's|^|\t/?attachment_id=|'
  sql "SELECT ID FROM wp_posts WHERE post_type = 'attachment' ORDER BY ID" | sed 's|^|wp_attachment_pages_enabled=1\t/?attachment_id=|'
  sql "SELECT t.term_id, tt.count FROM wp_terms t JOIN wp_term_taxonomy tt USING (term_id) WHERE tt.taxonomy = 'category' ORDER BY t.term_id" |
    while read -r id count; do paged "/?cat=$id" "$count"; done
  sql "SELECT t.slug, tt.count FROM wp_terms t JOIN wp_term_taxonomy tt USING (term_id) WHERE tt.taxonomy = 'post_tag' ORDER BY t.term_id" |
    while read -r slug count; do paged "/?tag=$slug" "$count"; done
  sql "SELECT REPLACE(t.slug, 'post-format-', ''), tt.count FROM wp_terms t JOIN wp_term_taxonomy tt USING (term_id) WHERE tt.taxonomy = 'post_format' ORDER BY t.term_id" |
    while read -r slug count; do paged "/?post_format=$slug" "$count"; done
  sql "SELECT ID, (SELECT COUNT(*) FROM wp_posts WHERE $POSTS AND post_author = u.ID) FROM wp_users u ORDER BY ID" |
    while read -r id count; do paged "/?author=$id" "$count"; done
  for format in '%Y' '%Y%m' '%Y%m%d'; do
    sql "SELECT DATE_FORMAT(post_date, '$format'), COUNT(*) FROM wp_posts WHERE $POSTS GROUP BY 1 ORDER BY 1" |
      while read -r m count; do paged "/?m=$m" "$count"; done
  done
  for s in lorem image post+format 'ελληνικά' nothingmatches '%22Hello+world%22' a; do
    printf '\t/?s=%s\n' "$s"
  done
  printf '\t/?s=a&paged=%s\n' 2 3
  for feed in rss2 atom rdf rss comments-rss2 comments-atom; do
    printf '\t/?feed=%s\n' "$feed"
  done
  printf '\t/?p=1148&feed=rss2\n\t/?cat=1&feed=rss2\n\t/?tag=content&feed=atom\n\t/?s=lorem&feed=rss2\n'
  printf '\t/?p=999999\n\t/?page_id=999999\n\t/?cat=999999\n\t/?tag=nope\n\t/no/such/path/\n'
  # Comment pages on the posts with the most comments.
  for options in 'page_comments=1&comments_per_page=5' 'page_comments=1&comments_per_page=5&default_comments_page=oldest' \
    'page_comments=1&comments_per_page=3&thread_comments=0'; do
    for id in 1148 1149; do
      printf '%s\t/?p=%s\n' "$options" "$id"
      for page in 1 2 4 5 9; do
        printf '%s\t/?p=%s&cpage=%s\n' "$options" "$id" "$page"
      done
    done
  done
  # A static front page and a page for posts.
  for options in 'show_on_front=page&page_on_front=701&page_for_posts=703' 'show_on_front=page&page_on_front=701' \
    'show_on_front=page&page_for_posts=703'; do
    for path in / '/?paged=2' '/?page_id=701' '/?page_id=703' '/?page_id=703&paged=2' '/?page_id=703&paged=7' '/?feed=rss2' '/?p=1241'; do
      printf '%s\t%s\n' "$options" "$path"
    done
  done
  # Last, as the first of them writes the navigation post later pages show:
  # the fallback converted from a classic menu, which is not ported.
  printf '_fallback=1\t%s\n' / '/?p=1241' '/?page_id=701'
} > "$OUT"
wc -l "$OUT"

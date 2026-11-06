#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the Link headers WordPress sends (the REST API root, the queried
# object's REST route and the shortlink) as
# crates/hyscribe-http/tests/golden/links.tsv:
#   method<TAB>hex(path and query)<TAB>status<TAB>Link headers joined by " | "
# Needs the default install oracle running (db.sh up, then php.sh up). Only
# reads from it.
set -euo pipefail

BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-http/tests/golden/links.tsv"
HEADERS=$(mktemp)
trap 'rm -f "$HEADERS"' EXIT

: > "$OUT"
record() {
  local method=$1 path=$2 args=(-s -D "$HEADERS" -o /dev/null)
  [[ "$method" == HEAD ]] && args+=(-I)
  curl "${args[@]}" "$BASE$path" > /dev/null
  status=$(head -n 1 "$HEADERS" | cut -d ' ' -f 2)
  links=$(grep -i '^link:' "$HEADERS" | cut -d ' ' -f 2- | tr -d '\r' | paste -sd '|' | sed 's/|/ | /g' || true)
  printf '%s\t%s\t%s\t%s\n' "$method" "$(printf '%s' "$path" | xxd -p | tr -d '\n')" "$status" "$links" >> "$OUT"
}
for path in '/' '/?p=1' '/?page_id=2' '/?p=1&cpage=1' '/?p=1&page=2' '/?p=999' '/?page_id=3' '/?p=2' '/?cat=1' '/?cat=999' \
  '/?cat=1&paged=9' '/?tag=x' '/?post_format=aside' '/?s=x' '/?s=hello' '/?m=2026' '/?m=202611' '/?year=2026&monthnum=13' \
  '/?paged=2' '/?paged=99' '/?p=-1' '/?feed=rss2' '/?feed=rss' '/?feed=atom' '/?feed=comments-rss2' '/?feed=rss2&p=1' \
  '/?feed=rss2&page_id=2' '/?feed=rss2&p=999' '/?feed=rss2&cat=1' '/?feed=rss2&paged=99' '/?feed=rss2&s=hello' '/?p=1&feed=rss2' \
  '/?favicon=1' '/?favicon=1&p=1' '/?favicon=1&p=2' '/?robots=1' '/?robots=1&p=1' '/?robots=1&p=2' '/?robots=1&cat=1' \
  '/?tb=1' '/?tb=1&p=1' '/?tb=1&page_id=2' '/?tb=1&p=999' '/?tb=1&p=2' '/x/1?tb=1' '/?sitemap=index' '/?sitemap=posts&sitemap-subtype=post' \
  '/?sitemap-stylesheet=index' '/wp-trackback.php?p=1' '/wp-trackback.php' '/?error=403' '/?error=404' '/?error=410&p=1' \
  '/?error=404&p=1' '/?preview=true&p=1' '/?preview=true' '/?preview=true&page_id=3' '/?attachment_id=1' '/?name=hello-world' \
  '/?pagename=sample-page' '/?name=nope' '/?p=1&replytocom=1' '/?p=1&unapproved=1&moderation-hash=x' '/?post_type=attachment' \
  '/?page_id=2&x=' '/?p=1&x='; do
  record GET "$path"
done
record HEAD '/?p=1'
record HEAD '/?tb=1&p=1'
record HEAD '/?favicon=1&p=1'
wc -l "$OUT"

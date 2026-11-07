#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records how WordPress answers the favicon, trackback, preview, unapproved
# comment and error query vars as crates/hyscribe-http/tests/golden/loader.tsv:
#   hex(path and query)<TAB>status<TAB>location<TAB>content type<TAB>hex(body)
# where an HTML body is reduced to its robots meta tag through its <title>,
# which holds the preview script, and the IDs of the posts it lists. Needs the default install
# oracle running (reference/default-install/db.sh up, then php.sh up).
set -euo pipefail

BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-http/tests/golden/loader.tsv"
TMP=$(mktemp)
HEADERS=$(mktemp)
trap 'rm -f "$TMP" "$HEADERS"' EXIT

: > "$OUT"
for path in '/?favicon=1' '/?favicon=0' '/?favicon=' '/?favicon=abc' '/?favicon=0.0' '/?favicon=1&p=1' '/?favicon=1&p=2' \
  '/?favicon=1&feed=rss2' '/?favicon=1&robots=1' '/?favicon=1&sitemap=index' '/?favicon=1&p=999' '/?favicon=1&s=x' \
  '/?favicon=1&paged=2' '/?favicon=1&p=-1' '/?favicon=1&name=nope' '/?favicon=1&tb=1&p=1' \
  '/?tb=1&p=1' '/?tb=1' '/?tb=0&p=1' '/?tb=1&page_id=2' '/?tb=1&p=2' '/?tb=1&p=999' '/?tb=abc&p=1' '/?tb=1&p=1&feed=rss2' \
  '/?tb=1&name=hello-world' '/?tb=1&p=1&x=y' '/?tb=1&cat=1' '/?tb=1&s=hello' '/5?tb=1' '/x/1?tb=1' '/x/2?tb=1' '/999?tb=1' \
  '/-1?tb=1' '/?tb=1&page_id=999' '/?tb=1&pagename=sample-page' '/?tb=1&m=2026' '/?tb=1&tb_id=1' '/x/1?tb=1&tb_id=1' '/x/1?tb=1&tb_id=0' \
  '/?tb=1&p=1&cpage=2' '/?tb=1&p=1&page=2' '/?tb=1&p=1&robots=1' '/?tb=1&sitemap=index' '/?tb=1&paged=2' '/?tb=1&p=-1' \
  '/?tb=%20&p=1' '/?tb=1&attachment_id=1' '/?tb=1&feed=rss' '/?tb=1&p=1&x=' '/?x=/1&tb=1' \
  '/?favicon=1&robots=1&p=2' '/?favicon=1&robots=0&p=2' '/?favicon=1&robots=1&x=' '/?favicon=1&p=-1&page_id=2' \
  '/?favicon=1&year=2026&monthnum=13' '/?favicon%5B%5D=1' '/?favicon=1&p%5B%5D=1' \
  '/?tb=1&feed=rss&paged=99' '/?tb=1&p=999&feed=rss2' '/?tb=1&p=-1&page_id=2' '/?tb%5B%5D=1&p=1' \
  '/x/1?tb=1&tb_id%5B%5D=1' '/?tb=1&year=2026&monthnum=13' '/?tb=1&p=1&feed=rss' '/?tb=1&p=999&feed=rss' \
  '/?preview=true' '/?preview=1&p=1' '/?preview=false&p=1' '/?preview=0&p=1' '/?preview=0' '/?preview=' '/?preview=true&page_id=2' \
  '/?preview=true&p=1&x=y' '/?preview=true&p=999' '/?preview=true&name=hello-world' '/?preview=true&cat=1' '/?preview=true&s=hello' \
  '/?preview=true&s=zzzz' '/?preview=true&m=2026' '/?preview=true&name=nope' '/?preview=true&pagename=sample-page' \
  '/?preview=true&p=1&feed=rss2' '/?preview=true&feed=rss' '/?preview=true&p=1&page=2' '/?preview=true&x=' '/?preview=true&page_id=2&x=' \
  '/?preview%5B%5D=1&p=1' '/?preview%5B%5D=1' '/?preview=true&attachment_id=1' '/?preview=true&p=2' '/?preview=true&paged=2' \
  '/?preview=true&p=999&feed=rss' '/?preview=true&feed=rss&paged=99' '/?preview=true&page_id=3' '/?preview=true&name=sam' \
  '/?preview=true&cat=1&x=' '/?x=&preview=true&p=1' '/?preview=true&p=-1' '/?preview=true&year=2026&monthnum=13' \
  '/?page_id=3&feed=rss' '/?p=3&feed=rss' '/?page_id=3&x=' '/?preview=true&p=3&x=y' '/?preview=true&name=hello-world&p=0' '/?page_id=3&feed=rss2&x=' '/?p=3&feed=rss2&x=' '/?page_id=3&feed=rss2&cat=' '/?page_id=3&cat=' \
  '/?attachment_id=3&cat=' '/?subpost_id=3&cat=' '/?attachment_id=3&preview=true&cat=' '/?p=-1&page_id=3&cat=' \
  '/?p=1&unapproved=1&moderation-hash=x' '/?p=1&unapproved=1' '/?p=1&moderation-hash=x' '/?unapproved=&moderation-hash=' '/?s=a&unapproved=1&moderation-hash=x' \
  '/?error=404' '/?error=403' '/?error=500' '/?error=502' '/?error=503' '/?error=410' '/?error=abc' '/?error=404&p=1' '/?p=1&error=404' \
  '/?error=403&p=1' '/?error=' '/?error=0' '/?error=404x' '/?error=+404' '/?error=404&sitemap=index' '/?error=403&robots=1' '/?error=404&p=2' \
  '/?error=200' '/?error=404&robots=1' '/?error=410&p=1' '/?error=410&p=2' '/?error=410&sitemap=index' '/?error=410&p=999' '/?error=404.0' \
  '/?error=%20404' '/?error=404%20' '/?error=4.04e2' '/?error=0404' '/?error=301' '/?error=418' '/?error=999' '/?error=1' '/?error=404&s=hello' \
  '/?error=404&paged=2' '/?error=403.5' '/?error=500x' '/?error=404&p=-1' '/?error=404&page_id=2' '/?page_id=2&error=404' \
  '/?error=404&attachment_id=1' '/?error=404&favicon=1' '/?error=404&tb=1&p=1' '/?error=404&preview=true' '/?error=404&cat=1' \
  '/?error=404&p=999' '/?error=404&p=1&page_id=2' '/?error=404&x=' '/?error=404&p=1&x=' '/?error=404&year=2026&monthnum=13' \
  '/?subpost_id=1&error=404' '/?subpost_id=2&attachment_id=1&error=404' \
  '/?p=-1&feed=rss2' '/?p=-1&feed=atom' '/?p=-1&page_id=2&feed=rss2' '/?error=404&feed=rss2' '/?p=-1&feed=rss' '/?p=-1&feed=comments-rss2' '/?p=-2&feed=rss2&cat=1' '/?name=hello-world&p=2' '/?pagename=sample-page&page_id=1' \
  '/?name=hello-world&m=2026' '/?p=1&page=x' '/?p=1&page=2&feed=rss2' \
  '/?pagename=sample-page&page=2' '/?name=nope&year=2026&monthnum=10' '/?p=999&post_type=post' '/?name=nope&post_type=post' \
  '/?post_type=attachment' '/?m=2026&post_type=attachment' '/?s=hello&post_type=attachment' '/?cat=1&post_type=attachment' \
  '/?post_type=post' '/?post_type=page' '/?s=sample&post_type=post' '/?s=sample&post_type=page' '/?s=sample' '/?post_type=nope' \
  '/?feed=rss2&post_type=attachment' '/?post_type=attachment&paged=2' '/?year=2026&post_type=attachment'; do
  curl -s -D "$HEADERS" -o "$TMP" "$BASE$path" > /dev/null
  status=$(head -n 1 "$HEADERS" | cut -d ' ' -f 2)
  location=$(grep -i '^location:' "$HEADERS" | cut -d ' ' -f 2 | tr -d '\r' || true)
  type=$(grep -i '^content-type:' "$HEADERS" | cut -d ' ' -f 2- | tr -d '\r' || true)
  if [[ "$type" == text/html* ]]; then
    body=$(python3 -I -c 'import re, sys; s=open(sys.argv[1],"rb").read(); i=s.find(b"<meta name=\x27robots\x27"); j=s.find(b"</title>"); ids=b",".join(re.findall(rb"wp-block-post post-(\d+)", s)); sys.stdout.write((s[i:j+8] + b"\nposts: " + ids).hex() if i >= 0 else "")' "$TMP")
  else
    body=$(xxd -p "$TMP" | tr -d '\n')
  fi
  printf '%s\t%s\t%s\t%s\t%s\n' "$(printf '%s' "$path" | xxd -p | tr -d '\n')" "$status" "$location" "$type" "$body" >> "$OUT"
done
wc -l "$OUT"

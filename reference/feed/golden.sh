#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the feed oracles in crates/hyscribe-render/tests/golden/: the
# excerpt and feed content (excerpt.tsv), the posts feed with options and
# post 1 changed inside a rolled back transaction (feed.tsv), and the posts
# feeds over HTTP with conditional requests (feeds-http.tsv). Needs the
# default install oracle running (reference/default-install/db.sh up, then
# php.sh up); feeds write nothing.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-render/tests/golden/excerpt.tsv"

"$DOCKER" exec -i "$NAME" php < "$ROOT/reference/feed/excerpt-golden.php" > "$OUT"
wc -l "$OUT"

FEED="$ROOT/crates/hyscribe-render/tests/golden/feed.tsv"
"$DOCKER" exec -i "$NAME" php < "$ROOT/reference/feed/feed-golden.php" > "$FEED"
wc -l "$FEED"

# The posts and comment feeds over HTTP, then conditional requests for the
# home feed:
#   hex(path)<TAB>hex(request headers)<TAB>hex(status, content type, last modified and etag)<TAB>hex(body)
BASE="${HYSCRIBE_PHP_BASE:-http://127.0.0.1:8081}"
HTTP="$ROOT/crates/hyscribe-render/tests/golden/feeds-http.tsv"
: > "$HTTP"
fetch() {
  local path="$1" sent="$2" response line
  local args=()
  while IFS= read -r line; do
    [ -n "$line" ] && args+=(-H "$line")
  done <<< "$sent"
  response=$(curl -s -D - "${args[@]}" "$BASE$path" | xxd -p | tr -d '\n')
  printf '%s' "$response" | python3 -I -c '
import sys
path, sent = sys.argv[1].encode(), sys.argv[2].encode()
r = bytes.fromhex(sys.stdin.read())
head, _, body = r.partition(b"\r\n\r\n")
lines = head.split(b"\r\n")
status = lines[0].split(b" ")[1]
fields = {l.split(b":", 1)[0].lower(): l.split(b":", 1)[1].strip() for l in lines[1:] if b":" in l}
kept = b"\n".join([status] + [fields.get(k, b"") for k in (b"content-type", b"last-modified", b"etag")])
print("\t".join(x.hex() for x in (path, sent, kept, body)))
' "$path" "$sent" >> "$HTTP"
}
for path in '/?feed=rss2' '/?feed=feed' '/?feed=rss2&cat=1' '/?s=hello&feed=rss2' '/?s&feed=rss2' '/?feed=rss2&m=2026' \
  '/?feed=rss2&cat=999' '/?feed=rss2&m=1999' '/?s=nothingmatches&feed=rss2' \
  '/?feed=comments-rss2' '/?feed=rss2&withcomments=1' '/?feed=rss2&p=1' '/?feed=comments-rss2&p=1' '/?feed=feed&p=1' \
  '/?feed=rss2&page_id=2' '/?feed=rss2&p=999' '/?feed=rss2&page_id=999' '/?feed=rss2&p=1&withoutcomments=1' \
  '/?feed=rss2&p=999&withoutcomments=1' '/?feed=rss2&p=1&withoutcomments=0' \
  '/?feed=atom' '/?feed=atom&cat=1' '/?s=hello&feed=atom' '/?feed=atom&m=1999' '/?feed=atom&p=1&withoutcomments=1' \
  '/?feed=comments-atom' '/?feed=atom&p=1' '/?feed=atom&page_id=2' '/?feed=atom&p=999' '/?feed=comments-atom&p=1'; do
  fetch "$path" ''
done
etag=$(curl -s -D - -o /dev/null "$BASE/?feed=rss2" | tr -d '\r' | sed -n 's/^ETag: //p')
modified=$(curl -s -D - -o /dev/null "$BASE/?feed=rss2" | tr -d '\r' | sed -n 's/^Last-Modified: //p')
for sent in "If-None-Match: $etag" 'If-None-Match: "other"' "If-Modified-Since: $modified" \
  'If-Modified-Since: Mon, 26 Oct 2026 08:08:15 GMT' 'If-Modified-Since: Fri, 30 Oct 2026 08:08:15 GMT' \
  'If-Modified-Since: not a date' 'If-Modified-Since: Tue, 27 Foo 2026 08:08:15 GMT' 'If-Modified-Since: Fri, 01 Jan 2027 00:00:00 GMT' "If-Modified-Since: $modified"$'\n'"If-None-Match: $etag" \
  "If-Modified-Since: $modified"$'\n''If-None-Match: "other"' 'If-Modified-Since: Mon, 26 Oct 2026 08:08:15 GMT'$'\n'"If-None-Match: $etag"; do
  fetch '/?feed=rss2' "$sent"
done
wc -l "$HTTP"

COMMENTS="$ROOT/crates/hyscribe-render/tests/golden/comment-feeds.tsv"
"$DOCKER" exec -i "$NAME" php < "$ROOT/reference/feed/comments-golden.php" > "$COMMENTS"
wc -l "$COMMENTS"

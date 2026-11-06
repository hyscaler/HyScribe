#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records WordPress's embed pages (the `embed` query var) as
# crates/hyscribe-http/tests/golden/embed.tsv:
#   hex(path and query)<TAB>status<TAB>location<TAB>X-WP-embed<TAB>hex(body of an embed page)
# The sharing dialog's random secret becomes 0123456789 and its random ID
# suffix RAND. Needs the default install oracle running (db.sh up, then
# php.sh up). Only reads from it.
set -euo pipefail

BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-http/tests/golden/embed.tsv"
TMP=$(mktemp)
HEADERS=$(mktemp)
trap 'rm -f "$TMP" "$HEADERS"' EXIT

: > "$OUT"
for path in '/?p=1&embed=true' '/?page_id=2&embed=true' '/?p=999&embed=true' '/?name=hello-world&embed=true' \
  '/?pagename=sample-page&embed=true' '/?p=1&embed=0' '/?p=1&embed=' '/?p=1&embed%5B%5D=1' '/?page_id=3&embed=true' \
  '/?p=2&embed=true' '/?embed=true' '/?cat=1&embed=true' '/?s=hello&embed=true' '/?p=1&embed=true&feed=rss2' \
  '/?p=1&embed=true&x=' '/?p=1&embed=true&cpage=1' '/?p=1&embed=true&page=2' '/?name=nope&embed=true' \
  '/?p=1&embed=true&preview=true' '/?p=1&embed=true&tb=1' '/?paged=99&embed=true' \
  '/?attachment_id=1&embed=true'; do
  curl -s -D "$HEADERS" -o "$TMP" "$BASE$path" > /dev/null
  status=$(head -n 1 "$HEADERS" | cut -d ' ' -f 2)
  location=$(grep -i '^location:' "$HEADERS" | cut -d ' ' -f 2 | tr -d '\r' || true)
  embed=$(grep -i '^x-wp-embed:' "$HEADERS" | cut -d ' ' -f 2 | tr -d '\r' || true)
  body=''
  if [[ -n "$embed" ]]; then
    body=$(python3 -I -c '
import re, sys
s = open(sys.argv[1], "rb").read()
m = re.search(rb"data-secret=&quot;([A-Za-z0-9]{10})&quot;", s)
if m:
    s = s.replace(m.group(1), b"0123456789")
s = re.sub(rb"(wp-embed-share-(?:tab|description)-(?:wordpress|html)-\d+)-\d+", rb"\1-RAND", s)
sys.stdout.write(s.hex())' "$TMP")
  fi
  printf '%s\t%s\t%s\t%s\t%s\n' "$(printf '%s' "$path" | xxd -p | tr -d '\n')" "$status" "$location" "$embed" "$body" >> "$OUT"
done
wc -l "$OUT"

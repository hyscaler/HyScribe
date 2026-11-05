#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records how the template loader answers the favicon and trackback query
# vars as crates/hyscribe-http/tests/golden/loader.tsv:
#   hex(path and query)<TAB>status<TAB>location<TAB>content type<TAB>hex(body)
# where an HTML body is reduced to its <title>. Needs the default install
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
  '/x/1?tb=1&tb_id%5B%5D=1' '/?tb=1&year=2026&monthnum=13' '/?tb=1&p=1&feed=rss' '/?tb=1&p=999&feed=rss'; do
  curl -s -D "$HEADERS" -o "$TMP" "$BASE$path" > /dev/null
  status=$(head -n 1 "$HEADERS" | cut -d ' ' -f 2)
  location=$(grep -i '^location:' "$HEADERS" | cut -d ' ' -f 2 | tr -d '\r' || true)
  type=$(grep -i '^content-type:' "$HEADERS" | cut -d ' ' -f 2- | tr -d '\r' || true)
  if [[ "$type" == text/html* ]]; then
    body=$(grep -o '<title>[^<]*</title>' "$TMP" | head -n 1 | tr -d '\n' | xxd -p | tr -d '\n' || true)
  else
    body=$(xxd -p "$TMP" | tr -d '\n')
  fi
  printf '%s\t%s\t%s\t%s\t%s\n' "$(printf '%s' "$path" | xxd -p | tr -d '\n')" "$status" "$location" "$type" "$body" >> "$OUT"
done
wc -l "$OUT"

#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records how the PHP oracle's web server answers conditional requests for
# a theme file, as crates/hyscribe-http/tests/golden/conditional.tsv:
#   method<TAB>header<TAB>value<TAB>header<TAB>value<TAB>status
# A value's ETAG and MODIFIED stand for the file's own ETag and
# Last-Modified, which HyScribe makes differently (an ETag from the file's
# SHA-256). Needs the oracle running (php.sh up); only reads from it.
set -euo pipefail

BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
FILE="/wp-content/themes/twentytwentyfive/style.css"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-http/tests/golden/conditional.tsv"
HEADERS=$(curl -sI "$BASE$FILE" | tr -d '\r')
ETAG=$(grep -i '^etag:' <<< "$HEADERS" | cut -d ' ' -f 2-)
MODIFIED=$(grep -i '^last-modified:' <<< "$HEADERS" | cut -d ' ' -f 2-)

cases=(
  "GET|If-Modified-Since|MODIFIED||"
  "HEAD|If-Modified-Since|MODIFIED||"
  "GET|If-Modified-Since|Tue, 11 Aug 2026 14:45:48 GMT||"
  "GET|If-Modified-Since|Thu, 13 Aug 2026 00:00:00 GMT||"
  "GET|If-Modified-Since|Thu, 01 Jan 2099 00:00:00 GMT||"
  "GET|If-Modified-Since|garbage||"
  "GET|If-Modified-Since|Thursday, 13-Aug-26 00:00:00 GMT||"
  "GET|If-Modified-Since|Thu Aug 13 00:00:00 2026||"
  "GET|If-None-Match|ETAG||"
  "GET|If-None-Match|W/ETAG||"
  "GET|If-None-Match|\"x\", ETAG||"
  "GET|If-None-Match|*||"
  "GET|If-None-Match|\"x\"||"
  "GET|If-None-Match|\"x\"|If-Modified-Since|MODIFIED"
  "GET|If-None-Match|ETAG|If-Modified-Since|Wed, 12 Aug 2020 00:00:00 GMT"
  "GET|If-Match|\"x\"||"
  "GET|If-Match|ETAG||"
  "GET|If-Match|W/ETAG||"
  "GET|If-Match|*||"
  "GET|If-Match|ETAG|If-None-Match|ETAG"
  "GET|If-Unmodified-Since|Wed, 12 Aug 2020 00:00:00 GMT||"
  "GET|If-Unmodified-Since|MODIFIED||"
  "GET|If-Unmodified-Since|garbage||"
  "GET|If-Match|ETAG|If-Unmodified-Since|Wed, 12 Aug 2020 00:00:00 GMT"
)
: > "$OUT"
for case in "${cases[@]}"; do
  IFS='|' read -r method name value name2 value2 <<< "$case"
  args=(-s -o /dev/null -w '%{http_code}' -X "$method")
  [ "$method" = HEAD ] && args=(-s -o /dev/null -w '%{http_code}' -I)
  fill() { local v="${1//ETAG/$ETAG}"; printf '%s' "${v//MODIFIED/$MODIFIED}"; }
  args+=(-H "$name: $(fill "$value")")
  [ -n "$name2" ] && args+=(-H "$name2: $(fill "$value2")")
  status=$(curl "${args[@]}" "$BASE$FILE")
  printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$method" "$name" "$value" "$name2" "$value2" "$status" >> "$OUT"
done
cat "$OUT"

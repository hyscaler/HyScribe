#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records how the PHP oracle's web server sends each file of wp-includes but
# its PHP, as crates/hyscribe-http/tests/golden/wp-includes-files.tsv:
#   path<TAB>status<TAB>content type<TAB>sha256 of the body<TAB>Last-Modified
# Needs the default install oracle running (db.sh up, then php.sh up). Only
# reads from it.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-http/tests/golden/wp-includes-files.tsv"
TMP=$(mktemp)
HEADERS=$(mktemp)
trap 'rm -f "$TMP" "$HEADERS"' EXIT

: > "$OUT"
"$DOCKER" exec "$NAME" sh -c "cd /var/www/html/wp-includes && find . -type f ! -name '*.php' | cut -c3- | LC_ALL=C sort" |
while read -r file; do
  path="/wp-includes/$file"
  curl -s -D "$HEADERS" -o "$TMP" "$BASE$path" > /dev/null
  status=$(head -n 1 "$HEADERS" | cut -d ' ' -f 2)
  type=$(grep -i '^content-type:' "$HEADERS" | cut -d ' ' -f 2- | tr -d '\r' || true)
  modified=$(grep -i '^last-modified:' "$HEADERS" | cut -d ' ' -f 2- | tr -d '\r' || true)
  printf '%s\t%s\t%s\t%s\t%s\n' "$path" "$status" "$type" "$(sha256sum "$TMP" | cut -d ' ' -f 1)" "$modified" >> "$OUT"
done
wc -l "$OUT"

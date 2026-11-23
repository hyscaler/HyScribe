#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's load-styles.php and load-scripts.php as
# crates/hyscribe-http/tests/golden/loaders.tsv:
#   loader<TAB>query<TAB>If-None-Match<TAB>status<TAB>content type<TAB>ETag<TAB>Cache-Control<TAB>sha256 of the body
# Needs the default install oracle running (db.sh up, then php.sh up). Only
# reads from it.
set -euo pipefail
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-http/tests/golden/loaders.tsv"
TMP=$(mktemp)
HEADERS=$(mktemp)
trap 'rm -f "$TMP" "$HEADERS"' EXIT
: > "$OUT"
record() {
  local loader=$1 query=$2 match=${3:-}
  : > "$TMP"
  curl -s -D "$HEADERS" -o "$TMP" ${match:+-H "If-None-Match: $match"} "$BASE/wp-admin/$loader.php?$query" > /dev/null
  local status type etag cache
  status=$(head -n 1 "$HEADERS" | cut -d ' ' -f 2)
  type=$(grep -i '^content-type:' "$HEADERS" | cut -d ' ' -f 2- | tr -d '\r' || true)
  etag=$(grep -i '^etag:' "$HEADERS" | cut -d ' ' -f 2- | tr -d '\r' || true)
  cache=$(grep -i '^cache-control:' "$HEADERS" | cut -d ' ' -f 2- | tr -d '\r' || true)
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$loader" "$query" "$match" "$status" "$type" "$etag" "$cache" "$(sha256sum "$TMP" | cut -d ' ' -f 1)" >> "$OUT"
}
DEPS="$ROOT/wordpress/wp-includes/dependencies.json"
for handle in $(php -r '$d = json_decode( file_get_contents( $argv[1] ), true ); echo implode( " ", array_keys( $d["styles"] ) );' "$DEPS"); do
  record load-styles "load=$handle"
done
for handle in $(php -r '$d = json_decode( file_get_contents( $argv[1] ), true ); echo implode( " ", array_keys( array_filter( $d["styles"], fn( $s ) => ! empty( $s["rtl"] ) ) ) );' "$DEPS"); do
  record load-styles "dir=rtl&load=$handle"
done
for handle in $(php -r '$d = json_decode( file_get_contents( $argv[1] ), true ); echo implode( " ", array_keys( $d["scripts"] ) );' "$DEPS"); do
  record load-scripts "load=$handle"
done
LOGIN_STYLES='c=0&dir=ltr&load%5Bchunk_0%5D=dashicons,buttons,forms,l10n,wp-base-styles,wp-tooltip,login&ver=7.1.3'
LOGIN_SCRIPTS='c=0&load%5Bchunk_0%5D=clipboard,jquery-core,jquery-migrate,zxcvbn-async,wp-hooks&ver=7.1.3'
for query in "$LOGIN_STYLES" 'load%5Bchunk_1%5D=login&load%5Bchunk_0%5D=dashicons' 'load%5B1%5D=forms&load%5B0%5D=buttons,' 'load=buttons,buttons,%3Cscript%3E,login' 'load=nope' 'load=' '' 'dir=rtl&load=dashicons,login,colors' 'load%5Bb%5D=login&load%5B0%5D=forms&load%5Ba%5D=buttons'; do
  record load-styles "$query"
done
for query in "$LOGIN_SCRIPTS" 'load=nope' '' 'load=jquery,jquery-core'; do
  record load-scripts "$query"
done
record load-styles "$LOGIN_STYLES" "$(grep -P "^load-styles\t\Q$LOGIN_STYLES\E\t" "$OUT" | cut -f 6)"
record load-styles "$LOGIN_STYLES" 'W/"other"'
wc -l "$OUT"

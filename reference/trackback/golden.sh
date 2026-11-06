#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records how WordPress answers wp-trackback.php and trackback POSTs, with
# trackbacks closed as HyScribe keeps them, as
# crates/hyscribe-http/tests/golden/trackback.tsv:
#   method<TAB>hex(path and query)<TAB>request content type<TAB>hex(form body)<TAB>status<TAB>location<TAB>content type<TAB>hex(body)
# A must-use plugin closes pings for the run, so no trackback is stored.
# Needs the default install oracle running (db.sh up, then php.sh up).
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-http/tests/golden/trackback.tsv"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp)
HEADERS=$(mktemp)
cleanup() {
  rm -f "$TMP" "$HEADERS"
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-closed-pings.php"
}
trap cleanup EXIT
"$DOCKER" exec "$NAME" sh -c "mkdir -p $MU && printf '%s\n' '<?php' \"add_filter( 'pings_open', '__return_false' );\" > $MU/hyscribe-closed-pings.php"

record() {
  local method=$1 path=$2 form=$3 type=''
  [[ "$method" == POST ]] && type=${4:-application/x-www-form-urlencoded}
  local args=(-s -D "$HEADERS" -o "$TMP")
  case "$method" in
    HEAD) args+=(-I) ;;
    POST) args+=(-X POST -H "Content-Type: $type" --data-binary "$form") ;;
  esac
  curl "${args[@]}" "$BASE$path" > /dev/null
  [[ "$method" == HEAD ]] && : > "$TMP"
  status=$(head -n 1 "$HEADERS" | cut -d ' ' -f 2)
  location=$(grep -i '^location:' "$HEADERS" | cut -d ' ' -f 2 | tr -d '\r' || true)
  ctype=$(grep -i '^content-type:' "$HEADERS" | cut -d ' ' -f 2- | tr -d '\r' || true)
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$method" "$(printf '%s' "$path" | xxd -p | tr -d '\n')" "$type" \
    "$(printf '%s' "$form" | xxd -p | tr -d '\n')" "$status" "$location" "$ctype" "$(xxd -p "$TMP" | tr -d '\n')" >> "$OUT"
}

: > "$OUT"
for path in '/wp-trackback.php?p=1' '/wp-trackback.php' '/wp-trackback.php?p=2' '/wp-trackback.php?page_id=2' \
  '/wp-trackback.php?p=999' '/wp-trackback.php?p=3' '/wp-trackback.php/1' '/wp-trackback.php/999' \
  '/wp-trackback.php?tb_id=1' '/wp-trackback.php?p=1&tb_id=1' '/wp-trackback.php?feed=rss2&p=1' '/wp-trackback.php?feed=rss2' \
  '/wp-trackback.php?favicon=1&p=1' '/wp-trackback.php?robots=1' '/wp-trackback.php?error=404&p=1' '/wp-trackback.php?error=403' \
  '/wp-trackback.php?error=410&p=1' '/wp-trackback.php?name=hello-world' '/wp-trackback.php?pagename=sample-page' \
  '/wp-trackback.php?s=hello' '/wp-trackback.php?cat=1' '/wp-trackback.php?p=1&tb=0' '/wp-trackback.php?attachment_id=1' \
  '/wp-trackback.php?p=-1' '/wp-trackback.php?p=1&x=' '/wp-trackback.php?preview=true&p=1' \
  '/wp-trackback.php?sitemap=index' '/wp-trackback.php?year=2026&monthnum=13'; do
  record GET "$path" ''
done
record HEAD '/wp-trackback.php?p=1' ''
record HEAD '/wp-trackback.php' ''
for path in '/?p=1&tb=1' '/wp-trackback.php?p=1' '/wp-trackback.php' '/wp-trackback.php?p=999' '/?tb=1' '/x/1?tb=1' \
  '/?tb=1&page_id=2' '/wp-trackback.php?p=3' '/?tb=1&p=1&tb_id=1'; do
  for form in '' 'blog_name=x' 'url=http://example.com/&title=T' 'url=http://example.com/' 'title=T' \
    'url=http://example.com/&title=T&blog_name=B&excerpt=E'; do
    record POST "$path" "$form"
  done
done
for form in 'url=x&title=T' 'url=javascript:alert(1)&title=T' 'url=http://example.com/&title=%3Cb%3E%3C/b%3E' \
  'url=http://example.com/&title=0' 'url=http://example.com/&title=%2541' 'url=http://example.com/&title=%5C' \
  'url=http://example.com/&title=%20%09' 'blog_name=%3Ci%3E' 'title=%5C%5C' 'charset=utf-7' 'charset=UTF-7&url=http://example.com/&title=T' \
  'charset=utf,-7&url=http://example.com/&title=T' 'charset=nope&url=http://example.com/&title=T' \
  'url=%20&title=T' 'url=http://example.com/&title=T&p=1'; do
  record POST '/wp-trackback.php?p=1' "$form"
done
record POST '/wp-trackback.php?p=1' 'url=http://example.com/&title=T' 'text/plain'
record POST '/wp-trackback.php?p=1' 'url=http://example.com/&title=T' 'APPLICATION/X-WWW-FORM-URLENCODED; charset=UTF-8'
record POST '/wp-trackback.php?p=1' '' 'application/x-www-form-urlencoded'
wc -l "$OUT"

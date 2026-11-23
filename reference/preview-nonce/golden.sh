#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's answers to a request whose preview_id and
# preview_nonce do not verify (_show_post_preview(), on init) at each entry
# point that loads WordPress, as crates/hyscribe-http/tests/golden/preview-nonce.tsv:
#   method<TAB>path and query<TAB>Accept<TAB>Content-Type<TAB>body<TAB>status<TAB>content type<TAB>Cache-Control<TAB>Expires<TAB>hex(body)
# Needs the default install oracle running (db.sh up, then php.sh up); it only
# reads: every request dies before its own work.
set -euo pipefail
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-http/tests/golden/preview-nonce.tsv"
TMP=$(mktemp)
HEADERS=$(mktemp)
trap 'rm -f "$TMP" "$HEADERS"' EXIT
: > "$OUT"
record() {
  local method=$1 path=$2 accept=${3:-} type=${4:-} body=${5:-}
  local args=(-s -D "$HEADERS" -o "$TMP" -X "$method")
  [ -z "$accept" ] || args+=(-H "Accept: $accept")
  [ -z "$type" ] || args+=(-H "Content-Type: $type")
  [ -z "$body" ] || args+=(--data-binary "$body")
  curl "${args[@]}" "$BASE$path" > /dev/null
  local status kind cache expires
  status=$(head -n 1 "$HEADERS" | cut -d ' ' -f 2)
  kind=$(grep -i '^content-type:' "$HEADERS" | cut -d ' ' -f 2- | tr -d '\r' || true)
  cache=$(grep -i '^cache-control:' "$HEADERS" | cut -d ' ' -f 2- | tr -d '\r' || true)
  expires=$(grep -i '^expires:' "$HEADERS" | cut -d ' ' -f 2- | tr -d '\r' || true)
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$method" "$path" "$accept" "$type" "$body" \
    "$status" "$kind" "$cache" "$expires" "$(xxd -p "$TMP" | tr -d '\n')" >> "$OUT"
}
Q='preview_id=1&preview_nonce=x'
for path in "/wp-admin/index.php?$Q" "/wp-admin/upload.php?$Q" "/wp-admin/?$Q" "/wp-admin/admin-post.php?$Q" \
  "/wp-admin/admin-post.php?action=x&$Q" "/wp-login.php?$Q" "/wp-login.php?action=logout&$Q" "/wp-cron.php?$Q" \
  "/wp-trackback.php?$Q" "/wp-links-opml.php?$Q" "/wp-signup.php?$Q" "/wp-activate.php?$Q" "/wp-mail.php?$Q"; do
  record GET "$path"
done
record POST "/wp-admin/admin-post.php?$Q" '' application/x-www-form-urlencoded 'action=x'
record POST "/wp-login.php?$Q" '' application/x-www-form-urlencoded 'log=admin&pwd=x'
record POST "/wp-comments-post.php?$Q" '' application/x-www-form-urlencoded 'comment_post_ID=1&comment=x'
record POST "/wp-comments-post.php?$Q"
record GET "/wp-comments-post.php?$Q"
# Ajax, whatever the Accept header says.
record GET "/wp-admin/admin-ajax.php?action=heartbeat&$Q"
record POST "/wp-admin/admin-ajax.php?$Q" '' application/x-www-form-urlencoded 'action=heartbeat'
record POST "/wp-admin/admin-ajax.php?$Q"
record GET "/wp-admin/admin-ajax.php?$Q" application/json
record POST "/wp-admin/admin-ajax.php?$Q" text/xml application/json '{}'
# The REST API, and the handlers a request's Accept and Content-Type pick.
for path in "/?rest_route=/&$Q" "/?rest_route=/wp/v2/posts&$Q" "/wp-json/?$Q" "/wp-json/wp/v2/posts?$Q" "/index.php?rest_route=/&$Q" \
  "/wp-json/?_jsonp=cb&$Q" "/?_jsonp=cb&$Q" "/?$Q"; do
  record GET "$path"
  record GET "$path" application/json
done
record GET "/wp-json/?$Q" 'text/html, application/json;q=0.9'
record GET "/wp-json/?$Q" 'application/vnd.api+json'
record GET "/wp-json/?$Q" 'application/jsonp'
record GET "/wp-json/?$Q" 'application/json-patch'
record GET "/wp-json/?$Q" 'APPLICATION/JSON; charset=utf-8'
record GET "/wp-json/?$Q" 'text/xml'
record GET "/wp-json/?$Q" 'application/rss+xml'
record GET "/wp-json/?$Q" 'application/atom+xml, text/html'
record GET "/wp-json/?$Q" 'application/json, text/xml'
record GET "/?$Q" 'text/xml+oembed'
record GET "/?$Q" 'application/xml+oembed'
record GET "/?$Q" 'application/xml'
record POST "/wp-json/wp/v2/posts?$Q" '' application/json '{"title":"x"}'
record POST "/wp-json/wp/v2/posts?$Q" '' 'application/json; charset=UTF-8' '{"title":"x"}'
record POST "/wp-json/wp/v2/posts?$Q" '' application/x-www-form-urlencoded 'title=x'
record POST "/?$Q" '' text/xml '<x/>'
record POST "/?$Q" '' application/rss+xml '<x/>'
record POST "/?$Q" '' 'text/xml; charset=UTF-8' '<x/>'
record POST "/?$Q" '' application/ld+json '{}'
# xmlrpc.php: its own handler, which has no server yet on init.
record GET "/xmlrpc.php?$Q"
record POST "/xmlrpc.php?$Q" '' text/xml '<methodCall><methodName>demo.sayHello</methodName></methodCall>'
record POST "/xmlrpc.php?$Q" application/json text/xml '<x/>'
record GET "/xmlrpc.php?rsd&$Q"
# The loaders do not load WordPress, and answer as ever.
record GET "/wp-admin/load-scripts.php?load=jquery&$Q"
record GET "/wp-admin/load-styles.php?load=login&$Q"
wc -l "$OUT"

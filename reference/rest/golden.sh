#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's REST API answers as a visitor and as each role,
# logged in through wp-login.php with reference/rest/users.sql's users
# (added and deleted after) and the fixture administrator, as
# crates/hyscribe-rest/tests/golden/rest.tsv, and the cases of
# reference/rest/schema-golden.php, fields-golden.php and hierarchy-golden.php
# as schema.tsv, fields.tsv and hierarchy.tsv beside it. With a list of
# requests as its argument, such as
# reference/rest/editor.list, it records those as <list name>.tsv instead,
# over the database php.sh runs on, with the rows of <list name>.sql beside
# it if there is one (added before, and its statements after a `-- cleanup`
# line run after) and the must-use plugin <list name>.php beside it if there
# is one (added for the run and removed after); on the Theme Test Data fixture
# (HYSCRIBE_PHP_DB=wordpress_ttd), whose content is not ours to commit, as
# <list name>-ttd.tsv with each body's SHA-256:
#   who<TAB>method<TAB>hex(target)<TAB>flags<TAB>status<TAB>hex(headers)<TAB>hex(body)[<TAB>hex(sent)]
# A list's line is method<TAB>target<TAB>flags, and a body to send as a
# fourth column, recorded as sent: JSON with the flag json, a URL-encoded
# form with form.
# A logged in user sends the nonce admin-ajax.php's rest-nonce action gives
# them as X-WP-Nonce, unless flags has nononce; %nonce% in a target, the
# headers and the body stands for the user's nonce (a visitor's for a
# visitor), and %preview_nonce% in a body for a preview link's. Flags:
# origin and originnull send an Origin header, pretty serves the request under /%postname%/ permalinks (reference/rewrite/permalinks.php),
# override sends X-HTTP-Method-Override: DELETE and put PUT, as the
# editors' apiFetch() sends its PUT and DELETE requests, localhost sends Host:
# localhost:8081, which wp_is_client_side_media_processing_enabled() takes
# as a secure context, refused marks a request HyScribe answers 501 (sent
# as any other), notrash marks a request recorded with EMPTY_TRASH_DAYS at
# 0 by the list's must-use plugin, for the tests to serve it so, restore lets the request
# write and puts the database back after it as it was once the users logged
# in (dates of the request's own time in its answer read %now%), ttd records the request on
# the Theme Test Data fixture only, default on the default install only (as
# one that PHP would write caches for there), skip=a+b sends the request
# as every identity but those listed, and writes
# sends it once as the administrator after the others, fails the run unless
# it writes to the database, and records the status as writes; hash keeps the
# body's SHA-256, as on the Theme Test Data fixture; secret has the random
# secret of an oEmbed answer's HTML read 0123456789. The headers kept are
# WordPress's own, one "name: value" a line in the order sent, with Apache's
# Accept-Encoding left out of Vary. Cron is off for the run, no one may post
# unfiltered HTML, as in HyScribe, and every
# table's checksum is compared before and after the REST requests: a REST
# request that writes fails the run. Logging in writes sessions, so start
# from a fresh db.sh up, then php.sh up, and take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-rest/tests/golden/rest.tsv"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp -d)
STATE="${1:-}"; PLUGIN="${STATE%.list}.php"; STATE="${STATE%.list}.sql"
[ -f "$STATE" ] || STATE=/dev/null
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only "${HYSCRIBE_PHP_DB:-wordpress}" "$@"; }
cleanup() {
  rm -rf "$TMP"
  echo "DELETE FROM wp_users WHERE ID BETWEEN 701 AND 704; DELETE FROM wp_usermeta WHERE user_id BETWEEN 701 AND 704;" | sql
  sed -n '/^-- cleanup$/,$p' "$STATE" | sql
  "$DOCKER" exec "$DB" rm -f /tmp/rest-golden.sql
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-permalinks.php" "$MU/hyscribe-no-cron.php" "$MU/hyscribe-list.php"
}
trap cleanup EXIT
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-permalinks.php" < "$ROOT/reference/rewrite/permalinks.php"
echo '<?php define( "DISABLE_WP_CRON", true ); define( "DISALLOW_UNFILTERED_HTML", true );' | "$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-no-cron.php"
[ -f "$PLUGIN" ] && "$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-list.php" < "$PLUGIN"
sql < "$ROOT/reference/rest/users.sql"
sed '/^-- cleanup$/,$d' "$STATE" | sql
checksums() { sql -N -e 'CHECKSUM TABLE wp_commentmeta, wp_comments, wp_links, wp_options, wp_postmeta, wp_posts, wp_term_relationships, wp_term_taxonomy, wp_termmeta, wp_terms, wp_usermeta, wp_users' < /dev/null; }

# method<TAB>target<TAB>flags
REQUESTS=$(cat <<'LIST'
GET	/?rest_route=/wp/v2/types
GET	/index.php?rest_route=/wp/v2/types/post
GET	/?rest_route=/wp/v2/types&context=edit
GET	/?rest_route=/wp/v2/types&context=embed
GET	/?rest_route=/wp/v2/types/page&context=edit
GET	/?rest_route=/wp/v2/types/post&context=embed
GET	/?rest_route=/wp/v2/types/attachment&context=edit
GET	/?rest_route=/wp/v2/types/wp_block&context=edit
GET	/?rest_route=/wp/v2/types/wp_navigation&context=edit
GET	/?rest_route=/wp/v2/types/wp_template&context=edit
GET	/?rest_route=/wp/v2/types/nope
GET	/?rest_route=/wp/v2/types/revision
GET	/?rest_route=/wp/v2/types/a.b
GET	/?rest_route=/WP/V2/Types/post
GET	/?rest_route=/wp/v2/types/POST
GET	/?rest_route=/wp/v2/types/
GET	/?rest_route=/wp/v2/types/post/%5C/
GET	/?rest_route=0/
GET	/?rest_route=/wp/v2/types&context=bogus
GET	/?rest_route=/wp/v2/types&context=
GET	/?rest_route=/wp/v2/types&context[]=edit
GET	/?rest_route=/wp/v2/types/post&type=page
GET	/?rest_route=/wp/v2/types/post&type[]=page
GET	/?rest_route=/wp/v2/types&_fields=slug,name
GET	/?rest_route=/wp/v2/types/post&_fields=labels.name,%20slug,_links.collection
GET	/?rest_route=/wp/v2/types/post&_fields=supports.editor,labels,labels.name&context=edit
GET	/?rest_route=/wp/v2/types/post&_fields=
GET	/?rest_route=/wp/v2/types/post&_fields[]=slug&_fields[]=name
GET	/?rest_route=/wp/v2/types/post&_fields=slug.x
GET	/?rest_route=/wp/v2/types/post&_fields=_embedded
GET	/?rest_route=/wp/v2/types/post&_embed&_fields=_embedded
GET	/?rest_route=/wp/v2/types/nope&_fields=slug
GET	/?rest_route=/wp/v2/types/post&_envelope
GET	/?rest_route=/wp/v2/types&_envelope=1&context=bogus
GET	/?rest_route=/wp/v2/types/post&_jsonp=cb.x_1
GET	/?rest_route=/wp/v2/types/post&_jsonp=bad(
GET	/?rest_route=/wp/v2/types/post&_jsonp=
GET	/?rest_route=/wp/v2/types/post&_jsonp[]=x
GET	/?rest_route=/nope&_jsonp=cb
GET	/?rest_route=/wp/v2/types/post&_pretty
GET	/?rest_route=/wp/v2/types/nope&_pretty=0
GET	/?rest_route=/wp/v2/types/post&_embed
HEAD	/?rest_route=/wp/v2/types
HEAD	/?rest_route=/wp/v2/types/post&context=edit
GET	/?rest_route=/nope
GET	/?rest_route=/wp/v2/types/post&_method=DELETE
GET	/?rest_route=/wp/v2/types/post	override
POST	/?rest_route=/wp/v2/types
PUT	/?rest_route=/nope
DELETE	/?rest_route=/wp/v2/types/post
OPTIONS	/?rest_route=/nope
OPTIONS	/?rest_route=/wp/v2/types
OPTIONS	/?rest_route=/nope&_fields=a
GET	/?rest_route=/wp/v2/types&_method[]=x
GET	/?rest_route=/wp/v2/types/post&_wpnonce=abc
GET	/?rest_route=/wp/v2/types/post&_wpnonce=%nonce%	nononce
GET	/?rest_route=/wp/v2/types&context=edit	nononce
GET	/?rest_route=/wp/v2/types/page	origin
GET	/?rest_route=/wp/v2/types/page	originnull
GET	/wp-json/wp/v2/types/post	pretty
GET	/wp-json/WP/v2/types/page/	pretty
GET	/wp-json/nope	pretty
GET	/index.php/wp-json/wp/v2/types?_fields=slug	pretty
GET	/?rest_route=/wp/v2/types/page	pretty
GET	/wp-json/wp/v2/types/post?rest_route=/wp/v2/types/page	pretty
GET	/?rest_route=/wp/v2/search&per_page=1
GET	/?rest_route=/wp/v2/search&per_page=1&page=2&search=
GET	/?rest_route=/
GET	/?rest_route=/wp/v2
GET	/?rest_route=/wp/v2/posts
GET	/?rest_route=/wp/v2/templates/twentytwentyfive//index
GET	/?rest_route=/wp-abilities/v1/categories/a-b
GET	/?rest_route=/wp/v2/types/post/x
LIST
)
if [ $# -gt 0 ]; then
  REQUESTS=$(grep -v '^#' "$1")
  OUT="$ROOT/crates/hyscribe-rest/tests/golden/$(basename "$1" .list)$([ "${HYSCRIBE_PHP_DB:-}" = wordpress_ttd ] && echo -ttd || true).tsv"
fi

curl_args() {
  local who="$1" flags="$2" nonce="$3"
  args=(-g)
  if [ "$who" != visitor ]; then
    local jar="$TMP/$who.jar"
    # curl matches cookies to the Host header sent.
    if [[ ",$flags," == *,localhost,* ]]; then
      sed 's/127\.0\.0\.1/localhost/' "$jar" > "$TMP/$who.localhost.jar"
      jar="$TMP/$who.localhost.jar"
    fi
    args+=(-b "$jar")
    [[ "$flags" == *nononce* ]] || args+=(-H "X-WP-Nonce: $nonce")
  fi
  [[ "$flags" == *originnull* ]] && args+=(-H "Origin: null")
  [[ "$flags" == *origin* && "$flags" != *originnull* ]] && args+=(-H "Origin: http://example.test")
  [[ "$flags" == *pretty* ]] && args+=(-H "X-HyScribe-Permalinks: /%postname%/|||")
  [[ "$flags" == *override* ]] && args+=(-H "X-HTTP-Method-Override: DELETE")
  [[ ",$flags," == *,put,* ]] && args+=(-H "X-HTTP-Method-Override: PUT")
  [[ ",$flags," == *,localhost,* ]] && args+=(-H "Host: localhost:8081")
  [[ ",$flags," == *,json,* ]] && args+=(-H "Content-Type: application/json")
  [[ ",$flags," == *,form,* ]] && args+=(-H "Content-Type: application/x-www-form-urlencoded")
  [ -z "$4" ] || args+=(--data-binary "$4")
  return 0
}

# The dates in an answer from $2 to $3 (Unix times) as %now%.
mask_now() {
  php -r 'echo preg_replace_callback( "/\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d/", fn( $m ) => strtotime( $m[0] . "Z" ) >= $argv[2] && strtotime( $m[0] . "Z" ) <= $argv[3] ? "%now%" : $m[0], file_get_contents( $argv[1] ) );' "$1" "$2" "$3"
}

declare -A NONCE
NONCE[visitor]=$("$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; echo wp_create_nonce( "wp_rest" );')
WHO=(visitor administrator editor author contributor subscriber)
for who in "${WHO[@]:1}"; do
  login=$who
  [ "$who" = administrator ] && login=admin
  curl -s -o /dev/null -c "$TMP/$who.jar" -b 'wordpress_test_cookie=WP%20Cookie%20check' \
    --data-urlencode "log=$login" --data-urlencode 'pwd=reference-only-password' -d 'testcookie=1' "$BASE/wp-login.php"
  NONCE[$who]=$(curl -s -b "$TMP/$who.jar" "$BASE/wp-admin/admin-ajax.php?action=rest-nonce")
  [[ "${NONCE[$who]}" =~ ^[0-9a-f]{10}$ ]] || { echo "no nonce for $who" >&2; exit 1; }
done

before=$(checksums)
if grep -q $'^[^\t]*\t[^\t]*\t[^\t]*restore' <<< "$REQUESTS"; then
  "$DOCKER" exec "$DB" sh -c "mariadb-dump -uroot -preference-only ${HYSCRIBE_PHP_DB:-wordpress} > /tmp/rest-golden.sql"
fi
: > "$OUT"
for who in "${WHO[@]}"; do
  nonce=${NONCE[$who]}
  while IFS=$'\t' read -r method target flags sent; do
    [[ "$flags" == *writes* ]] && continue
    [[ "$flags" == *ttd* && "${HYSCRIBE_PHP_DB:-}" != wordpress_ttd ]] && continue
    [[ ",$flags," == *,default,* && "${HYSCRIBE_PHP_DB:-}" = wordpress_ttd ]] && continue
    [[ "+${flags#*skip=}+" == *"+$who+"* && "$flags" == *skip=* ]] && continue
    curl_args "$who" "$flags" "$nonce" "${sent:-}"
    url="$BASE${target//%nonce%/$nonce}"
    start=$(date +%s)
    if [ "$method" = HEAD ]; then
      status=$(curl -s -I -o "$TMP/head" -w '%{http_code}' "${args[@]}" "$url")
      : > "$TMP/body"
    else
      status=$(curl -s -X "$method" -o "$TMP/body" -D "$TMP/head" -w '%{http_code}' "${args[@]}" "$url")
    fi
    if [[ "$flags" == *restore* ]]; then
      mask_now "$TMP/body" "$start" "$(date +%s)" > "$TMP/masked"
      mv "$TMP/masked" "$TMP/body"
      [ "$(checksums)" = "$before" ] ||
        "$DOCKER" exec "$DB" sh -c "mariadb -uroot -preference-only ${HYSCRIBE_PHP_DB:-wordpress} < /tmp/rest-golden.sql"
    fi
    headers=$(tr -d '\r' < "$TMP/head" | grep -iE '^(content-type|x-robots-tag|link|location|access-control-[a-z-]+|allow|vary|x-wp-[a-z]+|expires|cache-control):' |
      sed -E 's/^([^:]+):/\L\1:/; s/^(vary: .*),Accept-Encoding$/\1/' | grep -vx 'vary: Accept-Encoding' | sed "s/$nonce/%nonce%/g" || true)
    if [[ ",$flags," == *,secret,* ]]; then
      python3 -I -c '
import re, sys
s = open(sys.argv[1], "rb").read()
m = re.search(rb"#\?secret=([A-Za-z0-9]{10})", s)
open(sys.argv[1], "wb").write(s.replace(m.group(1), b"0123456789") if m else s)' "$TMP/body"
    fi
    if [ "${HYSCRIBE_PHP_DB:-}" = wordpress_ttd ] || [[ ",$flags," == *,hash,* ]]; then
      body=$(sed "s/$nonce/%nonce%/g; s/preview_nonce=[0-9a-f]\{10\}/preview_nonce=%preview_nonce%/g" "$TMP/body" | sha256sum | cut -c1-64 | tr -d '\n' | xxd -p | tr -d '\n')
    else
      body=$(sed "s/$nonce/%nonce%/g; s/preview_nonce=[0-9a-f]\{10\}/preview_nonce=%preview_nonce%/g" "$TMP/body" | xxd -p | tr -d '\n')
    fi
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s%s\n' "$who" "$method" "$(printf '%s' "$target" | xxd -p | tr -d '\n')" "$flags" "$status" \
      "$(printf '%s' "$headers" | xxd -p | tr -d '\n')" "$body" "${sent:+$'\t'$(printf '%s' "$sent" | xxd -p | tr -d '\n')}" >> "$OUT"
  done <<< "$REQUESTS"
done
if [ $# -gt 0 ]; then
  after=$(checksums)
  [ "$before" = "$after" ] || { echo "a REST request wrote to the database" >&2; diff <(echo "$before") <(echo "$after") >&2; exit 1; }
  while IFS=$'\t' read -r method target flags _; do
    [[ "$flags" == *writes* ]] || continue
    curl_args administrator "$flags" "${NONCE[administrator]}"
    curl -s -o /dev/null -X "$method" "${args[@]}" "$BASE$target"
    [ "$(checksums)" != "$after" ] || { echo "$target did not write" >&2; exit 1; }
    after=$(checksums)
    printf 'administrator\t%s\t%s\t%s\twrites\t\t\n' "$method" "$(printf '%s' "$target" | xxd -p | tr -d '\n')" "$flags" >> "$OUT"
  done <<< "$REQUESTS"
  wc -l "$OUT"
  exit 0
fi
"$DOCKER" cp "$ROOT/reference/rest/schema-golden.php" "$NAME:/tmp/schema-golden.php"
"$DOCKER" exec "$NAME" php /tmp/schema-golden.php > "$ROOT/crates/hyscribe-rest/tests/golden/schema.tsv"
"$DOCKER" cp "$ROOT/reference/rest/fields-golden.php" "$NAME:/tmp/fields-golden.php"
"$DOCKER" exec "$NAME" php /tmp/fields-golden.php > "$ROOT/crates/hyscribe-rest/tests/golden/fields.tsv"
"$DOCKER" cp "$ROOT/reference/rest/hierarchy-golden.php" "$NAME:/tmp/hierarchy-golden.php"
"$DOCKER" exec "$NAME" php /tmp/hierarchy-golden.php > "$ROOT/crates/hyscribe-rest/tests/golden/hierarchy.tsv"
after=$(checksums)
[ "$before" = "$after" ] || { echo "a REST request wrote to the database" >&2; diff <(echo "$before") <(echo "$after") >&2; exit 1; }
wc -l "$OUT" "$ROOT"/crates/hyscribe-rest/tests/golden/{schema,fields,hierarchy}.tsv

#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's REST API answers as a visitor and as each role,
# logged in through wp-login.php with reference/rest/users.sql's users
# (added and deleted after) and the fixture administrator, as
# crates/hyscribe-rest/tests/golden/rest.tsv, and the cases of
# reference/rest/schema-golden.php and fields-golden.php as schema.tsv and
# fields.tsv beside it:
#   who<TAB>method<TAB>hex(target)<TAB>flags<TAB>status<TAB>hex(headers)<TAB>hex(body)
# A logged in user sends the nonce admin-ajax.php's rest-nonce action gives
# them as X-WP-Nonce, unless flags has nononce; %nonce% in a target, the
# headers and the body stands for the user's nonce (a visitor's for a
# visitor). Flags: origin and originnull send an Origin header, pretty serves
# the request under /%postname%/ permalinks (reference/rewrite/permalinks.php),
# override sends X-HTTP-Method-Override: DELETE. The headers kept are
# WordPress's own, one "name: value" a line in the order sent, with Apache's
# Accept-Encoding left out of Vary. Cron is off for the run, and every
# table's checksum is compared before and after the REST requests: a REST
# request that writes fails the run. Logging in writes sessions, so start
# from a fresh db.sh up, then php.sh up, and take both down after.
#
# With arguments, it records the requests listed in the file $1 (method,
# target and flags, tab separated) as $2 instead, and no schema or fields
# cases; with HYSCRIBE_PHP_DB=wordpress_ttd (php.sh up with the same), on
# the Theme Test Data fixture, whose content is not ours to commit, each
# body is kept as the hex of its sha256.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="${2:-$ROOT/crates/hyscribe-rest/tests/golden/rest.tsv}"
DB_NAME="${HYSCRIBE_PHP_DB:-wordpress}"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp -d)
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only "$DB_NAME" "$@"; }
cleanup() {
  rm -rf "$TMP"
  echo "DELETE FROM wp_users WHERE ID BETWEEN 701 AND 704; DELETE FROM wp_usermeta WHERE user_id BETWEEN 701 AND 704;" | sql
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-permalinks.php" "$MU/hyscribe-no-cron.php"
}
trap cleanup EXIT
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-permalinks.php" < "$ROOT/reference/rewrite/permalinks.php"
echo '<?php define( "DISABLE_WP_CRON", true );' | "$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-no-cron.php"
sql < "$ROOT/reference/rest/users.sql"
checksums() { sql -N -e 'CHECKSUM TABLE wp_commentmeta, wp_comments, wp_links, wp_options, wp_postmeta, wp_posts, wp_term_relationships, wp_term_taxonomy, wp_termmeta, wp_terms, wp_usermeta, wp_users'; }

# method<TAB>target<TAB>flags
if [ $# -gt 0 ]; then
  REQUESTS=$(cat "$1")
else
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
fi

curl_args() {
  local who="$1" flags="$2" nonce="$3"
  args=(-g)
  if [ "$who" != visitor ]; then
    args+=(-b "$TMP/$who.jar")
    [[ "$flags" == *nononce* ]] || args+=(-H "X-WP-Nonce: $nonce")
  fi
  [[ "$flags" == *originnull* ]] && args+=(-H "Origin: null")
  [[ "$flags" == *origin* && "$flags" != *originnull* ]] && args+=(-H "Origin: http://example.test")
  [[ "$flags" == *pretty* ]] && args+=(-H "X-HyScribe-Permalinks: /%postname%/|||")
  [[ "$flags" == *override* ]] && args+=(-H "X-HTTP-Method-Override: DELETE")
  return 0
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
: > "$OUT"
for who in "${WHO[@]}"; do
  nonce=${NONCE[$who]}
  while IFS=$'\t' read -r method target flags; do
    curl_args "$who" "$flags" "$nonce"
    url="$BASE${target//%nonce%/$nonce}"
    if [ "$method" = HEAD ]; then
      status=$(curl -s -I -o "$TMP/head" -w '%{http_code}' "${args[@]}" "$url")
      : > "$TMP/body"
    else
      status=$(curl -s -X "$method" -o "$TMP/body" -D "$TMP/head" -w '%{http_code}' "${args[@]}" "$url")
    fi
    headers=$(tr -d '\r' < "$TMP/head" | grep -iE '^(content-type|x-robots-tag|link|access-control-[a-z-]+|allow|vary|x-wp-[a-z]+|expires|cache-control):' |
      sed -E 's/^([^:]+):/\L\1:/; s/^(vary: .*),Accept-Encoding$/\1/' | grep -vx 'vary: Accept-Encoding' | sed "s/$nonce/%nonce%/g" || true)
    if [ "$DB_NAME" = wordpress_ttd ]; then
      body=$(sed "s/$nonce/%nonce%/g" "$TMP/body" | sha256sum | cut -d' ' -f1)
    else
      body=$(sed "s/$nonce/%nonce%/g" "$TMP/body" | xxd -p | tr -d '\n')
    fi
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$who" "$method" "$(printf '%s' "$target" | xxd -p | tr -d '\n')" "$flags" "$status" \
      "$(printf '%s' "$headers" | xxd -p | tr -d '\n')" "$body" >> "$OUT"
  done <<< "$REQUESTS"
done
if [ $# -eq 0 ]; then
  "$DOCKER" cp "$ROOT/reference/rest/schema-golden.php" "$NAME:/tmp/schema-golden.php"
  "$DOCKER" exec "$NAME" php /tmp/schema-golden.php > "$ROOT/crates/hyscribe-rest/tests/golden/schema.tsv"
  "$DOCKER" cp "$ROOT/reference/rest/fields-golden.php" "$NAME:/tmp/fields-golden.php"
  "$DOCKER" exec "$NAME" php /tmp/fields-golden.php > "$ROOT/crates/hyscribe-rest/tests/golden/fields.tsv"
fi
after=$(checksums)
[ "$before" = "$after" ] || { echo "a REST request wrote to the database" >&2; diff <(echo "$before") <(echo "$after") >&2; exit 1; }
wc -l "$OUT"

#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records what the PHP oracle answers the front end comment posts of
# reference/comment-post/cases.list, as they really write, as
# crates/hyscribe-http/tests/golden/comment-post.tsv:
#   case<TAB>status<TAB>Location<TAB>Set-Cookie lines, joined by |<TAB>hex(body)
# The last comment's ID is %id%, a moderation hash %hash% and a cookie's
# expiry %expires%. In a body, %nonce% is user 1's unfiltered HTML nonce
# and %aN% N letters a. Each case runs its setup SQL, sends its first body (if
# any) and its body, then deletes the comments it added and undoes its
# setup, so each starts from the default install. The posts write, so
# start from a fresh db.sh up and php.sh up, and take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
HERE="$ROOT/reference/comment-post"
OUT="$ROOT/crates/hyscribe-http/tests/golden/comment-post.tsv"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp -d)
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress "$@"; }
cleanup() {
  rm -rf "$TMP"
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-comment-post.php"
}
trap cleanup EXIT
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-comment-post.php" < "$HERE/plugin.php"
# User 1's nonce for unfiltered HTML on post 1, without a session.
nonce=$("$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 1 ); echo wp_create_nonce( "unfiltered-html-comment_1" );')
: > "$OUT"
send() {
  local method=$1 user=$2 cookie=$3 data=${4//%nonce%/$nonce}
  local jar="hyscribe_user=$user"
  [ "$cookie" = - ] || jar="$jar; $cookie"
  local args=(-g -s -o "$TMP/body" -D "$TMP/head" -w '%{http_code}' -A 'HyScribe oracle' -b "$jar")
  if [ "$method" = MULTIPART ]; then
    # The pairs as multipart/form-data parts; a name starting with @ is a file.
    args+=(-X POST)
    while IFS= read -r -d '' arg; do args+=("$arg"); done < <(python3 -I -c '
import sys, urllib.parse as u
for n, pair in enumerate(sys.argv[1].split("&")):
    name, _, value = pair.partition("=")
    name, value = u.unquote_plus(name), u.unquote_plus(value)
    if name.startswith("@"):
        path = sys.argv[2] + "/file" + str(n)
        open(path, "w").write(value)
        sys.stdout.write("-F\0" + name[1:] + "=@" + path + ";filename=picture.txt;type=text/plain\0")
    else:
        sys.stdout.write("--form-string\0" + name + "=" + value + "\0")
' "$data" "$TMP")
  else
    args+=(-X "$method")
    if [ "$method" = POST ]; then
      # %aN% stands for N letters a.
      data=$(printf '%s' "$data" | perl -pe 's/%a(\d+)%/"a" x $1/ge')
      args+=(-H 'Content-Type: application/x-www-form-urlencoded' --data-binary "$data")
    fi
  fi
  curl "${args[@]}" "$BASE/wp-comments-post.php"
}
while IFS=$'\t' read -r case method user cookie setup undo first body; do
  [ "$setup" = - ] || sql -e "$setup" < /dev/null
  [ "$first" = - ] || send "$method" "$user" "$cookie" "$first" > /dev/null
  status=$(send "$method" "$user" "$cookie" "$body")
  id=$(sql -N -e 'SELECT MAX(comment_ID) FROM wp_comments' < /dev/null)
  location=$(grep -i '^location:' "$TMP/head" | cut -d' ' -f2- | tr -d '\r' || true)
  location=$(printf '%s' "$location" | sed -E "s/moderation-hash=[0-9a-f]{32}/moderation-hash=%hash%/; s/(comment-|unapproved=)$id\b/\1%id%/g")
  cookies=$(grep -i '^set-cookie:' "$TMP/head" | cut -d' ' -f2- | tr -d '\r' \
    | sed -E 's/expires=[^;]*; Max-Age=[0-9]+/%expires%/' | paste -sd '|' || true)
  printf '%s\t%s\t%s\t%s\t%s\n' "$case" "$status" "$location" "$cookies" "$(xxd -p < "$TMP/body" | tr -d '\n')" >> "$OUT"
  sql -e 'DELETE FROM wp_comments WHERE comment_ID > 1; DELETE FROM wp_commentmeta; UPDATE wp_posts SET comment_count = 1 WHERE ID = 1' < /dev/null
  [ "$undo" = - ] || sql -e "$undo" < /dev/null
done < <(grep -v '^#' "$HERE/cases.list")
wc -l "$OUT"

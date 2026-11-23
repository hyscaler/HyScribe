#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records what the PHP oracle answers the admin's actions and the REST API's
# writes before they write. Each case of reference/action-checks/cases.list
# is sent as each role (reference/admin-menu/as-role.php) with no nonce, a
# wrong one, another user's and a valid one, over the default install and
# reference/action-checks/content.sql (added, and deleted after), as
# crates/hyscribe-admin/tests/golden/action-checks.tsv:
#   role<TAB>case<TAB>nonce<TAB>status<TAB>hex(body)
# Nothing is written: reference/action-checks/stop.php stops a request at its
# first write and answers hyscribe-accepted, recorded as the status accepted
# (its query is printed for a review), and every table's checksum is
# compared before and after. Then
# reference/rest/golden.sh records reference/rest/writes.list with the same
# plugin. Needs a fresh db.sh up and php.sh up; take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
HERE="$ROOT/reference/action-checks"
OUT="$ROOT/crates/hyscribe-admin/tests/golden/action-checks.tsv"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp -d)
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress "$@"; }
checksums() { sql -N -e 'CHECKSUM TABLE wp_commentmeta, wp_comments, wp_links, wp_options, wp_postmeta, wp_posts, wp_term_relationships, wp_term_taxonomy, wp_termmeta, wp_terms, wp_usermeta, wp_users' < /dev/null; }
cleanup() {
  rm -rf "$TMP"
  sed -n '/^-- cleanup$/,$p' "$HERE/content.sql" | sql
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-stop.php" "$MU/hyscribe-as-role.php"
  "$ROOT/reference/admin-look/look.sh" remove || true
}
trap cleanup EXIT
"$ROOT/reference/admin-look/look.sh" add
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-stop.php" < "$HERE/stop.php"

# The REST API's writes, as each role logged in.
"$ROOT/reference/rest/golden.sh" "$ROOT/reference/rest/writes.list"

sed '/^-- cleanup$/,$d' "$HERE/content.sql" | sql
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
# The nonces of user 1 and of user 801, neither with a session.
ACTIONS=$(grep -v '^#' "$HERE/cases.list" | cut -f5 | sort -u)
# shellcheck disable=SC2086
"$DOCKER" exec "$NAME" php -r '
  require "/var/www/html/wp-load.php";
  foreach ( array_slice( $argv, 1 ) as $action ) {
    wp_set_current_user( 1 );
    $own = wp_create_nonce( $action );
    wp_set_current_user( 801 );
    echo $action, "\t", $own, "\t", wp_create_nonce( $action ), "\n";
  }' $ACTIONS > "$TMP/nonces"
before=$(checksums)
: > "$OUT"
for role in administrator editor author contributor subscriber; do
  while IFS=$'\t' read -r case method target body action field flags; do
    for variant in none wrong other valid; do
      own=$(awk -F'\t' -v a="$action" '$1 == a { print $2 }' "$TMP/nonces")
      other=$(awk -F'\t' -v a="$action" '$1 == a { print $3 }' "$TMP/nonces")
      case $variant in
        none) nonce='' ;;
        wrong) nonce=0123456789 ;;
        other) nonce=$other ;;
        valid) nonce=$own ;;
      esac
      data=$body; [ "$data" != - ] || data=""
      url="$BASE/wp-admin/$target"
      args=(-g -s -o "$TMP/body" -D "$TMP/head" -w '%{http_code}' -b "hyscribe_role=$role")
      if [ -n "$nonce" ]; then
        if [ "$method" = POST ]; then
          data="${data:+$data&}$field=$nonce"
        else
          url="$url&$field=$nonce"
        fi
      fi
      [ "$method" = POST ] && args+=(-H 'Content-Type: application/x-www-form-urlencoded' --data-raw "$data")
      [[ "${flags:-}" == *referer* ]] && args+=(-H "Referer: $BASE/wp-admin/edit.php?paged=2&updated=1")
      status=$(curl "${args[@]}" "$url")
      if [ "$(cat "$TMP/body")" = hyscribe-accepted ]; then
        printf '%s %s %s: %s\n' "$role" "$case" "$variant" "$(grep -i '^x-hyscribe-write:' "$TMP/head" | tr -d '\r')" >&2
        status=accepted
        : > "$TMP/body"
      fi
      printf '%s\t%s\t%s\t%s\t%s\n' "$role" "$case" "$variant" "$status" "$(xxd -p < "$TMP/body" | tr -d '\n')" >> "$OUT"
    done
  done < <(grep -v '^#' "$HERE/cases.list")
done
after=$(checksums)
[ "$before" = "$after" ] || { echo "an action wrote to the database" >&2; diff <(echo "$before") <(echo "$after") >&2; exit 1; }
wc -l "$OUT"

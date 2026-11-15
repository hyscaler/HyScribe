#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records what the PHP oracle answers the admin actions of
# reference/admin-saves/cases.list once their checks pass, as they really
# write, as crates/hyscribe-admin/tests/golden/admin-saves.tsv:
#   case<TAB>status<TAB>Location<TAB>hex(body)
# and the screen each redirect leads to as admin-saves-after.tsv:
#   case<TAB>status<TAB>hex(body)
# Each case is sent as user 1, an administrator
# (reference/admin-menu/as-role.php), with user 1's nonce, over the default
# install and reference/action-checks/content.sql (added, and deleted
# after), restored before each case and after its case's SQL, if any, ran.
# The actions write, so start from a fresh db.sh up and php.sh up, and take
# both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
HERE="$ROOT/reference/admin-saves"
CONTENT="$ROOT/reference/action-checks/content.sql"
OUT="$ROOT/crates/hyscribe-admin/tests/golden/admin-saves.tsv"
AFTER="$ROOT/crates/hyscribe-admin/tests/golden/admin-saves-after.tsv"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp -d)
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress "$@"; }
cleanup() {
  rm -rf "$TMP"
  "$DOCKER" exec "$DB" sh -c 'mariadb -uroot -preference-only wordpress < /tmp/admin-saves.sql; rm /tmp/admin-saves.sql' || true
  sed -n '/^-- cleanup$/,$p' "$CONTENT" | sql
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php"
}
trap cleanup EXIT
sed '/^-- cleanup$/,$d' "$CONTENT" | sql
"$DOCKER" exec "$DB" sh -c 'mariadb-dump -uroot -preference-only wordpress > /tmp/admin-saves.sql'
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
: > "$OUT"
: > "$AFTER"
while IFS=$'\t' read -r case method target body action field setup; do
  "$DOCKER" exec "$DB" sh -c 'mariadb -uroot -preference-only wordpress < /tmp/admin-saves.sql'
  [ -z "$setup" ] || sql -e "$setup" < /dev/null
  # User 1's nonce, without a session as the requests have none.
  nonce=$("$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 1 ); echo wp_create_nonce( $argv[1] );' "$action")
  url="$BASE/wp-admin/$target"
  args=(-g -s -o "$TMP/body" -D "$TMP/head" -w '%{http_code}' -b hyscribe_role=administrator)
  if [ "$method" = POST ]; then
    data=$body; [ "$data" != - ] || data=""
    args+=(-H 'Content-Type: application/x-www-form-urlencoded' --data-raw "${data:+$data&}$field=$nonce")
  else
    url="$url&$field=$nonce"
  fi
  status=$(curl "${args[@]}" "$url")
  location=$(grep -i '^location:' "$TMP/head" | cut -d' ' -f2- | tr -d '\r' || true)
  printf '%s\t%s\t%s\t%s\n' "$case" "$status" "$location" "$(xxd -p < "$TMP/body" | tr -d '\n')" >> "$OUT"
  [ "$status" = 302 ] || continue
  case "$location" in http*) ;; /*) location="$BASE$location" ;; *) location="$BASE/wp-admin/$location" ;; esac
  status=$(curl -g -s -o "$TMP/body" -w '%{http_code}' -b hyscribe_role=administrator "$location")
  printf '%s\t%s\t%s\n' "$case" "$status" "$(xxd -p < "$TMP/body" | tr -d '\n')" >> "$AFTER"
done < <(grep -v '^#' "$HERE/cases.list")
wc -l "$OUT" "$AFTER"

#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records what the PHP oracle answers the admin actions of
# reference/admin-saves/cases.list once their checks pass, as they really
# write, as crates/hyscribe-admin/tests/golden/admin-saves.tsv:
#   case<TAB>status<TAB>Location<TAB>hex(body)
# Each case is sent as user 1, an administrator
# (reference/admin-menu/as-role.php), with user 1's nonce, over the default
# install and reference/action-checks/content.sql, both as they were before
# each case (the content is added, and deleted after). Mail is taken as
# sent, as hs-admin answers (MVP phase 8 sends it). The actions write, so
# start from a fresh db.sh up and php.sh up, and take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
HERE="$ROOT/reference/admin-saves"
CONTENT="$ROOT/reference/action-checks/content.sql"
OUT="$ROOT/crates/hyscribe-admin/tests/golden/admin-saves.tsv"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp -d)
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress "$@"; }
# The content and user 1 as they were, and no user the cases added.
reset() {
  sed -n '/^-- cleanup$/,$p' "$CONTENT" | sql
  echo "DELETE FROM wp_users WHERE ID > 801 OR ID = 1; DELETE FROM wp_usermeta WHERE user_id > 801 OR user_id = 1;
    INSERT INTO wp_users SELECT * FROM hyscribe_user; INSERT INTO wp_usermeta SELECT * FROM hyscribe_usermeta;" | sql
  sed '/^-- cleanup$/,$d' "$CONTENT" | sql
}
cleanup() {
  rm -rf "$TMP"
  reset
  sed -n '/^-- cleanup$/,$p' "$CONTENT" | sql
  echo "DROP TABLE hyscribe_user, hyscribe_usermeta;" | sql
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php" "$MU/hyscribe-mail.php"
}
echo "CREATE TABLE hyscribe_user AS SELECT * FROM wp_users WHERE ID = 1;
  CREATE TABLE hyscribe_usermeta AS SELECT * FROM wp_usermeta WHERE user_id = 1;" | sql
trap cleanup EXIT
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
echo "<?php add_filter( 'pre_wp_mail', '__return_true' );" | "$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-mail.php"
: > "$OUT"
while IFS=$'\t' read -r case method target body action field; do
  reset
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
done < <(grep -v '^#' "$HERE/cases.list")
wc -l "$OUT"

#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's dashboard in states the default install lacks, as
# crates/hyscribe-admin/tests/golden/dashboard.tsv:
#   variant<TAB>role<TAB>hex(page)
# Cached Site Health counts, a private site and a user's widget layout come
# from filters (reference/dashboard/variants.php); the content variant adds
# reference/dashboard/content.sql and deletes it after; the f9 variants add
# a query string or the user's settings (reference/dashboard/variants.php). Masks as
# reference/admin-page/golden.sh. Start from a fresh db.sh up, then php.sh up,
# and take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-admin/tests/golden/dashboard.tsv"
MU=/var/www/html/wp-content/mu-plugins
TMP=$(mktemp)
NONCES=$(mktemp)
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress; }
remove_content() {
  echo 'DELETE FROM wp_posts WHERE ID BETWEEN 201 AND 205; DELETE FROM wp_comments WHERE comment_ID BETWEEN 10 AND 15;' | sql
}
cleanup() {
  rm -f "$TMP" "$NONCES"
  remove_content
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php" "$MU/hyscribe-variants.php"
}
trap cleanup EXIT
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-variants.php" < "$ROOT/reference/dashboard/variants.php"
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 1 ); foreach ( array_slice( $argv, 1 ) as $action ) { echo wp_create_nonce( $action ), "\t", $action, "\n"; }' \
  log-out heartbeat-nonce wp_rest update_can_compress_scripts welcome-panel-nonce screen-options-nonce replyto-comment \
  add-post closedpostboxes meta-box-order internal-linking community_events health-check-site-status \
  health-check-site-status-result approve-comment_1 delete-comment_1 approve-comment_10 delete-comment_10 \
  approve-comment_11 delete-comment_11 approve-comment_14 delete-comment_14 > "$NONCES"
record() {
  : > "$TMP"
  curl -sf -o "$TMP" -b "hyscribe_role=$2; hyscribe_variant=$1" "$BASE/wp-admin/index.php${3:-}"
  page=$(php -r '
    $p = file_get_contents( $argv[1] );
    foreach ( file( $argv[2], FILE_IGNORE_NEW_LINES ) as $line ) {
      list( $nonce, $action ) = explode( "\t", $line );
      $p = str_replace( $nonce, "%nonce:$action%", $p );
    }
    $p = preg_replace( "/\"time\":\"[0-9]+\"/", "\"time\":\"%time%\"", $p );
    $p = preg_replace( "/(name=\"post_ID\" value=\")[0-9]+/", "\\1%auto-draft%", $p );
    echo bin2hex( $p );' "$TMP" "$NONCES")
  printf '%s\t%s\t%s\n' "$1" "$2" "$page" >> "$OUT"
}
: > "$OUT"
for variant in health critical one passing layout; do
  record "$variant" administrator
done
# Query arguments that only change what is shown, and the user's menu folding.
record f9edit administrator '?edit=dashboard_primary'
record f9welcome editor '?welcome=0'
record f9remind administrator '?admin_email_remind_later=1'
record f9old administrator '?admin_email_remind_later=1'
record f9folded administrator
record f9unfold editor
remove_content
sql < "$ROOT/reference/dashboard/content.sql"
for role in administrator editor author subscriber; do
  record content "$role"
done
wc -l "$OUT"

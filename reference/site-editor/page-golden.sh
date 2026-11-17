#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's site editor page (site-editor.php) for the
# administrator, the one role that reaches it, in the variants PHP serves
# the same page for, as crates/hyscribe-admin/tests/golden/site-editor.tsv,
# one row each:
#   case<TAB>part<TAB>value
# The parts, from reference/site-editor/parts.php: `settings.<key>` for each
# key of the editor's settings, `categories`, `bindings` and `blocks` (the
# inline scripts of wp-blocks), `preloads` (the paths PHP preloads) and
# `html` (hex of the page, those values left out). Another value over 2000
# bytes is kept as `sha256:<hex> <length>`. Nonces become %nonce:<action>%,
# the user settings' time %time%. With RAW=<dir> each page is kept there,
# also masked. The navigation menu and theme mods php.sh's first page view
# wrote are taken out first, so the first view writes the theme's
# global styles post as on the fixture: start from a fresh db.sh up and
# php.sh up, and take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
MU=/var/www/html/wp-content/mu-plugins
OUT="$ROOT/crates/hyscribe-admin/tests/golden/site-editor.tsv"
TMP=$(mktemp -d)
cleanup() {
  rm -rf "$TMP"
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php"
}
trap cleanup EXIT
"$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress <<'SQL'
DELETE FROM wp_posts WHERE ID = 4 AND post_type = 'wp_navigation';
DELETE FROM wp_options WHERE option_name = 'theme_mods_twentytwentyfive';
ALTER TABLE wp_posts AUTO_INCREMENT = 4;
SQL
"$DOCKER" exec -i "$NAME" sh -c "mkdir -p $MU && cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
# User 1's nonces, without a session as the requests have none.
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 1 ); foreach ( array_slice( $argv, 1 ) as $action ) { echo wp_create_nonce( $action ), "\t", $action, "\n"; }' \
  wp_rest heartbeat-nonce log-out media-form media-send-to-editor update_can_compress_scripts \
  screen-options-nonce internal-linking > "$TMP/nonces"
: > "$OUT"
record() {
  local case=$1 query=$2
  status=$(curl -s -o "$TMP/page" -w '%{http_code}' -b hyscribe_role=administrator "$BASE/wp-admin/site-editor.php$query")
  [ "$status" = 200 ] || { echo "PHP answered $status for $query" >&2; exit 1; }
  [ -z "${RAW:-}" ] || cp "$TMP/page" "$RAW/$case.html"
  php "$ROOT/reference/site-editor/parts.php" "$TMP/page" "$TMP/nonces" "$case" ${RAW:+"$RAW"} >> "$OUT"
}
record se-first ''
record se-again ''
record se-page '?p=%2Fpage'
record se-page-2 '?p=/page/2&canvas=edit'
record se-page-gone '?p=/page/99'
record se-draft '?p=/&postId=3'
record se-post '?p=/&postId=1&postType=post'
record se-styles '?p=/styles&postType=wp_global_styles'
record se-template '?p=/wp_template/twentytwentyfive//index&canvas=edit'
wc -l "$OUT"

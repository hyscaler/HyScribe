#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the PHP oracle's Plugins screen and actions with the plugin the
# test extension stands in for (reference/plugins/hyscribe-test, MVP 11.3),
# installed in the container's plugins folder for the run and the only
# plugin listed (variants.php):
# - the screen in the variants of variants.php (the x24 ones with update
#   rows) and query arguments, as
#   crates/hyscribe-admin/tests/golden/plugins.tsv:
#   variant<TAB>query<TAB>role<TAB>hex(page)
# - the actions of actions.list, each from the options as they were, as
#   crates/hyscribe-http/tests/golden/plugin-actions.tsv:
#   case<TAB>status<TAB>Location<TAB>hex(body)<TAB>active_plugins<TAB>recently_activated
#   (the Location's nonces masked as the body's)
#   the options as the action left them: value and autoload, or - for none,
#   recently_activated's times as %time%.
# - the extension's admin pages (SDK 0.4) in the x14page variant, the
#   plugin active and ext_title set, as
#   crates/hyscribe-http/tests/golden/extension-admin.tsv:
#   variant<TAB>path<TAB>role<TAB>status<TAB>hex(page)
# - the body tag of front end pages as a visitor with the plugin active and
#   its page hooks on (x13hooks) or set to trap (x13fail), as
#   crates/hyscribe-http/tests/golden/extension-pages.tsv:
#   variant<TAB>path<TAB>hex(body tag)
# Nonces are masked as %nonce:action%, as reference/appearance/golden.sh
# masks them. The states are written with SQL and the options table is put
# back after every request, as viewing the screen writes; start from a
# fresh db.sh up and php.sh up, and take both down after.
set -euo pipefail
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
BASE="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
HERE="$ROOT/reference/plugins"
SCREENS="$ROOT/crates/hyscribe-admin/tests/golden/plugins.tsv"
ACTIONS="$ROOT/crates/hyscribe-http/tests/golden/plugin-actions.tsv"
PAGES="$ROOT/crates/hyscribe-http/tests/golden/extension-pages.tsv"
ADMIN="$ROOT/crates/hyscribe-http/tests/golden/extension-admin.tsv"
MU=/var/www/html/wp-content/mu-plugins
PLUGIN=/var/www/html/wp-content/plugins/hyscribe-test
TMP=$(mktemp)
NONCES=$(mktemp)
OPTIONS=$(mktemp)
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress "$@"; }
"$DOCKER" exec "$DB" mariadb-dump -uroot -preference-only wordpress wp_options > "$OPTIONS"
cleanup() {
  sql < "$OPTIONS"
  rm -f "$TMP" "$TMP.h" "$NONCES" "$OPTIONS"
  "$DOCKER" exec "$NAME" rm -rf "$MU/hyscribe-as-role.php" "$MU/hyscribe-variants.php" "$PLUGIN"
  "$ROOT/reference/admin-look/look.sh" remove || true
}
trap cleanup EXIT
"$ROOT/reference/admin-look/look.sh" add
"$DOCKER" exec "$NAME" mkdir -p "$MU" "$PLUGIN"
"$DOCKER" exec -i "$NAME" sh -c "cat > $PLUGIN/hyscribe-test.php" < "$HERE/hyscribe-test/hyscribe-test.php"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-as-role.php" < "$ROOT/reference/admin-menu/as-role.php"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-variants.php" < "$HERE/variants.php"
file=hyscribe-test/hyscribe-test.php
actions="log-out heartbeat-nonce wp_rest update_can_compress_scripts updates bulk-plugins screen-options-nonce activate-plugin_$file deactivate-plugin_$file activate-plugin_missing/missing.php activate-plugin_../x.php plugin-activation-error_$file hyscribe_test-options"
# shellcheck disable=SC2086
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 1 ); foreach ( array_slice( $argv, 1 ) as $action ) { echo wp_create_nonce( $action ), "\t", $action, "\n"; }' \
  $actions > "$NONCES"
nonce() { awk -F'\t' -v a="$1" '$2 == a { print $1 }' "$NONCES"; }
mask() {
  php -r '
    $p = file_get_contents( $argv[1] );
    foreach ( file( $argv[2], FILE_IGNORE_NEW_LINES ) as $line ) {
      list( $nonce, $action ) = explode( "\t", $line );
      $p = str_replace( $nonce, "%nonce:$action%", $p );
    }
    $p = preg_replace( "/\"time\":\"[0-9]+\"/", "\"time\":\"%time%\"", $p );
    echo bin2hex( $p );' "$1" "$NONCES"
}
# The state of a variant, in SQL: active_plugins, recently_activated and the
# plugin's ext_hooks.
state() {
  local active='a:0:{}' recent='' hooks='' title=''
  case "$1" in
    x13active | x24active) active="a:1:{i:0;s:31:\"$file\";}" ;;
    x14page) active="a:1:{i:0;s:31:\"$file\";}" title="Hello World" ;;
    x13hooks) active="a:1:{i:0;s:31:\"$file\";}" hooks=on ;;
    x13fail) hooks=trap ;;
    x13missing) active="a:2:{i:0;s:31:\"$file\";i:1;s:19:\"missing/missing.php\";}" ;;
    x13other) active="a:1:{i:0;s:23:\"wordfence/wordfence.php\";}" ;;
    x13both) active="a:2:{i:0;s:31:\"$file\";i:1;s:23:\"wordfence/wordfence.php\";}" ;;
    x13recent) recent="a:2:{s:31:\"$file\";i:4102444800;s:11:\"old/old.php\";i:1000000000;}" ;;
  esac
  printf "UPDATE wp_options SET option_value = '%s' WHERE option_name = 'active_plugins';\n" "$active" | sql
  if [ -n "$recent" ]; then
    printf "INSERT INTO wp_options (option_name, option_value, autoload) VALUES ('recently_activated', '%s', 'off');\n" "$recent" | sql
  fi
  if [ -n "$title" ]; then
    printf "INSERT INTO wp_options (option_name, option_value, autoload) VALUES ('ext_title', '%s', 'auto');\n" "$title" | sql
  fi
  if [ -n "$hooks" ]; then
    printf "INSERT INTO wp_options (option_name, option_value, autoload) VALUES ('ext_hooks', '%s', 'auto');\n" "$hooks" | sql
  fi
}
: > "$SCREENS"
# A view: variant, query as recorded, role, and the query asked if another.
screen() {
  state "$1"
  : > "$TMP"
  status=$(curl -s -o "$TMP" -w '%{http_code}' -b "hyscribe_role=$3; hyscribe_variant=$1" "$BASE/wp-admin/plugins.php${4:-$2}")
  [ "$status" = 200 ] || { echo "PHP answered $status for $1 $2" >&2; exit 1; }
  printf '%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$(mask "$TMP")" >> "$SCREENS"
  sql < "$OPTIONS"
}
screen x13inactive '' administrator
screen x13active '' administrator
screen x13missing '' administrator
screen x13recent '' administrator
screen x13active '?activate=true&plugin_status=all&paged=1&s=' administrator
screen x13inactive '?deactivate=true&plugin_status=all&paged=1&s=' administrator
screen x13active '?plugin_status=active' administrator
screen x13active '?plugin_status=inactive' administrator
screen x13inactive '?plugin_status=inactive' administrator
screen x13inactive '?plugin_status=search' administrator
screen x13recent '?plugin_status=recently_activated' administrator
screen x13hidden '' administrator
screen x13hooks '' administrator
screen x13inactive '?error=true&plugin=hyscribe-test%2Fhyscribe-test.php&_error_nonce=0123456789' administrator
# ADR 0024's update rows (variants.php).
for variant in x24update x24active x24revoked x24hidden; do screen "$variant" '' administrator; done
# The actions: each case's variant, role, and request with the nonce of
# the action named (- for none).
option() {
  printf "SELECT option_value, autoload FROM wp_options WHERE option_name = '%s';\n" "$1" | sql -N \
    | sed -E 's/i:[0-9]{10};/i:%time%;/g' | tr '\t' ' '
}
: > "$ACTIONS"
failed=
while IFS=$'\t' read -r case variant role query action; do
  state "$variant"
  case "$action" in
    -) ;;
    bad) query="$query&_wpnonce=0123456789" ;;
    *) query="$query&_wpnonce=$(nonce "${action/\%file\%/$file}")" ;;
  esac
  : > "$TMP"
  out=$(curl -s -o "$TMP" -D "$TMP.h" -w '%{http_code}' -b "hyscribe_role=$role; hyscribe_variant=$variant" "$BASE/wp-admin/plugins.php?$query")
  location=$(grep -i '^location:' "$TMP.h" | sed 's/^[Ll]ocation: //' | tr -d '\r' || true)
  [ "$case" = activate-fails ] && failed="?${location#*plugins.php?}"
  while IFS=$'\t' read -r value name; do location=${location//$value/%nonce:$name%}; done < "$NONCES"
  active=$(option active_plugins)
  recent=$(option recently_activated)
  printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$case" "$out" "${location:--}" "$(mask "$TMP")" "${active:--}" "${recent:--}" >> "$ACTIONS"
  sql < "$OPTIONS"
done < <(grep -v '^#' "$HERE/actions.list")
# The screen the failed activation sends to, with the nonce it made.
masked="$failed"
while IFS=$'\t' read -r value name; do masked=${masked//$value/%nonce:$name%}; done < "$NONCES"
screen x13inactive "$masked" administrator "$failed"
# The extension's admin pages, each as a role.
: > "$ADMIN"
for asked in administrator:options-general.php?page=hyscribe-test administrator:admin.php?page=hyscribe-test-top \
  administrator:admin.php?page=hyscribe-test-sub editor:admin.php?page=hyscribe-test-top \
  editor:admin.php?page=hyscribe-test-sub editor:options-general.php?page=hyscribe-test; do
  role=${asked%%:*} path=${asked#*:}
  state x14page
  : > "$TMP"
  status=$(curl -s -o "$TMP" -w '%{http_code}' -b "hyscribe_role=$role; hyscribe_variant=x14page" "$BASE/wp-admin/$path")
  printf '%s\t%s\t%s\t%s\t%s\n' x14page "$path" "$role" "$status" "$(mask "$TMP")" >> "$ADMIN"
  sql < "$OPTIONS"
done
# The front end as a visitor: user 1 no longer logged in.
"$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-as-role.php"
: > "$PAGES"
for variant in x13hooks x13fail; do
  for path in / '/?p=1'; do
    state "$variant"
    : > "$TMP"
    status=$(curl -s -o "$TMP" -w '%{http_code}' "$BASE$path")
    [ "$status" = 200 ] || { echo "PHP answered $status for $variant $path" >&2; exit 1; }
    printf '%s\t%s\t%s\n' "$variant" "$path" "$(grep -o '<body class="[^"]*">' "$TMP" | tr -d '\n' | xxd -p | tr -d '\n')" >> "$PAGES"
    sql < "$OPTIONS"
  done
done
wc -l "$SCREENS" "$ACTIONS" "$PAGES" "$ADMIN"

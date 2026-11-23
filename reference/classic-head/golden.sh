#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records what a classic theme's wp_head() and wp_footer() print (ADR 0023),
# with the theme's own callbacks and WordPress 6.9's hoisting of late styles
# into the head, as PHP prints them where its template loader has picked
# the theme's template: reference/classic-head/head.php renders a template
# of the two only. Into crates/hyscribe-http/tests/golden/classic-head-<slug>.tsv:
#   page<TAB>options<TAB>hex(path)<TAB>status<TAB>hex(body)
# for each request of reference/pages/default.list with the theme active,
# then with reference/classic-head/content.sql added (a post with an
# excerpt; option _head=content, which is no option) for / and /?p=971,
# which it deletes after; then PHP's critical error page for a theme's fatal
# error (option _fatal=1).
#   reference/classic-head/golden.sh [slug]   (default hello-elementor)
# The theme is active through the options of each request
# (reference/pages/options.php); the plugins are must-use plugins for the
# run, removed after. Needs reference/themes/pin.sh <slug> before php.sh up;
# start from a fresh db.sh up (page views write, an open question) and reset it after.
set -euo pipefail

SLUG="${1:-hello-elementor}"
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
BASE="${HYSCRIBE_PHP_BASE:-http://127.0.0.1:${HYSCRIBE_PHP_PORT:-8081}}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-http/tests/golden/classic-head-$SLUG.tsv"
MU=/var/www/html/wp-content/mu-plugins
HEADERS=$(mktemp)
BODY=$(mktemp)
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress; }
cleanup() {
  rm -f "$HEADERS" "$BODY"
  echo "DELETE FROM wp_posts WHERE ID = 971;" | sql
  "$DOCKER" exec "$NAME" rm -rf "$MU/hyscribe-options.php" "$MU/hyscribe-head.php" "$MU/hyscribe-head"
}
trap cleanup EXIT
"$DOCKER" exec "$NAME" mkdir -p "$MU/hyscribe-head"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-options.php" < "$ROOT/reference/pages/options.php"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-head.php" < "$ROOT/reference/classic-head/head.php"
for page in page fatal; do
  "$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-head/$page.php" < "$ROOT/reference/classic-head/$page.php"
done
: > "$OUT"
record() {
  local options="$1" path="$2" status
  curl -g -s -D "$HEADERS" -o "$BODY" -H "X-HyScribe-Options: $options" "$BASE$path"
  status=$(head -n 1 "$HEADERS" | cut -d ' ' -f 2)
  printf 'page\t%s\t%s\t%s\t%s\n' "$options" "$(printf '%s' "$path" | xxd -p | tr -d '\n')" "$status" \
    "$(xxd -p < "$BODY" | tr -d '\n')" >> "$OUT"
}
THEME="template=$SLUG&stylesheet=$SLUG"
while IFS= read -r line; do
  # Not by `read`, which drops a leading tab, and so empty options.
  options=${line%%$'\t'*}
  record "$THEME${options:+&$options}" "${line#*$'\t'}"
done < "$ROOT/reference/pages/default.list"
sql < "$ROOT/reference/classic-head/content.sql"
for path in / '/?p=971'; do
  record "_head=content&$THEME" "$path"
done
record "_fatal=1&$THEME" /
wc -l "$OUT"

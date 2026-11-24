#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records the template tags the host answers for a classic theme (ADR 0023)
# as PHP answers them where its template loader has picked the template, for
# each request of reference/pages/default.list with the theme active, into
# crates/hyscribe-http/tests/golden/theme-tags-<slug>.tsv:
#   page<TAB>options<TAB>hex(path)<TAB>status
#   tag<TAB>name<TAB>hex(value)            the lines of reference/theme-tags/tags.php
# once over the default install, then with reference/theme-tags/content.sql
# (a custom logo and two menus), which it adds and deletes after; those
# requests carry the option _tags=content, which is no option.
#   reference/theme-tags/golden.sh [slug]   (default hello-elementor)
# The theme is active through the options of each request
# (reference/pages/options.php); both plugins are must-use plugins for the
# run, removed after. Needs reference/themes/pin.sh <slug> before php.sh up;
# start from a fresh db.sh up (page views write) and reset it after.
set -euo pipefail

SLUG="${1:-hello-elementor}"
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
DB="${HYSCRIBE_DB_CONTAINER:-hyscribe-default-install-db}"
BASE="${HYSCRIBE_PHP_BASE:-http://127.0.0.1:${HYSCRIBE_PHP_PORT:-8081}}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-http/tests/golden/theme-tags-$SLUG.tsv"
MU=/var/www/html/wp-content/mu-plugins
HEADERS=$(mktemp)
BODY=$(mktemp)
sql() { "$DOCKER" exec -i "$DB" mariadb -uroot -preference-only wordpress; }
remove() {
  echo "DELETE FROM wp_posts WHERE ID BETWEEN 961 AND 963; DELETE FROM wp_postmeta WHERE meta_id BETWEEN 961 AND 978;
    DELETE FROM wp_terms WHERE term_id IN (961, 962); DELETE FROM wp_term_taxonomy WHERE term_taxonomy_id IN (961, 962);
    DELETE FROM wp_term_relationships WHERE term_taxonomy_id IN (961, 962); DELETE FROM wp_options WHERE option_id = 961;" | sql
}
cleanup() {
  rm -f "$HEADERS" "$BODY"
  remove
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-options.php" "$MU/hyscribe-tags.php"
}
trap cleanup EXIT
"$DOCKER" exec "$NAME" mkdir -p "$MU"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-options.php" < "$ROOT/reference/pages/options.php"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-tags.php" < "$ROOT/reference/theme-tags/tags.php"
: > "$OUT"
record() {
  local theme="$1" line options path status
  while IFS= read -r line; do
    # Not by `read`, which drops a leading tab, and so empty options.
    options=${line%%$'\t'*}
    path=${line#*$'\t'}
    options="$theme${options:+&$options}"
    curl -g -s -D "$HEADERS" -o "$BODY" -H "X-HyScribe-Options: $options" "$BASE$path"
    status=$(head -n 1 "$HEADERS" | cut -d ' ' -f 2)
    printf 'page\t%s\t%s\t%s\n' "$options" "$(printf '%s' "$path" | xxd -p | tr -d '\n')" "$status" >> "$OUT"
    # Only a page the loader reached prints the tags.
    if [ "$status" = 200 ] || [ "$status" = 404 ]; then
      grep '^hyscribe-tag' "$BODY" | sed 's/^hyscribe-tag/tag/' >> "$OUT" || true
    fi
  done < "$ROOT/reference/pages/default.list"
}
record "template=$SLUG&stylesheet=$SLUG"
sql < "$ROOT/reference/theme-tags/content.sql"
record "_tags=content&template=$SLUG&stylesheet=$SLUG"
wc -l "$OUT"

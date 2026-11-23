#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records which template file PHP's template loader picks for a classic
# theme (ADR 0023) on each request of reference/pages/default.list, as
# crates/hyscribe-http/tests/golden/classic-hierarchy-<slug>.tsv:
#   file<TAB>path                                  each PHP file of the theme
#   page<TAB>options<TAB>hex(path)<TAB>status<TAB>template
# the template a path inside the theme (or wp-includes/... for core's
# theme-compat files), - when the loader picked none (a redirect, a feed).
#   reference/themes/hierarchy-golden.sh [slug]   (default twentytwentyone)
# The theme is active through the options of each request
# (reference/pages/options.php), with reference/themes/hierarchy.php telling
# the template; both are must-use plugins for the run, removed after. Needs
# reference/themes/pin.sh <slug> before php.sh up; start from a fresh
# db.sh up (page views write) and reset it after.
set -euo pipefail

SLUG="${1:-twentytwentyone}"
DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
BASE="${HYSCRIBE_PHP_BASE:-http://127.0.0.1:${HYSCRIBE_PHP_PORT:-8081}}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-http/tests/golden/classic-hierarchy-$SLUG.tsv"
MU=/var/www/html/wp-content/mu-plugins
HEADERS=$(mktemp)
cleanup() {
  rm -f "$HEADERS"
  "$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-options.php" "$MU/hyscribe-hierarchy.php"
}
trap cleanup EXIT
"$DOCKER" exec "$NAME" mkdir -p "$MU"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-options.php" < "$ROOT/reference/pages/options.php"
"$DOCKER" exec -i "$NAME" sh -c "cat > $MU/hyscribe-hierarchy.php" < "$ROOT/reference/themes/hierarchy.php"
"$DOCKER" exec "$NAME" sh -c "cd /var/www/html/wp-content/themes/$SLUG && find . -name '*.php' | sed 's|^\./||' | LC_ALL=C sort" |
  sed 's/^/file\t/' > "$OUT"
theme="template=$SLUG&stylesheet=$SLUG"
while IFS= read -r line; do
  # Not by `read`, which drops a leading tab, and so empty options.
  options=${line%%$'\t'*}
  path=${line#*$'\t'}
  options="$theme${options:+&$options}"
  curl -g -s -D "$HEADERS" -o /dev/null -H "X-HyScribe-Options: $options" "$BASE$path"
  status=$(head -n 1 "$HEADERS" | cut -d ' ' -f 2)
  template=$(grep -i '^x-hyscribe-template:' "$HEADERS" | cut -d ' ' -f 2- | tr -d '\r' || true)
  key=$(printf '%s' "$path" | xxd -p | tr -d '\n')
  printf 'page\t%s\t%s\t%s\t%s\n' "$options" "$key" "$status" "${template:--}" >> "$OUT"
done < "$ROOT/reference/pages/default.list"
wc -l "$OUT"

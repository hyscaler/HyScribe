#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Records PHP WordPress's theme.json engine as the oracle for
# hyscribe-theme-json, from the default install (reference/default-install,
# PHP container and front end up). Writes crates/hyscribe-theme-json/tests/golden/:
#   registry.txt, registered-styles.tsv, post-comments.json   block registry inputs
#   theme-styles/<dir>/<file>.json   the theme's block style variation partials
#   settings.ser, styles.ser         wp_get_global_settings(), wp_get_global_styles()
#   raw-<layer>.ser                  WP_Theme_JSON_Resolver layers
#   metadata.ser                     WP_Theme_JSON::get_blocks_metadata()
#   stylesheet.css, stylesheet-all.css, blocks.tsv   generated CSS
#   page.css                         the front page's global-styles-inline-css
#   cases.tsv                        cases.txt run through cases.php
# Read only: nothing in the containers or the database changes.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
URL="${HYSCRIBE_PHP_URL:-http://127.0.0.1:8081/}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/crates/hyscribe-theme-json/tests/golden"
THEME="/var/www/html/wp-content/themes/twentytwentyfive"

php() {
  "$DOCKER" exec -i "$NAME" php -- "$1" < "$ROOT/reference/theme-json/golden.php"
}

rm -rf "$OUT"
mkdir -p "$OUT/theme-styles"
php registry > "$OUT/registry.txt"
php styles > "$OUT/registered-styles.tsv"
php post-comments > "$OUT/post-comments.json"
php settings > "$OUT/settings.ser"
php styles-merged > "$OUT/styles.ser"
for layer in core blocks theme user merged; do
  php "raw-$layer" > "$OUT/raw-$layer.ser"
done
php metadata > "$OUT/metadata.ser"
php stylesheet > "$OUT/stylesheet.css"
php stylesheet-all > "$OUT/stylesheet-all.css"
php blocks > "$OUT/blocks.tsv"
{
  cat "$ROOT/reference/theme-json/cases.php"
  printf "\nrun( <<<'CASES'\n"
  cat "$ROOT/reference/theme-json/cases.txt"
  printf "\nCASES\n);\n"
} | "$DOCKER" exec -i "$NAME" php > "$OUT/cases.tsv"

# Partials are the styles/ JSON files that name blockTypes.
"$DOCKER" exec "$NAME" sh -c "cd $THEME/styles && grep -l blockTypes -r . | sort | tar -cf - -T -" \
  | tar -xf - -C "$OUT/theme-styles"

# The inline style element of the front page, without the sourceURL comment.
curl -fsS "$URL" | awk '
  /<style id="global-styles-inline-css">/ { on = 1; next }
  on && /^\/\*# sourceURL=global-styles-inline-css \*\/$/ { on = 0 }
  on { print }
' > "$OUT/page.css"
echo "wrote $OUT"

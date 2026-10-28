#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Exports the active theme of the PHP WordPress oracle (php.sh) as a theme
# port: its templates, parts, theme.json and style.css as they are, its
# PHP patterns run once into block markup (patterns.json), the style
# partials that name block types (styles.json, file name to contents), and
# the block styles registered with the theme active (block-styles.tsv).
#   reference/themes/export.sh [slug]   (default twentytwentyfive)
# Writes themes/<slug>/. Needs reference/default-install/php.sh up.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
SLUG="${1:-twentytwentyfive}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/themes/$SLUG"
SRC="/var/www/html/wp-content/themes/$SLUG"

rm -rf "$OUT"
mkdir -p "$OUT"
for item in templates parts theme.json style.css; do
  "$DOCKER" cp "$NAME:$SRC/$item" "$OUT/$item"
done
"$DOCKER" cp "$ROOT/reference/themes/export.php" "$NAME:/tmp/export.php"
"$DOCKER" exec "$NAME" php /tmp/export.php > "$OUT/patterns.json"
# shellcheck disable=SC2016
"$DOCKER" exec "$NAME" php -r '
  $dir = "'"$SRC"'/styles";
  $partials = array();
  foreach ( array( "blocks", "sections" ) as $kind ) {
    foreach ( glob( "$dir/$kind/*.json" ) ?: array() as $file ) {
      $json = file_get_contents( $file );
      if ( str_contains( $json, "blockTypes" ) ) {
        $partials[ "$kind/" . basename( $file ) ] = $json;
      }
    }
  }
  ksort( $partials );
  echo json_encode( $partials, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE ), "\n";
' > "$OUT/styles.json"
"$DOCKER" exec "$NAME" php -r '
  require "/var/www/html/wp-load.php";
  foreach ( WP_Block_Styles_Registry::get_instance()->get_all_registered() as $block => $styles ) {
    foreach ( array_keys( $styles ) as $style ) {
      echo $block, "\t", $style, "\n";
    }
  }
' > "$OUT/block-styles.tsv"
echo "wrote $OUT ($(ls "$OUT/templates" | wc -l) templates, $(ls "$OUT/parts" | wc -l) parts)"

#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Exports the active theme of the PHP WordPress oracle (php.sh) as a theme
# port: every file but its PHP as it is, its PHP patterns run
# once into block markup (patterns.json), the style
# partials that name block types (styles.json, file name to contents), and
# the block styles and pattern categories registered with the theme active
# (registered.json, from reference/rest/registered.php).
#   reference/themes/export.sh [slug]   (default twentytwentyfive)
# Writes themes/<slug>/. Needs reference/default-install/php.sh up (for a theme
# WordPress 7.1.3 does not bundle, themes/pinned.tsv, run
# reference/themes/pin.sh first, which php.sh then mounts). The PHP
# runs with that theme active through a must-use plugin (theme.php), which
# is removed after, whatever theme the database has.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
SLUG="${1:-twentytwentyfive}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/themes/$SLUG"
SRC="/var/www/html/wp-content/themes/$SLUG"

MU=/var/www/html/wp-content/mu-plugins
"$DOCKER" exec "$NAME" mkdir -p "$MU"
"$DOCKER" cp "$ROOT/reference/themes/theme.php" "$NAME:$MU/hyscribe-theme.php"
trap '"$DOCKER" exec "$NAME" rm -f "$MU/hyscribe-theme.php"' EXIT
php_in_theme() { "$DOCKER" exec -e HYSCRIBE_THEME="$SLUG" "$NAME" php "$@"; }

rm -rf "$OUT"
mkdir -p "$OUT"
"$DOCKER" cp "$NAME:$SRC/." "$OUT/"
find "$OUT" -name '*.php' -delete
find "$OUT" -type d -empty -delete
"$DOCKER" cp "$ROOT/reference/themes/export.php" "$NAME:/tmp/export.php"
php_in_theme /tmp/export.php > "$OUT/patterns.json"
# shellcheck disable=SC2016
php_in_theme -r '
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
"$DOCKER" cp "$ROOT/reference/rest/registered.php" "$NAME:/tmp/registered.php"
php_in_theme /tmp/registered.php theme > "$OUT/registered.json"
echo "wrote $OUT ($(ls "$OUT/templates" | wc -l) templates, $(ls "$OUT/parts" | wc -l) parts)"

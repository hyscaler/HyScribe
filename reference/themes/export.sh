#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Exports the active theme of the PHP WordPress oracle (php.sh) as a theme
# port: its templates, parts, theme.json and style.css as they are, and its
# PHP patterns run once into block markup (patterns.json).
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
echo "wrote $OUT ($(ls "$OUT/templates" | wc -l) templates, $(ls "$OUT/parts" | wc -l) parts)"

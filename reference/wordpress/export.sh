#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Exports the parts of WordPress core that HyScribe needs as data, from the
# PHP WordPress oracle (reference/default-install/php.sh up):
#   wordpress/wp-includes/theme.json                core theme.json defaults
#   wordpress/wp-includes/blocks/<name>/block.json  core block metadata
#   wordpress/wp-includes/blocks/<name>/*.min.css   core block styles (LTR)
#   wordpress/wp-includes/css/dist/block-library/*.min.css
#   wordpress/VERSION
# All GPL-2.0-or-later (ADR 0004). Re-run when the pinned version changes.
set -euo pipefail

DOCKER="${DOCKER:-docker}"
NAME="${HYSCRIBE_PHP_CONTAINER:-hyscribe-default-install-php}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/wordpress"

rm -rf "$OUT"
mkdir -p "$OUT"
"$DOCKER" exec "$NAME" sh -c 'cd /var/www/html && tar -cf - \
  wp-includes/theme.json \
  $(find wp-includes/blocks -name block.json) \
  $(find wp-includes/blocks -name "*.min.css" ! -name "*-rtl.min.css") \
  $(find wp-includes/css/dist/block-library -name "*.min.css" ! -name "*-rtl.min.css")' | tar -xf - -C "$OUT"
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-includes/version.php"; echo $wp_version, "\n";' > "$OUT/VERSION"
echo "wrote $OUT: WordPress $(cat "$OUT/VERSION"), $(find "$OUT" -name block.json | wc -l) blocks"

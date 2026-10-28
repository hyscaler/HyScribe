#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Exports the parts of WordPress core that HyScribe needs as data, from the
# PHP WordPress oracle (reference/default-install/php.sh up):
#   wordpress/wp-includes/theme.json                core theme.json defaults
#   wordpress/wp-includes/blocks/<name>/block.json  core block metadata
#   wordpress/wp-includes/blocks/registry.json      the registered block types
#   wordpress/wp-includes/blocks/registered.json    their block.json, in registration order
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
# The block type registry as WordPress builds it: each core block's final
# attributes (block.json plus those block supports register) and supports.
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; $out = array(); foreach ( WP_Block_Type_Registry::get_instance()->get_all_registered() as $name => $type ) { if ( str_starts_with( $name, "core/" ) ) { $out[ $name ] = array( "attributes" => $type->attributes, "supports" => $type->supports, "uses_context" => $type->uses_context, "provides_context" => $type->provides_context ); } } ksort( $out ); echo wp_json_encode( $out, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE );' > "$OUT/wp-includes/blocks/registry.json"
# Each registered core block's block.json as written, in registration
# order; a block registered without one gets its name and supports.
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; $out = array(); foreach ( WP_Block_Type_Registry::get_instance()->get_all_registered() as $name => $type ) { if ( ! str_starts_with( $name, "core/" ) ) { continue; } $file = ABSPATH . WPINC . "/blocks/" . substr( $name, 5 ) . "/block.json"; $out[] = array( $name, is_file( $file ) ? file_get_contents( $file ) : wp_json_encode( array( "name" => $name, "supports" => $type->supports ), JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES ) . "\n" ); } echo wp_json_encode( $out, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE );' > "$OUT/wp-includes/blocks/registered.json"
cat > "$OUT/README.md" <<'README'
# WordPress core data

Files from WordPress core that HyScribe uses as data: the core `theme.json`, every core block's `block.json`, the block types as WordPress registers them (`blocks/registry.json`) and their `block.json` in registration order (`blocks/registered.json`), and the block library styles. Exported by `reference/wordpress/export.sh` from the pinned version in `VERSION`. Never edit by hand.

License: GPL-2.0-or-later, as WordPress (ADR 0004).
README
echo "wrote $OUT: WordPress $(cat "$OUT/VERSION"), $(find "$OUT" -name block.json | wc -l) blocks"

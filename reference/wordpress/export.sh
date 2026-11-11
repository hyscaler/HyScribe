#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Exports the parts of WordPress core that HyScribe needs as data, from the
# PHP WordPress oracle (reference/default-install/php.sh up):
#   wordpress/wp-includes/theme.json                core theme.json defaults
#   wordpress/wp-includes/blocks/<name>/block.json  core block metadata
#   wordpress/wp-includes/blocks/registry.json      the registered block types
#   wordpress/wp-includes/blocks/registered.json    their block.json, in registration order
#   wordpress/wp-includes/kses-post.json            wp_kses_allowed_html( 'post' )
#   wordpress/wp-includes/mime-types.json           get_allowed_mime_types() for a visitor
#   wordpress/wp-includes/blocks/<name>/*.min.css   core block styles (LTR)
#   wordpress/wp-includes/css/dist/block-library/*.min.css
#   wordpress/wp-includes/css/wp-block-template-skip-link.min.css
#   wordpress/wp-includes/js/wp-emoji-loader.min.js the emoji detection script
#   wordpress/wp-includes/css/wp-embed-template.min.css, js/wp-embed-template.min.js
#     and js/wp-embed.min.js                         the embed template's style and scripts
#   wordpress/wp-includes/script-modules.json      the registered script modules
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
  $(find wp-includes/css/dist/block-library -name "*.min.css" ! -name "*-rtl.min.css") \
  wp-includes/css/wp-block-template-skip-link.min.css \
  wp-includes/css/wp-embed-template.min.css \
  wp-includes/js/wp-embed-template.min.js \
  wp-includes/js/wp-embed.min.js \
  wp-includes/js/wp-emoji-loader.min.js' | tar -xf - -C "$OUT"
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-includes/version.php"; echo $wp_version, "\n";' > "$OUT/VERSION"
# The block type registry as WordPress builds it: each core block's final
# attributes (block.json plus those block supports register) and supports.
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; $out = array(); foreach ( WP_Block_Type_Registry::get_instance()->get_all_registered() as $name => $type ) { if ( str_starts_with( $name, "core/" ) ) { $out[ $name ] = array( "attributes" => $type->attributes, "supports" => $type->supports, "uses_context" => $type->uses_context, "provides_context" => $type->provides_context, "view_script_module_ids" => $type->view_script_module_ids ); } } ksort( $out ); echo wp_json_encode( $out, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE );' > "$OUT/wp-includes/blocks/registry.json"
# Each registered core block's block.json as written, in registration
# order; a block registered without one gets its name and supports.
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; $out = array(); foreach ( WP_Block_Type_Registry::get_instance()->get_all_registered() as $name => $type ) { if ( ! str_starts_with( $name, "core/" ) ) { continue; } $file = ABSPATH . WPINC . "/blocks/" . substr( $name, 5 ) . "/block.json"; $out[] = array( $name, is_file( $file ) ? file_get_contents( $file ) : wp_json_encode( array( "name" => $name, "supports" => $type->supports ), JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES ) . "\n" ); } echo wp_json_encode( $out, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE );' > "$OUT/wp-includes/blocks/registered.json"
# The script modules wp_default_script_modules() registers, by ID, with
# sources relative to the site.
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; do_action( "wp_default_scripts", wp_scripts() ); $registered = ( new ReflectionProperty( "WP_Script_Modules", "registered" ) )->getValue( wp_script_modules() ); ksort( $registered ); foreach ( $registered as &$module ) { $module["src"] = str_replace( site_url(), "", $module["src"] ); } echo wp_json_encode( $registered, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES ), "\n";' > "$OUT/wp-includes/script-modules.json"
# The MIME types a visitor may upload, which Atom enclosures are checked
# against.
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 0 ); echo wp_json_encode( array_values( get_allowed_mime_types() ), JSON_UNESCAPED_SLASHES ), "\n";' > "$OUT/wp-includes/mime-types.json"
# The tags and attributes wp_kses_post() allows.
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; echo wp_json_encode( wp_kses_allowed_html( "post" ), JSON_UNESCAPED_SLASHES );' > "$OUT/wp-includes/kses-post.json"
cat > "$OUT/README.md" <<'README'
# WordPress core data

Files from WordPress core that HyScribe uses as data: the core `theme.json`, every core block's `block.json`, the block types as WordPress registers them (`blocks/registry.json`) and their `block.json` in registration order (`blocks/registered.json`), the tags and attributes `wp_kses_post()` allows (`kses-post.json`), the MIME types a visitor may upload (`mime-types.json`), the registered script modules (`script-modules.json`), the block library and skip link styles, the emoji detection script, and the embed template's style and scripts. Exported by `reference/wordpress/export.sh` from the pinned version in `VERSION`. Never edit by hand.

License: GPL-2.0-or-later, as WordPress (ADR 0004).
README
echo "wrote $OUT: WordPress $(cat "$OUT/VERSION"), $(find "$OUT" -name block.json | wc -l) blocks"

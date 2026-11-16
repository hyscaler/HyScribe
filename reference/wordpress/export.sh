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
#   wordpress/wp-includes/kses-data.json            wp_kses_allowed_html( 'data' )
#   wordpress/wp-includes/mime-types.json           get_allowed_mime_types() for a visitor, by extensions
#   wordpress/wp-includes/capabilities.json         the post types', taxonomies' and statuses' capability data
#   wordpress/wp-includes/rest-routes.json          the REST API's namespaces and routes, with each endpoint's methods
#   wordpress/wp-includes/rest-descriptions.json    each route's description, as OPTIONS gives it
#   wordpress/wp-includes/rest-post-types.json      the post types, as the REST API types route describes those it shows
#   wordpress/wp-includes/rest-taxonomies.json      the taxonomies, as the REST API taxonomies route describes those it shows
#   wordpress/wp-includes/rest-settings.json        the settings the REST API settings route shows, with their options and schemas
#   wordpress/wp-includes/rest-registered.json      the block types and core pattern categories, as the REST API gives them (reference/rest/registered.php)
#   wordpress/wp-includes/dependencies.json         the styles and scripts load-styles.php and load-scripts.php serve
#   wordpress/wp-includes/registry.json             the styles and scripts an admin request registers, with placeholders for its URLs and nonces
#   wordpress/wp-includes/icons.json                the registered icons' sanitized SVG
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
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; $out = array(); foreach ( WP_Icons_Registry::get_instance()->get_registered_icons() as $icon ) { $out[ $icon["name"] ] = $icon["content"]; } ksort( $out ); echo wp_json_encode( $out, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE );' > "$OUT/wp-includes/icons.json"
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; wp_set_current_user( 0 ); echo wp_json_encode( get_allowed_mime_types(), JSON_UNESCAPED_SLASHES ), "\n";' > "$OUT/wp-includes/mime-types.json"
# What map_meta_cap() reads: each post type's capabilities and whether it maps
# meta capabilities, $post_type_meta_caps, each taxonomy's capabilities, and
# whether each post status is public or private.
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; global $post_type_meta_caps; $types = array(); foreach ( get_post_types( array(), "objects" ) as $name => $type ) { $types[ $name ] = array( "map_meta_cap" => $type->map_meta_cap, "capability_type" => is_array( $type->capability_type ) ? $type->capability_type[0] : $type->capability_type, "cap" => $type->cap ); } $taxonomies = array(); foreach ( get_taxonomies( array(), "objects" ) as $name => $taxonomy ) { $taxonomies[ $name ] = array( "cap" => $taxonomy->cap ); } $statuses = array(); foreach ( get_post_stati( array(), "objects" ) as $name => $status ) { $statuses[ $name ] = array( "public" => $status->public, "private" => $status->private ); } ksort( $types ); ksort( $taxonomies ); ksort( $statuses ); $meta = $post_type_meta_caps; ksort( $meta ); echo wp_json_encode( array( "post_types" => $types, "post_type_meta_caps" => $meta, "taxonomies" => $taxonomies, "post_statuses" => $statuses ), JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES ), "\n";' > "$OUT/wp-includes/capabilities.json"
# The REST API's namespaces and its routes in registration order: each
# route's regex, namespace and the methods of each of its endpoints, as
# WP_REST_Server matches requests to them.
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; $server = rest_get_server(); $routes = array(); foreach ( $server->get_routes() as $route => $handlers ) { $methods = array(); foreach ( $handlers as $handler ) { $methods[] = array_keys( array_filter( $handler["methods"] ) ); } $routes[] = array( $route, $server->get_route_options( $route )["namespace"] ?? "", $methods ); } echo wp_json_encode( array( "namespaces" => $server->get_namespaces(), "routes" => $routes ), JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES ), "\n";' > "$OUT/wp-includes/rest-routes.json"
# Each route's description by its regex, as rest_handle_options_request()
# gives it: get_data_for_route() in the help context, with the REST URL
# before a route as %rest_url%.
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; $server = rest_get_server(); $out = array(); foreach ( $server->get_routes() as $route => $handlers ) { $out[ $route ] = $server->get_data_for_route( $route, $handlers, "help" ); } echo str_replace( rtrim( rest_url(), "/" ), "%rest_url%", wp_json_encode( $out, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE ) ), "\n";' > "$OUT/wp-includes/rest-descriptions.json"
# Every post type: those shown in REST with the fields the types route gives
# them in the edit context, in its order, without links, the others false.
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; $out = array(); foreach ( get_post_types( array(), "objects" ) as $type ) { if ( ! $type->show_in_rest ) { $out[ $type->name ] = false; continue; } $taxonomies = wp_list_filter( get_object_taxonomies( $type->name, "objects" ), array( "show_in_rest" => true ) ); $out[ $type->name ] = array( "capabilities" => $type->cap, "description" => $type->description, "hierarchical" => $type->hierarchical, "has_archive" => $type->has_archive, "visibility" => array( "show_in_nav_menus" => (bool) $type->show_in_nav_menus, "show_ui" => (bool) $type->show_ui ), "viewable" => is_post_type_viewable( $type ), "labels" => $type->labels, "name" => $type->label, "slug" => $type->name, "icon" => $type->menu_icon, "supports" => get_all_post_type_supports( $type->name ), "taxonomies" => array_values( wp_list_pluck( $taxonomies, "name" ) ), "rest_base" => ! empty( $type->rest_base ) ? $type->rest_base : $type->name, "rest_namespace" => ! empty( $type->rest_namespace ) ? $type->rest_namespace : "wp/v2", "template" => $type->template ?? array(), "template_lock" => ! empty( $type->template_lock ) ? $type->template_lock : false ); } echo wp_json_encode( $out, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES ), "\n";' > "$OUT/wp-includes/rest-post-types.json"
# Every taxonomy: those shown in REST with the fields the taxonomies route
# gives them in the edit context, in its order, without links, the others
# false.
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; $out = array(); foreach ( get_taxonomies( array(), "objects" ) as $tax ) { if ( ! $tax->show_in_rest ) { $out[ $tax->name ] = false; continue; } $out[ $tax->name ] = array( "name" => $tax->label, "slug" => $tax->name, "capabilities" => $tax->cap, "description" => $tax->description, "labels" => $tax->labels, "types" => array_values( $tax->object_type ), "show_cloud" => $tax->show_tagcloud, "hierarchical" => $tax->hierarchical, "rest_base" => ! empty( $tax->rest_base ) ? $tax->rest_base : $tax->name, "rest_namespace" => $tax->rest_namespace, "visibility" => array( "public" => (bool) $tax->public, "publicly_queryable" => (bool) $tax->publicly_queryable, "show_admin_column" => (bool) $tax->show_admin_column, "show_in_nav_menus" => (bool) $tax->show_in_nav_menus, "show_in_quick_edit" => (bool) $tax->show_in_quick_edit, "show_ui" => (bool) $tax->show_ui ) ); } echo wp_json_encode( $out, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES ), "\n";' > "$OUT/wp-includes/rest-taxonomies.json"
# The settings the settings route shows, in its order: each one's option and
# schema.
"$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; rest_get_server(); class HyScribe_Settings extends WP_REST_Settings_Controller { public function options() { return $this->get_registered_options(); } } $out = array(); foreach ( ( new HyScribe_Settings() )->options() as $name => $args ) { unset( $args["schema"]["title"], $args["schema"]["description"] ); $out[ $name ] = array( "option" => $args["option_name"], "schema" => $args["schema"] ); } echo wp_json_encode( $out, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES ), "\n";' > "$OUT/wp-includes/rest-settings.json"
# The styles and scripts as load-styles.php and load-scripts.php register
# them, booted as they boot, without the rest of WordPress: each handle's
# file, version and, for styles, whether it has an RTL twin.
"$DOCKER" exec "$NAME" php -r 'define( "ABSPATH", "/var/www/html/" ); define( "WPINC", "wp-includes" ); define( "WP_CONTENT_DIR", ABSPATH . "wp-content" ); require ABSPATH . "wp-admin/includes/noop.php"; foreach ( array( "theme", "class-wp-theme-json-resolver", "global-styles-and-settings", "script-loader", "version" ) as $file ) { require_once ABSPATH . WPINC . "/$file.php"; } $styles = new WP_Styles(); wp_default_styles( $styles ); $scripts = new WP_Scripts(); wp_default_scripts( $scripts ); wp_default_packages_vendor( $scripts ); wp_default_packages_scripts( $scripts ); $out = array(); foreach ( array( "styles" => $styles, "scripts" => $scripts ) as $kind => $registry ) { foreach ( $registry->registered as $handle => $item ) { $out[ $kind ][ $handle ] = array( "src" => $item->src, "ver" => $item->ver ); if ( "styles" === $kind && ! empty( $item->extra["rtl"] ) ) { $out[ $kind ][ $handle ]["rtl"] = true; } } ksort( $out[ $kind ] ); } echo json_encode( $out, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES ), "\n";' > "$OUT/wp-includes/dependencies.json"
# The styles and scripts an admin request registers, everything WP_Styles and
# WP_Scripts print from. What depends on the site or the request is a
# placeholder: the REST root, admin URLs, other site URLs, the REST nonce,
# user-profile's user ID and nonce, and utils' user ID and time.
"$DOCKER" exec "$NAME" php -r 'define( "WP_ADMIN", true ); require "/var/www/html/wp-load.php"; $out = array(); foreach ( array( "styles" => wp_styles(), "scripts" => wp_scripts() ) as $kind => $registry ) { foreach ( $registry->registered as $handle => $item ) { $out[ $kind ][ $handle ] = array( "src" => $item->src, "deps" => $item->deps, "ver" => $item->ver, "args" => $item->args, "extra" => $item->extra, "textdomain" => $item->textdomain ); } ksort( $out[ $kind ] ); } $out["scripts"]["user-profile"]["extra"]["data"] = str_replace( array( "\"user_id\":\"0\"", wp_create_nonce( "reset-password-for-0" ) ), array( "\"user_id\":\"%user_id%\"", "%nonce:reset-password-for-user%" ), $out["scripts"]["user-profile"]["extra"]["data"] ); $out["scripts"]["utils"]["extra"]["data"] = preg_replace( array( "/\"uid\":\"0\"/", "/\"time\":\"\\d+\"/" ), array( "\"uid\":\"%user_id%\"", "\"time\":\"%time%\"" ), $out["scripts"]["utils"]["extra"]["data"] ); $json = json_encode( $out, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE ); $json = str_replace( array( rest_url(), admin_url(), "\\\"" . admin_url( "", "relative" ), site_url(), wp_create_nonce( "wp_rest" ) ), array( "%rest_url%", "%admin_url%", "\\\"%admin_path%", "%site_url%", "%nonce:wp_rest%" ), $json ); echo $json, "\n";' > "$OUT/wp-includes/registry.json"
# The tags and attributes wp_kses_post() and wp_kses_data() allow.
for context in post data; do
  "$DOCKER" exec "$NAME" php -r 'require "/var/www/html/wp-load.php"; echo wp_json_encode( wp_kses_allowed_html( $argv[1] ), JSON_UNESCAPED_SLASHES );' "$context" > "$OUT/wp-includes/kses-$context.json"
done
"$DOCKER" cp "$ROOT/reference/rest/registered.php" "$NAME:/tmp/registered.php"
"$DOCKER" exec "$NAME" php /tmp/registered.php core > "$OUT/wp-includes/rest-registered.json"
cat > "$OUT/README.md" <<'README'
# WordPress core data

Files from WordPress core that HyScribe uses as data: the core `theme.json`, every core block's `block.json`, the block types as WordPress registers them (`blocks/registry.json`) and their `block.json` in registration order (`blocks/registered.json`), the tags and attributes `wp_kses_post()` and `wp_kses_data()` allow (`kses-post.json`, `kses-data.json`), the MIME types a visitor may upload (`mime-types.json`), the capability data of the post types, taxonomies and post statuses (`capabilities.json`), the REST API's routes (`rest-routes.json`) and their descriptions (`rest-descriptions.json`), the post types and taxonomies it shows (`rest-post-types.json`, `rest-taxonomies.json`), its settings (`rest-settings.json`) and the block types and core pattern categories it gives (`rest-registered.json`), the styles and scripts the admin's concatenating loaders serve (`dependencies.json`), the styles and scripts an admin request registers with placeholders for its URLs and nonces (`registry.json`), the registered script modules (`script-modules.json`), the registered icons' sanitized SVG (`icons.json`), the block library and skip link styles, the emoji detection script, and the embed template's style and scripts. Exported by `reference/wordpress/export.sh` from the pinned version in `VERSION`. Never edit by hand.

License: GPL-2.0-or-later, as WordPress (ADR 0004).
README
echo "wrote $OUT: WordPress $(cat "$OUT/VERSION"), $(find "$OUT" -name block.json | wc -l) blocks"

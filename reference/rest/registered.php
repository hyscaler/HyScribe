<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Run inside the PHP WordPress container by reference/wordpress/export.sh
// (`core`) and reference/themes/export.sh (`theme`): prints, as JSON, what
// the editors' REST routes read from the block and pattern registries.
//   core: {"block_types": [each block type as the block types route gives
//          it in the edit context, without links and without the styles
//          registered with register_block_style()],
//          "pattern_categories": [the categories core registers]}
//   theme: {"block_styles": [[block, style], ...] registered with the theme
//           active, "partial_styles": [[block, style], ...] its style
//           partials register, "pattern_categories": [those it registers]}

require '/var/www/html/wp-load.php';

$styles     = WP_Block_Styles_Registry::get_instance();
$categories = WP_Block_Pattern_Categories_Registry::get_instance();

function hyscribe_styles( $styles ) {
	$out = array();
	foreach ( $styles->get_all_registered() as $block => $registered ) {
		foreach ( $registered as $style ) {
			$out[] = array( $block, $style );
		}
	}
	return $out;
}

function hyscribe_unregister_styles( $styles ) {
	foreach ( $styles->get_all_registered() as $block => $registered ) {
		foreach ( array_keys( $registered ) as $name ) {
			$styles->unregister( $block, $name );
		}
	}
}

$registered = $categories->get_all_registered();
foreach ( $registered as $category ) {
	$categories->unregister( $category['name'] );
}
_register_core_block_patterns_and_categories();
$core = $categories->get_all_registered();
$core_names = wp_list_pluck( $core, 'name' );

if ( 'theme' === $argv[1] ) {
	$block_styles = hyscribe_styles( $styles );
	WP_Theme_JSON_Resolver::get_theme_data();
	$partial_styles = array_slice( hyscribe_styles( $styles ), count( $block_styles ) );
	$theme = array_values( array_filter( $registered, fn( $c ) => ! in_array( $c['name'], $core_names, true ) ) );
	$out   = array( 'block_styles' => $block_styles, 'partial_styles' => $partial_styles, 'pattern_categories' => $theme );
} else {
	$controller = new WP_REST_Block_Types_Controller();
	$request    = new WP_REST_Request( 'GET', '/wp/v2/block-types' );
	$request->set_param( 'context', 'edit' );
	$types = array();
	foreach ( WP_Block_Type_Registry::get_instance()->get_all_registered() as $type ) {
		hyscribe_unregister_styles( $styles );
		$types[] = $controller->prepare_item_for_response( $type, $request )->get_data();
	}
	$out = array( 'block_types' => $types, 'pattern_categories' => $core );
}
echo wp_json_encode( $out, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE ), "\n";

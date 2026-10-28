<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Differential cases for hyscribe-theme-json, run by golden.sh inside the
// PHP WordPress container (default install). Each case is a kind and a
// JSON array of arguments; each output line is
//   kind<TAB>hex(arguments JSON)<TAB>hex(serialize(result))
// The arguments are JSON so that Rust decodes exactly what PHP decodes.
// Cases reach the branches the default install does not: they call the
// WordPress functions (protected ones through reflection) on edge inputs.

require '/var/www/html/wp-load.php';

// Notices go to stderr, so that a case PHP warns about shows up there.
ini_set( 'display_errors', 'stderr' );
error_reporting( E_ALL );

// Resolve once, so the theme's block style variations are registered.
WP_Theme_JSON_Resolver::get_merged_data();

function call( $method, ...$args ) {
	return ( new ReflectionMethod( 'WP_Theme_JSON', $method ) )->invoke( null, ...$args );
}

function stylesheet_of( $tree ) {
	$blocks = array();
	foreach ( $tree->get_styles_block_nodes() as $node ) {
		$blocks[] = array( $node['name'] ?? wp_get_block_name_from_theme_json_path( $node['path'] ), $tree->get_styles_for_block( $node ) );
	}
	return array(
		'sheet'  => $tree->get_stylesheet(),
		'custom' => $tree->get_stylesheet( array( 'custom-css' ) ),
		'blocks' => $blocks,
	);
}

$kinds = array(
	'kebab'         => static fn( $a ) => _wp_to_kebab_case( $a[0] ),
	'float_string'  => static fn( $a ) => (string) $a[0],
	'round'         => static fn( $a ) => round( $a[0], $a[1] ),
	'to_float'      => static fn( $a ) => (float) $a[0],
	'is_numeric'    => static fn( $a ) => is_numeric( $a[0] ),
	'append'        => static fn( $a ) => call( 'append_to_selector', $a[0], $a[1] ),
	'prepend'       => static fn( $a ) => call( 'prepend_to_selector', $a[0], $a[1] ),
	'split'         => static fn( $a ) => call( 'split_selector_list', $a[0] ),
	'scope'         => static fn( $a ) => WP_Theme_JSON::scope_selector( $a[0], $a[1] ),
	'variation'     => static fn( $a ) => call( 'get_block_style_variation_selector', $a[0], $a[1] ),
	'is_safe'       => static fn( $a ) => call( 'is_safe_css_declaration', $a[0], $a[1] ),
	'nest_custom'   => static fn( $a ) => WP_Theme_JSON::process_blocks_custom_css( $a[0], $a[1] ),
	'font_size'     => static fn( $a ) => wp_get_typography_font_size_value( $a[0], $a[1] ),
	'spacing'       => static fn( $a ) => call( 'compute_spacing_sizes', $a[0] ),
	'merge_spacing' => static fn( $a ) => call( 'merge_spacing_sizes', $a[0], $a[1] ),
	'media_queries' => static fn( $a ) => WP_Theme_JSON::get_viewport_media_queries( $a[0] ),
	'viewport'      => static fn( $a ) => call( 'sanitize_viewport_settings', $a[0] ),
	'migrate'       => static fn( $a ) => WP_Theme_JSON_Schema::migrate( $a[0], $a[1] ),
	'new'           => static fn( $a ) => ( new WP_Theme_JSON( $a[0], $a[1] ) )->get_raw_data(),
	'merge'         => static function ( $a ) {
		$tree = new WP_Theme_JSON( $a[0], $a[1] );
		$tree->merge( new WP_Theme_JSON( $a[2], $a[3] ) );
		return $tree->get_raw_data();
	},
	'stylesheet'    => static fn( $a ) => stylesheet_of( new WP_Theme_JSON( $a[0] ) ),
	// Registers a block, so these cases come last.
	'block_meta'    => static function ( $a ) {
		register_block_type( $a[0], $a[1] );
		return call( 'get_blocks_metadata' )[ $a[0] ];
	},
);

// golden.sh appends a call with the lines of cases.txt: "kind JSON".
function run( $text ) {
	global $kinds;
	foreach ( explode( "\n", trim( $text ) ) as $line ) {
		if ( '' === $line || '#' === $line[0] ) {
			continue;
		}
		list( $kind, $json ) = explode( ' ', $line, 2 );
		run_case( $kind, $json );
	}
}

function run_case( $kind, $json ) {
	global $kinds;
	$args = json_decode( $json, true );
	if ( ! is_array( $args ) ) {
		fwrite( STDERR, "bad case: $json\n" );
		exit( 1 );
	}
	if ( 'user' === $kind ) {
		// get_user_data() on a wp_global_styles post with this content.
		wp_clean_theme_json_cache();
		remove_all_filters( 'wp_theme_json_data_user' );
		$content = $args[0];
		add_filter(
			'wp_theme_json_data_user',
			static function () use ( $content ) {
				$config  = array();
				$decoded = json_decode( $content, true );
				if ( JSON_ERROR_NONE === json_last_error() && is_array( $decoded ) && ! empty( $decoded['isGlobalStylesUserThemeJSON'] ) ) {
					unset( $decoded['isGlobalStylesUserThemeJSON'] );
					$config = $decoded;
				}
				return new WP_Theme_JSON_Data( $config, 'custom' );
			}
		);
		$settings = wp_get_global_settings();
		$styles   = wp_get_global_styles();
		$all      = wp_get_global_stylesheet();
		wp_clean_theme_json_cache();
		add_filter( 'wp_theme_json_get_style_nodes', 'wp_filter_out_block_nodes' );
		$inline = wp_get_global_stylesheet() . wp_get_global_stylesheet( array( 'custom-css' ) );
		remove_filter( 'wp_theme_json_get_style_nodes', 'wp_filter_out_block_nodes' );
		$result = array( $settings, $styles, $all, $inline, stylesheet_of( WP_Theme_JSON_Resolver::get_merged_data() )['blocks'] );
	} else {
		$result = $kinds[ $kind ]( $args );
	}
	echo $kind, "\t", bin2hex( $json ), "\t", bin2hex( serialize( $result ) ), "\n";
}


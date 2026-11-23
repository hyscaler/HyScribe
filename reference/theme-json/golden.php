<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for hyscribe-theme-json. Run by golden.sh inside the PHP
// WordPress container (default install, Twenty Twenty-Five, no user global
// styles); the first argument picks what to print:
//   registry   block names in registration order, one per line
//   styles     block styles registered from PHP, "block<TAB>name" lines
//   post-comments  the PHP-registered core/post-comments block, as block.json
//   settings   serialize( wp_get_global_settings() )
//   styles-merged  serialize( wp_get_global_styles() )
//   raw-<origin>   serialize() of the raw data of one resolver layer
//   metadata   serialize() of WP_Theme_JSON::get_blocks_metadata()
//   stylesheet the global-styles stylesheet as wp_enqueue_global_styles()
//              builds it (block nodes filtered out, custom CSS appended)
//   stylesheet-all  wp_get_global_stylesheet() with no filter
//   blocks     "block<TAB>css" for every get_styles_block_nodes() node

require '/var/www/html/wp-load.php';

$what = $argv[1] ?? '';

if ( 'registry' === $what ) {
	foreach ( WP_Block_Type_Registry::get_instance()->get_all_registered() as $name => $type ) {
		echo $name, "\n";
	}
	exit;
}

if ( 'styles' === $what ) {
	foreach ( WP_Block_Styles_Registry::get_instance()->get_all_registered() as $block => $styles ) {
		foreach ( $styles as $name => $style ) {
			echo $block, "\t", $name, "\n";
		}
	}
	exit;
}

if ( 'post-comments' === $what ) {
	$type = WP_Block_Type_Registry::get_instance()->get_registered( 'core/post-comments' );
	echo wp_json_encode(
		array(
			'name'     => $type->name,
			'supports' => $type->supports,
		),
		JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES
	), "\n";
	exit;
}

if ( 'settings' === $what ) {
	echo serialize( wp_get_global_settings() );
	exit;
}

if ( 'styles-merged' === $what ) {
	echo serialize( wp_get_global_styles() );
	exit;
}

if ( str_starts_with( $what, 'raw-' ) ) {
	$layers = array(
		'raw-core'   => static fn() => WP_Theme_JSON_Resolver::get_core_data(),
		'raw-blocks' => static fn() => WP_Theme_JSON_Resolver::get_block_data(),
		'raw-theme'  => static fn() => WP_Theme_JSON_Resolver::get_theme_data(),
		'raw-user'   => static fn() => WP_Theme_JSON_Resolver::get_user_data(),
		'raw-merged' => static fn() => WP_Theme_JSON_Resolver::get_merged_data(),
	);
	// Resolve everything first so later layers see the same block styles.
	WP_Theme_JSON_Resolver::get_merged_data();
	echo serialize( $layers[ $what ]()->get_raw_data() );
	exit;
}

if ( 'metadata' === $what ) {
	WP_Theme_JSON_Resolver::get_merged_data();
	$method = new ReflectionMethod( 'WP_Theme_JSON', 'get_blocks_metadata' );
	echo serialize( $method->invoke( null ) );
	exit;
}

if ( 'stylesheet-all' === $what ) {
	echo wp_get_global_stylesheet();
	exit;
}

if ( 'stylesheet' === $what ) {
	add_filter( 'wp_theme_json_get_style_nodes', 'wp_filter_out_block_nodes' );
	echo wp_get_global_stylesheet(), wp_get_global_stylesheet( array( 'custom-css' ) );
	exit;
}

if ( 'blocks' === $what ) {
	$tree = WP_Theme_JSON_Resolver::get_merged_data();
	foreach ( $tree->get_styles_block_nodes() as $metadata ) {
		$name = $metadata['name'] ?? wp_get_block_name_from_theme_json_path( $metadata['path'] );
		echo $name, "\t", $tree->get_styles_for_block( $metadata ), "\n";
	}
	exit;
}

fwrite( STDERR, "unknown output: $what\n" );
exit( 1 );

<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for block style variations: wp_render_block_style_variation_support_styles()
// over Twenty Twenty-Five's variations and the synthetic ones of
// variations.json (read from stdin). Each line is
//   hex(json {block, className, counter})<TAB>hex(json {className, css})
// where counter is wp_unique_id()'s before the call, css what the
// block-style-variation-styles handle got and instance the class
// wp_render_block_style_variation_class_name() adds to the first tag.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

// Synthetic variations with every kind of data the support handles.
$synthetic = json_decode( file_get_contents( 'php://stdin' ), true );
foreach ( $synthetic['styles'] as list( $block, $style ) ) {
	register_block_style( $block, array( 'name' => $style, 'label' => $style ) );
}
add_filter(
	'wp_theme_json_data_theme',
	static function ( $data ) use ( $synthetic ) {
		return $data->update_with( $synthetic['theme'] );
	}
);
WP_Theme_JSON_Resolver::clean_cached_data();

$tree  = WP_Theme_JSON_Resolver::get_merged_data()->get_raw_data();
$cases = array();
foreach ( $tree['styles']['blocks'] as $block => $node ) {
	foreach ( array_keys( $node['variations'] ?? array() ) as $variation ) {
		$cases[] = array( $block, "is-style-$variation" );
		$cases[] = array( $block, "custom is-style-default is-style-$variation more" );
	}
}
foreach ( array( 'is-style-unknown', 'is-style-unknown is-style-section-1', 'is-style-default', '', 'is-style-section-1-x', 'is-style-section-1:x', 'is-style-section-1-', 'is-style-section-1: x', 'xis-style-section-1', 'a-is-style-section-1', 'is-style-defaultish is-style-section-1', 'is-style-', 'is-style-section-1is-style-section-2', 'is-style-foo--3', 'is-style-foo--3x is-style-a-b--12', 'is-style-a--b--7', 'xis-style-foo--3', 'is-style---4 is-style-x--', 'is-style-x--5_' ) as $class ) {
	$cases[] = array( 'core/group', $class );
}
$cases[] = array( 'core/paragraph', 'is-style-section-1' );
$cases[] = array( 'core/group', null );

foreach ( $cases as list( $block, $class ) ) {
	wp_styles()->remove( 'block-style-variation-styles' );
	$counter = (int) wp_unique_id();
	$parsed  = array(
		'blockName'    => $block,
		'attrs'        => null === $class ? array() : array( 'className' => $class ),
		'innerBlocks'  => array(),
		'innerHTML'    => '',
		'innerContent' => array(),
	);
	$result = wp_render_block_style_variation_support_styles( $parsed );
	$css    = wp_styles()->get_data( 'block-style-variation-styles', 'after' );
	$name   = $result['attrs']['className'] ?? '';
	$found  = is_string( $name ) && preg_match( '/\bis-style-(\S+?--\d+)\b/', $name, $m );
	echo bin2hex( wp_json_encode( array( 'block' => $block, 'className' => $class, 'counter' => $counter ) ) ), "\t",
		bin2hex( wp_json_encode( array( 'className' => $result['attrs']['className'] ?? null, 'css' => $css ? implode( "\n", $css ) : '', 'instance' => $found ? $m[0] : null ) ) ), "\n";
}

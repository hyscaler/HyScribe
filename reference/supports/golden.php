<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for hyscribe-render's block wrapper attributes: for every block
// (name and attributes) in the theme's templates, parts and patterns, plus
// synthetic cases, prints
//   hex(block name)<TAB>hex(attrs JSON)<TAB>hex(get_block_wrapper_attributes())
// Run through golden.sh inside the PHP WordPress oracle.

require '/var/www/html/wp-load.php';

mt_srand( 20261027 );

$cases = array();
$collect = function ( $blocks ) use ( &$collect, &$cases ) {
	foreach ( $blocks as $block ) {
		if ( $block['blockName'] ) {
			$cases[ $block['blockName'] . "\0" . wp_json_encode( $block['attrs'] ) ] = array( $block['blockName'], $block['attrs'] );
		}
		$collect( $block['innerBlocks'] );
	}
};
$theme = get_stylesheet_directory();
foreach ( array_merge( glob( "$theme/templates/*.html" ), glob( "$theme/parts/*.html" ) ) as $file ) {
	$collect( parse_blocks( file_get_contents( $file ) ) );
}
foreach ( WP_Block_Patterns_Registry::get_instance()->get_all_registered() as $pattern ) {
	$collect( parse_blocks( $pattern['content'] ) );
}

// Synthetic attributes over a few blocks with many supports.
$values = array(
	'className'       => array( 'custom', 'a  b', '', 'is-style-x extra' ),
	'align'           => array( 'wide', 'full', 'left', 'nonsense' ),
	'fontSize'        => array( 'small', 'x-large', 'Weird Name' ),
	'fontFamily'      => array( 'manrope', 'fira-code' ),
	'textColor'       => array( 'contrast', 'accent-1' ),
	'backgroundColor' => array( 'base', 'accent-2' ),
	'gradient'        => array( 'some-gradient' ),
	'borderColor'     => array( 'contrast' ),
	'anchor'          => array( 'my-id', '' ),
	'style'           => array(
		array( 'spacing' => array( 'padding' => array( 'top' => 'var:preset|spacing|30', 'left' => '10px' ) ) ),
		array( 'spacing' => array( 'margin' => '1em', 'padding' => '2px' ) ),
		array( 'typography' => array( 'fontSize' => '20px', 'lineHeight' => '1.4', 'letterSpacing' => '1px', 'textTransform' => 'uppercase', 'fontStyle' => 'italic', 'fontWeight' => '700', 'textDecoration' => 'underline' ) ),
		array( 'typography' => array( 'fontSize' => 'clamp(1rem, 2vw, 3rem)' ) ),
		array( 'color' => array( 'text' => '#ff0000', 'background' => 'var:preset|color|base', 'gradient' => 'linear-gradient(red,blue)' ) ),
		array( 'elements' => array( 'link' => array( 'color' => array( 'text' => 'var:preset|color|contrast' ) ) ) ),
		array( 'border' => array( 'width' => '1px', 'style' => 'solid', 'radius' => '4px', 'color' => '#000' ) ),
		array( 'border' => array( 'top' => array( 'width' => '2px', 'color' => 'var:preset|color|contrast' ) ) ),
		array( 'dimensions' => array( 'minHeight' => '50vh', 'aspectRatio' => '16/9' ) ),
		array( 'shadow' => 'var:preset|shadow|natural' ),
		array( 'position' => array( 'type' => 'sticky', 'top' => '0px' ) ),
		array( 'spacing' => array( 'blockGap' => '1em' ) ),
		array( 'background' => array( 'backgroundImage' => array( 'url' => 'https://example.test/a.jpg' ), 'backgroundSize' => 'cover' ) ),
	),
);
$blocks = array( 'core/group', 'core/paragraph', 'core/site-title', 'core/post-title', 'core/post-date', 'core/navigation', 'core/heading', 'core/query', 'core/post-content', 'core/template-part', 'core/search', 'core/spacer' );
foreach ( $blocks as $name ) {
	$cases[ "$name\0{}" ] = array( $name, array() );
	foreach ( $values as $attribute => $options ) {
		foreach ( $options as $option ) {
			$attrs = array( $attribute => $option );
			$cases[ $name . "\0" . wp_json_encode( $attrs ) ] = array( $name, $attrs );
		}
	}
}
for ( $i = 0; $i < 400; $i++ ) {
	$name  = $blocks[ mt_rand( 0, count( $blocks ) - 1 ) ];
	$attrs = array();
	foreach ( array_rand( $values, mt_rand( 2, 4 ) ) as $attribute ) {
		$attrs[ $attribute ] = $values[ $attribute ][ mt_rand( 0, count( $values[ $attribute ] ) - 1 ) ];
	}
	$cases[ $name . "\0" . wp_json_encode( $attrs ) ] = array( $name, $attrs );
}

foreach ( $cases as list( $name, $attrs ) ) {
	WP_Block_Supports::$block_to_render = array( 'blockName' => $name, 'attrs' => $attrs );
	$json = wp_json_encode( (object) $attrs );
	echo bin2hex( $name ), "\t", bin2hex( $json ), "\t", bin2hex( get_block_wrapper_attributes() ), "\n";
}

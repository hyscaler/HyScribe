<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for hyscribe-render's style module. Prints one line per style:
//   p<TAB>0|1<TAB>hex(style JSON)<TAB>hex(serialize(parse_block_styles()))
// (the second field is convert_vars_to_classnames). Run through
// style-golden.sh inside the PHP WordPress oracle.

require '/var/www/html/wp-load.php';

// Every style attribute in the theme, plus synthetic ones.
$styles = array();
$collect = function ( $blocks ) use ( &$collect, &$styles ) {
	foreach ( $blocks as $block ) {
		if ( isset( $block['attrs']['style'] ) ) {
			$styles[ wp_json_encode( $block['attrs']['style'] ) ] = $block['attrs']['style'];
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
$synthetic = array(
	array(),
	array( 'color' => array() ),
	array( 'color' => array( 'text' => 'var:preset|color|accent-1', 'background' => '#fff', 'gradient' => 'var:preset|gradient|vivid-red' ) ),
	array( 'color' => array( 'text' => 0, 'background' => '0' ) ),
	array( 'spacing' => array( 'padding' => array( 'top' => 'var:preset|spacing|30', 'left' => '0', 'right' => 0, 'paddingInline' => '1px' ) ) ),
	array( 'spacing' => array( 'padding' => '1em', 'margin' => 'var:preset|spacing|40' ) ),
	array( 'spacing' => array( 'margin' => array( 'top' => 'var:preset|unknown|x' ) ) ),
	array( 'border' => array( 'color' => 'var:preset|color|contrast', 'radius' => array( 'topLeft' => '2px' ), 'width' => '1px', 'style' => 'solid' ) ),
	array( 'border' => array( 'top' => array( 'color' => 'var:preset|color|contrast', 'width' => '2px', 'style' => '' ), 'left' => array( 'nope' => '1px' ), 'bottom' => 'x' ) ),
	array( 'border' => array( 'radius' => 'var:preset|border-radius|small' ) ),
	array( 'shadow' => 'var:preset|shadow|natural' ),
	array( 'shadow' => '0 0 1px red' ),
	array( 'dimensions' => array( 'minHeight' => '50vh', 'aspectRatio' => '16/9', 'height' => 'var:preset|dimension|tall', 'objectFit' => 'cover' ) ),
	array( 'background' => array( 'backgroundImage' => array( 'url' => 'https://example.test/a.jpg' ), 'gradient' => 'linear-gradient(red,blue)', 'backgroundSize' => 'cover' ) ),
	array( 'background' => array( 'backgroundImage' => 'none', 'gradient' => 'var:preset|gradient|g' ) ),
	array( 'background' => array( 'backgroundImage' => array( 'url' => '' ), 'backgroundRepeat' => 'no-repeat' ) ),
	array( 'background' => array( 'backgroundImage' => array( 'id' => 5 ) ) ),
	array( 'typography' => array( 'fontSize' => 'var:preset|font-size|x-large', 'fontFamily' => 'var:preset|font-family|fira-code', 'lineHeight' => 1.5, 'letterSpacing' => '1px', 'textColumns' => 2, 'writingMode' => 'vertical-rl', 'textIndent' => '1em', 'fontWeight' => '700', 'fontStyle' => 'italic', 'textTransform' => 'uppercase', 'textDecoration' => 'none' ) ),
	array( 'typography' => array( 'fontSize' => 'var:preset|font-size|Weird Name' ) ),
	array( 'elements' => array( 'link' => array( 'color' => array( 'text' => 'red' ) ) ) ),
	'not an array',
	array( 'background' => array( 'backgroundImage' => array( 'url' => 5 ) ) ),
	array( 'background' => array( 'backgroundImage' => array( 'url' => true ) ) ),
	array( 'background' => array( 'backgroundImage' => array( 'url' => 1.5 ) ) ),
	array( 'background' => array( 'backgroundImage' => 7 ) ),
	array( 'border' => array( 'top' => array( '0' => 'x', 'top' => '1px', 'width' => 3 ) ) ),
	array( 'color' => array( 'text' => array( 'x' ) ) ),
	array( 'spacing' => array( 'padding' => array( '0' => '1px', 'top' => array( 'x' ) ) ) ),
	array( 'typography' => array( 'fontSize' => 12, 'lineHeight' => true ) ),
	array( 'border' => array( 'color' => array( 'top' => 'red' ), 'top' => array( 'color' => 'blue' ) ) ),
	array( 'spacing' => array( 'padding' => 'var:preset|unknown|x' ) ),
);
foreach ( $synthetic as $style ) {
	$styles[ wp_json_encode( $style ) ] = $style;
}
foreach ( $styles as $json => $style ) {
	foreach ( array( false, true ) as $convert ) {
		$parsed = WP_Style_Engine::parse_block_styles( $style, array( 'convert_vars_to_classnames' => $convert ) );
		echo "p\t", $convert ? '1' : '0', "\t", bin2hex( $json ), "\t", bin2hex( serialize( $parsed ) ), "\n";
	}
}

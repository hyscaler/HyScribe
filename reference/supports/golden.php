<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for hyscribe-render's block wrapper attributes: for every block
// (name and attributes) in the theme's templates, parts and patterns, plus
// synthetic cases, prints
//   hex(block name)<TAB>hex(attrs JSON)<TAB>hex(get_block_wrapper_attributes( $extra ))<TAB>hex(extra JSON)
// With the argument `types`, prints the synthetic block types it registers
// instead, in the format of wordpress/wp-includes/blocks/registry.json.
// Run through golden.sh inside the PHP WordPress oracle.

require '/var/www/html/wp-load.php';

// Block types with supports no core block has: every support as `true`,
// skipped serialization of each feature, loose flags, and none at all.
// They declare no attributes of their own, so
// prepare_attributes_for_render() passes any value through.
$all      = array( 'fontSize', 'fontFamily', 'fontStyle', 'fontWeight', 'lineHeight', 'textAlign', 'textColumns', 'textDecoration', 'textTransform', 'letterSpacing', 'textIndent', 'writingMode' );
$features = array_fill_keys( array( 'fontSize', '__experimentalFontFamily', '__experimentalFontStyle', '__experimentalFontWeight', 'lineHeight', 'textAlign', 'textColumns', '__experimentalTextDecoration', '__experimentalTextTransform', '__experimentalLetterSpacing', 'textIndent', '__experimentalWritingMode' ), true );
$border   = array( 'color' => true, 'radius' => true, 'style' => true, 'width' => true );
$size     = array( 'minHeight' => true, 'minWidth' => true, 'height' => true, 'width' => true );
$types    = array(
	'hyscribe-test/bare'        => array( 'align' => true, 'anchor' => true, 'ariaLabel' => true, 'className' => true, 'customClassName' => true, 'color' => true, '__experimentalBorder' => true, 'typography' => true, 'shadow' => true, 'dimensions' => true, 'spacing' => true ),
	'hyscribe-test/none'        => array(),
	'hyscribe-test/skips'       => array(
		'color'                => array( 'text' => true, 'background' => true, 'gradients' => true, '__experimentalSkipSerialization' => array( 'text' ) ),
		'typography'           => $features + array( '__experimentalSkipSerialization' => array_slice( $all, 0, 6 ) ),
		'__experimentalBorder' => $border + array( '__experimentalSkipSerialization' => array( 'radius', 'style' ) ),
		'spacing'              => array( 'padding' => true, 'margin' => true, '__experimentalSkipSerialization' => array( 'margin' ) ),
		'dimensions'           => $size + array( '__experimentalSkipSerialization' => array( 'height', 'minWidth' ) ),
	),
	'hyscribe-test/skips-more'  => array(
		'color'                => array( 'text' => true, 'background' => true, 'gradients' => true, '__experimentalSkipSerialization' => array( 'gradients', 'background' ) ),
		'typography'           => $features + array( '__experimentalSkipSerialization' => array_slice( $all, 6 ) ),
		'__experimentalBorder' => $border + array( '__experimentalSkipSerialization' => array( 'width', 'color' ) ),
		'spacing'              => array( 'padding' => true, 'margin' => true, '__experimentalSkipSerialization' => array( 'padding' ) ),
		'dimensions'           => $size + array( '__experimentalSkipSerialization' => array( 'width', 'minHeight' ) ),
	),
	'hyscribe-test/skip-whole'  => array(
		'className'            => false,
		'customClassName'      => false,
		'color'                => array( 'text' => true, '__experimentalSkipSerialization' => array( null ) ),
		'typography'           => $features + array( '__experimentalSkipSerialization' => 'yes' ),
		'__experimentalBorder' => $border,
		'border'               => array( '__experimentalSkipSerialization' => true ),
		'spacing'              => array( 'padding' => true, '__experimentalSkipSerialization' => 1 ),
		'dimensions'           => $size + array( '__experimentalSkipSerialization' => true ),
		'shadow'               => array( '__experimentalSkipSerialization' => true ),
		'ariaLabel'            => array( '__experimentalSkipSerialization' => true ),
	),
	'hyscribe-test/loose-flags' => array(
		'align'                => array(),
		'anchor'               => 'yes',
		'className'            => null,
		'color'                => array( 'text' => false, 'gradients' => 1, '__experimentalSkipSerialization' => 0 ),
		'typography'           => array( 'fontSize' => 'yes', 'lineHeight' => 0, 'textAlign' => array( 'left' ), '__experimentalSkipSerialization' => array() ),
		'__experimentalBorder' => array( 'color' => 1, 'width' => array(), 'radius' => false ),
		'background'           => array( 'gradient' => true ),
		'spacing'              => array( 'margin' => array( 'top' ) ),
		'dimensions'           => array( 'height' => 'yes', 'width' => array() ),
		'shadow'               => array(),
	),
);
foreach ( $types as $name => $supports ) {
	register_block_type( $name, array( 'supports' => $supports ) );
}
if ( 'types' === ( $argv[1] ?? '' ) ) {
	$out = array();
	foreach ( array_keys( $types ) as $name ) {
		$type         = WP_Block_Type_Registry::get_instance()->get_registered( $name );
		$out[ $name ] = array(
			'attributes' => $type->attributes,
			'supports'   => $type->supports,
		);
	}
	echo wp_json_encode( $out, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE );
	exit;
}

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

// Every value shape the supports read, on the synthetic types and on core
// blocks with many supports.
$sides  = array(
	'top'    => array( 'width' => '1px', 'color' => 'var:preset|color|base', 'style' => 'solid' ),
	'right'  => array( 'width' => 2, 'color' => '#222', 'style' => 'dotted' ),
	'bottom' => array( 'color' => 'red' ),
	'left'   => array( 'width' => '3px', 'style' => 'double' ),
);
$shapes = array(
	array(),
	array( 'textColor' => 'contrast', 'backgroundColor' => 'base', 'gradient' => 'g', 'borderColor' => 'contrast', 'fontSize' => 'small', 'fontFamily' => 'manrope', 'align' => 'wide', 'className' => 'x  y', 'anchor' => 'a', 'ariaLabel' => 'Label' ),
	array(
		'style' => array(
			'color'      => array( 'text' => '#111', 'background' => '#222', 'gradient' => 'linear-gradient(red,blue)' ),
			'typography' => array( 'fontSize' => '2rem', 'fontFamily' => 'var:preset|font-family|body', 'fontStyle' => 'var:preset|font-style|italic', 'fontWeight' => 'var:preset|font-weight|bold', 'lineHeight' => '1.5', 'textAlign' => 'center', 'textColumns' => '2', 'textDecoration' => 'var:preset|text-decoration|underline', 'textTransform' => 'var:preset|text-transform|upperCase', 'letterSpacing' => 'var:preset|letter-spacing|wideOne', 'writingMode' => 'vertical-rl', 'textIndent' => '1em' ),
			'border'     => array( 'radius' => 4, 'width' => 2, 'style' => 'dashed', 'color' => '#333' ) + $sides,
			'spacing'    => array( 'padding' => array( 'top' => '1px', 'left' => 'var:preset|spacing|20' ), 'margin' => array( 'top' => '2px', 'bottom' => '3px' ) ),
			'dimensions' => array( 'minHeight' => '10px', 'minWidth' => '20px', 'height' => '30px', 'width' => '40px' ),
			'shadow'     => '0 0 1px red',
		),
	),
	array( 'style' => array( 'typography' => array( 'fontFamily' => 'Georgia, "Times New Roman", Times, serif', 'fontStyle' => 'normal', 'fontWeight' => 700, 'lineHeight' => 1.4, 'textDecoration' => '', 'textTransform' => 'none', 'letterSpacing' => '2px', 'fontSize' => 0 ) ) ),
	array( 'style' => array( 'border' => array( 'radius' => '4', 'width' => '1.5', 'color' => 'var:preset|color|base' ) ) ),
	array( 'style' => array( 'border' => array( 'radius' => array( 'topLeft' => '1px', 'bottomRight' => '2px' ) ) + $sides ) ),
	array( 'style' => array( 'border' => array( 'top' => 'x', 'left' => array( 'width' => null ) ), 'typography' => 'y', 'spacing' => 'z' ) ),
	array( 'style' => 'abc' ),
	array( 'style' => array() ),
	array( 'textColor' => 5, 'ariaLabel' => 7, 'anchor' => 7, 'align' => 3, 'className' => true ),
	array( 'className' => array( 'a' ), 'ariaLabel' => null, 'anchor' => '' ),
	array( 'className' => 1.5, 'align' => '' ),
	array( 'gradient' => 'g', 'style' => array( 'background' => array( 'gradient' => 'linear-gradient(red,blue)' ) ) ),
	array( 'style' => array( 'background' => array( 'gradient' => '' ), 'color' => array( 'gradient' => 'linear-gradient(red,blue)' ) ) ),
	array( 'style' => array( 'spacing' => array( 'margin' => '1px', 'padding' => '2px' ), 'shadow' => 'var:preset|shadow|natural' ) ),
	// Blank and tag-only values, and keys sanitize_key() folds together.
	array( 'style' => array( 'spacing' => array( 'margin' => " \t", 'padding' => array( 'top' => '<b></b>', 'tøp' => '1px', 'tp' => '2px', 'left' => ' 3px ' ) ) ) ),
);
$names = array_merge( array_keys( $types ), array( 'core/group', 'core/paragraph', 'core/button', 'core/icon', 'core/navigation' ) );
foreach ( $names as $name ) {
	foreach ( $shapes as $attrs ) {
		$cases[ $name . "\0" . wp_json_encode( $attrs ) ] = array( $name, $attrs );
	}
}

// Extra attributes, merged with what the supports give.
$extras = array(
	array( 'class' => 'extra  custom wp-block-group', 'style' => 'color: red;', 'id' => 'x', 'aria-label' => 'L', 'data-x' => 'y<">', 'tabindex' => 3, 'hidden' => true, 'n' => null, 'a' => array( 1 ), 'f' => 1.5, 0 => 'zero' ),
	array( 'style' => ' ;; ', 'class' => '' ),
	array( 'style' => 'background:url(javascript:x);width:1px', 'class' => "a\tb\nc\x0bd\x0ce\re a" ),
	array( 'style' => '0', 'id' => '', 'aria-label' => '' ),
	array( 'class' => 7, 'style' => false, 'id' => 0, 'aria-label' => array( 'x' ) ),
	array( 'title' => 'T', 'class' => 'alignwide' ),
);
foreach ( array( 'core/group', 'core/paragraph', 'hyscribe-test/bare', 'hyscribe-test/none', 'core/nope' ) as $name ) {
	foreach ( array_slice( $shapes, 0, 3 ) as $attrs ) {
		foreach ( $extras as $extra ) {
			$cases[ $name . "\0" . wp_json_encode( $attrs ) . "\0" . wp_json_encode( $extra ) ] = array( $name, $attrs, $extra );
		}
	}
}

foreach ( $cases as $case ) {
	list( $name, $attrs ) = $case;
	$extra = $case[2] ?? array();
	WP_Block_Supports::$block_to_render = array( 'blockName' => $name, 'attrs' => $attrs );
	$json = wp_json_encode( (object) $attrs );
	echo bin2hex( $name ), "\t", bin2hex( $json ), "\t", bin2hex( get_block_wrapper_attributes( $extra ) ), "\t", bin2hex( wp_json_encode( (object) $extra ) ), "\n";
}

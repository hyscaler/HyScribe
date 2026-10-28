<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for hyscribe-render's layout support: wp_render_layout_support_flag()
// on every block of the theme's templates, parts and patterns, and on
// synthetic layouts, with the block's own markup as content. Each line is
//   hex(name)<TAB>hex(attrs JSON)<TAB>hex(parentLayout JSON)<TAB>hex(first chunk)<TAB>hex(content)<TAB>hex(result)<TAB>hex(block supports CSS)
// The first chunk is empty for a block without inner blocks. Lines that
// start with "style" call wp_get_layout_style() directly:
//   style<TAB>hex(arguments JSON)<TAB>hex(CSS)
// and the line "types" carries the block types registered here, which
// golden.sh writes to layout-types.json. Run through golden.sh inside the
// PHP WordPress oracle; nothing is written to the database.

require '/var/www/html/wp-load.php';

function layout_case( $block, $parent_layout ) {
	if ( null !== $parent_layout ) {
		$block['parentLayout'] = $parent_layout;
	}
	WP_Style_Engine_CSS_Rules_Store::remove_all_stores();
	$content = $block['innerHTML'];
	$result  = wp_render_layout_support_flag( $content, $block );
	$css     = wp_style_engine_get_stylesheet_from_context( 'block-supports', array( 'prettify' => false ) );
	$first   = ( is_string( $block['innerContent'][0] ?? null ) && count( $block['innerContent'] ) > 1 ) ? $block['innerContent'][0] : '';
	echo bin2hex( $block['blockName'] ), "\t", bin2hex( wp_json_encode( (object) $block['attrs'] ) ), "\t", bin2hex( wp_json_encode( $parent_layout ) ), "\t", bin2hex( $first ), "\t", bin2hex( $content ), "\t", bin2hex( $result ), "\t", bin2hex( $css ), "\n";
}

// Block types of no core block's shape: gaps skipped for blockGap alone
// and for all spacing, and a plugin namespace.
$types = array(
	'hyscribe/gap-features' => array( 'layout' => true, 'spacing' => array( 'blockGap' => true, '__experimentalSkipSerialization' => array( 'blockGap' ) ) ),
	'hyscribe/gap-skipped'  => array( 'layout' => array( 'default' => array( 'type' => 'flex' ) ), 'spacing' => array( 'blockGap' => true, '__experimentalSkipSerialization' => true ) ),
	'hyscribe/legacy'       => array( '__experimentalLayout' => array( 'default' => array( 'type' => 'constrained' ) ) ),
);
foreach ( $types as $name => $supports ) {
	register_block_type( $name, array( 'supports' => $supports ) );
}
$registry = array();
foreach ( $types as $name => $supports ) {
	$registry[ $name ] = array( 'attributes' => array(), 'supports' => $supports );
}
echo "types\t", bin2hex( wp_json_encode( $registry ) ), "\n";

$seen = array();
function layout_walk( $blocks, $parent_layout ) {
	global $seen;
	foreach ( $blocks as $block ) {
		if ( ! $block['blockName'] ) {
			continue;
		}
		$key = $block['blockName'] . wp_json_encode( $block['attrs'] ) . wp_json_encode( $parent_layout ) . $block['innerHTML'];
		if ( ! isset( $seen[ $key ] ) ) {
			$seen[ $key ] = true;
			layout_case( $block, $parent_layout );
		}
		layout_walk( $block['innerBlocks'], $block['attrs']['layout'] ?? null );
	}
}

$theme = get_stylesheet_directory();
foreach ( array_merge( glob( "$theme/templates/*.html" ), glob( "$theme/parts/*.html" ) ) as $file ) {
	layout_walk( parse_blocks( file_get_contents( $file ) ), null );
}
foreach ( WP_Block_Patterns_Registry::get_instance()->get_all_registered() as $pattern ) {
	layout_walk( parse_blocks( $pattern['content'] ), null );
}

// Synthetic layouts over a group, a column and a paragraph.
$layouts = array(
	array( 'type' => 'constrained', 'contentSize' => '620px', 'wideSize' => '1000px', 'justifyContent' => 'left' ),
	array( 'type' => 'constrained', 'justifyContent' => 'right' ),
	array( 'inherit' => true ),
	array( 'contentSize' => 'calc(100% - 2rem);color:red' ),
	array( 'type' => 'flex', 'orientation' => 'vertical', 'justifyContent' => 'stretch', 'verticalAlignment' => 'space-between' ),
	array( 'type' => 'flex', 'orientation' => 'vertical' ),
	array( 'type' => 'flex', 'flexWrap' => 'nowrap', 'justifyContent' => 'space-between', 'verticalAlignment' => 'stretch' ),
	array( 'type' => 'flex', 'justifyContent' => 'left', 'verticalAlignment' => 'bottom' ),
	array( 'type' => 'grid', 'columnCount' => 3, 'minimumColumnWidth' => '12rem' ),
	array( 'type' => 'grid', 'columnCount' => 4, 'rowCount' => 2 ),
	array( 'type' => 'grid', 'minimumColumnWidth' => '200px', 'autoFit' => true ),
	array( 'type' => 'grid' ),
	array( 'type' => 'unknown' ),
);
$gaps = array( null, '2rem', 'var:preset|spacing|40', array( 'top' => '1em', 'left' => 'var:preset|spacing|20' ), array( 'top' => '1em' ), '0', 'x(y)', array( 'left' => 'a/*b' ) );
foreach ( $layouts as $layout ) {
	foreach ( $gaps as $gap ) {
		$attrs = array( 'layout' => $layout );
		if ( null !== $gap ) {
			$attrs['style'] = array( 'spacing' => array( 'blockGap' => $gap, 'padding' => array( 'left' => '0', 'right' => '2rem' ) ) );
		}
		$markup = '<!-- wp:group ' . wp_json_encode( $attrs ) . ' --><div class="wp-block-group"><!-- wp:paragraph --><p>x</p><!-- /wp:paragraph --></div><!-- /wp:group -->';
		layout_case( parse_blocks( $markup )[0], null );
	}
}
$children = array(
	array( 'selfStretch' => 'fixed', 'flexSize' => '300px' ),
	array( 'selfStretch' => 'fixedNoShrink', 'flexSize' => '20%' ),
	array( 'selfStretch' => 'fill' ),
	array( 'selfStretch' => 'fit' ),
	array( 'columnStart' => 2, 'columnSpan' => 3, 'rowStart' => 1, 'rowSpan' => 2 ),
	array( 'columnSpan' => 2 ),
	array( 'columnStart' => 3 ),
	array( 'rowSpan' => 4 ),
	array( 'columnStart' => 0, 'columnSpan' => '0', 'rowStart' => '', 'rowSpan' => null ),
	array( 'selfStretch' => 'fixed' ),
);
$parents = array( null, array( 'type' => 'grid', 'minimumColumnWidth' => '10rem' ), array( 'type' => 'grid', 'columnCount' => 3 ), array( 'type' => 'grid', 'minimumColumnWidth' => '240px', 'columnCount' => 2 ), array( 'type' => 'grid', 'minimumColumnWidth' => '50%' ) );
foreach ( $children as $child ) {
	foreach ( $parents as $parent ) {
		foreach ( array( 'paragraph' => '<p>x</p>', 'group' => '<div class="wp-block-group"><p>x</p></div>' ) as $name => $html ) {
			$markup = '<!-- wp:' . $name . ' ' . wp_json_encode( array( 'style' => array( 'layout' => $child ) ) ) . ' -->' . $html . '<!-- /wp:' . $name . ' -->';
			layout_case( parse_blocks( $markup )[0], $parent );
		}
	}
}
// The inner wrapper: a group with an extra inner container, and a block
// whose first chunk closes what it opens.
layout_case( parse_blocks( '<!-- wp:group {"layout":{"type":"flex"}} --><div class="wp-block-group"><div class="wp-block-group__inner-container"><!-- wp:paragraph --><p>x</p><!-- /wp:paragraph --></div></div><!-- /wp:group -->' )[0], null );
layout_case( parse_blocks( '<!-- wp:group {"layout":{"type":"flex"}} --><div class="wp-block-group"><span class="x">a</span><br><!-- wp:paragraph --><p>x</p><!-- /wp:paragraph --></div><!-- /wp:group -->' )[0], null );
layout_case( parse_blocks( '<!-- wp:group {"layout":{"type":"flex"}} --><div class="wp-block-group"><img class="photo" src="x"><!-- wp:paragraph --><p>x</p><!-- /wp:paragraph --></div><!-- /wp:group -->' )[0], null );
layout_case( parse_blocks( '<!-- wp:group {"layout":{"type":"flex"}} --><section><div class="missing"><!-- wp:paragraph --><p>x</p><!-- /wp:paragraph --></div></section><!-- /wp:group -->' )[0], null );
layout_case( parse_blocks( '<!-- wp:group --><!-- /wp:group -->' )[0], null );
foreach ( array_keys( $types ) as $name ) {
	foreach ( array( null, '2rem', array( 'top' => '1em' ) ) as $gap ) {
		$attrs = null === $gap ? array() : array( 'style' => array( 'spacing' => array( 'blockGap' => $gap ) ) );
		layout_case( parse_blocks( '<!-- wp:' . $name . ' ' . wp_json_encode( (object) $attrs ) . ' --><div>x</div><!-- /wp:' . $name . ' -->' )[0], null );
	}
}
// A child in a grid whose minimum width has no unit, or no number.
foreach ( array( '12', 'abc', '10rem10rem' ) as $width ) {
	layout_case( parse_blocks( '<!-- wp:paragraph {"style":{"layout":{"columnSpan":2}}} --><p>x</p><!-- /wp:paragraph -->' )[0], array( 'type' => 'grid', 'minimumColumnWidth' => $width ) );
}
// Gap sides that are not scalars.
layout_case( parse_blocks( '<!-- wp:group {"style":{"spacing":{"blockGap":{"top":["x"],"left":"1em"}}},"layout":{"type":"flex"}} --><div class="wp-block-group"></div><!-- /wp:group -->' )[0], null );

// wp_get_layout_style() with fallback gaps no core block gives.
$styles = array(
	array( 'flex', array( 'left' => '3px' ), array( 'top' => '1px', 'left' => '2px' ) ),
	array( 'flex', array( 'top' => '1px' ), array( 'left' => '2px', 'top' => '9px' ) ),
	array( 'flex', array( 'top' => '1px' ), array( 'x' => '7px' ) ),
	array( 'grid', null, array( 'top' => '1px', 'left' => '2px' ) ),
	array( 'grid', null, array( 'top' => '1px' ) ),
	array( 'grid', array( 'left' => '5px' ), array( 'top' => '1px' ) ),
	array( 'grid', '0', '1em' ),
	array( 'grid', 0, '1em' ),
	array( 'grid', '2em', '1em', false ),
	array( 'grid', '2em', '1em', true, true ),
	array( 'grid', null, '1em', true, false ),
	array( 'flex', '2em', '1em', false ),
	array( 'flex', '2em', '1em', true, true ),
	array( 'default', '2em', '1em', true, true ),
	array( 'constrained', '2em', '1em', false ),
);
foreach ( $styles as $style ) {
	list( $type, $gap, $fallback ) = $style;
	$supported = $style[3] ?? true;
	$skip      = $style[4] ?? false;
	WP_Style_Engine_CSS_Rules_Store::remove_all_stores();
	$css = wp_get_layout_style( '.c', array( 'type' => $type ), $supported, $gap, $skip, $fallback );
	echo 'style', "\t", bin2hex( wp_json_encode( array( $type, $gap, $fallback, $supported, $skip ) ) ), "\t", bin2hex( $css ), "\n";
}

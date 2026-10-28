<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints do_blocks() for search block markup. Lines are
//   hex(markup)<TAB>wp_unique_id() counter<TAB>hex(search query)<TAB>hex(html)
// Only pure rendering runs; the database is only read.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

// The elements support (wp-elements-* classes) is not ported yet.
remove_filter( 'render_block', 'wp_render_elements_class_name', 10 );
remove_filter( 'render_block_data', 'wp_render_elements_support_styles', 10 );

function search( $attrs ) {
	$json = $attrs ? ' ' . wp_json_encode( $attrs, JSON_UNESCAPED_SLASHES ) : '';
	return "<!-- wp:search$json /-->";
}

$border = array( 'width' => '2px', 'color' => '#f00', 'style' => 'dashed', 'radius' => '8px' );
$sides  = array(
	'top'    => array( 'color' => 'var:preset|color|accent-1', 'width' => '1px', 'style' => 'solid' ),
	'right'  => array( 'color' => '#00f' ),
	'bottom' => array( 'width' => '3px' ),
	'left'   => array( 'style' => 'dotted', 'color' => 'var:preset|color|contrast' ),
);
$cases = array(
	search( array() ),
	search( array( 'label' => 'Search', 'showLabel' => false, 'placeholder' => 'Type here...', 'buttonText' => 'Search' ) ),
	search( array( 'showLabel' => true, 'label' => 'Find <b>it</b>', 'fontSize' => 'small', 'fontFamily' => 'manrope' ) ),
	search( array( 'label' => '', 'buttonText' => '' ) ),
	search( array( 'label' => '0', 'buttonText' => '<em>Go</em> <script>x</script>' ) ),
	search( array( 'buttonPosition' => 'button-inside' ) ),
	search( array( 'buttonPosition' => 'button-outside', 'buttonUseIcon' => false ) ),
	search( array( 'buttonPosition' => 'button-outside', 'buttonUseIcon' => true ) ),
	search( array( 'buttonPosition' => 'no-button', 'buttonUseIcon' => true ) ),
	search( array( 'buttonPosition' => 'no-button', 'textColor' => 'contrast', 'style' => array( 'color' => array( 'background' => '#eee' ) ) ) ),
	search( array( 'buttonPosition' => 'button-only' ) ),
	search( array( 'buttonPosition' => 'button-only', 'buttonUseIcon' => true, 'isSearchFieldHidden' => true ) ),
	search( array( 'buttonPosition' => 'other' ) ),
	search( array( 'width' => 50, 'widthUnit' => '%' ) ),
	search( array( 'width' => 50.7, 'widthUnit' => 'px' ) ),
	search( array( 'width' => 50 ) ),
	search( array( 'style' => array( 'border' => $border ) ) ),
	search( array( 'buttonPosition' => 'button-inside', 'style' => array( 'border' => $border ) ) ),
	search( array( 'style' => array( 'border' => $sides ) ) ),
	search( array( 'buttonPosition' => 'button-inside', 'style' => array( 'border' => $sides ) ) ),
	search( array( 'style' => array( 'border' => array( 'radius' => array( 'topLeft' => '4px', 'topRight' => null, 'bottomLeft' => 'var:preset|border-radius|smallRadius', 'bottomRight' => '0' ) ) ) ) ),
	search( array( 'buttonPosition' => 'button-inside', 'style' => array( 'border' => array( 'radius' => array( 'topLeft' => '4px', 'topRight' => '0px', 'bottomLeft' => 'var:preset|border-radius|small', 'bottomRight' => 6 ) ) ) ) ),
	search( array( 'buttonPosition' => 'button-inside', 'style' => array( 'border' => array( 'radius' => 12 ) ) ) ),
	search( array( 'buttonPosition' => 'button-inside', 'style' => array( 'border' => array( 'radius' => '0' ) ) ) ),
	search( array( 'buttonPosition' => 'button-inside', 'style' => array( 'border' => array( 'radius' => 'var:preset|border-radius|bigOne' ) ) ) ),
	search( array( 'style' => array( 'border' => array( 'radius' => '0px' ) ) ) ),
	search( array( 'borderColor' => 'accent-2' ) ),
	search( array( 'buttonPosition' => 'button-inside', 'borderColor' => 'accent-2', 'style' => array( 'border' => array( 'color' => '#123' ) ) ) ),
	search( array( 'textColor' => 'base', 'backgroundColor' => 'contrast' ) ),
	search( array( 'gradient' => 'cool', 'style' => array( 'color' => array( 'text' => '#111', 'gradient' => 'linear-gradient(red,blue)' ) ) ) ),
	search( array( 'buttonPosition' => 'no-button', 'style' => array( 'color' => array( 'text' => '#111', 'background' => '#222', 'gradient' => 'linear-gradient(red,blue)' ) ) ) ),
	search( array( 'style' => array( 'typography' => array( 'fontSize' => '20px', 'fontFamily' => 'serif', 'letterSpacing' => '1px', 'fontWeight' => '700', 'fontStyle' => 'italic', 'lineHeight' => '1.5', 'textTransform' => 'uppercase', 'textDecoration' => 'underline' ) ) ) ),
	search( array( 'showLabel' => true, 'style' => array( 'typography' => array( 'textDecoration' => 'line-through', 'fontSize' => 'clamp(1rem, 2vw, 3rem)' ) ) ) ),
	search( array( 'showLabel' => false, 'style' => array( 'typography' => array( 'textDecoration' => 'underline' ) ) ) ),
	search( array( 'query' => array( 'post_type' => 'page', 'cat' => 3, 'x"y' => '<a>' ) ) ),
	search( array( 'query' => array() ) ),
	search( array( 'query' => array( 'a', 'b' ) ) ),
	search( array( 'buttonPosition' => '', 'buttonUseIcon' => true ) ),
	search( array( 'style' => array( 'border' => array( 'radius' => array( '4px', 'var:preset|border-radius|x', 'TopLeft' => '2px' ) ) ) ) ),
	search( array( 'tagName' => 'search' ) ),
	search( array( 'tagName' => 'search', 'buttonPosition' => 'button-only' ) ),
	search( array( 'tagName' => 'div' ) ),
	search( array( 'className' => 'is-style-x', 'align' => 'wide', 'anchor' => 'find' ) ),
	search( array( 'placeholder' => '"Quoted" & <tag>' ) ),
	search( array( 'showLabel' => true, 'label' => '' ) ),
);
foreach ( $cases as $markup ) {
	foreach ( array( '', 'hello "world" <b>&amp;</b>' ) as $query ) {
		$GLOBALS['wp_query'] = new WP_Query();
		if ( '' !== $query ) {
			$GLOBALS['wp_query']->set( 's', $query );
		}
		$counter = (int) wp_unique_id();
		echo bin2hex( $markup ), "\t", $counter, "\t", bin2hex( $query ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
		$GLOBALS['wp_query'] = $GLOBALS['wp_the_query'];
	}
}

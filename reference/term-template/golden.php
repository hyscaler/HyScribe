<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints WP_Term_Query's terms for the arguments the term template gives
// it, and terms query blocks on the home page and on term archives, over
// the Theme Test Data fixture. The fixture's content is not ours to commit,
// so lines hold hashes:
//   query<TAB>hex(args JSON)<TAB>sha256(term IDs, comma separated)
//   block<TAB>hex(query)<TAB>hex(markup)<TAB>sha256(html)
// With RAW=1 the IDs and html themselves are printed instead. Nothing is
// written.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

$raw  = getenv( 'RAW' );
$hash = static fn( $text ) => $raw ? $text : hash( 'sha256', $text );

$cases = array();
foreach ( array( 'category', 'post_tag', 'post_format', 'none' ) as $taxonomy ) {
	foreach ( array( 'name', 'count', 'slug', 'term_id', 'id', '', 'none', 'include', 'term_order', 'TERM_ORDER', 'description', 'parent', 'NAME', 'bogus' ) as $orderby ) {
		foreach ( array( 'ASC', 'desc' ) as $order ) {
			foreach ( array( true, false ) as $hide_empty ) {
				$cases[] = array( 'taxonomy' => $taxonomy, 'number' => 'none' === $orderby ? 0 : 5, 'order' => $order, 'orderby' => $orderby, 'hide_empty' => $hide_empty );
			}
		}
	}
}
foreach ( array( 'asc', 'Asc', '', null, 1 ) as $order ) {
	$cases[] = array( 'taxonomy' => 'post_tag', 'number' => 3, 'order' => $order, 'orderby' => 'count', 'hide_empty' => false );
}
foreach ( array( 0, 3, 39, 49, 64, -1 ) as $parent ) {
	foreach ( array( 0, 2 ) as $number ) {
		$cases[] = array( 'taxonomy' => 'category', 'number' => $number, 'order' => 'asc', 'orderby' => 'name', 'hide_empty' => true, 'parent' => $parent );
		// Two of the fixture's tags share a name, which MariaDB orders either way.
		$cases[] = array( 'taxonomy' => 'post_tag', 'number' => $number, 'order' => 'asc', 'orderby' => 'term_id', 'hide_empty' => false, 'parent' => $parent );
	}
}
foreach ( array( 38, 39, 49, 64, -1 ) as $child_of ) {
	foreach ( array( 0, 3 ) as $number ) {
		foreach ( array( 'name', 'term_id' ) as $orderby ) {
			$cases[] = array( 'taxonomy' => 'category', 'number' => $number, 'order' => 'desc', 'orderby' => $orderby, 'hide_empty' => false, 'child_of' => $child_of );
		}
	}
	$cases[] = array( 'taxonomy' => 'category', 'number' => 0, 'order' => 'asc', 'orderby' => 'name', 'hide_empty' => true, 'parent' => 39, 'child_of' => $child_of );
	$cases[] = array( 'taxonomy' => 'category', 'number' => 0, 'order' => 'asc', 'orderby' => 'name', 'hide_empty' => false, 'parent' => 0, 'child_of' => $child_of );
}
foreach ( array( array( 64, 39, 1, 9999 ), array( 61, 60 ), array( 0 ) ) as $include ) {
	foreach ( array( 'include', 'name' ) as $orderby ) {
		foreach ( array( 'category', 'post_tag' ) as $taxonomy ) {
			$cases[] = array( 'taxonomy' => $taxonomy, 'number' => 2, 'order' => 'asc', 'orderby' => $orderby, 'hide_empty' => true, 'include' => $include );
			$cases[] = array( 'taxonomy' => $taxonomy, 'number' => 0, 'order' => 'desc', 'orderby' => $orderby, 'hide_empty' => false, 'include' => $include );
		}
	}
}
// Columns every term shares order a single term, so ties cannot move it.
foreach ( array( 'term_group', 'taxonomy', 'term_taxonomy_id' ) as $orderby ) {
	$cases[] = array( 'taxonomy' => 'category', 'number' => 0, 'order' => 'asc', 'orderby' => $orderby, 'hide_empty' => true, 'include' => array( 64 ) );
}
foreach ( $cases as $args ) {
	$ids = array_map( static fn( $term ) => $term->term_id, ( new WP_Term_Query( $args ) )->get_terms() );
	echo 'query', "\t", bin2hex( wp_json_encode( $args ) ), "\t", $hash( implode( ',', $ids ) ), "\n";
}

$tag     = get_terms( array( 'taxonomy' => 'post_tag', 'orderby' => 'term_id', 'number' => 1 ) )[0]->term_id;
$inner   = '<!-- wp:term-name {"isLink":true,"level":3} /--><!-- wp:term-count {"bracketType":"square"} /--><!-- wp:term-description /-->';
$wrap    = static fn( $attrs, $template = '' ) => '<!-- wp:terms-query ' . $attrs . ' --><div class="wp-block-terms-query"><!-- wp:term-template ' . $template . ' -->' . $inner . '<!-- /wp:term-template --></div><!-- /wp:terms-query -->';
$markups = array(
	$wrap( '{}', '{"style":{"elements":{"link":{"color":{"text":"red"}}}},"fontSize":"small"}' ),
	$wrap( '{}' ),
	$wrap( '{"termQuery":{"perPage":3,"taxonomy":"category","order":"desc","orderBy":"count","include":[],"hideEmpty":true,"showNested":true,"inherit":false}}' ),
	$wrap( '{"termQuery":{"perPage":10,"taxonomy":"category","order":"asc","orderBy":"name","include":[],"hideEmpty":true,"showNested":false,"inherit":true}}' ),
	$wrap( '{"termQuery":{"perPage":6,"taxonomy":"category","order":"desc","orderBy":"term_id","include":[],"hideEmpty":true,"showNested":true,"inherit":false}}' ),
	$wrap( '{"termQuery":{"perPage":10,"taxonomy":"category","order":"asc","orderBy":"name","include":[],"hideEmpty":false,"showNested":true,"inherit":true}}' ),
	$wrap( '{"termQuery":{"perPage":4,"taxonomy":"post_tag","order":"asc","orderBy":"name","include":[],"hideEmpty":true,"showNested":false,"inherit":false}}' ),
	$wrap( '{"termQuery":{"perPage":4,"taxonomy":"post_tag","order":"asc","orderBy":"name","include":[' . $tag . ',"64","-' . $tag . '"],"hideEmpty":false,"showNested":false,"inherit":false}}' ),
	$wrap( '{"termQuery":{"perPage":5,"taxonomy":"category","order":"asc","orderBy":"name","include":[64,39],"hideEmpty":false,"showNested":false,"inherit":false}}', '{"className":"x","layout":{"type":"grid","columnCount":2}}' ),
	$wrap( '{"termQuery":{"perPage":5,"taxonomy":"nope","order":"asc","orderBy":"name","include":[],"hideEmpty":false,"showNested":false,"inherit":false}}' ),
	$wrap( '{"termQuery":{}}' ),
	'<!-- wp:term-template -->' . $inner . '<!-- /wp:term-template -->',
);
foreach ( array( '', 'cat=39', 'cat=49', 'tag_id=' . $tag ) as $query ) {
	$GLOBALS['wp'] = new WP();
	wp( $query );
	foreach ( $markups as $i => $markup ) {
		// The link color's `wp-elements-` class counts up: once, first.
		if ( 0 === $i && '' !== $query ) {
			continue;
		}
		$html = render_block( parse_blocks( $markup )[0] );
		echo 'block', "\t", bin2hex( $query ), "\t", bin2hex( $markup ), "\t", $hash( $html ), "\n";
	}
}

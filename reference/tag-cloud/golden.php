<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints do_blocks() for tag cloud block markup over sets of terms that
// the terms_pre_query filter gives get_terms() (most used first, as the
// query sorts them), without the elements support, which is checked on its
// own. Taxonomies register their pretty links at init, so a structure set
// by filter here would not reach the term links. Lines are
//   hex(markup)<TAB>hex(JSON terms: id, name, slug, count)<TAB>hex(html)
// Nothing is read from or written to the term tables.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

remove_filter( 'render_block', 'wp_render_elements_class_name', 10 );
remove_filter( 'render_block_data', 'wp_render_elements_support_styles', 10 );

function cloud( $attrs ) {
	$json = $attrs ? ' ' . wp_json_encode( $attrs, JSON_UNESCAPED_SLASHES ) : '';
	return "<!-- wp:tag-cloud$json /-->";
}

$cases = array(
	cloud( array() ),
	cloud( array( 'showTagCounts' => true ) ),
	cloud( array( 'numberOfTags' => 2 ) ),
	cloud( array( 'smallestFontSize' => '1em', 'largestFontSize' => '3em' ) ),
	cloud( array( 'smallestFontSize' => '12px', 'largestFontSize' => '12px' ) ),
	cloud( array( 'smallestFontSize' => '20pt', 'largestFontSize' => '10pt' ) ),
	cloud( array( 'smallestFontSize' => '1.5rem', 'largestFontSize' => '2.25rem' ) ),
	cloud( array( 'smallestFontSize' => '10', 'largestFontSize' => '20' ) ),
	cloud( array( 'smallestFontSize' => "8pt\n" ) ),
	cloud( array( 'smallestFontSize' => 'px', 'largestFontSize' => 'x' ) ),
	cloud( array( 'smallestFontSize' => '8 pt' ) ),
	cloud( array( 'smallestFontSize' => '50%', 'largestFontSize' => '150%' ) ),
	cloud( array( 'taxonomy' => 'category' ) ),
	cloud( array( 'taxonomy' => 'post_format' ) ),
	cloud( array( 'taxonomy' => 'nope' ) ),
	cloud( array( 'align' => 'center', 'className' => 'extra', 'fontSize' => 'small' ) ),
);
$term_sets = array(
	array(),
	array( array( 9, 'Foo', 'foo', 1 ) ),
	array( array( 9, 'zeta', 'zeta', 1200 ), array( 10, 'Alpha & <b>Co</b>', 'alpha', 30 ), array( 11, 'img10', 'img10', 3 ), array( 12, 'img2', 'img2', 3 ), array( 13, 'Beta', 'beta', 1 ) ),
	array( array( 21, 'b', 'b', 5 ), array( 22, 'B', 'b-2', 5 ), array( 23, 'a', 'a', 5 ) ),
	array( array( 41, 'c', 'post-format-chat', 7 ), array( 42, 'g', 'post-format-gallery', 6 ), array( 43, 'l', 'post-format-link', 5 ), array( 44, 'i', 'post-format-image', 4 ), array( 45, 's', 'post-format-status', 3 ), array( 46, 'v', 'post-format-video', 2 ), array( 47, 'a', 'post-format-audio', 1 ) ),
	array( array( 31, 'x', 'post-format-quote', 4 ), array( 32, 'y', 'post-format-aside', 2 ), array( 33, 'z', 'post-format-', 1 ), array( 34, 'w', 'standard', 1 ) ),
);

$GLOBALS['wp'] = new WP();
wp( '' );
foreach ( $term_sets as $set ) {
	remove_all_filters( 'terms_pre_query' );
	add_filter(
		'terms_pre_query',
		static function ( $terms, $query ) use ( $set ) {
			$taxonomy = $query->query_vars['taxonomy'][0];
			return array_map(
				static fn( $t ) => new WP_Term( (object) array( 'term_id' => $t[0], 'name' => $t[1], 'slug' => $t[2], 'count' => $t[3], 'taxonomy' => $taxonomy, 'term_taxonomy_id' => $t[0], 'parent' => 0 ) ),
				array_slice( $set, 0, $query->query_vars['number'] )
			);
		},
		10,
		2
	);
	foreach ( $cases as $markup ) {
		echo bin2hex( $markup ), "\t", bin2hex( wp_json_encode( $set ) ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
	}
}

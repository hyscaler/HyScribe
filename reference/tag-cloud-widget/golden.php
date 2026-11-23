<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints do_blocks() for the tag cloud widget in a legacy widget block
// (signed under the fixed salt `test-salt`) over sets of terms that the
// terms_pre_query filter gives get_terms() (most used first, as the query
// sorts them). Lines are
//   hex(markup)<TAB>hex(JSON terms: id, name, slug, count)<TAB>hex(html)
// Nothing is read from or written to the term tables.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

add_filter( 'salt', static fn() => 'test-salt' );

function widget( $instance ) {
	$serialized = serialize( $instance );
	$attrs      = array(
		'idBase'   => 'tag_cloud',
		'instance' => array(
			'encoded' => base64_encode( $serialized ),
			'hash'    => wp_hash( $serialized ),
		),
	);
	return '<!-- wp:legacy-widget ' . wp_json_encode( $attrs ) . ' /-->';
}

$cases = array(
	widget( array() ),
	widget( array( 'count' => 1 ) ),
	widget( array( 'count' => '0' ) ),
	widget( array( 'title' => 'Words & <i>these</i>' ) ),
	widget( array( 'title' => '0' ) ),
	widget( array( 'taxonomy' => '' ) ),
	widget( array( 'taxonomy' => 'post_tag' ) ),
	widget( array( 'taxonomy' => 'category' ) ),
	widget( array( 'taxonomy' => 'post_format' ) ),
	widget( array( 'taxonomy' => 'nope' ) ),
	widget( array( 'taxonomy' => 'nav_menu' ) ),
	widget( array( 'taxonomy' => 'link_category' ) ),
	widget( array( 'taxonomy' => 'wp_theme' ) ),
	widget( array( 'taxonomy' => 'wp_template_part_area' ) ),
	widget( array( 'taxonomy' => 'wp_pattern_category' ) ),
	widget( array( 'taxonomy' => 'category', 'title' => 'Topics', 'count' => true ) ),
);
$term_sets = array(
	array(),
	array( array( 9, 'Foo', 'foo', 1 ) ),
	array( array( 9, 'zeta', 'zeta', 1200 ), array( 10, 'Alpha & <b>Co</b>', 'alpha', 30 ), array( 11, 'img10', 'img10', 3 ), array( 12, 'img2', 'img2', 3 ), array( 13, 'Beta', 'beta', 1 ) ),
	array( array( 41, 'c', 'post-format-chat', 7 ), array( 42, 'g', 'post-format-gallery', 6 ) ),
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

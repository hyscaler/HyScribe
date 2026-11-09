<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints do_blocks() for categories block markup over categories and tags
// inserted for it and rolled back after, on several requests, without
// the elements support, which is checked on its own. Lines are
//   hex(markup)<TAB>hex(request)<TAB>hex(state JSON)<TAB>hex(html)
// where the state holds each taxonomy's terms by name (ID, name, slug,
// parent, count), its published posts per term, and the queried term.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

remove_filter( 'render_block', 'wp_render_elements_class_name', 10 );
remove_filter( 'render_block_data', 'wp_render_elements_support_styles', 10 );

global $wpdb;
$wpdb->query( 'START TRANSACTION' );
$parent = wp_insert_term( 'Parent', 'category' )['term_id'];
$child  = wp_insert_term( 'Child & <b>co</b>', 'category', array( 'parent' => $parent, 'slug' => 'child' ) )['term_id'];
$grand  = wp_insert_term( 'Grand', 'category', array( 'parent' => $child ) )['term_id'];
$other  = wp_insert_term( 'Other', 'category' )['term_id'];
wp_insert_term( 'Lonely', 'category' );
wp_insert_term( 'zempty', 'post_tag' );
wp_set_object_terms( 1, array( $child, $other, 1 ), 'category' );
wp_set_object_terms( 1, array( 'news', 'Alpha' ), 'post_tag' );
set_post_format( 1, 'aside' );
clean_term_cache( array( $parent, $child, $grand, $other ), 'category' );

function block( $attrs ) {
	$json = $attrs ? ' ' . wp_json_encode( $attrs, JSON_UNESCAPED_SLASHES ) : '';
	return "<!-- wp:categories$json /-->";
}
$cases = array(
	block( array() ),
	block( array( 'showPostCounts' => true ) ),
	block( array( 'showHierarchy' => true ) ),
	block( array( 'showHierarchy' => true, 'showPostCounts' => true ) ),
	block( array( 'showEmpty' => true ) ),
	block( array( 'showEmpty' => true, 'showHierarchy' => true ) ),
	block( array( 'showOnlyTopLevel' => true ) ),
	block( array( 'showOnlyTopLevel' => true, 'showHierarchy' => true, 'showPostCounts' => true ) ),
	block( array( 'displayAsDropdown' => true ) ),
	block( array( 'displayAsDropdown' => true, 'showHierarchy' => true, 'showPostCounts' => true, 'showEmpty' => true ) ),
	block( array( 'displayAsDropdown' => true, 'showLabel' => false, 'label' => 'Pick <em>one</em><script>x</script>' ) ),
	block( array( 'taxonomy' => 'post_tag' ) ),
	block( array( 'taxonomy' => 'post_tag', 'displayAsDropdown' => true, 'showEmpty' => true ) ),
	block( array( 'taxonomy' => 'post_format' ) ),
	block( array( 'taxonomy' => 'post_format', 'displayAsDropdown' => true ) ),
	block( array( 'taxonomy' => 'nope' ) ),
	block( array( 'className' => 'extra', 'fontSize' => 'small', 'showHierarchy' => true, 'showEmpty' => true, 'showPostCounts' => true ) ),
	'<!-- wp:query {"queryId":7,"enhancedPagination":true,"query":{"inherit":false}} -->' . block( array() ) . '<!-- /wp:query -->',
);

$state = array();
foreach ( array( 'category', 'post_tag', 'post_format' ) as $taxonomy ) {
	$terms = get_terms( array( 'taxonomy' => $taxonomy, 'hide_empty' => false, 'orderby' => 'name' ) );
	$rows  = $wpdb->get_results( $wpdb->prepare( "SELECT tt.term_id, tr.object_id FROM $wpdb->term_relationships tr JOIN $wpdb->term_taxonomy tt ON tt.term_taxonomy_id = tr.term_taxonomy_id JOIN $wpdb->posts p ON tr.object_id = p.ID WHERE tt.taxonomy = %s AND p.post_type = 'post' AND p.post_status = 'publish'", $taxonomy ) );
	$state[ $taxonomy ] = array(
		'terms' => array_map( static fn( $t ) => array( $t->term_id, $t->name, $t->slug, $t->parent, $t->count ), $terms ),
		'posts' => array_map( static fn( $r ) => array( (int) $r->term_id, (int) $r->object_id ), $rows ),
	);
}

// Not $request: WordPress sets that global.
foreach ( array( '', "cat=$child", "cat=$grand", "cat=$parent", 'tag=news', 'p=1', 'post_format=aside', 'category_name=-1' ) as $query ) {
	$GLOBALS['wp'] = new WP();
	wp( $query );
	$queried = ( is_category() || is_tag() || is_tax() ) ? get_queried_object() : null;
	$state['queried'] = $queried ? array( $queried->taxonomy, $queried->term_id ) : null;
	foreach ( $cases as $markup ) {
		echo bin2hex( $markup ), "\t", bin2hex( $query ), "\t", bin2hex( wp_json_encode( $state ) ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
	}
}
$wpdb->query( 'ROLLBACK' );

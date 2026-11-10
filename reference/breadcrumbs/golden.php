<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints breadcrumbs blocks on requests over pages, categories and posts
// inserted for it and rolled back after. Lines are
//   site<TAB>hex(JSON posts, terms, post terms)
//   case<TAB>hex(markup)<TAB>hex(JSON request)<TAB>hex(html)

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

global $wpdb;
$wpdb->query( 'START TRANSACTION' );
$page   = static fn( $title, $parent = 0 ) => wp_insert_post( array( 'post_title' => $title, 'post_type' => 'page', 'post_status' => 'publish', 'post_parent' => $parent, 'post_date' => '2026-01-01 00:00:00', 'post_content' => 'one<!--nextpage-->two' ) );
$parent = $page( 'Parent' );
$child  = $page( 'Child & <em>co</em>', $parent );
$grand  = $page( 'Grand', $child );
$blog   = $page( 'Blog' );
$news   = wp_insert_term( 'News', 'category' )['term_id'];
$local  = wp_insert_term( 'Local <b>news</b>', 'category', array( 'parent' => $news, 'slug' => 'local' ) )['term_id'];
$make   = static fn( $title, $args ) => wp_insert_post( $args + array( 'post_title' => $title, 'post_status' => 'publish', 'post_date' => '2026-11-05 10:00:00' ) );
$x      = $make( 'Story <em>one</em>', array( 'post_category' => array( $local ), 'tags_input' => array( 'red' ), 'post_content' => 'a<!--nextpage-->b' ) );
$y      = $make( 'Tagged', array( 'tags_input' => array( 'blue' ), 'post_content' => 'common' ) );
$z      = $make( '', array( 'post_content' => 'Untitled common' ) );
$w      = $make( 'Second', array( 'post_category' => array( $local ) ) );
// Parents WordPress would refuse, written directly: a loop of pages, a
// page under a missing one, and a loop of categories with a post in it.
$loop_a = $page( 'Loop A' );
$loop_b = $page( 'Loop B', $loop_a );
$orphan = $page( 'Orphan' );
$wpdb->update( $wpdb->posts, array( 'post_parent' => $loop_b ), array( 'ID' => $loop_a ) );
$wpdb->update( $wpdb->posts, array( 'post_parent' => 999999 ), array( 'ID' => $orphan ) );
$ring_a = wp_insert_term( 'Ring A', 'category' )['term_id'];
$ring_b = wp_insert_term( 'Ring B', 'category', array( 'parent' => $ring_a ) )['term_id'];
$wpdb->update( $wpdb->term_taxonomy, array( 'parent' => $ring_b ), array( 'term_id' => $ring_a, 'taxonomy' => 'category' ) );
$ringed = $make( 'Ringed', array( 'post_category' => array( $ring_a ) ) );
clean_post_cache( $loop_a );
clean_post_cache( $orphan );
clean_term_cache( array( $ring_a, $ring_b ), 'category' );
wp_set_object_terms( $y, array(), 'category' );
clean_term_cache( array( $news, $local ), 'category' );
delete_option( 'category_children' );

$site = array( 'posts' => array(), 'terms' => array(), 'post_terms' => array() );
foreach ( array( 1, 2, $parent, $child, $grand, $blog, $x, $y, $z, $w, $loop_a, $loop_b, $orphan, $ringed ) as $id ) {
	$p               = get_post( $id );
	$site['posts'][] = array( $p->ID, $p->post_title, $p->post_type, $p->post_parent, $p->post_name );
	foreach ( array( 'category', 'post_tag', 'post_format' ) as $taxonomy ) {
		foreach ( wp_get_object_terms( $id, $taxonomy ) as $t ) {
			$site['post_terms'][] = array( $id, $taxonomy, $t->term_id );
		}
	}
}
foreach ( array( 'category', 'post_tag' ) as $taxonomy ) {
	foreach ( get_terms( array( 'taxonomy' => $taxonomy, 'hide_empty' => false ) ) as $t ) {
		$site['terms'][] = array( $t->term_id, $t->name, $t->slug, $t->parent, $taxonomy );
	}
}
echo "site\t", bin2hex( wp_json_encode( $site ) ), "\n";

$markups = array(
	'<!-- wp:breadcrumbs /-->',
	'<!-- wp:breadcrumbs {"showOnHomePage":true} /-->',
	'<!-- wp:breadcrumbs {"showHomeItem":false,"showCurrentItem":false} /-->',
	'<!-- wp:breadcrumbs {"separator":"\\\\ \\"·\\"","showOnHomePage":true,"className":"extra"} /-->',
	'<!-- wp:breadcrumbs {"separator":"\\"","showOnHomePage":true} /-->',
	'<!-- wp:breadcrumbs {"separator":"\\\\","showOnHomePage":true} /-->',
	'<!-- wp:breadcrumbs {"separator":"›","showOnHomePage":true} /-->',
);
$requests = array(
	array( '', array() ),
	array( 'paged=2', array() ),
	array( 's=hello+world', array() ),
	array( 's=one+two+three+four+five+six+seven+eight+nine+ten+eleven+%3Cb%3E', array() ),
	array( 's=common&paged=2', array() ),
	array( 'p=99999', array() ),
	array( 'm=2026', array() ),
	array( 'm=202611', array() ),
	array( 'm=20261105&paged=2', array() ),
	array( 'year=2026&monthnum=11&day=5', array() ),
	array( "cat=$local", array() ),
	array( "cat=$local&paged=2", array() ),
	array( 'tag=red', array() ),
	array( "p=$x", array() ),
	array( "p=$x&page=2", array() ),
	array( "p=$x&cpage=2", array() ),
	array( "p=$x&page=1&cpage=1", array() ),
	array( "cat=$local&paged=1", array() ),
	array( 'w=45', array() ),
	array( "page_id=$loop_a", array() ),
	array( "page_id=$orphan", array() ),
	array( "p=$ringed", array() ),
	array( "p=$y", array() ),
	array( "p=$z", array() ),
	array( "page_id=$grand", array() ),
	array( "page_id=$parent", array() ),
	array( '', array( 'show_on_front' => 'page', 'page_on_front' => $parent, 'page_for_posts' => $blog ) ),
	array( 'page=2', array( 'show_on_front' => 'page', 'page_on_front' => $parent, 'page_for_posts' => $blog ) ),
	array( 'page=1', array( 'show_on_front' => 'page', 'page_on_front' => $parent, 'page_for_posts' => $blog ) ),
	array( "page_id=$blog&paged=2", array( 'show_on_front' => 'page', 'page_on_front' => $parent, 'page_for_posts' => $blog ) ),
	array( "p=$x", array( 'show_on_front' => 'page', 'page_on_front' => $parent, 'page_for_posts' => $blog ) ),
	array( "p=$x", array( 'page_for_posts' => $blog ) ),
);
// One post a page, so that lists have a second page.
add_filter( 'pre_option_posts_per_page', static fn() => '1' );
// Not $request or $post: WordPress sets those globals.
foreach ( $requests as list( $query, $options ) ) {
	foreach ( array( 'show_on_front', 'page_on_front', 'page_for_posts' ) as $name ) {
		remove_all_filters( "pre_option_$name" );
	}
	foreach ( $options as $name => $value ) {
		add_filter( "pre_option_$name", static fn() => $value );
	}
	$_SERVER['REQUEST_URI'] = '/' . ( '' === $query ? '' : "?$query" );
	$GLOBALS['wp']          = new WP();
	wp( $query );
	$queried = get_queried_object();
	$vars    = array();
	foreach ( array( 'page', 'paged', 'cpage', 'year', 'monthnum', 'day', 'm' ) as $name ) {
		$vars[ $name ] = get_query_var( $name );
	}
	$state = array(
		'options'   => (object) $options,
		'uri'       => $_SERVER['REQUEST_URI'],
		'front'     => is_front_page(),
		'home'      => is_home(),
		'search'    => is_search() ? get_query_var( 's' ) : null,
		'not_found' => is_404(),
		'date'      => is_date(),
		'term'      => $queried instanceof WP_Term ? array( $queried->taxonomy, $queried->term_id ) : null,
		'paged'     => (int) get_query_var( 'paged' ),
		'post'      => $GLOBALS['post'] instanceof WP_Post ? $GLOBALS['post']->ID : 0,
		'vars'      => $vars,
	);
	foreach ( $markups as $markup ) {
		echo "case\t", bin2hex( $markup ), "\t", bin2hex( wp_json_encode( $state ) ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
	}
}
$wpdb->query( 'ROLLBACK' );

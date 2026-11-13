<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints do_blocks() for the recent posts widget in a legacy widget block
// (signed under the fixed salt `test-salt`), over posts inserted for it and
// rolled back after, on several requests. Lines are
//   hex(markup)<TAB>hex(request)<TAB>hex(state JSON)<TAB>hex(html)
// where the state holds the published posts and pages newest first (ID,
// title, local date, password, type) and the queried object's ID.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

global $wpdb;
$wpdb->query( 'START TRANSACTION' );
add_filter( 'salt', static fn() => 'test-salt' );

$insert = static function ( $title, $date, $extra = array() ) {
	return wp_insert_post(
		array_merge(
			array(
				'post_title'    => $title,
				'post_status'   => 'publish',
				'post_date'     => $date,
				'post_date_gmt' => $date,
			),
			$extra
		)
	);
};
$insert( 'Older', '2026-01-01 09:00:00' );
// A post with no title, content or excerpt is refused, so these have content.
$insert( '', '2026-02-01 09:00:00', array( 'post_content' => 'x' ) );
$insert( '0', '2026-03-01 09:00:00', array( 'post_content' => 'x' ) );
$insert( 'Secret', '2026-04-01 09:00:00', array( 'post_password' => 'pw' ) );
$insert( 'Private one', '2026-05-01 09:00:00', array( 'post_status' => 'private' ) );
$insert( 'Draft', '2026-05-02 09:00:00', array( 'post_status' => 'draft' ) );
$sticky = $insert( 'Sticky & <b>bold</b> "quoted"', '2026-06-01 09:00:00' );
stick_post( $sticky );

function widget( $instance ) {
	$serialized = serialize( $instance );
	$attrs      = array(
		'idBase'   => 'recent-posts',
		'instance' => array(
			'encoded' => base64_encode( $serialized ),
			'hash'    => wp_hash( $serialized ),
		),
	);
	return '<!-- wp:legacy-widget ' . wp_json_encode( $attrs ) . ' /-->';
}
$cases = array(
	widget( array() ),
	widget( array( 'number' => 2 ) ),
	widget( array( 'number' => 0 ) ),
	widget( array( 'number' => 'abc' ) ),
	widget( array( 'number' => -3 ) ),
	widget( array( 'number' => '20' ) ),
	widget( array( 'number' => '99999999999' ) ),
	widget( array( 'show_date' => true ) ),
	widget( array( 'show_date' => '0', 'number' => 1 ) ),
	widget( array( 'title' => '0', 'number' => 1 ) ),
	widget( array( 'title' => 'Fresh & <i>new</i>', 'show_date' => 1, 'number' => 3 ) ),
);

$posts = $wpdb->get_results( "SELECT ID, post_title, post_date, post_password, post_type FROM $wpdb->posts WHERE post_type IN ( 'post', 'page' ) AND post_status = 'publish' ORDER BY post_date DESC, ID DESC" );
$state = array( 'posts' => array_map( static fn( $p ) => array( (int) $p->ID, $p->post_title, $p->post_date, $p->post_password, $p->post_type ), $posts ) );

// Not $request: WordPress sets that global.
foreach ( array( '', 'p=1', "p=$sticky" ) as $query ) {
	$GLOBALS['wp'] = new WP();
	wp( $query );
	$state['queried'] = get_queried_object_id();
	foreach ( $cases as $markup ) {
		echo bin2hex( $markup ), "\t", bin2hex( $query ), "\t", bin2hex( wp_json_encode( $state ) ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
	}
}

// With no published post the widget prints nothing.
$wpdb->query( "UPDATE $wpdb->posts SET post_status = 'draft' WHERE post_type = 'post'" );
// WP_Query caches its results until posts change through the API.
wp_cache_flush();
$GLOBALS['wp'] = new WP();
wp( '' );
$state = array(
	'posts'   => array(),
	'queried' => 0,
);
echo bin2hex( $cases[0] ), "\t", '', "\t", bin2hex( wp_json_encode( $state ) ), "\t", bin2hex( do_blocks( $cases[0] ) ), "\n";
$wpdb->query( 'ROLLBACK' );

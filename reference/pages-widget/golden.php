<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints do_blocks() for the pages widget in a legacy widget block (signed
// under the fixed salt `test-salt`), over a tree of pages inserted for it
// and rolled back after, on several requests, then again with a static
// front page and a posts page. Lines are
//   hex(markup)<TAB>hex(request)<TAB>hex(state JSON)<TAB>hex(html)
// where the state holds the posts, pages and attachments (ID, title,
// parent, menu order, password, local date, name, type, status, local
// modified date), the
// reading options and the request's flags and queried object.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

global $wpdb;
$wpdb->query( 'START TRANSACTION' );
add_filter( 'salt', static fn() => 'test-salt' );

$day      = 0;
$add_page = static function ( $title, $extra = array() ) use ( &$day ) {
	++$day;
	$date = sprintf( '2026-01-%02d 09:00:00', $day );
	return wp_insert_post(
		array_merge(
			array(
				'post_title'    => $title,
				'post_content'  => 'x',
				'post_type'     => 'page',
				'post_status'   => 'publish',
				'post_date'     => $date,
				'post_date_gmt' => $date,
			),
			$extra
		)
	);
};
$alpha      = $add_page( 'Alpha', array( 'menu_order' => 2 ) );
$child      = $add_page( 'Alpha child', array( 'post_parent' => $alpha ) );
$grandchild = $add_page( 'Grandchild', array( 'post_parent' => $child ) );
$second     = $add_page( 'Alpha second', array( 'post_parent' => $alpha, 'menu_order' => 1 ) );
$beta       = $add_page( 'Beta', array( 'menu_order' => 1 ) );
$zed        = $add_page( 'Zed & <b>bold</b> "quoted"' );
$untitled   = $add_page( '' );
$secret     = $add_page( 'Secret', array( 'menu_order' => 3, 'post_password' => 'pw' ) );
$add_page( 'Private one', array( 'post_status' => 'private' ) );
$draft  = $add_page( 'Draft', array( 'post_status' => 'draft' ) );
$orphan = $add_page( 'Orphan', array( 'post_parent' => $draft ) );
$attachment = wp_insert_attachment(
	array(
		'post_title'     => 'Picture',
		'post_mime_type' => 'image/png',
		'post_status'    => 'inherit',
	),
	false,
	$child
);

// Modified dates in the opposite order to the IDs.
$wpdb->query( "UPDATE $wpdb->posts SET post_modified = DATE_SUB( '2026-12-31 09:00:00', INTERVAL ID DAY ) WHERE post_type = 'page'" );

function widget( $instance ) {
	$serialized = serialize( $instance );
	$attrs      = array(
		'idBase'   => 'pages',
		'instance' => array(
			'encoded' => base64_encode( $serialized ),
			'hash'    => wp_hash( $serialized ),
		),
	);
	return '<!-- wp:legacy-widget ' . wp_json_encode( $attrs ) . ' /-->';
}
$top   = implode( ',', array( 2, $alpha, $beta, $zed, $untitled, $secret, $orphan ) );
$cases = array(
	widget( array() ),
	widget( array( 'sortby' => 'post_title' ) ),
	widget( array( 'sortby' => 'ID' ) ),
	widget( array( 'sortby' => 'menu_order' ) ),
	widget( array( 'sortby' => 'post_date, ID' ) ),
	widget( array( 'sortby' => 'post_modified_gmt' ) ),
	widget( array( 'sortby' => 'modified_gmt' ) ),
	widget( array( 'sortby' => 'bogus' ) ),
	widget( array( 'sortby' => '0' ) ),
	widget( array( 'sortby' => "post_title\0" ) ),
	widget( array( 'sortby' => 'post_title, post_title' ) ),
	widget( array( 'sortby' => ' , ' ) ),
	widget( array( 'title' => 'Site & <i>map</i>', 'sortby' => 'post_title' ) ),
	widget( array( 'title' => '0', 'exclude' => (string) $alpha ) ),
	widget( array( 'exclude' => " $beta , x$zed;" ) ),
	widget( array( 'exclude' => $top ) ),
	widget( array( 'exclude' => "$top,$child,$grandchild,$second" ) ),
);

$state = static function () use ( $wpdb ) {
	$posts = $wpdb->get_results( "SELECT ID, post_title, post_parent, menu_order, post_password, post_date, post_name, post_type, post_status, post_modified FROM $wpdb->posts WHERE post_type IN ( 'post', 'page', 'attachment' ) ORDER BY ID" );
	return array(
		'posts'   => array_map( static fn( $p ) => array( (int) $p->ID, $p->post_title, (int) $p->post_parent, (int) $p->menu_order, $p->post_password, $p->post_date, $p->post_name, $p->post_type, $p->post_status, $p->post_modified ), $posts ),
		'options' => array(
			'show_on_front'  => get_option( 'show_on_front' ),
			'page_on_front'  => get_option( 'page_on_front' ),
			'page_for_posts' => get_option( 'page_for_posts' ),
		),
	);
};
$run = static function ( $queries ) use ( $cases, $state ) {
	foreach ( $queries as $query ) {
		$GLOBALS['wp'] = new WP();
		wp( $query );
		$now            = $state();
		$now['singular'] = is_singular();
		$now['home']     = is_home();
		$now['queried']  = get_queried_object_id();
		foreach ( $cases as $markup ) {
			echo bin2hex( $markup ), "\t", bin2hex( $query ), "\t", bin2hex( wp_json_encode( $now ) ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
		}
	}
};
$run( array( '', "page_id=$child", "page_id=$alpha", 'p=1', "attachment_id=$attachment" ) );

update_option( 'show_on_front', 'page' );
update_option( 'page_on_front', $beta );
update_option( 'page_for_posts', $zed );
$run( array( '', "page_id=$zed", 'p=1' ) );
$wpdb->query( 'ROLLBACK' );

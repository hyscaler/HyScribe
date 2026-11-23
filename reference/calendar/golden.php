<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints do_blocks() for calendar block markup over the default install
// with posts inserted around its own and rolled back after, on several
// requests, permalink structures and week starts, without the elements
// support, which is checked on its own. Lines are
//   hex(markup)<TAB>hex(request)<TAB>hex(permalink structure)<TAB>start_of_week<TAB>time()<TAB>hex(JSON published post dates)<TAB>hex(html)
// The options are set by filter.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

remove_filter( 'render_block', 'wp_render_elements_class_name', 10 );
remove_filter( 'render_block_data', 'wp_render_elements_support_styles', 10 );

global $wpdb;
$wpdb->query( 'START TRANSACTION' );
add_filter( 'salt', static fn() => 'test-salt' );
foreach ( array( '2026-09-03 10:00:00', '2026-09-30 23:59:59', '2026-11-02 00:00:00', '2025-02-14 12:00:00' ) as $date ) {
	wp_insert_post( array( 'post_title' => "At $date", 'post_status' => 'publish', 'post_date' => $date, 'post_date_gmt' => $date ) );
}
wp_insert_post( array( 'post_title' => 'Draft', 'post_status' => 'draft', 'post_date' => '2026-08-08 08:00:00' ) );
$dates = $wpdb->get_col( "SELECT post_date FROM $wpdb->posts WHERE post_type = 'post' AND post_status = 'publish' ORDER BY post_date" );

function calendar( $attrs ) {
	$json = $attrs ? ' ' . wp_json_encode( $attrs, JSON_UNESCAPED_SLASHES ) : '';
	return "<!-- wp:calendar$json /-->";
}
// The calendar widget in a legacy widget block, signed under `test-salt`.
function widget( $instance ) {
	$serialized = serialize( $instance );
	$attrs      = array(
		'idBase'   => 'calendar',
		'instance' => array(
			'encoded' => base64_encode( $serialized ),
			'hash'    => wp_hash( $serialized ),
		),
	);
	return '<!-- wp:legacy-widget ' . wp_json_encode( $attrs ) . ' /-->';
}
$cases = array(
	calendar( array() ),
	calendar( array( 'month' => 2, 'year' => 2025 ) ),
	calendar( array( 'textColor' => 'contrast', 'backgroundColor' => 'base' ) ),
	calendar( array( 'style' => array( 'color' => array( 'text' => '#123', 'background' => '#fed' ), 'elements' => array( 'link' => array( 'color' => array( 'text' => '#f00' ) ) ) ) ) ),
	calendar( array( 'align' => 'center', 'className' => 'extra', 'fontSize' => 'small' ) ),
	// Only PHP's first calendar widget is `#calendar_wrap`.
	widget( array() ),
	widget( array( 'title' => 'When & <b>where</b>' ) ),
);
// Not $request: WordPress sets that global.
$queries = array( '', 'm=202610', 'year=2026&monthnum=9', 'm=2026', 'm=2026&w=44', 'm=20261027', 'p=1', 'year=2025&monthnum=2', 'year=2026&monthnum=13', 'monthnum=9', 'year=2026', 'm=2026&w=4', 'm=2026&w=5', 'year=2025&monthnum=12' );
// A structure with %year% alone leaves the block's month and year unused.
foreach ( array( '', '/%year%/%monthnum%/%postname%/', '/%postname%/', '/%year%/%postname%/' ) as $structure ) {
	remove_all_filters( 'pre_option_permalink_structure' );
	add_filter( 'pre_option_permalink_structure', static fn() => $structure );
	$GLOBALS['wp_rewrite']->init();
	foreach ( '' === $structure ? array( '1', '0', '3', '-1' ) : array( '1' ) as $start ) {
		remove_all_filters( 'pre_option_start_of_week' );
		add_filter( 'pre_option_start_of_week', static fn() => $start );
		foreach ( $queries as $query ) {
			$_GET = array();
			parse_str( $query, $_GET );
			$GLOBALS['wp'] = new WP();
			wp( $query );
			wp_cache_delete( 'get_calendar', 'calendar' );
			foreach ( $cases as $markup ) {
				echo bin2hex( $markup ), "\t", bin2hex( $query ), "\t", bin2hex( $structure ), "\t", $start, "\t", time(), "\t", bin2hex( wp_json_encode( $dates ) ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
			}
		}
	}
}
// With no published post, get_calendar() prints nothing.
$wpdb->query( "UPDATE $wpdb->posts SET post_status = 'draft' WHERE post_type = 'post'" );
remove_all_filters( 'pre_option_permalink_structure' );
remove_all_filters( 'pre_option_start_of_week' );
$GLOBALS['wp'] = new WP();
wp( '' );
wp_cache_delete( 'get_calendar', 'calendar' );
$markup = widget( array() );
echo bin2hex( $markup ), "\t", '', "\t", '', "\t", get_option( 'start_of_week' ), "\t", time(), "\t", bin2hex( wp_json_encode( array() ) ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
$wpdb->query( 'ROLLBACK' );

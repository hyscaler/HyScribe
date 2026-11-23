<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints do_blocks() for archives block markup over the default install,
// for several requests, permalink structures and week starts. Lines are
//   hex(markup)<TAB>wp_unique_id() counter<TAB>hex(request)<TAB>hex(permalink structure)<TAB>start_of_week<TAB>get_the_ID()<TAB>hex(html)
// The options are set by filter; the database is only read.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

function archives( $attrs ) {
	$json = $attrs ? ' ' . wp_json_encode( $attrs, JSON_UNESCAPED_SLASHES ) : '';
	return "<!-- wp:archives$json /-->";
}

$cases = array(
	archives( array() ),
	archives( array( 'showPostCounts' => true ) ),
	archives( array( 'type' => 'yearly' ) ),
	archives( array( 'type' => 'yearly', 'showPostCounts' => true ) ),
	archives( array( 'type' => 'daily' ) ),
	archives( array( 'type' => 'daily', 'showPostCounts' => true ) ),
	archives( array( 'type' => '' ) ),
	archives( array( 'type' => 'nope' ) ),
	archives( array( 'type' => 'nope', 'displayAsDropdown' => true ) ),
	archives( array( 'displayAsDropdown' => true ) ),
	archives( array( 'displayAsDropdown' => true, 'showLabel' => false ) ),
	archives( array( 'displayAsDropdown' => true, 'showPostCounts' => true, 'type' => 'yearly' ) ),
	archives( array( 'displayAsDropdown' => true, 'type' => 'daily' ) ),
	archives( array( 'displayAsDropdown' => true, 'type' => '' ) ),
	archives( array( 'align' => 'wide', 'className' => 'extra', 'fontSize' => 'small' ) ),
	archives( array( 'displayAsDropdown' => true, 'style' => array( 'typography' => array( 'lineHeight' => '2' ), 'spacing' => array( 'padding' => array( 'top' => '1em' ) ) ) ) ),
	archives( array( 'showPostCounts' => 1, 'displayAsDropdown' => 0 ) ),
	archives( array( 'type' => 'weekly' ) ),
	archives( array( 'type' => 'weekly', 'showPostCounts' => true ) ),
	archives( array( 'type' => 'weekly', 'displayAsDropdown' => true ) ),
	archives( array( 'type' => 'postbypost' ) ),
	archives( array( 'type' => 'postbypost', 'showPostCounts' => true, 'displayAsDropdown' => true ) ),
	archives( array( 'type' => 'alpha' ) ),
	archives( array( 'type' => 'alpha', 'displayAsDropdown' => true ) ),
);
// Not $request: WordPress sets that global.
$queries    = array( '', 'year=2026', 'm=202610', 'year=2026&monthnum=10', 'year=2026&monthnum=10&day=27', 'year=2025', 'cat=1', 'p=1', 'year=2026&w=44', 'm=2026&w=44', 'year=2026&w=43' );
$structures = array( '', '/%year%/%monthnum%/%postname%/', '/%postname%/', '/archives/%post_id%' );

// Weeks from Monday (the default install's), Sunday, Tuesday (the day
// of its post) and Wednesday, the others only with plain permalinks.
foreach ( $structures as $structure ) {
	remove_all_filters( 'pre_option_permalink_structure' );
	add_filter( 'pre_option_permalink_structure', static fn() => $structure );
	$GLOBALS['wp_rewrite']->init();
	foreach ( '' === $structure ? array( '1', '0', '2', '3' ) : array( '1' ) as $start ) {
		remove_all_filters( 'pre_option_start_of_week' );
		add_filter( 'pre_option_start_of_week', static fn() => $start );
		foreach ( $queries as $query ) {
			$GLOBALS['wp'] = new WP();
			wp( $query );
			foreach ( $cases as $markup ) {
				$counter = (int) wp_unique_id();
				echo bin2hex( $markup ), "\t", $counter, "\t", bin2hex( $query ), "\t", bin2hex( $structure ), "\t", $start, "\t", (int) get_the_ID(), "\t", bin2hex( do_blocks( $markup ) ), "\n";
			}
		}
	}
}

<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints do_blocks() for archives block markup over the default install,
// for several requests and permalink structures. Lines are
//   hex(markup)<TAB>wp_unique_id() counter<TAB>hex(request)<TAB>hex(permalink structure)<TAB>hex(html)
// The structure is set by filter; the database is only read.

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
);
// Not $request: WordPress sets that global.
$queries    = array( '', 'year=2026', 'm=202610', 'year=2026&monthnum=10', 'year=2026&monthnum=10&day=27', 'year=2025', 'cat=1', 'p=1' );
$structures = array( '', '/%year%/%monthnum%/%postname%/', '/%postname%/', '/archives/%post_id%' );

foreach ( $structures as $structure ) {
	remove_all_filters( 'pre_option_permalink_structure' );
	add_filter( 'pre_option_permalink_structure', static fn() => $structure );
	$GLOBALS['wp_rewrite']->init();
	foreach ( $queries as $query ) {
		$GLOBALS['wp'] = new WP();
		wp( $query );
		foreach ( $cases as $markup ) {
			$counter = (int) wp_unique_id();
			echo bin2hex( $markup ), "\t", $counter, "\t", bin2hex( $query ), "\t", bin2hex( $structure ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
		}
	}
}

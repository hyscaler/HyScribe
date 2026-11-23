<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// get_page_by_path() over pages inserted for it and rolled back after, as
//   hex(path)<TAB>hex(state JSON)
// for crates/hyscribe-http/tests/golden/page-path.tsv: the candidate rows
// (pages and attachments named one of the path's parts, as
// get_page_by_path() selects them) and the ID it returns, 0 for none.

require '/var/www/html/wp-load.php';

global $wpdb;
$wpdb->query( 'START TRANSACTION' );
$page = static fn( $name, $parent = 0, $type = 'page', $status = 'publish' ) => wp_insert_post( array( 'post_title' => $name, 'post_name' => $name, 'post_parent' => $parent, 'post_type' => $type, 'post_status' => $status ) );
$parent = $page( 'parent' );
$child  = $page( 'child', $parent );
$page( 'grandchild', $child );
$page( 'child', 0, 'attachment', 'inherit' );
$other = $page( 'other' );
$page( 'leaf', $other );
$page( 'leaf' );
$page( 'draft-page', 0, 'page', 'draft' );
$page( 'solo', 0, 'attachment', 'inherit' );
$w1 = $page( 'w1' );
$w2 = $page( 'w2', $w1 );
$w3 = $page( 'w3', $w2 );
$page( 'w4', $w3 );
// An attachment before a page of the same name, which wp_insert_post()
// would rename.
$wpdb->insert( $wpdb->posts, array( 'post_title' => 'dup', 'post_name' => 'dup', 'post_type' => 'attachment', 'post_status' => 'inherit', 'post_content' => '', 'post_excerpt' => '', 'to_ping' => '', 'pinged' => '', 'post_content_filtered' => '' ) );
$wpdb->insert( $wpdb->posts, array( 'post_title' => 'dup', 'post_name' => 'dup', 'post_type' => 'page', 'post_status' => 'publish', 'post_content' => '', 'post_excerpt' => '', 'to_ping' => '', 'pinged' => '', 'post_content_filtered' => '' ) );

$paths = array(
	'parent', 'parent/child', 'parent/child/grandchild', 'child', 'grandchild', 'child/grandchild', 'other/child', 'parent/grandchild',
	'leaf', 'other/leaf', 'parent/leaf', '/parent/child/', 'Parent/Child', 'parent%2Fchild', 'parent child', 'sample-page', 'nope',
	'draft-page', '', '/', 'parent//child', 'other/leaf/extra', 'solo', 'w1/w2/w3/w4', 'w1/w3/w2/w4', 'dup',
);
foreach ( $paths as $path ) {
	$parts = array_map( 'sanitize_title_for_query', explode( '/', trim( str_replace( array( '%2F', '%20' ), array( '/', ' ' ), rawurlencode( urldecode( $path ) ) ), '/' ) ) );
	$in    = "'" . implode( "','", esc_sql( $parts ) ) . "'";
	$rows  = $wpdb->get_results( "SELECT ID, post_name, post_parent, post_type FROM $wpdb->posts WHERE post_name IN ($in) AND post_type IN ('page','attachment')", ARRAY_A );
	$found = get_page_by_path( $path );
	echo bin2hex( $path ), "\t", bin2hex( wp_json_encode( array( 'rows' => $rows, 'id' => $found ? $found->ID : 0 ) ) ), "\n";
}
$wpdb->query( 'ROLLBACK' );

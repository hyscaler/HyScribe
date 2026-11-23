<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// How WP_Query::parse_query() reads the `cat` query var, over categories
// inserted for it and rolled back after, as
//   hex(cat)<TAB>hex(state JSON)
// for crates/hyscribe-http/tests/golden/category-query.tsv: the stored
// category hierarchy, whether the query is a category archive or the home
// list, and the category IDs in and out, children included as
// WP_Tax_Query::clean_query() adds them.

require '/var/www/html/wp-load.php';

global $wpdb;
$wpdb->query( 'START TRANSACTION' );
$parent = wp_insert_term( 'Parent', 'category' )['term_id'];
$child  = wp_insert_term( 'Child', 'category', array( 'parent' => $parent ) )['term_id'];
$grand  = wp_insert_term( 'Grand', 'category', array( 'parent' => $child ) )['term_id'];
$other  = wp_insert_term( 'Other', 'category' )['term_id'];
_get_term_hierarchy( 'category' );
$hierarchy = maybe_serialize( get_option( 'category_children' ) );

$cases = array(
	"$parent", "$child", "$grand", "-$parent", "-$child", "$parent,$other", "$other $parent", "-$parent,1", "1,-$parent", "-$parent,-$other",
	'abc', '0', '', '00', '1', '-1', '999', '-999', "%2C$parent", "$parent%20$other", "12abc", " $parent", "$parent,,$other", "$parent\t$other",
	"$parent,$parent", '1e3', '-0', "+$parent",
);
foreach ( $cases as $cat ) {
	$query = new WP_Query();
	$query->parse_query( array( 'cat' => $cat ) );
	$in     = array();
	$not_in = array();
	foreach ( $query->tax_query->queries as $clause ) {
		if ( ! is_array( $clause ) || empty( $clause['terms'] ) ) {
			continue;
		}
		$terms = array();
		foreach ( $clause['terms'] as $term ) {
			$terms   = array_merge( $terms, get_term_children( $term, 'category' ) );
			$terms[] = $term;
		}
		if ( 'NOT IN' === $clause['operator'] ) {
			$not_in = array_merge( $not_in, $terms );
		} else {
			$in = array_merge( $in, $terms );
		}
	}
	$in     = array_values( array_unique( array_map( 'intval', $in ) ) );
	$not_in = array_values( array_unique( array_map( 'intval', $not_in ) ) );
	sort( $in );
	sort( $not_in );
	$state = array(
		'hierarchy' => $hierarchy,
		'category'  => $query->is_category,
		'home'      => $query->is_home,
		'in'        => $in,
		'not_in'    => $not_in,
	);
	echo bin2hex( $cat ), "\t", bin2hex( wp_json_encode( $state ) ), "\n";
}
$wpdb->query( 'ROLLBACK' );

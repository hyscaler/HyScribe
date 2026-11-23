<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The terms WP_Query::parse_search() finds in a search string, as
//   hex(search)<TAB>hex(JSON list of terms)
// for crates/hyscribe-render/tests/golden/search-terms.tsv. Only reads.

require '/var/www/html/wp-load.php';

final class Search_Terms extends WP_Query {
	public function terms( $search ) {
		$query_vars = array( 's' => $search );
		$this->parse_search( $query_vars );
		return $query_vars['search_terms'];
	}
}

$searches = array(
	'hello', 'hello world', 'a,b+c	d', 'a b', 'plum x', 'x', '-', 'plum -', 'The cat', 'THE', 'about it', '0', '0 plum', '00',
	'"plum pie"', '"plum pie" tart', '"unclosed plum', '"unclosed plum ', '" open', 'pie "', 'tart "plum pie', '"" plum', '""', '" spaced "', "'quoted'", "''x''",
	'"a"', '"ab"', 'plum"pie"', 'pie"', '"plum"pie', 'one two three four five six seven eight nine', 'one two three four five six seven eight nine ten',
	'a b c d e f g h i j k', 'the the the', '   ', ',+', ' plum ', "plum\npie", "\r\n", 'naïve café', 'Ünïcode', 'İstanbul', 'plum\\"pie', 'back\\\\slash',
	'trailing\\', 'p\\0q', '"quoted, with comma" and+plus', '+plum', ',plum,', "\tplum\t", '%_', 'www.example.com', 'WWW',
);
foreach ( $searches as $search ) {
	$terms = ( new Search_Terms() )->terms( $search );
	echo bin2hex( $search ), "\t", bin2hex( wp_json_encode( $terms ) ), "\n";
}

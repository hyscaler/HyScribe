<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// redirect_canonical()'s last clean up of a query string, copied from
// wp-includes/canonical.php (WordPress 7.1.3), as
//   hex(query)<TAB>hex(cleaned)
// for crates/hyscribe-http/tests/golden/clean-query.tsv:
//   php reference/canonical/clean-golden.php > crates/hyscribe-http/tests/golden/clean-query.tsv

$punctuation_pattern = implode(
	'|',
	array_map(
		'preg_quote',
		array( ' ', '%20', '!', '%21', '"', '%22', "'", '%27', '(', '%28', ')', '%29', ',', '%2C', '.', '%2E', ';', '%3B', '{', '%7B', '}', '%7D', '%E2%80%9C', '%E2%80%9D' )
	)
);

function clean( $query, $punctuation_pattern ) {
	$query = preg_replace( "#((^|&)(p|page_id|cat|tag)=[^&]*?)($punctuation_pattern)+$#", '$1', $query );
	return trim( preg_replace( '#(^|&)(p|page_id|cat|tag)=?(&|$)#', '&', $query ), '&' );
}

$queries = array(
	'', 'p=1', 'p=', 'p', 'p=&cat=&x=1', 'cat=&p=1', 'p=1&cat=', 'x=1&p=1.', 'p=1%20', 'p=1)', 'p=1%E2%80%9D', 'p=1%E2%80%9C%E2%80%9D', 'p=.',
	'x=1&tag=', '&p=&cat=', 'p=1.&x=1', 'pp=', 'cat=1%2e', 'cat=1..%2E.', 'x=1&page_id=2 ', 'page_idx=', 'page_id=&p=&cat=&tag=',
	'p=&&cat=', '&&p=', 'p=1&', 'x=1&p=', 'p&cat', 'tag=a!', 'tag=a!b', 'cat=%2C1', 'x=a.', 'p=1;%3B{%7B}%7D', "p=1'", 'p=1"', 'p=1(',
	'p=1%2', 'p=1%2C%2', 'cat=1&cat=', 'feed=rss2&p=1.', 'p=é.', 'p=1 ', 'p=1+', 'cat=-1,', 'cat=,', '&', '&&', 'p=1&&', '=&p=',
);
foreach ( $queries as $query ) {
	echo bin2hex( $query ), "\t", bin2hex( clean( $query, $punctuation_pattern ) ), "\n";
}

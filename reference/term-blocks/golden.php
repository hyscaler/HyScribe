<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints the term name and term count blocks on a category archive, with
// the term's name and count set in memory, and on pages with no term.
// Lines are
//   hex(query)<TAB>hex(name)<TAB>count<TAB>hex(markup)<TAB>hex(html)
// The names avoid query variables, which wp() makes globals. Nothing is
// written.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

$markups = array(
	'<!-- wp:term-name /-->',
	'<!-- wp:term-name {"level":2,"isLink":true} /-->',
	'<!-- wp:term-name {"level":"3"} /-->',
	'<!-- wp:term-name {"level":0.0} /-->',
	'<!-- wp:term-name {"level":null,"isLink":"yes"} /-->',
	'<!-- wp:term-name {"textAlign":"center","style":{"elements":{"link":{"color":{"text":"red"}}}},"align":"wide","fontSize":"small"} /-->',
	'<!-- wp:term-count /-->',
	'<!-- wp:term-count {"bracketType":"none"} /-->',
	'<!-- wp:term-count {"bracketType":"square"} /-->',
	'<!-- wp:term-count {"bracketType":"curly"} /-->',
	'<!-- wp:term-count {"bracketType":"angle","className":"x"} /-->',
	'<!-- wp:term-count {"bracketType":"other"} /-->',
	'<!-- wp:term-count {"bracketType":true} /-->',
);
$terms   = array( array( 'Uncategorized', 1 ), array( 'Tom & <b>"Jerry"</b>', 0 ), array( 'Big', 12345 ) );

foreach ( array( 'cat=1', '', 'author=1' ) as $query ) {
	foreach ( $terms as list( $term_name, $term_count ) ) {
		$set = static function ( $term ) use ( $term_name, $term_count ) {
			$term->name  = $term_name;
			$term->count = $term_count;
			return $term;
		};
		add_filter( 'get_term', $set );
		$GLOBALS['wp'] = new WP();
		wp( $query );
		foreach ( $markups as $markup ) {
			$html = render_block( parse_blocks( $markup )[0] );
			echo bin2hex( $query ), "\t", bin2hex( $term_name ), "\t", $term_count, "\t", bin2hex( $markup ), "\t", bin2hex( $html ), "\n";
		}
		remove_filter( 'get_term', $set );
	}
}

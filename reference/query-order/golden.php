<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Seeded random orders of WP_Query over the default install, read only.
// Run in the PHP container; lines are seed<TAB>post IDs, comma separated.
require '/var/www/html/wp-load.php';

foreach ( range( 0, 40 ) as $seed ) {
	$query = new WP_Query(
		array(
			'post_type'      => array( 'post', 'page' ),
			'orderby'        => "RAND($seed)",
			'posts_per_page' => 10,
			'fields'         => 'ids',
		)
	);
	echo $seed, "\t", implode( ',', $query->posts ), "\n";
}

<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Run by export.sh inside the PHP WordPress container: prints the block
// patterns the active theme registers, as JSON {slug: {title, content}},
// with their PHP already run (en_US). Theme ports store this output, so
// HyScribe never needs PHP for patterns. The footer credit names HyScribe
//.

require '/var/www/html/wp-load.php';

$patterns = array();
foreach ( WP_Block_Patterns_Registry::get_instance()->get_all_registered() as $pattern ) {
	$theme_prefix = get_stylesheet() . '/';
	if ( ! str_starts_with( $pattern['name'], $theme_prefix ) ) {
		continue;
	}
	$patterns[ $pattern['name'] ] = array(
		'title'   => $pattern['title'],
		'content' => str_replace(
			'Designed with <a href="https://wordpress.org" rel="nofollow">WordPress</a>',
			'Designed with <a href="https://github.com/hyscaler/HyScribe" rel="nofollow">HyScribe</a>',
			$pattern['content']
		),
	);
}
ksort( $patterns );
echo wp_json_encode( $patterns, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE ), "\n";

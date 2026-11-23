<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Records WP_Rewrite::rewrite_rules() for one permalink setup, as lines of
//   regex<TAB>query
// The options come from filters set before WordPress loads, so taxonomies
// register their permastructs as on a site with these settings, and
// nothing is written to the database. Run in the PHP container:
//   docker exec -i hyscribe-default-install-php php /dev/stdin <structure> [category base] [tag base] [page on front] < reference/rewrite/rules-golden.php
// reference/rewrite/golden.sh runs it for every setup.
$options = array(
	'permalink_structure' => $argv[1] ?? '',
	'category_base'       => $argv[2] ?? '',
	'tag_base'            => $argv[3] ?? '',
);
if ( ! empty( $argv[4] ) ) {
	$options['page_on_front'] = $argv[4];
	$options['show_on_front'] = 'page';
}
$GLOBALS['wp_filter'] = array();
foreach ( $options as $name => $value ) {
	$GLOBALS['wp_filter'][ "pre_option_$name" ][10][] = array(
		'function'      => fn() => $value,
		'accepted_args' => 1,
	);
}
// As wp_rewrite_rules() builds them, which would also save them.
function hyscribe_rules() {
	$GLOBALS['wp_rewrite']->matches = 'matches';
	return $GLOBALS['wp_rewrite']->rewrite_rules();
}
$GLOBALS['wp_filter']['pre_option_rewrite_rules'][10][] = array(
	'function'      => 'hyscribe_rules',
	'accepted_args' => 1,
);
require '/var/www/html/wp-load.php';
foreach ( hyscribe_rules() as $regex => $query ) {
	echo $regex, "\t", $query, "\n";
}

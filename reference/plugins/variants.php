<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Must-use plugin for reference/plugins/golden.sh, through filters only:
// the only plugin listed is the one the test extension stands in for
// (the image's Akismet and Hello Dolly, and the must-use plugins, are
// hidden), and x13hidden gives user 1 an option that hides the Plugins
// list's Description column.
add_filter(
	'all_plugins',
	function ( $plugins ) {
		return array_intersect_key( $plugins, array( 'hyscribe-test/hyscribe-test.php' => true ) );
	}
);
add_filter( 'show_advanced_plugins', '__return_false' );
if ( 'x13hidden' === ( $_COOKIE['hyscribe_variant'] ?? '' ) ) {
	add_filter(
		'get_user_option_managepluginscolumnshidden',
		function () {
			return array( 'description' );
		}
	);
}

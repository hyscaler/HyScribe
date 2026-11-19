<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A must-use plugin for reference/pages/golden.sh. A request carrying
// X-HyScribe-Options: name=value&name=value (a query string) is served as
// on a site with those options. They come from option filters, and the
// rewrite rules of a permalink structure are built without being saved,
// so nothing is written to the database.
// Pages do not depend on the network: every outbound request fails, as
// HyScribe makes none. The navigation block's fallback does not
// create a post (WP_Navigation_Fallback) unless the options carry
// _fallback=1; names starting with _ are no options.
add_filter( 'pre_http_request', fn() => new WP_Error( 'http_request_failed', 'Offline.' ) );
$hyscribe_options = array();
if ( isset( $_SERVER['HTTP_X_HYSCRIBE_OPTIONS'] ) ) {
	parse_str( $_SERVER['HTTP_X_HYSCRIBE_OPTIONS'], $hyscribe_options );
}
if ( empty( $hyscribe_options['_fallback'] ) ) {
	add_filter( 'wp_navigation_should_create_fallback', '__return_false' );
}
foreach ( $hyscribe_options as $hyscribe_name => $hyscribe_value ) {
	if ( '_' !== $hyscribe_name[0] ) {
		add_filter( "pre_option_$hyscribe_name", fn() => $hyscribe_value );
	}
}
if ( isset( $hyscribe_options['permalink_structure'] ) ) {
	add_filter(
		'pre_option_rewrite_rules',
		function () {
			$GLOBALS['wp_rewrite']->matches = 'matches';
			return $GLOBALS['wp_rewrite']->rewrite_rules();
		}
	);
}

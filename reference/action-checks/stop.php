<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A must-use plugin for reference/action-checks/golden.sh: a request that
// passes an action's checks is stopped before it writes, and answered
// `hyscribe-accepted` with an X-HyScribe-Write header. An admin request
// (wp-admin, admin-ajax.php) stops at its first query that writes, which the
// header shows; a REST request with a write method stops once its permission
// callback passes, before its callback. Cron is off and its schedule kept as
// it is, which every admin page view would otherwise write first; no one may
// post unfiltered HTML.
if ( ! defined( 'DISABLE_WP_CRON' ) ) {
	define( 'DISABLE_WP_CRON', true );
}
if ( ! defined( 'DISALLOW_UNFILTERED_HTML' ) ) {
	define( 'DISALLOW_UNFILTERED_HTML', true );
}
add_filter(
	'pre_update_option_cron',
	function ( $value, $old ) {
		return $old;
	},
	10,
	2
);
// Nor do the update checks of plugins and themes, which hs-admin does not
// make.
remove_action( 'admin_init', '_maybe_update_plugins' );
remove_action( 'admin_init', '_maybe_update_themes' );

function hyscribe_accepted( $what ) {
	while ( ob_get_level() ) {
		ob_end_clean();
	}
	header( 'Content-Type: text/plain' );
	header( 'X-HyScribe-Write: ' . preg_replace( '/\s+/', ' ', substr( $what, 0, 200 ) ) );
	echo 'hyscribe-accepted';
	exit;
}

add_filter(
	'query',
	function ( $query ) {
		$login = isset( $GLOBALS['pagenow'] ) && 'wp-login.php' === $GLOBALS['pagenow'];
		if ( ! is_admin() || $login || ! preg_match( '/^\s*(INSERT|UPDATE|DELETE|REPLACE)\b/i', $query ) ) {
			return $query;
		}
		// A transient is a cache the screens fill before any check: not
		// written, as if the write failed.
		if ( preg_match( "/`?\\w*_options`?\\s.*'_(site_)?transient_/s", $query ) ) {
			return '';
		}
		hyscribe_accepted( $query );
	}
);

add_filter(
	'rest_dispatch_request',
	function ( $result, $request ) {
		if ( in_array( $request->get_method(), array( 'POST', 'PUT', 'PATCH', 'DELETE' ), true ) ) {
			hyscribe_accepted( $request->get_route() );
		}
		return $result;
	},
	10,
	2
);

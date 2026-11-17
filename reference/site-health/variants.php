<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Must-use plugin for reference/site-health/golden.sh, through filters only.
// The Info tab lists what a HyScribe installation has: Twenty Twenty-Five
// alone and no PHP plugin (the image's other themes, Akismet and Hello
// Dolly, and the recording's must-use plugins are left out). No request
// leaves the server, as HyScribe makes none, so the update checks
// find nothing, and the wordpress.org reachability row is left out.
add_filter(
	'pre_http_request',
	function () {
		return new WP_Error( 'hyscribe_offline', 'HyScribe makes no calls to wordpress.org.' );
	}
);
add_filter(
	'debug_information',
	function ( $info ) {
		unset( $info['wp-core']['fields']['dotorg_communication'] );
		$info['wp-mu-plugins']['fields']       = array();
		$info['wp-plugins-active']['fields']   = array();
		$info['wp-plugins-inactive']['fields'] = array();
		$info['wp-themes-inactive']['fields']  = array();
		return $info;
	}
);

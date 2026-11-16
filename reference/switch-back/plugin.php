<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A must-use plugin for reference/switch-back/run.sh: PHP WordPress serves
// the database HyScribe wrote with no cron, no update checks and no calls
// to other sites, so every warning in its debug log is about the data.
// run.sh turns WP_DEBUG and WP_DEBUG_LOG on in wp-config.php, which
// defines WP_DEBUG before any plugin loads.
define( 'DISABLE_WP_CRON', true );

foreach ( array( 'update_core', 'update_plugins', 'update_themes' ) as $transient ) {
	add_filter(
		"pre_site_transient_$transient",
		function () {
			return (object) array(
				'updates'         => array(),
				'response'        => array(),
				'translations'    => array(),
				'version_checked' => $GLOBALS['wp_version'],
				'last_checked'    => time(),
			);
		}
	);
}

// The General screen's language list, which would ask WordPress.org.
add_filter(
	'translations_api',
	function () {
		return array( 'translations' => array() );
	}
);

add_filter(
	'pre_http_request',
	function () {
		return new WP_Error( 'switch_back_offline', 'No outbound requests during the switch-back suite.' );
	}
);

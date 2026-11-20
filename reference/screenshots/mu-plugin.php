<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A must-use plugin for reference/screenshots/golden.sh: PHP WordPress with
// HyScribe's answers where hs-admin differs on purpose. No WordPress update
// is offered (an open question: hs-admin shows none), and no plugin or theme update
// either: the site reaches no other server (the wp-config.php blocks them).
// Like HyScribe, it has no plugins but the must-use one and only the
// embedded block themes (reference/appearance/variants.php), makes no
// calls for the dashboard's Events and News (OQ pending: the Ajax request
// answers 501, as HyScribe's does), and knows a list of translations, which
// the General settings screen needs to show its Site Language row.
add_filter( 'all_plugins', '__return_empty_array' );
add_filter( 'show_advanced_plugins', '__return_false' );
add_filter( 'wp_prepare_themes_for_js', function ( $themes ) {
	return array_intersect_key( $themes, array_flip( array( 'twentytwentyfive', 'twentytwentyfour', 'twentytwentythree', 'twentytwentytwo' ) ) );
} );
add_filter( 'pre_site_transient_available_translations', function () {
	return array(
		'de_DE' => array(
			'language'     => 'de_DE',
			'english_name' => 'German',
			'native_name'  => 'Deutsch',
			'iso'          => array( 'de' ),
			'strings'      => array( 'continue' => 'Weiter' ),
		),
	);
} );
foreach ( array( 'dashboard-widgets', 'get-community-events' ) as $action ) {
	add_action( "wp_ajax_$action", function () {
		status_header( 501 );
		exit;
	}, 0 );
}
add_filter( 'pre_site_transient_update_core', function () {
	return (object) array(
		'updates'         => array(),
		'version_checked' => $GLOBALS['wp_version'],
		'last_checked'    => time(),
	);
} );
add_filter( 'pre_site_transient_update_plugins', function () {
	return (object) array(
		'last_checked' => time(),
		'checked'      => array(),
		'response'     => array(),
		'no_update'    => array(),
	);
} );
add_filter( 'pre_site_transient_update_themes', function () {
	return (object) array(
		'last_checked' => time(),
		'checked'      => array(),
		'response'     => array(),
		'no_update'    => array(),
	);
} );

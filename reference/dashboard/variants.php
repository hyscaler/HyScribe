<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Must-use plugin for reference/dashboard/golden.sh: the dashboard's
// variants, chosen by the hyscribe_variant cookie, through filters only.
$hyscribe_variant = $_COOKIE['hyscribe_variant'] ?? '';
$hyscribe_health  = array(
	'health'   => '{"good":5,"recommended":1,"critical":0}',
	'critical' => '{"good":1,"recommended":2,"critical":2}',
	'one'      => '{"good":9,"recommended":0,"critical":1}',
	'passing'  => '{"good":9,"recommended":0,"critical":0}',
);
if ( isset( $hyscribe_health[ $hyscribe_variant ] ) ) {
	add_filter(
		'pre_transient_health-check-site-status-result',
		function () use ( $hyscribe_health, $hyscribe_variant ) {
			return $hyscribe_health[ $hyscribe_variant ];
		}
	);
}
if ( 'layout' === $hyscribe_variant ) {
	add_filter( 'pre_option_blog_public', fn() => '0' );
	$hyscribe_user_options = array(
		'metaboxhidden_dashboard'   => array( 'dashboard_activity' ),
		'closedpostboxes_dashboard' => array( 'dashboard_right_now' ),
		'meta-box-order_dashboard'  => array(
			'side'   => 'dashboard_site_health,dashboard_primary',
			'normal' => 'dashboard_quick_press',
		),
	);
	foreach ( $hyscribe_user_options as $name => $value ) {
		add_filter( "get_user_option_{$name}", fn() => $value );
	}
	add_filter(
		'get_user_metadata',
		fn( $value, $object_id, $key ) => 'show_welcome_panel' === $key ? array( '0' ) : $value,
		10,
		3
	);
}
// The f9 variants: an admin email reminder just postponed or long ago, and
// a folded or never folded menu from the user's settings.
$hyscribe_lifespans = array(
	'f9remind' => time() + 3 * DAY_IN_SECONDS,
	'f9old'    => 1,
);
if ( isset( $hyscribe_lifespans[ $hyscribe_variant ] ) ) {
	add_filter( 'pre_option_admin_email_lifespan', fn() => $hyscribe_lifespans[ $hyscribe_variant ] );
}
$hyscribe_settings = array(
	'f9folded' => 'mfold=f',
	'f9unfold' => 'unfold=1&mfold=o',
);
if ( isset( $hyscribe_settings[ $hyscribe_variant ] ) ) {
	add_filter( 'get_user_option_user-settings', fn() => $hyscribe_settings[ $hyscribe_variant ] );
}

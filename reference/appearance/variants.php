<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Must-use plugin for reference/appearance/golden.sh, through filters only.
// Every request sees the themes and plugins a HyScribe installation has:
// the embedded block themes (the fixture child of 10.3 among them), and no PHP plugin (the image's Akismet and
// Hello Dolly, and these must-use plugins, are hidden). a5auto turns on the
// theme's automatic updates; a5nooptions and a5noswitch (g9nooptions and
// g9noswitch too) deny user 1 edit_theme_options and switch_themes, and
// a5hidden gives user options that hide columns of the Plugins and Export
// Personal Data lists and set how many they show.
add_filter( 'all_plugins', '__return_empty_array' );
add_filter( 'show_advanced_plugins', '__return_false' );
add_filter(
	'wp_prepare_themes_for_js',
	function ( $themes ) {
		$embedded = array(
			'twentytwentyfive'  => true,
			'twentytwentyfour'  => true,
			'twentytwentythree' => true,
			'twentytwentytwo'   => true,
			'hyscribe-fixture-child' => true,
		);
		return array_intersect_key( $themes, $embedded );
	}
);
$hyscribe_variant = $_COOKIE['hyscribe_variant'] ?? '';
if ( 'a5auto' === $hyscribe_variant ) {
	add_filter(
		'pre_option_auto_update_themes',
		function () {
			return array( 'twentytwentyfive' );
		}
	);
}
if ( 'a5hidden' === $hyscribe_variant ) {
	$hyscribe_user_options = array(
		'manageexport-personal-datacolumnshidden' => array( 'status' ),
		'export_personal_data_requests_per_page'  => '50',
		'managepluginscolumnshidden'              => array( 'description' ),
		'plugins_per_page'                        => '10',
	);
	foreach ( $hyscribe_user_options as $hyscribe_name => $hyscribe_value ) {
		add_filter(
			"get_user_option_$hyscribe_name",
			function () use ( $hyscribe_value ) {
				return $hyscribe_value;
			}
		);
	}
}
$hyscribe_denied = array(
	'a5nooptions' => 'edit_theme_options',
	'a5noswitch'  => 'switch_themes',
	'g9nooptions' => 'edit_theme_options',
	'g9noswitch'  => 'switch_themes',
);
if ( isset( $hyscribe_denied[ $hyscribe_variant ] ) ) {
	add_filter(
		'user_has_cap',
		function ( $allcaps ) use ( $hyscribe_denied, $hyscribe_variant ) {
			$allcaps[ $hyscribe_denied[ $hyscribe_variant ] ] = false;
			return $allcaps;
		},
		1
	);
}

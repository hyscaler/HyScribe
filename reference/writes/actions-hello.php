<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The write oracle's Hello Elementor version write (MVP 10.5, ADR 0023),
// loaded beside actions.php: hello_maybe_update_theme_version_in_db(),
// taken from the pinned theme's own functions.php (reference/themes/pin.sh,
// mounted by php.sh) and run as its after_setup_theme callback runs it in
// an admin request, over the option as each case leaves it. A stored version
// that is not older writes nothing, which the oracle does not record.

$hello_update = function () {
	$source = file_get_contents( get_theme_root() . '/hello-elementor/functions.php' );
	if ( ! defined( 'HELLO_ELEMENTOR_VERSION' ) ) {
		preg_match( "/define\( 'HELLO_ELEMENTOR_VERSION', '([^']+)' \);/", $source, $version );
		define( 'HELLO_ELEMENTOR_VERSION', $version[1] );
	}
	if ( ! function_exists( 'hello_maybe_update_theme_version_in_db' ) ) {
		preg_match( '/function hello_maybe_update_theme_version_in_db\(\) \{\n.*?\n\}\n/s', $source, $function );
		eval( $function[0] );
	}
	hello_maybe_update_theme_version_in_db();
};

// A case over the stored `$version`, none for no option.
$hello_case = function ( $version ) use ( $hello_update ) {
	return array(
		'setup'  => function () use ( $version ) {
			delete_option( 'hello_theme_version' );
			if ( null !== $version ) {
				add_option( 'hello_theme_version', $version );
			}
		},
		'action' => $hello_update,
	);
};

return array(
	'hello-version-missing' => $hello_case( null ),
	'hello-version-older'   => $hello_case( '3.0.0' ),
);

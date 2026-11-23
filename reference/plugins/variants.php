<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Must-use plugin for reference/plugins/golden.sh, through filters only:
// the only plugin listed is the one the test extension stands in for
// (the image's Akismet and Hello Dolly, and the must-use plugins, are
// hidden), and x13hidden and x24hidden give user 1 an option that hides
// the Plugins list's Description column.
// The x24 variants are ADR 0024's update rows: the update transient names
// the plugin (x24revoked as needing a newer PHP, which gives the error
// notice), wp_plugin_update_row() prints its row as for a user who may not
// update, and in_plugin_update_message adds HyScribe's command; nothing is
// written.
add_filter(
	'all_plugins',
	function ( $plugins ) {
		return array_intersect_key( $plugins, array( 'hyscribe-test/hyscribe-test.php' => true ) );
	}
);
add_filter( 'show_advanced_plugins', '__return_false' );
$hyscribe_variant = $_COOKIE['hyscribe_variant'] ?? '';
if ( 0 === strpos( $hyscribe_variant, 'x24' ) ) {
	$hyscribe_file    = 'hyscribe-test/hyscribe-test.php';
	$hyscribe_revoked = 'x24revoked' === $hyscribe_variant;
	add_filter(
		'pre_site_transient_update_plugins',
		function () use ( $hyscribe_file, $hyscribe_revoked ) {
			$offer = array(
				'id'          => 'hyscribe-test',
				'plugin'      => $hyscribe_file,
				'new_version' => $hyscribe_revoked ? '0.1.0' : '0.2.0',
			);
			if ( $hyscribe_revoked ) {
				$offer['requires_php'] = '100.0';
			}
			return (object) array(
				'last_checked' => time(),
				'checked'      => array( $hyscribe_file => '0.1.0' ),
				'response'     => array( $hyscribe_file => (object) $offer ),
				'no_update'    => array(),
			);
		}
	);
	add_action( "after_plugin_row_$hyscribe_file", 'wp_plugin_update_row', 10, 2 );
	add_action(
		"in_plugin_update_message-$hyscribe_file",
		function () use ( $hyscribe_revoked ) {
			echo $hyscribe_revoked
				? ' It no longer runs: install a version that is not revoked with <code>hyscribe plugin update hyscribe-test</code>, or delete it with <code>hyscribe plugin delete hyscribe-test</code>.'
				: ' Install it on the server with <code>hyscribe plugin update hyscribe-test</code>.';
		}
	);
}
if ( in_array( $hyscribe_variant, array( 'x13hidden', 'x24hidden' ), true ) ) {
	add_filter(
		'get_user_option_managepluginscolumnshidden',
		function () {
			return array( 'description' );
		}
	);
}

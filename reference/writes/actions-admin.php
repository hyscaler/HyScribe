<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The write oracle's admin actions (MVP phase 3), loaded beside actions.php:
// a case of a wp-admin screen's switch, taken from WordPress's own source
// and run over the request hs-admin's handler gets, past the checks, with
// the case's `exit` a return. docs/ADMIN.md, "Actions", says how to add one.

// get_editable_roles().
require_once ABSPATH . 'wp-admin/includes/user.php';

// The case `$name` of `wp-admin/$screen`, up to the next case.
$admin_case = function ( $screen, $name ) {
	$source = file_get_contents( ABSPATH . "wp-admin/$screen" );
	preg_match( "/\n\tcase '$name':\n(.*?)\n\tcase '/s", $source, $found );
	return str_replace( 'exit;', 'return;', $found[1] );
};

$oracle_user = function ( $login ) {
	return wp_insert_user(
		array(
			'user_login' => $login,
			'user_pass'  => 'A pass phrase',
			'user_email' => "$login@example.test",
		)
	);
};

return array(
	// users.php's bulk role change of another user and of user 1, an
	// administrator given a role that cannot promote users. A second user is
	// left as it is: HyScribe's test rolls back a change of its role.
	'admin-users-promote' => array(
		'setup'  => function () use ( $oracle_user ) {
			return array( $oracle_user( 'oracle-promoted' ), $oracle_user( 'oracle-kept' ) );
		},
		'action' => function ( $ids ) use ( $admin_case ) {
			$_GET     = array(
				'changeit' => 'Change',
				'new_role' => 'editor',
				'users'    => array( (string) $ids[0], '1' ),
				'_wpnonce' => wp_create_nonce( 'bulk-users' ),
			);
			$_REQUEST = $_GET;
			// What users.php sets before its switch.
			$redirect     = 'users.php';
			$current_user = wp_get_current_user();
			$wp_roles     = wp_roles();
			eval( $admin_case( 'users.php', 'promote' ) );
		},
	),
);

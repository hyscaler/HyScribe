<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The write oracle's actions of the login screen's lost password and reset
// password forms (MVP 8.3), beside actions.php: WordPress's own functions
// over what wp-login.php hands them, with mail taken as sent, as HyScribe
// answers until its transport sends it.

return array(
	// The lost password form: a key for a user found by their login, one
	// found by their email with spaces around it, and none for a login no
	// user has.
	'login-lostpassword' => array(
		'setup'  => function () use ( $oracle_user ) {
			return array( $oracle_user( 'oracle-lost' ), $oracle_user( 'oracle-mail' ) );
		},
		'action' => function () {
			add_filter( 'pre_wp_mail', '__return_true' );
			foreach ( array( 'oracle-lost', ' oracle-mail@example.test ', 'nobody' ) as $login ) {
				$_POST = wp_slash( array( 'user_login' => $login ) );
				retrieve_password();
			}
		},
	),
	// The reset password form's reset, of a user with a reset key and the
	// default password nag, and of one without either.
	'login-resetpass'    => array(
		'setup'  => function () use ( $oracle_user ) {
			$nagged = $oracle_user( 'oracle-nagged', array( 'user_activation_key' => '1:key' ) );
			update_user_meta( $nagged, 'default_password_nag', true );
			return array( $nagged, $oracle_user( 'oracle-plain' ) );
		},
		'action' => function ( $ids ) {
			add_filter( 'pre_wp_mail', '__return_true' );
			foreach ( $ids as $id ) {
				reset_password( get_userdata( $id ), wp_slash( " It's new " ) );
			}
		},
	),
	// The registration form: a user for a login and an address, and the
	// refusals that write nothing: a login with a space before it, a taken
	// login, a taken address, a login too long, an empty form. As
	// wp-login.php runs it, outside the admin.
	'login-register'     => array(
		'setup'  => function () use ( $oracle_user ) {
			return $oracle_user( 'oracle-taken' );
		},
		'action' => function () {
			add_filter( 'pre_wp_mail', '__return_true' );
			remove_action( 'user_register', array( 'WP_Internal_Pointers', 'dismiss_pointers_for_new_users' ) );
			$cases = array(
				array( 'oracle-new', 'oracle-new@example.test' ),
				array( ' 10', 'ten@example.test' ),
				array( 'oracle-taken', 'free@example.test' ),
				array( 'oracle-fresh', 'oracle-taken@example.test' ),
				array( str_repeat( 'a', 61 ), 'long@example.test' ),
				array( '', '' ),
			);
			foreach ( $cases as $case ) {
				register_new_user( $case[0], $case[1] );
			}
		},
	),
);

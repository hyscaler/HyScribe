<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// What the write oracle's actions share, required by actions.php before
// the actions-*.php files, which see these variables.

// The admin's code and hooks, loaded for every action: get_editable_roles(),
// the Ajax handlers, wp_delete_user(), set_current_screen() and the like.
require_once ABSPATH . 'wp-admin/includes/admin.php';
require_once ABSPATH . 'wp-admin/includes/ajax-actions.php';

// The code of `wp-admin/$file` that `$pattern` captures first, with its
// `exit;` and `die();` a return.
$admin_code = function ( $file, $pattern ) {
	preg_match( $pattern, file_get_contents( ABSPATH . "wp-admin/$file" ), $found );
	return str_replace( array( 'exit;', 'die();' ), 'return;', $found[1] );
};

// The code of `wp-admin/$file` after `$from` up to `$to`.
$admin_block = function ( $file, $from, $to ) use ( $admin_code ) {
	return $admin_code( $file, '/' . preg_quote( $from, '/' ) . '(.*?)' . preg_quote( $to, '/' ) . '/s' );
};

// Runs `$run` with what it prints dropped, and wp_die() or a redirect
// ending it as exit would; as an admin-ajax.php request with `$ajax`.
$admin_run = function ( $run, $ajax = false ) {
	$stop     = fn() => fn() => throw new Exception( 'stopped' );
	$redirect = fn() => throw new Exception( 'redirected' );
	add_filter( 'wp_die_handler', $stop );
	add_filter( 'wp_die_ajax_handler', $stop );
	add_filter( 'wp_redirect', $redirect );
	if ( $ajax ) {
		add_filter( 'wp_doing_ajax', '__return_true' );
	}
	ob_start();
	try {
		$run();
	} catch ( Exception $stopped ) {
		unset( $stopped );
	}
	ob_end_clean();
	remove_filter( 'wp_die_handler', $stop );
	remove_filter( 'wp_die_ajax_handler', $stop );
	remove_filter( 'wp_redirect', $redirect );
	remove_filter( 'wp_doing_ajax', '__return_true' );
};

// A user named `$login`, with `$more` of wp_insert_user()'s fields.
$oracle_user = function ( $login, $more = array() ) {
	return wp_insert_user(
		array_merge(
			array(
				'user_login' => $login,
				'user_pass'  => 'A pass phrase',
				'user_email' => "$login@example.test",
			),
			$more
		)
	);
};

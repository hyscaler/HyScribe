<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The write oracle's user actions of the admin (MVP 3.4), beside
// actions-admin.php: each runs the screen's own code from WordPress's
// source over the request hs-admin's handler gets, past the checks, with
// its `exit` and `die()` a return. Mail is taken as sent, as hs-admin
// answers (MVP phase 8 sends it).

// The code of `wp-admin/$screen` from the line after `$from` up to `$to`.
$admin_block = function ( $screen, $from, $to ) {
	$source = file_get_contents( ABSPATH . "wp-admin/$screen" );
	$start  = strpos( $source, $from ) + strlen( $from );
	$code   = substr( $source, $start, strpos( $source, $to, $start ) - $start );
	return str_replace( array( 'exit;', 'die();' ), 'return;', $code );
};

// The admin's code and hooks, which an admin request loads.
$admin = function () {
	require_once ABSPATH . 'wp-admin/includes/admin.php';
	require_once ABSPATH . 'wp-admin/includes/ajax-actions.php';
	add_filter( 'pre_wp_mail', '__return_true' );
	// User 1's logged in cookie, whose session wp_set_auth_cookie() keeps.
	$_COOKIE[ LOGGED_IN_COOKIE ] = wp_generate_auth_cookie( 1, time() + HOUR_IN_SECONDS, 'logged_in', 'oracle-token' );
};

$new_user = function ( $login, $role = 'author' ) {
	return wp_insert_user(
		array(
			'user_login' => $login,
			'user_pass'  => 'A pass phrase',
			'user_email' => "$login@example.test",
			'role'       => $role,
		)
	);
};

// users.php's switch case `$name`, up to the next case.
$users_case = function ( $name ) use ( $admin_block ) {
	return $admin_block( 'users.php', "\n\tcase '$name':\n", "\n\tcase '" );
};

// user-edit.php's save, as user 1 on `$screen`, of the user in `$_POST`.
$user_update = function ( $screen ) use ( $admin_block ) {
	$_REQUEST = $_POST;
	$user_id  = (int) $_POST['user_id'];
	define( 'IS_PROFILE_PAGE', 'profile.php' === $screen || 1 === $user_id );
	$wp_http_referer = remove_query_arg( array( 'update', 'delete_count', 'user_id' ), sanitize_url( $_REQUEST['wp_http_referer'] ?? '' ) );
	global $wpdb;
	eval( $admin_block( 'user-edit.php', "\n\tcase 'update':\n", "\n\t\t// Intentional fall-through" ) );
};

// The Ajax action `$action`, its JSON answer printed nowhere.
$ajax = function ( $action ) {
	add_filter( 'wp_doing_ajax', '__return_true' );
	add_filter(
		'wp_die_ajax_handler',
		fn() => function () {
			throw new Exception( 'answered' );
		}
	);
	ob_start();
	try {
		call_user_func( 'wp_ajax_' . str_replace( '-', '_', $action ) );
	} catch ( Exception $answered ) {
		ob_end_clean();
	}
};

return array(
	// users.php's deletion: a user's posts deleted with them, another's
	// passed to user 1, and user 1 kept.
	'admin-users-dodelete'       => array(
		'setup'  => function () use ( $new_user ) {
			$ids = array( $new_user( 'oracle-deleted' ), $new_user( 'oracle-heir' ) );
			foreach ( array_combine( array( 'Deleted post', 'Passed post' ), $ids ) as $title => $id ) {
				wp_insert_post(
					array(
						'post_author'  => $id,
						'post_title'   => $title,
						'post_content' => 'Text.',
						'post_status'  => 'publish',
					)
				);
			}
			return $ids;
		},
		'action' => function ( $ids ) use ( $admin, $users_case ) {
			$admin();
			$_REQUEST     = array(
				'action'        => 'dodelete',
				'users'         => array( (string) $ids[0], (string) $ids[1], '1' ),
				'delete_option' => array(
					$ids[0] => 'delete',
					$ids[1] => 'reassign',
					1       => 'delete',
				),
				'reassign_user' => array( $ids[1] => '1' ),
				'_wpnonce'      => wp_create_nonce( 'delete-users' ),
			);
			$_POST        = $_REQUEST;
			$redirect     = 'users.php';
			$current_user = wp_get_current_user();
			eval( $users_case( 'dodelete' ) );
		},
	),
	// users.php's password reset links, for user 1, two other users (one
	// whose login, an address, is its email) and one whose login is
	// not found once sanitized.
	'admin-users-resetpassword'  => array(
		'setup'  => function () use ( $new_user ) {
			global $wpdb;
			$wpdb->insert(
				$wpdb->users,
				array(
					'user_login'      => 'oracle <i>',
					'user_email'      => 'odd@example.test',
					'user_registered' => '2026-01-01 00:00:00',
					'display_name'    => 'odd',
				)
			);
			$odd = $wpdb->insert_id;
			$at  = wp_insert_user(
				array(
					'user_login' => 'oracle@reset.test',
					'user_pass'  => 'A pass phrase',
					'user_email' => 'oracle@reset.test',
					'role'       => 'author',
				)
			);
			return array( $new_user( 'oracle-reset' ), $at, $odd );
		},
		'action' => function ( $ids ) use ( $admin, $users_case ) {
			$admin();
			$_GET         = array(
				'action'   => 'resetpassword',
				'users'    => array( (string) $ids[0], (string) $ids[1], (string) $ids[2], '1' ),
				'_wpnonce' => wp_create_nonce( 'bulk-users' ),
			);
			$_REQUEST     = $_GET;
			$redirect     = 'users.php';
			$current_user = wp_get_current_user();
			eval( $users_case( 'resetpassword' ) );
		},
	),
	// user-new.php's new users: an editor told of the account, and one with
	// no role who is not.
	'admin-user-new'             => array(
		'action' => function () use ( $admin, $admin_block ) {
			$admin();
			$posts = array(
				array(
					'user_login'             => 'oracle-new',
					'email'                  => 'oracle-new@example.test',
					'first_name'             => "O'New",
					'last_name'              => 'User',
					'url'                    => '/home',
					'pass1'                  => " It's a pass ",
					'pass2'                  => " It's a pass ",
					'role'                   => 'editor',
					'send_user_notification' => '1',
				),
				array(
					'user_login' => 'oracle-quiet',
					'email'      => 'oracle-quiet@example.test',
					'url'        => 'example.test/home',
					'pass1'      => 'Quiet',
					'pass2'      => 'Quiet',
					'role'       => '',
				),
			);
			foreach ( $posts as $post ) {
				$_POST    = wp_slash(
					$post + array(
						'action'               => 'createuser',
						'_wpnonce_create-user' => wp_create_nonce( 'create-user' ),
						'createuser'           => 'Add User',
					)
				);
				$_REQUEST = $_POST;
				eval( $admin_block( 'user-new.php', "'createuser' === \$_REQUEST['action'] ) {\n", "\n}\n\n// Used in the HTML title tag." ) );
			}
		},
	),
	// user-new.php's new user by one who may create users but not list them.
	'admin-user-new-unlisted'    => array(
		'setup'  => function () use ( $new_user ) {
			$id = $new_user( 'oracle-creator', 'subscriber' );
			( new WP_User( $id ) )->add_cap( 'create_users' );
			return $id;
		},
		'action' => function ( $id ) use ( $admin, $admin_block ) {
			$admin();
			wp_set_current_user( $id );
			$_POST    = wp_slash(
				array(
					'action'               => 'createuser',
					'_wpnonce_create-user' => wp_create_nonce( 'create-user' ),
					'user_login'           => 'oracle-created',
					'email'                => 'oracle-created@example.test',
					'pass1'                => 'Created',
					'pass2'                => 'Created',
					'role'                 => 'editor',
				)
			);
			$_REQUEST = $_POST;
			eval( $admin_block( 'user-new.php', "'createuser' === \$_REQUEST['action'] ) {\n", "\n}\n\n// Used in the HTML title tag." ) );
		},
	),
	// user-edit.php's save of another user: every field, a role and a password.
	'admin-user-edit'            => array(
		'setup'  => function () use ( $new_user ) {
			return $new_user( 'oracle-edited' );
		},
		'action' => function ( $id ) use ( $admin, $user_update ) {
			$admin();
			$_POST = wp_slash(
				array(
					'action'              => 'update',
					'user_id'             => (string) $id,
					'_wpnonce'            => wp_create_nonce( "update-user_$id" ),
					'wp_http_referer'     => '/wp-admin/users.php?role=author&update=x',
					'email'               => 'oracle-edited-2@example.test',
					'url'                 => 'https://example.test/"me"',
					'first_name'          => ' First <b>Name</b> ',
					'last_name'           => 'Last',
					'nickname'            => 'Nick',
					'display_name'        => 'Nick',
					'description'         => " About <script>x</script> me \n",
					'role'                => 'editor',
					'locale'              => 'site-default',
					'rich_editing'        => 'false',
					'syntax_highlighting' => 'false',
					'admin_color'         => 'ocean',
					'comment_shortcuts'   => 'true',
					'use_ssl'             => '1',
					'pass1'               => 'A new pass',
					'pass2'               => 'A new pass',
				)
			);
			$user_update( 'user-edit.php' );
		},
	),
	// profile.php's save of user 1: a new password and an email change,
	// which waits for its confirmation; the role asked for cannot promote
	// users, so user 1 keeps theirs.
	'admin-profile'              => array(
		'action' => function () use ( $admin, $user_update ) {
			$admin();
			$_POST = wp_slash(
				array(
					'action'          => 'update',
					'user_id'         => '1',
					'_wpnonce'        => wp_create_nonce( 'update-user_1' ),
					'email'           => 'new-admin@example.test',
					'url'             => '',
					'nickname'        => 'admin',
					'display_name'    => 'admin',
					'admin_color'     => 'fresh',
					'role'            => 'editor',
					'admin_bar_front' => '1',
					'locale'          => '',
					'pass1'           => 'Another pass',
					'pass2'           => 'Another pass',
				)
			);
			$user_update( 'profile.php' );
		},
	),
	// admin-ajax.php's color scheme of user 1.
	'admin-color-scheme'         => array(
		'action' => function () use ( $admin, $ajax ) {
			$admin();
			// What admin_init registers.
			register_admin_color_schemes();
			$_POST    = array(
				'action'       => 'save-user-color-scheme',
				'color_scheme' => 'ocean',
				'nonce'        => wp_create_nonce( 'save-color-scheme' ),
			);
			$_REQUEST = $_POST;
			$ajax( 'save-user-color-scheme' );
		},
	),
	// admin-ajax.php's password reset link from Edit User.
	'admin-send-password-reset'  => array(
		'setup'  => function () use ( $new_user ) {
			return $new_user( 'oracle-link' );
		},
		'action' => function ( $id ) use ( $admin, $ajax ) {
			$admin();
			$_POST    = array(
				'action'  => 'send-password-reset',
				'user_id' => (string) $id,
				'nonce'   => wp_create_nonce( "reset-password-for-$id" ),
			);
			$_REQUEST = $_POST;
			$ajax( 'send-password-reset' );
		},
	),
);

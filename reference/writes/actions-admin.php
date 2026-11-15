<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The write oracle's admin actions (MVP phase 3), loaded beside actions.php:
// a case of a wp-admin screen's switch, taken from WordPress's own source
// and run over the request hs-admin's handler gets, past the checks, with
// the case's `exit` a return. docs/ADMIN.md, "Actions", says how to add one.

// get_editable_roles(), the Ajax handlers and set_current_screen().
require_once ABSPATH . 'wp-admin/includes/admin.php';
require_once ABSPATH . 'wp-admin/includes/ajax-actions.php';

// The case `$name` of `wp-admin/$screen`, up to the next case.
$admin_case = function ( $screen, $name ) {
	$source = file_get_contents( ABSPATH . "wp-admin/$screen" );
	preg_match( "/\n\tcase '$name':\n(.*?)\n\tcase '/s", $source, $found );
	return str_replace( 'exit;', 'return;', $found[1] );
};

// edit-comments.php's bulk actions, from its `if ( $doaction )` to its end,
// over the request in $_GET, with what the screen sets before them.
$comments_bulk = function ( $get ) {
	$_GET     = $get;
	$_REQUEST = $_GET;
	$source   = file_get_contents( ABSPATH . 'wp-admin/edit-comments.php' );
	preg_match( "/\nif \( \\\$doaction \) \{\n(.*?)\n\} elseif/s", $source, $found );
	set_current_screen( 'edit-comments' );
	$wp_list_table = _get_list_table( 'WP_Comments_List_Table' );
	$pagenum       = $wp_list_table->get_pagenum();
	$doaction      = $wp_list_table->current_action();
	eval( str_replace( 'exit;', 'return;', $found[1] ) );
};

// An Ajax action's handler over $_POST, its answer's wp_die() thrown.
$comment_ajax = function ( $handler, $post ) {
	$_POST    = $post;
	$_REQUEST = $_POST;
	add_filter( 'wp_doing_ajax', '__return_true' );
	add_filter(
		'wp_die_ajax_handler',
		fn() => function () {
			throw new RuntimeException( 'wp_die' );
		}
	);
	ob_start();
	try {
		$handler();
	} catch ( RuntimeException $died ) {
		unset( $died );
	}
	ob_end_clean();
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
	// edit-comments.php's bulk Spam of comment 1 and of a comment missing,
	// which is left out.
	'admin-comments-spam'       => array(
		'action' => function () use ( $comments_bulk ) {
			$comments_bulk(
				array(
					'action'           => 'spam',
					'delete_comments'  => array( '1', (string) HYSCRIBE_MISSING_ID ),
					'_wp_http_referer' => '/wp-admin/edit-comments.php',
					'_wpnonce'         => wp_create_nonce( 'bulk-comments' ),
				)
			);
		},
	),
	// Comment 1 moved to the Trash, then the Trash emptied.
	'admin-comments-empty-trash' => array(
		'action' => function () use ( $comments_bulk ) {
			$nonce = wp_create_nonce( 'bulk-comments' );
			$comments_bulk(
				array(
					'action'   => 'trash',
					'ids'      => '1',
					'_wpnonce' => $nonce,
				)
			);
			$comments_bulk(
				array(
					'action'            => '-1',
					'delete_all'        => 'Empty Trash',
					'comment_status'    => 'trash',
					'pagegen_timestamp' => '2099-01-01 00:00:00',
					'_wpnonce'          => $nonce,
				)
			);
		},
	),
	// comment.php's Unapprove of comment 1.
	'admin-comment-unapprove'   => array(
		'action' => function () use ( $admin_case ) {
			$_GET     = array(
				'action'   => 'unapprovecomment',
				'c'        => '1',
				'_wpnonce' => wp_create_nonce( 'approve-comment_1' ),
			);
			$_REQUEST = $_GET;
			$action   = 'unapprovecomment';
			eval( str_replace( 'die;', 'return;', $admin_case( 'comment.php', 'unapprovecomment' ) ) );
		},
	),
	// The list's Unapprove of comment 1, through admin-ajax.php.
	'admin-comment-dim'         => array(
		'action' => function () use ( $comment_ajax ) {
			$comment_ajax(
				'wp_ajax_dim_comment',
				array(
					'action'      => 'dim-comment',
					'id'          => '1',
					'new'         => 'unapproved',
					'_ajax_nonce' => wp_create_nonce( 'approve-comment_1' ),
				)
			);
		},
	),
	// A comment of a trashed post moved to the Trash through admin-ajax.php,
	// which cannot take it out again: `post-trashed` is no status to set.
	'admin-comment-ajax-untrash-kept' => array(
		'setup'  => function () {
			return wp_insert_comment(
				array(
					'comment_post_ID'  => 1,
					'comment_author'   => 'A reader',
					'comment_content'  => 'A comment.',
					'comment_approved' => 'post-trashed',
				)
			);
		},
		'action' => function ( $id ) use ( $comment_ajax ) {
			foreach ( array( 'trash', 'untrash' ) as $flag ) {
				$comment_ajax(
					'wp_ajax_delete_comment',
					array(
						'action'      => 'delete-comment',
						'id'          => (string) $id,
						$flag         => '1',
						'_ajax_nonce' => wp_create_nonce( "delete-comment_$id" ),
					)
				);
			}
		},
	),
	// The list's Trash of comment 1, through admin-ajax.php.
	'admin-comment-ajax-trash'  => array(
		'action' => function () use ( $comment_ajax ) {
			$comment_ajax(
				'wp_ajax_delete_comment',
				array(
					'action'      => 'delete-comment',
					'id'          => '1',
					'trash'       => '1',
					'_ajax_nonce' => wp_create_nonce( 'delete-comment_1' ),
				)
			);
		},
	),
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

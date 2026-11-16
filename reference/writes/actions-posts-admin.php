<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The write oracle's actions of the posts and pages lists (MVP 3.1), from
// wp-admin's own code over the request hs-admin's handler gets, past the
// checks: post.php's trash, restore and delete cases, edit.php's bulk
// actions block, and wp_ajax_inline_save(). An `exit` is a return, a
// wp_die() an exception, and what the code prints is dropped.

// The admin's includes, loaded only when such an action runs.
$posts_admin_load = function () {
	require_once ABSPATH . 'wp-admin/includes/admin.php';
	require_once ABSPATH . 'wp-admin/includes/ajax-actions.php';
	add_filter( 'wp_die_handler', fn() => fn() => throw new RuntimeException( 'wp_die' ) );
	add_filter( 'wp_die_ajax_handler', fn() => fn() => throw new RuntimeException( 'wp_die' ) );
};

// Runs `$code` with its output dropped and a wp_die() ending it.
$posts_admin_run = function ( $code ) {
	ob_start();
	try {
		$code();
	} catch ( RuntimeException $died ) {
		unset( $died );
	}
	ob_end_clean();
};

// post.php's case `$action` on the post `$id`, with post.php's globals.
$posts_admin_post = function ( $id, $action ) use ( $posts_admin_load, $posts_admin_run ) {
	$posts_admin_load();
	$_GET     = array(
		'post'     => (string) $id,
		'action'   => $action,
		'_wpnonce' => wp_create_nonce( "$action-post_$id" ),
	);
	$_POST    = array();
	$_REQUEST = $_GET;
	$source   = file_get_contents( ABSPATH . 'wp-admin/post.php' );
	preg_match( "/\n\tcase '$action':\n(.*?)\n\tcase '/s", $source, $found );
	$posts_admin_run(
		function () use ( $id, $found ) {
			global $post_type, $post_type_object, $post;
			$post_id          = $id;
			$post             = get_post( $id );
			$post_type        = $post->post_type;
			$post_type_object = get_post_type_object( $post_type );
			$sendback         = admin_url( 'edit.php' );
			eval( str_replace( 'exit;', 'return;', $found[1] ) );
		}
	);
};

// edit.php's bulk actions over `$request`, as the posts list sends it.
$posts_admin_edit = function ( $request ) use ( $posts_admin_load, $posts_admin_run ) {
	$posts_admin_load();
	$_GET     = $request + array( '_wpnonce' => wp_create_nonce( 'bulk-posts' ) );
	$_POST    = array();
	$_REQUEST = $_GET;
	$source   = file_get_contents( ABSPATH . 'wp-admin/edit.php' );
	preg_match( "/\nif \( \\\$doaction \) \{\n(.*?)\n\} elseif/s", $source, $found );
	$posts_admin_run(
		function () use ( $found ) {
			$post_type     = $_REQUEST['post_type'] ?? 'post';
			$parent_file   = 'post' === $post_type ? 'edit.php' : "edit.php?post_type=$post_type";
			$post_new_file = 'post' === $post_type ? 'post-new.php' : "post-new.php?post_type=$post_type";
			$pagenum       = 1;
			// WP_Posts_List_Table::current_action().
			$doaction = isset( $_REQUEST['delete_all'] ) ? 'delete_all' : $_REQUEST['action'];
			eval( str_replace( 'exit;', 'return;', $found[1] ) );
		}
	);
};

// admin-ajax.php's inline-save over `$fields`, as Quick Edit sends them.
$posts_admin_inline = function ( $fields ) use ( $posts_admin_load, $posts_admin_run ) {
	$posts_admin_load();
	add_filter( 'wp_doing_ajax', '__return_true' );
	$_GET     = array();
	$_POST    = wp_slash( $fields + array( '_inline_edit' => wp_create_nonce( 'inlineeditnonce' ) ) );
	$_REQUEST = $_POST;
	$posts_admin_run( 'wp_ajax_inline_save' );
};

// Another user's fresh edit lock on the post `$id`.
$posts_admin_lock = function ( $id ) {
	$user = wp_insert_user(
		array(
			'user_login' => 'oracle-editing',
			'user_pass'  => 'A pass phrase',
			'user_email' => 'oracle-editing@example.test',
		)
	);
	update_post_meta( $id, '_edit_lock', time() . ":$user" );
};

// Quick Edit's fields of the fixture's post 1 or page 2, as the form holds
// them: the page's comments closed, and it has no pings, format,
// categories or tags.
$posts_admin_quick = function ( $id ) {
	$fields = array(
		'post_title'     => 'Hello world!',
		'post_name'      => 'hello-world',
		'mm'             => '10',
		'jj'             => '27',
		'aa'             => '2026',
		'hh'             => '08',
		'mn'             => '08',
		'ss'             => '15',
		'post_author'    => '1',
		'post_password'  => '',
		'_status'        => 'publish',
		'screen'         => 'edit-post',
		'post_view'      => 'list',
		'action'         => 'inline-save',
		'post_type'      => 'post',
		'post_ID'        => (string) $id,
		'edit_date'      => 'true',
		'post_status'    => '',
		'comment_status' => 'open',
		'ping_status'    => 'open',
		'post_format'    => '0',
	);
	foreach ( array( 'mm', 'jj', 'aa', 'hh', 'mn' ) as $unit ) {
		$fields[ "hidden_$unit" ] = $fields[ $unit ];
	}
	if ( 2 === $id ) {
		unset( $fields['comment_status'], $fields['ping_status'], $fields['post_format'] );
	}
	return $fields;
};

return array(
	// post.php's trash of the post with a comment, and its restore and
	// delete from the Trash.
	'admin-post-trash'             => array(
		'action' => function () use ( $posts_admin_post ) {
			$posts_admin_post( 1, 'trash' );
		},
	),
	'admin-post-untrash'           => array(
		'setup'  => function () {
			wp_trash_post( 1 );
		},
		'action' => function () use ( $posts_admin_post ) {
			$posts_admin_post( 1, 'untrash' );
		},
	),
	'admin-post-delete'            => array(
		'setup'  => function () {
			wp_trash_post( 1 );
		},
		'action' => function () use ( $posts_admin_post ) {
			$posts_admin_post( 1, 'delete' );
		},
	),
	// edit.php's bulk trash of two pages, one of them locked by another user.
	'admin-posts-bulk-trash'       => array(
		'setup'  => function () use ( $posts_admin_lock ) {
			$posts_admin_lock( 2 );
		},
		'action' => function () use ( $posts_admin_edit ) {
			$posts_admin_edit(
				array(
					'post_type' => 'page',
					'action'    => 'trash',
					'post'      => array( '2', '3' ),
				)
			);
		},
	),
	// The trashed notice's Undo: the post gets its status back.
	'admin-posts-bulk-undo'        => array(
		'setup'  => function () {
			wp_trash_post( 1 );
		},
		'action' => function () use ( $posts_admin_edit ) {
			$posts_admin_edit(
				array(
					'doaction' => 'undo',
					'action'   => 'untrash',
					'ids'      => '1',
				)
			);
		},
	),
	// A bulk delete: the trashed post is deleted, the page only trashed.
	'admin-posts-bulk-delete'      => array(
		'setup'  => function () {
			wp_trash_post( 1 );
		},
		'action' => function () use ( $posts_admin_edit ) {
			$posts_admin_edit(
				array(
					'action' => 'delete',
					'post'   => array( '1', '2' ),
				)
			);
		},
	),
	// Empty Trash of posts, which leaves the trashed page.
	'admin-posts-empty-trash'      => array(
		'setup'  => function () {
			wp_trash_post( 1 );
			wp_trash_post( 2 );
		},
		'action' => function () use ( $posts_admin_edit ) {
			$posts_admin_edit(
				array(
					'action'      => '-1',
					'delete_all'  => 'Empty Trash',
					'post_status' => 'trash',
				)
			);
		},
	),
	// Bulk Edit of two posts: pending, comments closed, sticky, a tag added.
	'admin-posts-bulk-edit'        => array(
		'setup'  => function () {
			return wp_insert_post(
				array(
					'import_id'    => 960,
					'post_title'   => 'A bulk edited post',
					'post_content' => 'Its content.',
					'post_status'  => 'draft',
					'post_author'  => 1,
				)
			);
		},
		'action' => function ( $id ) use ( $posts_admin_edit ) {
			$posts_admin_edit(
				array(
					'post_type'      => 'post',
					'action'         => 'edit',
					'bulk_edit'      => 'Update',
					'post'           => array( '1', (string) $id ),
					'post_category'  => array( '0' ),
					'tax_input'      => array( 'post_tag' => 'Bulk tag' ),
					'post_author'    => '-1',
					'comment_status' => 'closed',
					'ping_status'    => '',
					'_status'        => 'pending',
					'post_format'    => '-1',
					'sticky'         => 'sticky',
				)
			);
		},
	),
	// Quick Edit of the post: a new title and slug, a new date, tags and
	// sticky; and of the page: its parent, order and template.
	'admin-posts-inline-save'      => array(
		'action' => function () use ( $posts_admin_inline, $posts_admin_quick ) {
			$posts_admin_inline(
				array(
					'post_title'    => 'Hello again',
					'post_name'     => 'hello-again',
					'jj'            => '28',
					'post_category' => array( '0', '1' ),
					'tax_input'     => array( 'post_tag' => 'First tag, second , ,' ),
					'sticky'        => 'sticky',
				) + $posts_admin_quick( 1 )
			);
		},
	),
	// A title with HTML, filtered by kses, and as an owner's .env lets user 1
	// post it unfiltered (run.php leaves DISALLOW_UNFILTERED_HTML undefined).
	'admin-posts-inline-save-html' => array(
		'action' => function () use ( $posts_admin_inline, $posts_admin_quick ) {
			$posts_admin_inline( array( 'post_title' => '<b>Hi</b> <script>x()</script>' ) + $posts_admin_quick( 1 ) );
		},
	),
	'admin-posts-inline-save-html-unfiltered-html' => array(
		'action' => function () use ( $posts_admin_inline, $posts_admin_quick ) {
			$posts_admin_inline( array( 'post_title' => '<b>Hi</b> <script>x()</script>' ) + $posts_admin_quick( 1 ) );
		},
	),
	'admin-pages-inline-save'      => array(
		'setup'  => function () {
			return wp_insert_post(
				array(
					'post_title'  => 'A parent page',
					'post_type'   => 'page',
					'post_status' => 'publish',
					'post_author' => 1,
				)
			);
		},
		'action' => function ( $id ) use ( $posts_admin_inline, $posts_admin_quick ) {
			$posts_admin_inline(
				array(
					'post_title'    => 'Sample Page',
					'post_name'     => 'sample-page',
					'post_type'     => 'page',
					'screen'        => 'edit-page',
					'post_parent'   => (string) $id,
					'menu_order'    => '3',
					'page_template' => 'default',
				) + $posts_admin_quick( 2 )
			);
		},
	),
);

<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The write oracle's admin actions (MVP phase 3), loaded beside actions.php:
// a case of a wp-admin screen's switch, taken from WordPress's own source
// and run over the request hs-admin's handler gets, past the checks, with
// the case's `exit` a return. docs/ADMIN.md, "Actions", says how to add one.

// get_editable_roles().
require_once ABSPATH . 'wp-admin/includes/user.php';

// The case `$name` of `wp-admin/$screen`, up to the next case or the default.
$admin_case = function ( $screen, $name ) {
	$source = file_get_contents( ABSPATH . "wp-admin/$screen" );
	preg_match( "/\n\tcase '$name':\n(.*?)\n\t(case '|default:)/s", $source, $found );
	return str_replace( 'exit;', 'return;', $found[1] );
};

// The Ajax actions and the list tables they print rows with.
require_once ABSPATH . 'wp-admin/includes/admin.php';
require_once ABSPATH . 'wp-admin/includes/ajax-actions.php';

// `wp_ajax_$action()` over `$post` (unslashed, as sent), its answer
// thrown away: wp_die() ends it by an exception instead of the process.
$admin_ajax = function ( $action, $post ) {
	$_POST    = wp_slash( $post + array( 'action' => $action ) );
	$_REQUEST = $_POST;
	add_filter( 'wp_doing_ajax', '__return_true' );
	$die = function () {
		return function () {
			throw new RuntimeException( 'wp_die' );
		};
	};
	add_filter( 'wp_die_ajax_handler', $die );
	ob_start();
	try {
		call_user_func( 'wp_ajax_' . str_replace( '-', '_', $action ) );
	} catch ( RuntimeException $died ) {
		unset( $died );
	}
	ob_end_clean();
	remove_filter( 'wp_die_ajax_handler', $die );
	remove_filter( 'wp_doing_ajax', '__return_true' );
};

// The case `$name` of edit-tags.php over `$request` (unslashed), with the
// globals the screen sets before its switch.
$terms_case = function ( $name, $request, $post = true ) use ( $admin_case ) {
	$request  = wp_slash( $request );
	$_POST    = $post ? $request : array();
	$_GET     = $post ? array() : $request;
	$_REQUEST = $request;
	$taxonomy = $request['taxonomy'];
	$tax      = get_taxonomy( $taxonomy );
	$referer  = '/wp-admin/edit-tags.php?taxonomy=' . $taxonomy;
	$location = false;
	// The case's `break` leaves this switch.
	eval( 'switch ( 1 ) { default: ' . $admin_case( 'edit-tags.php', $name ) . ' }' );
};

$oracle_term = function ( $name, $taxonomy, $args = array() ) {
	return wp_insert_term( $name, $taxonomy, $args )['term_id'];
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
	// edit-tags.php's add form, a category with slashes in its texts.
	'admin-terms-add'        => array(
		'action' => function () use ( $terms_case ) {
			$terms_case(
				'add-tag',
				array(
					'action'           => 'add-tag',
					'taxonomy'         => 'category',
					'tag-name'         => "Oracle's added",
					'slug'             => '',
					'parent'           => '-1',
					'description'      => 'Some <b>text</b> \ here',
					'_wpnonce_add-tag' => wp_create_nonce( 'add-tag' ),
				)
			);
		},
	),
	// The add form through admin-ajax.php: a child category, then Quick Edit
	// giving its parent the child's slug, which fails.
	'admin-terms-add-child'  => array(
		'action' => function () use ( $admin_ajax ) {
			$admin_ajax(
				'add-tag',
				array(
					'screen'           => 'edit-category',
					'taxonomy'         => 'category',
					'tag-name'         => 'Child category',
					'slug'             => '',
					'parent'           => '1',
					'description'      => '',
					'_wpnonce_add-tag' => wp_create_nonce( 'add-tag' ),
				)
			);
			$admin_ajax(
				'inline-save-tax',
				array(
					'taxonomy'     => 'category',
					'tax_ID'       => '1',
					'name'         => 'Uncategorized',
					'slug'         => 'child-category',
					'_inline_edit' => wp_create_nonce( 'taxinlineeditnonce' ),
				)
			);
		},
	),
	// The add form through admin-ajax.php: a tag.
	'admin-terms-add-tag'    => array(
		'action' => function () use ( $admin_ajax ) {
			$admin_ajax(
				'add-tag',
				array(
					'screen'           => 'edit-post_tag',
					'taxonomy'         => 'post_tag',
					'tag-name'         => 'Fresh tag',
					'slug'             => '',
					'description'      => 'About it',
					'_wpnonce_add-tag' => wp_create_nonce( 'add-tag' ),
				)
			);
		},
	),
	// Quick Edit of a category whose description has a backslash, which the
	// save strips as it passes the description unslashed.
	'admin-terms-inline'     => array(
		'setup'  => function () use ( $oracle_term ) {
			return $oracle_term( 'Oracle inline', 'category', array( 'description' => wp_slash( 'Kept \ text' ) ) );
		},
		'action' => function ( $id ) use ( $admin_ajax ) {
			$admin_ajax(
				'inline-save-tax',
				array(
					'taxonomy'     => 'category',
					'tax_ID'       => (string) $id,
					'name'         => 'Quick & named',
					'slug'         => '',
					'_inline_edit' => wp_create_nonce( 'taxinlineeditnonce' ),
				)
			);
		},
	),
	// The edit screen's save of a category, given a parent.
	'admin-terms-edited'     => array(
		'setup'  => function () use ( $oracle_term ) {
			return array( $oracle_term( 'Oracle parent', 'category' ), $oracle_term( 'Oracle edited', 'category' ) );
		},
		'action' => function ( $ids ) use ( $terms_case ) {
			$terms_case(
				'editedtag',
				array(
					'action'      => 'editedtag',
					'tag_ID'      => (string) $ids[1],
					'taxonomy'    => 'category',
					'name'        => 'Oracle "edited"',
					'slug'        => 'Oracle Edited Slug',
					'description' => 'Line <script>x</script>',
					'parent'      => (string) $ids[0],
					'_wpnonce'    => wp_create_nonce( 'update-tag_' . $ids[1] ),
				)
			);
		},
	),
	// A row's Delete of a category with a child.
	'admin-terms-delete'     => array(
		'setup'  => function () use ( $oracle_term ) {
			$parent = $oracle_term( 'Oracle doomed', 'category' );
			$oracle_term( 'Oracle orphan', 'category', array( 'parent' => $parent ) );
			return $parent;
		},
		'action' => function ( $id ) use ( $terms_case ) {
			$terms_case(
				'delete',
				array(
					'action'   => 'delete',
					'taxonomy' => 'category',
					'tag_ID'   => (string) $id,
					'_wpnonce' => wp_create_nonce( 'delete-tag_' . $id ),
				),
				false
			);
		},
	),
	// The bulk Delete of two tags and a term no row has.
	'admin-terms-bulk-delete' => array(
		'setup'  => function () use ( $oracle_term ) {
			return array( $oracle_term( 'Oracle one', 'post_tag' ), $oracle_term( 'Oracle two', 'post_tag' ) );
		},
		'action' => function ( $ids ) use ( $terms_case ) {
			$terms_case(
				'bulk-delete',
				array(
					'action'      => 'delete',
					'taxonomy'    => 'post_tag',
					'delete_tags' => array( (string) $ids[0], (string) $ids[1], (string) HYSCRIBE_MISSING_ID ),
					'_wpnonce'    => wp_create_nonce( 'bulk-tags' ),
				)
			);
		},
	),
	// A row's Delete through admin-ajax.php.
	'admin-terms-delete-ajax' => array(
		'setup'  => function () use ( $oracle_term ) {
			return $oracle_term( 'Oracle ajax doomed', 'post_tag' );
		},
		'action' => function ( $id ) use ( $admin_ajax ) {
			$admin_ajax(
				'delete-tag',
				array(
					'tag_ID'      => (string) $id,
					'taxonomy'    => 'post_tag',
					'_ajax_nonce' => wp_create_nonce( 'delete-tag_' . $id ),
				)
			);
		},
	),
);

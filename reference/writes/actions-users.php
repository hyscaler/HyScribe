<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The write oracle's actions for users, options, transients and meta (MVP
// 2.4), loaded beside actions.php. Option and meta names start with
// `oracle_`, so they never clash with the fixture's.

// wp_delete_user().
require_once ABSPATH . 'wp-admin/includes/user.php';

$new_user = function ( $login, $more = array() ) {
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

// A meta row as stored, without add_metadata()'s serialization.
$raw_meta = function ( $id, $key, $value ) {
	global $wpdb;
	$wpdb->insert(
		$wpdb->postmeta,
		array(
			'post_id'    => $id,
			'meta_key'   => $key,
			'meta_value' => $value,
		)
	);
};

// Option rows as stored: new ones, or new values of existing ones.
$raw_options = function ( $rows ) {
	global $wpdb;
	foreach ( $rows as $name => $value ) {
		$wpdb->query( $wpdb->prepare( "INSERT INTO $wpdb->options (option_name, option_value, autoload) VALUES (%s, %s, 'auto-on') ON DUPLICATE KEY UPDATE option_value = VALUES(option_value)", $name, $value ) );
	}
};

return array(
	'user-insert-roles'      => array(
		'action' => function () use ( $new_user ) {
			$new_user( 'oracle-subscriber' );
			$new_user(
				'oracle-editor',
				array(
					'role'       => 'editor',
					'first_name' => 'Ada',
					'last_name'  => 'Lovelace',
				)
			);
			$new_user(
				'oracle-author',
				array(
					'role'       => 'author',
					'first_name' => 'Only First',
				)
			);
			$new_user(
				'oracle-contributor',
				array(
					'role'      => 'contributor',
					'last_name' => 'Only Last',
				)
			);
			$new_user(
				'oracle-admin',
				array(
					'role'         => 'administrator',
					'nickname'     => 'Boss',
					'display_name' => 'The Boss',
				)
			);
			// The same nicename three times, and no role at all.
			$new_user( 'Jane Doe', array( 'user_email' => 'jane@example.test' ) );
			$new_user(
				'jane.doe',
				array(
					'user_email' => 'jane.doe@example.test',
					'role'       => '',
				)
			);
			$new_user( 'jane-doe', array( 'user_email' => 'jane-doe@example.test' ) );
			// Taken: nothing is written.
			$new_user( 'oracle-editor', array( 'user_email' => 'other@example.test' ) );
			$new_user( 'oracle-other', array( 'user_email' => 'oracle-editor@example.test' ) );
			$new_user( '<b></b>' );
		},
	),
	'user-insert-every-field' => array(
		'action' => function () use ( $new_user ) {
			$new_user(
				'  Oracle\\Full!  ',
				array(
					'user_email'           => ' full@example.test ',
					'user_nicename'        => 'Full Name',
					'user_url'             => 'https://example.test/<b>me</b>?a=1&b=2',
					'user_registered'      => '2020-01-02 03:04:05',
					'user_activation_key'  => '123:abc',
					'display_name'         => 'Full <em>Display</em>',
					'nickname'             => "O'Neil & <b>co</b>",
					'first_name'           => 'Back\\slash',
					'last_name'            => "Tab\tand  space",
					'description'          => '<a href="https://example.test" rel="me" target="_blank" onclick="x()">Me</a> & <b>bold</b> <script>x</script>',
					'rich_editing'         => 'false',
					'syntax_highlighting'  => 'false',
					'infinite_scrolling'   => 'false',
					'comment_shortcuts'    => 'yes',
					'admin_color'          => 'fresh!<x>',
					'use_ssl'              => 1,
					'show_admin_bar_front' => 'false',
					'locale'               => 'fr_FR',
					'role'                 => 'author',
					'meta_input'           => array(
						'oracle_custom' => 'one',
						'oracle_list'   => array( 1, 'two' ),
					),
				)
			);
		},
	),
	'user-update'            => array(
		'setup'  => function () use ( $new_user ) {
			return $new_user(
				'oracle-update',
				array(
					'first_name'          => 'Old',
					'description'         => 'Old \\ bio',
					'user_activation_key' => '1:key',
				)
			);
		},
		'action' => function ( $id ) {
			wp_update_user(
				array(
					'ID'          => $id,
					'first_name'  => 'New',
					'user_email'  => 'new@example.test',
					'admin_color' => 'light',
					'role'        => 'editor',
				)
			);
		},
	),
	'user-update-password'   => array(
		'setup'  => function () use ( $new_user ) {
			return $new_user( 'oracle-password', array( 'user_activation_key' => '1:key' ) );
		},
		'action' => function ( $id ) {
			wp_update_user(
				array(
					'ID'        => $id,
					'user_pass' => 'Another pass phrase',
				)
			);
		},
	),
	'user-insert-with-id'    => array(
		'setup'  => function () use ( $new_user ) {
			return $new_user( 'oracle-raw', array( 'user_activation_key' => '1:key' ) );
		},
		'action' => function ( $id ) {
			// An update's user_pass is taken for a hash and stored as given.
			wp_insert_user(
				array(
					'ID'         => $id,
					'user_login' => 'oracle-raw',
					'user_email' => 'oracle-raw@example.test',
					'user_pass'  => 'stored-as-given',
				)
			);
		},
	),
	'user-set-role'          => array(
		'setup'  => function () use ( $new_user ) {
			return $new_user( 'oracle-role' );
		},
		'action' => function ( $id ) {
			$user = new WP_User( $id );
			$user->set_role( 'subscriber' );
			$user->set_role( 'author' );
		},
	),
	'user-add-remove-role'   => array(
		'setup'  => function () use ( $new_user ) {
			return $new_user( 'oracle-roles' );
		},
		'action' => function ( $id ) {
			$user = new WP_User( $id );
			$user->add_role( 'editor' );
			$user->add_role( 'editor' );
			$user->remove_role( 'subscriber' );
			$user->remove_role( 'author' );
		},
	),
	'user-unknown-role'      => array(
		'setup'  => function () use ( $new_user ) {
			return $new_user( 'oracle-ghost' );
		},
		'action' => function ( $id ) {
			$user = new WP_User( $id );
			$user->add_role( '' );
			$user->set_role( 'ghost' );
			$user->set_role( 'ghost' );
		},
	),
	'user-delete'            => array(
		'setup'  => function () use ( $new_user ) {
			return $new_user( 'oracle-delete' );
		},
		'action' => function ( $id ) {
			wp_delete_user( $id );
		},
	),
	'user-delete-reassign'   => array(
		'setup'  => function () use ( $new_user ) {
			return $new_user( 'oracle-heir' );
		},
		'action' => function ( $id ) {
			wp_delete_user( 1, $id );
		},
	),
	'option-add'             => array(
		'action' => function () {
			add_option( 'oracle_text', 'A value' );
			add_option( '  oracle_trimmed  ', 2 );
			add_option(
				'oracle_list',
				array(
					'a' => 1,
					'b' => array( true, null, 1.5 ),
				),
				'',
				false
			);
			add_option( 'oracle_yes', 'x', '', 'yes' );
			add_option( 'oracle_no', 'x', '', 'no' );
			add_option( 'oracle_true', 'x', '', true );
			add_option( 'oracle_serialized', 'a:1:{i:0;i:1;}' );
			add_option( 'blogname', 'Not added' );
			add_option( '', 'Not added' );
		},
	),
	'option-update'          => array(
		'setup'  => function () use ( $raw_options ) {
			add_option( 'oracle_off', 'one' );
			$raw_options(
				array(
					'oracle_same'  => 'one',
					'oracle_auto'  => 'one',
					'oracle_large' => 'one',
					'oracle_false' => 'b:0;',
					'siteurl'      => 'http://127.0.0.1:8081/',
					'home'         => '',
				)
			);
		},
		'action' => function () {
			update_option( 'oracle_same', 'one' );
			update_option( 'oracle_auto', array( 'x' ) );
			update_option( 'oracle_large', str_repeat( 'x', 150001 ) );
			update_option( 'oracle_off', 'two', false );
			update_option( 'oracle_missing', 'new' );
			update_option( 'oracle_false', 'set' );
			// Unchanged as get_option() reads them: no trailing slash, and an
			// empty home is the site URL.
			update_option( 'siteurl', 'http://127.0.0.1:8081' );
			update_option( 'home', 'http://127.0.0.1:8081' );
		},
	),
	'option-delete'          => array(
		'action' => function () {
			delete_option( 'blogdescription' );
			delete_option( 'oracle_none' );
		},
	),
	'transient-set'          => array(
		'action' => function () {
			set_transient( 'oracle', 'v' );
			set_transient( 'oracle_expiring', array( 1 ), 3600 );
			set_site_transient( 'oracle', 'v' );
			set_site_transient( 'oracle_expiring', 'v', 60 );
		},
	),
	'transient-update'       => array(
		'setup'  => function () {
			set_transient( 'oracle_a', 'one' );
			set_transient( 'oracle_b', 'one', 3600 );
			set_transient( 'oracle_c', 'one' );
			set_site_transient( 'oracle_s', 'one', 60 );
			set_site_transient( 'oracle_t', 'one' );
		},
		'action' => function () {
			set_transient( 'oracle_a', 'two' );
			set_transient( 'oracle_b', 'two', 7200 );
			set_transient( 'oracle_c', 'two', 60 );
			set_site_transient( 'oracle_s', 'two', 120 );
			set_site_transient( 'oracle_t', 'two' );
		},
	),
	'transient-delete'       => array(
		'setup'  => function () {
			set_transient( 'oracle_a', 'one' );
			set_transient( 'oracle_b', 'one', 3600 );
			set_site_transient( 'oracle_s', 'one', 60 );
		},
		'action' => function () {
			delete_transient( 'oracle_a' );
			delete_transient( 'oracle_b' );
			delete_site_transient( 'oracle_s' );
			delete_transient( 'oracle_none' );
			delete_site_transient( 'oracle_none' );
		},
	),
	'meta-add'               => array(
		'action' => function () {
			add_metadata( 'post', 1, 'oracle_key', 'one' );
			add_metadata( 'post', 1, 'oracle_key', 'two' );
			add_metadata( 'post', 1, 'oracle_unique', 'u', true );
			add_metadata( 'post', 1, 'oracle_unique', 'u2', true );
			add_metadata( 'comment', 1, 'oracle_list', array( 'a' => 'b\\c' ) );
			add_metadata( 'term', 1, 'oracle_slash', 'a\\b' );
			add_metadata( 'user', 1, 'oracle_int', 5 );
			add_metadata( 'user', 1, 'oracle_\\key', false );
			add_metadata( 'post', 0, 'oracle_key', 'none' );
			add_metadata( 'post', 1, '', 'none' );
			add_metadata( 'post', 1, '0', 'none' );
		},
	),
	'meta-update'            => array(
		'setup'  => function () use ( $raw_meta ) {
			add_metadata( 'post', 1, 'oracle_one', 'one' );
			add_metadata( 'post', 1, 'oracle_two', 'a' );
			add_metadata( 'post', 1, 'oracle_two', 'b' );
			add_metadata( 'post', 1, 'oracle_both', 'a' );
			add_metadata( 'post', 1, 'oracle_both', 'b' );
			add_metadata( 'post', 1, 'oracle_list', array( 1 ) );
			$raw_meta( 1, 'oracle_str', 's:3:"abc";' );
			$raw_meta( 1, 'oracle_num', '5' );
		},
		'action' => function () {
			update_metadata( 'post', 1, 'oracle_one', 'one' );
			update_metadata( 'post', 1, 'oracle_list', array( 1 ) );
			update_metadata( 'post', 1, 'oracle_str', 'abc' );
			update_metadata( 'post', 1, 'oracle_num', 5 );
			update_metadata( 'post', 1, 'oracle_two', 'c', 'b' );
			update_metadata( 'post', 1, 'oracle_two', 'd', 'none' );
			update_metadata( 'post', 1, 'oracle_both', 'z' );
			update_metadata( 'post', 1, 'oracle_new', 'n' );
			update_metadata( 'user', 1, 'oracle_list', array( 1, 2 ) );
			update_metadata( 'term', 1, 'oracle_new', 'n', 'ignored' );
			update_metadata( 'post', 0, 'oracle_one', 'none' );
			update_metadata( 'post', 1, '', 'none' );
		},
	),
	'meta-null'              => array(
		'setup'  => function () {
			add_metadata( 'post', 1, 'oracle_was', 'x' );
		},
		'action' => function () {
			add_metadata( 'post', 1, 'oracle_null', null );
			add_metadata( 'user', 1, 'oracle_null', null );
			update_metadata( 'post', 1, 'oracle_was', null );
			update_metadata( 'post', 1, 'oracle_null', null );
		},
	),
	'meta-delete'            => array(
		'setup'  => function () {
			add_metadata( 'post', 1, 'oracle_del', 'a' );
			add_metadata( 'post', 1, 'oracle_del', 'b' );
			add_metadata( 'post', 1, 'oracle_del', 'c' );
			add_metadata( 'post', 1, 'oracle_all', 'x' );
			add_metadata( 'post', 2, 'oracle_all', 'x' );
			add_metadata( 'post', 3, 'oracle_all', 'y' );
			add_metadata( 'post', 1, 'oracle_every', 'z' );
			add_metadata( 'post', 2, 'oracle_every', 'z' );
			add_metadata( 'post', 1, 'oracle_list', array( 1 ) );
			add_metadata( 'post', 2, 'oracle_zero', '0' );
		},
		'action' => function () {
			delete_metadata( 'post', 1, 'oracle_del', 'a' );
			delete_metadata( 'post', 0, 'oracle_all', 'x', true );
			delete_metadata( 'post', 0, 'oracle_every', '', true );
			delete_metadata( 'post', 1, 'oracle_list', array( 1 ) );
			delete_metadata( 'post', 2, 'oracle_zero', 0 );
			delete_metadata( 'post', 1, 'oracle_del', 'none' );
			delete_metadata( 'post', 1, 'oracle_del' );
			delete_metadata( 'post', 0, 'oracle_del' );
			delete_metadata( 'post', 1, '' );
		},
	),
);

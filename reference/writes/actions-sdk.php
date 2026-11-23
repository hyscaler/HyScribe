<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The write oracle's actions for SDK 0.2's host calls (MVP 11.4), loaded
// beside actions.php: transients read and deleted, and roles added,
// removed and given capabilities, as extensions do them. Names start with
// `oracle_`, so they never clash with the fixture's.

return array(
	'transient-expired'     => array(
		'setup'  => function () {
			set_transient( 'oracle_old', 'old', 3600 );
			set_transient( 'oracle_fresh', 'fresh', 3600 );
			set_transient( 'oracle_kept', 'kept' );
			update_option( '_transient_timeout_oracle_old', time() - 10 );
			add_option( '_transient_timeout_oracle_lone', time() - 10, '', false );
			add_option( '_transient_timeout_oracle_kept', time() - 10, '', false );
		},
		'action' => function () {
			get_transient( 'oracle_old' );
			get_transient( 'oracle_fresh' );
			get_transient( 'oracle_lone' );
			get_transient( 'oracle_kept' );
		},
	),
	'site-transient-delete' => array(
		'setup'  => function () {
			set_site_transient( 'oracle_a', 'one' );
			set_site_transient( 'oracle_b', 'one', 3600 );
			foreach ( array( 'update_core', 'oracle_c' ) as $name ) {
				add_option( "_site_transient_$name", 'x', '', false );
				add_option( "_site_transient_timeout_$name", time() - 10, '', false );
			}
		},
		'action' => function () {
			delete_site_transient( 'oracle_a' );
			delete_site_transient( 'oracle_b' );
			delete_site_transient( 'oracle_none' );
			get_site_transient( 'update_core' );
			get_site_transient( 'oracle_c' );
		},
	),
	'roles-add'             => array(
		'action' => function () {
			add_role(
				'oracle_manager',
				'Oracle Manager',
				array(
					'read'       => true,
					'oracle_cap' => true,
					'edit_posts' => false,
					'read'       => false,
				)
			);
			add_role( 'editor', 'Again' );
			add_role( '0', 'Empty name' );
			add_role( 'oracle_empty', 'Empty' );
		},
	),
	'roles-caps'            => array(
		'action' => function () {
			get_role( 'administrator' )->add_cap( 'oracle_cap' );
			get_role( 'editor' )->add_cap( 'oracle_cap', false );
			get_role( 'author' )->remove_cap( 'upload_files' );
			get_role( 'author' )->remove_cap( 'oracle_none' );
			get_role( 'contributor' )->add_cap( 'read', true );
			wp_roles()->add_cap( 'oracle_missing', 'oracle_cap' );
		},
	),
	'roles-remove'          => array(
		'setup'  => function () {
			add_role( 'oracle_gone', 'Gone', array( 'read' => true ) );
			update_option( 'default_role', 'oracle_gone' );
		},
		'action' => function () {
			remove_role( 'oracle_gone' );
			remove_role( 'oracle_none' );
			remove_role( 'contributor' );
		},
	),
);

<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The write oracle's dashboard and screen state writes (MVP 3.6), loaded
// beside actions.php: WordPress's own functions, and the Quick Draft case of
// wp-admin/post.php, over the request hs-admin gets, with what they print
// dropped and wp_die() and redirects ending them as exit would.

// Runs `$run` as a request of the admin (or of admin-ajax.php with `$ajax`).
$views_run = function ( $run, $ajax = false ) use ( $admin_run ) {
	require_once ABSPATH . 'wp-admin/includes/dashboard.php';
	$GLOBALS['current_screen'] = WP_Screen::get( 'dashboard' );
	$admin_run( $run, $ajax );
};

// The request's `$_POST`, slashed as wp_magic_quotes() leaves it, with
// user 1's nonce of `$action` in `$field`.
$views_post = function ( $fields, $action, $field ) {
	$fields[ $field ] = wp_create_nonce( $action );
	$_POST            = wp_slash( $fields );
	$_REQUEST         = $_POST;
};

// An auto-draft as the dashboard writes it, kept as the user's.
$views_draft = function () {
	$post = get_default_post_to_edit( 'post', true );
	update_user_option( 1, 'dashboard_quick_press_last_post_id', (int) $post->ID );
	return $post->ID;
};

return array(
	// The Quick Draft form's first auto-draft.
	'admin-quick-press'            => array(
		'action' => function () use ( $views_run ) {
			$views_run( 'wp_dashboard_quick_press' );
		},
	),
	// A new one when the user's is a draft now.
	'admin-quick-press-stale'      => array(
		'setup'  => function () use ( $views_draft ) {
			$id = $views_draft();
			wp_update_post(
				array(
					'ID'          => $id,
					'post_status' => 'draft',
				)
			);
		},
		'action' => function () use ( $views_run ) {
			$views_run( 'wp_dashboard_quick_press' );
		},
	),
	// post.php's Quick Draft save, then the form's new auto-draft.
	'admin-quickdraft-save'        => array(
		'setup'  => $views_draft,
		'action' => function ( $id ) use ( $views_run, $views_post, $admin_block ) {
			$views_post(
				array(
					'action'     => 'post-quickdraft-save',
					'post_ID'    => (string) $id,
					'post_type'  => 'post',
					'post_title' => 'A quick draft',
					'content'    => "One & \"two\"\r\nthree",
				),
				'add-post',
				'_wpnonce'
			);
			$code = $admin_block( 'post.php', "\n\tcase 'post-quickdraft-save':\n", "\n\tcase '" );
			$views_run(
				function () use ( $code ) {
					eval( $code );
				}
			);
		},
	),
	// wp_user_settings() with a cookie newer than the saved settings.
	'admin-user-settings-newer'    => array(
		'action' => function () use ( $views_run ) {
			$_COOKIE['wp-settings-1']      = 'mfold=f&editor=html';
			$_COOKIE['wp-settings-time-1'] = '1';
			$views_run( 'wp_user_settings' );
		},
	),
	// A list's ?mode= (set_user_setting()), then wp_user_settings().
	'admin-user-settings-mode'     => array(
		'action' => function () use ( $views_run ) {
			$_COOKIE['wp-settings-1']      = 'mfold=f';
			$_COOKIE['wp-settings-time-1'] = '1';
			$views_run(
				function () {
					set_user_setting( 'posts_list_mode', 'excerpt' );
					wp_user_settings();
				}
			);
		},
	),
	// The dashboard's ?welcome=0, from WP_Screen::render_screen_options().
	'admin-welcome-hidden'         => array(
		'action' => function () use ( $views_run, $admin_block ) {
			$_GET['welcome'] = '0';
			$code            = $admin_block( 'includes/class-wp-screen.php', "if ( isset( \$_GET['welcome'] ) ) {", '} else' );
			$views_run(
				function () use ( $code ) {
					eval( $code );
				}
			);
		},
	),
	'admin-ajax-closed-postboxes'  => array(
		'action' => function () use ( $views_run, $views_post ) {
			$views_post(
				array(
					'closed' => 'dashboard_activity,,0',
					'hidden' => 'dashboard_primary,submitdiv',
					'page'   => 'dashboard',
				),
				'closedpostboxes',
				'closedpostboxesnonce'
			);
			$views_run( 'wp_ajax_closed_postboxes', true );
		},
	),
	'admin-ajax-hidden-columns'    => array(
		'action' => function () use ( $views_run, $views_post ) {
			$views_post(
				array(
					'hidden' => 'author,tags',
					'page'   => 'edit-post',
				),
				'screen-options-nonce',
				'screenoptionnonce'
			);
			$views_run( 'wp_ajax_hidden_columns', true );
		},
	),
	'admin-ajax-meta-box-order'    => array(
		'action' => function () use ( $views_run, $views_post ) {
			$views_post(
				array(
					'order'        => array(
						'normal' => 'dashboard_quick_press,dashboard_site_health',
						'side'   => 'dashboard_right_now',
					),
					'page_columns' => '2',
					'page'         => 'dashboard',
				),
				'meta-box-order',
				'_ajax_nonce'
			);
			$views_run( 'wp_ajax_meta_box_order', true );
		},
	),
	'admin-ajax-welcome-panel'     => array(
		'action' => function () use ( $views_run, $views_post ) {
			$views_post( array( 'visible' => '' ), 'welcome-panel-nonce', 'welcomepanelnonce' );
			$views_run( 'wp_ajax_update_welcome_panel', true );
		},
	),
	// admin.php's set_screen_options() for the posts list.
	'admin-screen-options'         => array(
		'action' => function () use ( $views_run, $views_post ) {
			$views_post(
				array(
					'wp_screen_options' => array(
						'option' => 'edit_post_per_page',
						'value'  => '5',
					),
				),
				'screen-options-nonce',
				'screenoptionnonce'
			);
			$views_run( 'set_screen_options' );
		},
	),
);

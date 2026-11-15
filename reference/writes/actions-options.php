<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The write oracle's Settings saves (MVP 3.5), loaded beside actions.php:
// options.php, the Permalinks screen's save and the Privacy screen's
// actions, taken from WordPress's own source and run over the request
// hs-admin's handlers get, past the checks, with `exit` a return. The
// oracle defines WP_HOME and WP_SITEURL; these cases run as a site without
// them, as HyScribe's tests do, so the addresses can be saved.

// The admin's functions and hooks: add_settings_error(), update_home_siteurl().
$admin = function () {
	require_once ABSPATH . 'wp-admin/includes/admin.php';
	remove_filter( 'option_home', '_config_wp_home' );
	remove_filter( 'option_siteurl', '_config_wp_siteurl' );
};

// `$_GET` and `$_POST` as wp_magic_quotes() leaves them.
$request = function ( $post, $get = array() ) {
	$_GET     = wp_slash( $get );
	$_POST    = wp_slash( $post );
	$_REQUEST = array_merge( $_GET, $_POST );
};

// The part of wp-admin/$file from $from up to $to.
$source = function ( $file, $from, $to ) {
	$text  = file_get_contents( ABSPATH . "wp-admin/$file" );
	$start = strpos( $text, $from );
	$code  = substr( $text, $start, strpos( $text, $to, $start ) - $start );
	$code  = str_replace( array( "! defined( 'WP_SITEURL' )", "! defined( 'WP_HOME' )" ), 'true', $code );
	return str_replace( 'exit;', 'return;', $code );
};

// options.php's handling of its request, up to its page.
$options_php = function () use ( $source ) {
	eval( $source( 'options.php', '$action      =', "require_once ABSPATH . 'wp-admin/admin-header.php';" ) );
};

// A save of the options page $page with the fields $fields.
$save = function ( $page, $fields ) use ( $admin, $request, $options_php ) {
	$admin();
	$request(
		array_merge(
			array(
				'option_page'      => $page,
				'action'           => 'update',
				'_wpnonce'         => wp_create_nonce( "$page-options" ),
				'_wp_http_referer' => "/wp-admin/options-$page.php",
			),
			$fields
		)
	);
	$options_php();
};

// A pending change of the admin email, as update_option_new_admin_email() leaves it.
$pending = function () {
	add_option(
		'adminhash',
		array(
			'hash'     => '0123456789abcdef0123456789abcdef',
			'newemail' => 'new@example.test',
		),
		'',
		false
	);
	add_option( 'new_admin_email', 'new@example.test' );
};

// Pretty permalinks, as a request on such a site starts with them: the
// taxonomies registered again add their archives' structures.
$postname = function () {
	$GLOBALS['wp_rewrite']->set_permalink_structure( '/%postname%/' );
	create_initial_taxonomies();
};

// What options.php keeps after a save.
$kept = function () {
	set_transient(
		'settings_errors',
		array(
			array(
				'setting' => 'general',
				'code'    => 'settings_updated',
				'message' => 'Settings saved.',
				'type'    => 'success',
			),
		),
		30
	);
};

// options-head.php's settings_errors() on the screen sent back.
$shown = function () use ( $admin, $request ) {
	$admin();
	$request( array(), array( 'settings-updated' => 'true' ) );
	get_settings_errors();
};

return array(
	// Every General field changed: a custom date format, a UTC offset, a new
	// admin email, which waits for its confirmation, and an HTTPS home on a
	// site no longer fresh, which then needs its content migrated and
	// refreshes the rewrite rules.
	'admin-options-general'        => array(
		'setup'  => function () use ( $postname ) {
			$postname();
			update_option( 'fresh_site', '0' );
		},
		'action' => function () use ( $save ) {
			$save(
				'general',
				array(
					'blogname'           => 'An "Oracle" <b>site</b> & co',
					'blogdescription'    => ' Just a tagline ',
					'siteurl'            => 'http://default-install.test',
					'home'               => 'https://default-install.test/',
					'new_admin_email'    => 'Oracle@Example.test',
					'users_can_register' => '1',
					'default_role'       => 'editor',
					'WPLANG'             => '',
					'timezone_string'    => 'UTC+5.5',
					'date_format'        => '\c\u\s\t\o\m',
					'date_format_custom' => 'j F Y',
					'time_format'        => 'H:i',
					'time_format_custom' => 'g:i A',
					'start_of_week'      => '0',
					'site_icon'          => '0',
				)
			);
		},
	),
	// Fields PHP refuses: an unknown time zone, a site address and an email
	// that are not one, each a settings error, and nothing saved of them; a
	// home on another host, which needs no HTTPS migration.
	'admin-options-general-errors' => array(
		'setup'  => function () {
			update_option( 'https_migration_required', true );
		},
		'action' => function () use ( $save ) {
			$save(
				'general',
				array(
					'blogname'        => 'HyScribe Default Install',
					'siteurl'         => 'default-install.test',
					'home'            => 'http://example.test',
					'new_admin_email' => 'not an email',
					'timezone_string' => 'Mars/Base',
					'date_format'     => 'F j, Y',
					'time_format'     => 'g:i a',
					'start_of_week'   => '1',
					'default_role'    => 'king',
				)
			);
		},
	),
	'admin-options-writing'        => array(
		'action' => function () use ( $save ) {
			$save(
				'writing',
				array(
					'default_category'       => '2',
					'default_post_format'    => 'aside',
					'mailserver_url'         => 'mail.example.test<b>x</b>',
					'mailserver_port'        => '-995',
					'mailserver_login'       => 'oracle@example.test',
					'mailserver_pass'        => 'a <secret>',
					'default_email_category' => '1',
					'ping_sites'             => "https://rpc.pingomatic.com/\n\n  http://example.test/x  \njavascript:alert(1)",
				)
			);
		},
	),
	// A static front page, which refreshes the rewrite rules.
	'admin-options-reading'        => array(
		'setup'  => $postname,
		'action' => function () use ( $save ) {
			$save(
				'reading',
				array(
					'posts_per_page' => '-7',
					'posts_per_rss'  => '0',
					'rss_use_excerpt' => '1',
					'show_on_front'  => 'page',
					'page_on_front'  => '2',
					'page_for_posts' => '0',
					'blog_public'    => '0',
				)
			);
		},
	),
	// Unchecked boxes are not sent, and save as empty.
	'admin-options-discussion'     => array(
		'action' => function () use ( $save ) {
			$save(
				'discussion',
				array(
					'default_comment_status'  => 'open',
					'require_name_email'      => '1',
					'comment_max_links'       => '-3',
					'moderation_keys'         => "spam\n spam \n\nviagra",
					'disallowed_keys'         => "\n",
					'close_comments_days_old' => '14.5',
					'thread_comments'         => '1',
					'thread_comments_depth'   => '7',
					'comments_per_page'       => 'x',
					'default_comments_page'   => 'oldest',
					'comment_order'           => 'desc',
					'avatar_rating'           => 'PG',
					'avatar_default'          => 'identicon',
				)
			);
		},
	),
	'admin-options-media'          => array(
		'action' => function () use ( $save ) {
			$save(
				'media',
				array(
					'thumbnail_size_w' => '200',
					'thumbnail_size_h' => 'x',
					'medium_size_w'    => '300',
					'medium_size_h'    => '300',
					'large_size_w'     => '2048',
					'large_size_h'     => '1024.9',
				)
			);
		},
	),
	// The privacy group has no options left: the save fails.
	'admin-options-privacy'        => array(
		'action' => function () use ( $save ) {
			$save( 'privacy', array( 'blog_public' => '0' ) );
		},
	),
	'admin-options-permalink'      => array(
		'action' => function () use ( $admin, $request, $source ) {
			global $wp_rewrite;
			$admin();
			$request(
				array(
					'selection'           => 'custom',
					'permalink_structure' => 'archives//%post_id%#',
					'category_base'       => 'topics/ ',
					'tag_base'            => '#',
					'_wpnonce'            => wp_create_nonce( 'update-permalink' ),
				)
			);
			// What the screen sets before, on Apache with mod_rewrite.
			$home_path           = ABSPATH;
			$iis7_permalinks     = false;
			$is_nginx            = false;
			$is_caddy            = false;
			$permalink_structure = get_option( 'permalink_structure' );
			$index_php_prefix    = '';
			$blog_prefix         = '';
			eval( $source( 'options-permalink.php', '$category_base = get_option', 'flush_rewrite_rules();' ) );
		},
	),
	// A pending email change confirmed with its key, and one dismissed.
	'admin-options-adminhash'      => array(
		'setup'  => $pending,
		'action' => function () use ( $admin, $request, $options_php ) {
			$admin();
			$request( array(), array( 'adminhash' => '0123456789abcdef0123456789abcdef' ) );
			$options_php();
		},
	),
	'admin-options-dismiss'        => array(
		'setup'  => $pending,
		'action' => function () use ( $admin, $request, $options_php ) {
			$admin();
			$request(
				array(),
				array(
					'dismiss'  => 'new_admin_email',
					'_wpnonce' => wp_create_nonce( 'dismiss-1-new_admin_email' ),
				)
			);
			$options_php();
		},
	),
	// options-privacy.php's new draft of the suggested policy.
	'admin-privacy-create'         => array(
		'action' => function () use ( $admin, $request, $source ) {
			$admin();
			$request(
				array(
					'action'   => 'create-privacy-page',
					'_wpnonce' => wp_create_nonce( 'create-privacy-page' ),
				)
			);
			$action = $_POST['action'];
			eval( $source( 'options-privacy.php', 'if ( ! empty( $action ) ) {', '// If a Privacy Policy page ID is available' ) );
		},
	),
	// The screen a save sends back shows the notices it kept and deletes
	// them, so a reload does not show them again; an expired one is deleted.
	'admin-options-notice-shown'   => array(
		'setup'  => $kept,
		'action' => $shown,
	),
	'admin-options-notice-expired' => array(
		'setup'  => function () use ( $kept ) {
			$kept();
			update_option( '_transient_timeout_settings_errors', time() - 60 );
		},
		'action' => $shown,
	),
);

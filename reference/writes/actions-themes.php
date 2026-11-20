<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The write oracle's theme switches (MVP 10.2), loaded beside actions.php:
// themes.php's `activate` case, taken from WordPress's own source and run
// over the request hs-admin's handler gets, past the checks, with `exit` a
// return, in the admin as the oracle's web requests are, and what
// check_theme_switched() writes on the next load. A web request's own
// writes were compared with these: the switch writes the old theme's
// `sidebars_widgets` theme mod from the admin's `$sidebars_widgets`, and
// the next load carries the widgets and the menu locations over.

// The site every case starts from: PHP's first page view leaves a pattern
// cache, named after the server's folder, and the theme's mods, so those are
// put as the tests' fixture has them (no cache, and the mods a first view
// writes).
$themes_base = function () {
	global $wpdb;
	$wpdb->query( "DELETE FROM {$wpdb->options} WHERE option_name LIKE '%wp_theme_files_patterns-%' OR option_name = 'theme_mods_twentytwentyfive'" );
	wp_cache_flush();
	update_option( 'theme_mods_twentytwentyfive', array( 'custom_css_post_id' => -1 ) );
};

// themes.php's activation of `$stylesheet`, as the administrator's request.
$themes_activate = function ( $stylesheet ) use ( $admin_code, $admin_run ) {
	set_current_screen( 'themes' );
	// What the admin has loaded before the screen's code runs.
	$GLOBALS['sidebars_widgets'] = wp_get_sidebars_widgets();
	$_GET     = array(
		'action'     => 'activate',
		'stylesheet' => $stylesheet,
		'_wpnonce'   => wp_create_nonce( 'switch-theme_' . $stylesheet ),
	);
	$_REQUEST = $_GET;
	$code     = $admin_code( 'themes.php', "/if \( 'activate' === \\\$_GET\['action'\] \) \{\n(.*?)\n\t\} elseif \( 'resume'/s" );
	$admin_run( fn() => eval( $code ) );
};

// The next load: a new process has no `$sidebars_widgets` yet, and registers
// the widgets the options hold now.
$themes_next_load = function () {
	global $wp_widget_factory, $wp_registered_widgets;
	unset( $GLOBALS['sidebars_widgets'] );
	$wp_registered_widgets = array();
	foreach ( $wp_widget_factory->widgets as $widget ) {
		$widget->_register();
	}
	check_theme_switched();
};

// A case: its setup after the site every case starts from, and its action.
$themes_case = function ( $setup, $action ) use ( $themes_base ) {
	return array(
		'setup'  => function () use ( $themes_base, $setup ) {
			$themes_base();
			if ( $setup ) {
				$setup();
			}
		},
		'action' => $action,
	);
};

// Widgets in the states a site leaves them in: text widgets 2 and 3, one of
// them orphaned, one inactive, a widget gone, and an empty sidebar.
$themes_widgets = function () {
	update_option(
		'widget_text',
		array(
			2              => array(
				'title' => 'T',
				'text'  => 'x',
			),
			3              => array(
				'title' => 'U',
				'text'  => 'y',
			),
			'_multiwidget' => 1,
		)
	);
	update_option(
		'sidebars_widgets',
		array(
			'wp_inactive_widgets' => array( 'text-2' ),
			'orphaned_widgets_1'  => array( 'text-3' ),
			'sidebar-1'           => array( 'block-2', 'gone-2', 'block-3' ),
			'sidebar-2'           => array(),
			'array_version'       => 3,
		)
	);
};

// Widgets in the sidebars, one inactive.
$themes_kept = function () {
	update_option(
		'sidebars_widgets',
		array(
			'wp_inactive_widgets' => array( 'block-6' ),
			'sidebar-1'           => array( 'block-2', 'block-3' ),
			'sidebar-2'           => array( 'block-4', 'block-5' ),
			'array_version'       => 3,
		)
	);
};

return array(
	'admin-themes-activate-twentytwentyfour'  => $themes_case(
		null,
		fn() => $themes_activate( 'twentytwentyfour' )
	),
	'admin-themes-activate-twentytwentythree' => $themes_case(
		null,
		fn() => $themes_activate( 'twentytwentythree' )
	),
	'admin-themes-activate-active'            => $themes_case(
		null,
		fn() => $themes_activate( 'twentytwentyfive' )
	),
	'admin-themes-next-load-twentytwentyfour' => $themes_case(
		fn() => $themes_activate( 'twentytwentyfour' ),
		$themes_next_load
	),
	// Back to the theme the site had, which kept its widgets.
	'admin-themes-switch-back'                => $themes_case(
		function () use ( $themes_activate, $themes_next_load ) {
			$themes_activate( 'twentytwentyfour' );
			$themes_next_load();
		},
		fn() => $themes_activate( 'twentytwentyfive' )
	),
	'admin-themes-next-load-back'             => $themes_case(
		function () use ( $themes_activate, $themes_next_load ) {
			$themes_activate( 'twentytwentyfour' );
			$themes_next_load();
			$themes_activate( 'twentytwentyfive' );
		},
		$themes_next_load
	),
	// Widgets orphaned, inactive, gone and empty, carried to a block theme.
	'admin-themes-widgets-carried'            => $themes_case(
		$themes_widgets,
		function () use ( $themes_activate, $themes_next_load ) {
			$themes_activate( 'twentytwentyfour' );
			$themes_next_load();
		}
	),
	// Widgets back in the sidebars the theme had, but those still inactive.
	'admin-themes-widgets-revived'            => $themes_case(
		function () use ( $themes_kept, $themes_activate, $themes_next_load ) {
			$themes_kept();
			$themes_activate( 'twentytwentyfour' );
			$themes_next_load();
		},
		function () use ( $themes_activate, $themes_next_load ) {
			$themes_activate( 'twentytwentyfive' );
			$themes_next_load();
		}
	),
	// A site without a sidebar, which gets back to a theme that kept none.
	'admin-themes-no-sidebars'                => $themes_case(
		function () use ( $themes_activate, $themes_next_load ) {
			update_option( 'sidebars_widgets', array( 'array_version' => 3 ) );
			$themes_activate( 'twentytwentyfour' );
			$themes_next_load();
			$themes_activate( 'twentytwentyfive' );
		},
		$themes_next_load
	),
	// Menu locations the theme had, which a block theme has none to keep.
	'admin-themes-menu-locations'             => $themes_case(
		fn() => set_theme_mod( 'nav_menu_locations', array( 'primary' => 7 ) ),
		function () use ( $themes_activate, $themes_next_load ) {
			$themes_activate( 'twentytwentyfour' );
			$themes_next_load();
		}
	),
	// A site with no `sidebars_widgets` option at all.
	'admin-themes-no-widgets-option'          => $themes_case(
		fn() => delete_option( 'sidebars_widgets' ),
		function () use ( $themes_activate, $themes_next_load ) {
			$themes_activate( 'twentytwentyfour' );
			$themes_next_load();
		}
	),
	// Theme mods that are not an array.
	'admin-themes-broken-mods'                => $themes_case(
		fn() => update_option( 'theme_mods_twentytwentyfive', 'broken' ),
		fn() => $themes_activate( 'twentytwentyfour' )
	),
	// A site whose theme has no mods yet, with and without the theme's name kept.
	'admin-themes-no-mods'                    => $themes_case(
		fn() => delete_option( 'theme_mods_twentytwentyfive' ),
		fn() => $themes_activate( 'twentytwentyfour' )
	),
	'admin-themes-no-mods-named'              => $themes_case(
		function () {
			delete_option( 'theme_mods_twentytwentyfive' );
			update_option( 'current_theme', 'Twenty Twenty-Five' );
		},
		fn() => $themes_activate( 'twentytwentyfour' )
	),
	// A widget the theme had that is not inactive now, and a site that emptied every sidebar.
	'admin-themes-widgets-lost'               => $themes_case(
		function () use ( $themes_kept, $themes_activate, $themes_next_load ) {
			$themes_kept();
			$themes_activate( 'twentytwentyfour' );
			$themes_next_load();
			update_option(
				'sidebars_widgets',
				array(
					'wp_inactive_widgets' => array( 'block-3', 'block-4', 'block-5', 'block-6' ),
					'array_version'       => 3,
				)
			);
		},
		function () use ( $themes_activate, $themes_next_load ) {
			$themes_activate( 'twentytwentyfive' );
			$themes_next_load();
		}
	),
	'admin-themes-widgets-emptied'            => $themes_case(
		function () use ( $themes_kept, $themes_activate, $themes_next_load ) {
			$themes_kept();
			$themes_activate( 'twentytwentyfour' );
			$themes_next_load();
			update_option(
				'sidebars_widgets',
				array(
					'wp_inactive_widgets' => array(),
					'sidebar-1'           => array(),
					'array_version'       => 3,
				)
			);
		},
		function () use ( $themes_activate, $themes_next_load ) {
			$themes_activate( 'twentytwentyfive' );
			$themes_next_load();
		}
	),
	// Inactive and orphaned widgets listed after a sidebar.
	'admin-themes-widgets-order'              => $themes_case(
		function () {
			update_option(
				'sidebars_widgets',
				array(
					'sidebar-1'           => array( 'block-2' ),
					'wp_inactive_widgets' => array( 'block-3' ),
					'sidebar-2'           => array( 'block-4' ),
					'orphaned_widgets_2'  => array( 'block-5' ),
					'array_version'       => 3,
				)
			);
		},
		function () use ( $themes_activate, $themes_next_load ) {
			$themes_activate( 'twentytwentyfour' );
			$themes_next_load();
		}
	),
	// A switch the Customizer carried the menus and widgets over for.
	'admin-themes-via-customizer'             => $themes_case(
		function () use ( $themes_activate ) {
			$themes_activate( 'twentytwentyfour' );
			update_option( 'theme_switched_via_customizer', true );
		},
		$themes_next_load
	),
	// The pattern caches of the themes switched, as PHP leaves them, and
	// another's, which a switch leaves.
	'admin-themes-pattern-caches'             => $themes_case(
		function () {
			foreach ( array( 'twentytwentyfive', 'twentytwentyfour', 'other' ) as $stylesheet ) {
				$patterns = array( 'one.php' => array( 'slug' => "$stylesheet/one" ) );
				set_site_transient( 'wp_theme_files_patterns-' . md5( get_theme_root() . '/' . $stylesheet ), array( 'version' => '1', 'patterns' => $patterns ), DAY_IN_SECONDS );
			}
		},
		fn() => $themes_activate( 'twentytwentyfour' )
	),
);

<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The write oracle's navigation fallback (WP_Navigation_Fallback, MVP 8.5
// follow-up), loaded beside actions.php: what a visitor's page view writes
// when the navigation block has no menu and no `wp_navigation` post exists,
// and a classic menu does. The setup makes the classic menus as the
// administrator and takes out the post php.sh's first page view wrote; the
// action runs as a visitor, as a page view does.

$navigation_menu = function ( $name, $items ) {
	$menu_id = wp_create_nav_menu( $name );
	$ids     = array();
	foreach ( $items as $position => $item ) {
		$parent = isset( $item['parent'] ) ? $ids[ $item['parent'] ] : 0;
		unset( $item['parent'] );
		$ids[] = wp_update_nav_menu_item(
			$menu_id,
			0,
			array(
				'menu-item-type'      => 'custom',
				'menu-item-status'    => 'publish',
				'menu-item-position'  => $position + 1,
				'menu-item-parent-id' => $parent,
			) + $item
		);
	}
	return $menu_id;
};

$navigation_visitor = function () {
	global $wpdb;
	$wpdb->query( "DELETE FROM {$wpdb->posts} WHERE post_type = 'wp_navigation'" );
	wp_cache_flush();
	wp_set_current_user( 0 );
};

// WP_Navigation_Fallback::get_fallback(), as `block_core_navigation_get_fallback_blocks()` calls it.
$navigation_get = function () {
	WP_Navigation_Fallback::get_fallback();
};

$plain = array(
	array(
		'menu-item-title' => 'twitter.com',
		'menu-item-url'   => 'https://twitter.com/wordpress',
	),
	array(
		'menu-item-title' => 'Docs & "News" <b>bold</b> -- Ελληνικά',
		'menu-item-url'   => 'https://example.test/docs?a=1&b=2#top',
	),
);

return array(
	'navigation-fallback-classic'          => array(
		'setup'  => function () use ( $navigation_menu, $navigation_visitor, $plain ) {
			$navigation_menu( 'An older menu', $plain );
			$navigation_menu(
				'Oracle Menu',
				array(
					array(
						'menu-item-title' => 'Home',
						'menu-item-url'   => 'https://example.test/',
					),
					array(
						'menu-item-title'       => 'About',
						'menu-item-url'         => 'https://example.test/about',
						'menu-item-description' => 'Who we are',
						'menu-item-attr-title'  => 'The about page',
						'menu-item-target'      => '_blank',
						'menu-item-xfn'         => 'nofollow',
						'menu-item-classes'     => 'tall wide',
					),
					array(
						'menu-item-title' => 'Team',
						'menu-item-url'   => 'https://example.test/team',
						'parent'          => 1,
					),
					array(
						'menu-item-title' => 'Careers',
						'menu-item-url'   => 'https://example.test/careers',
						'parent'          => 2,
					),
					array(
						'menu-item-title' => 'Contact',
						'menu-item-url'   => '#',
					),
				)
			);
			$navigation_visitor();
		},
		'action' => $navigation_get,
	),
	'navigation-fallback-escapes'          => array(
		'setup'  => function () use ( $navigation_menu, $navigation_visitor, $plain ) {
			$navigation_menu( 'Escapes', $plain );
			$navigation_visitor();
		},
		'action' => $navigation_get,
	),
	'navigation-fallback-slug'             => array(
		'setup'  => function () use ( $navigation_menu, $navigation_visitor, $plain ) {
			$menu_id = $navigation_menu( 'Odd Name', array_slice( $plain, 0, 1 ) );
			wp_update_term( $menu_id, 'nav_menu', array( 'slug' => 'a-slug-of-its-own' ) );
			$navigation_visitor();
		},
		'action' => $navigation_get,
	),
	'navigation-fallback-primary-location' => array(
		'setup'  => function () use ( $navigation_menu, $navigation_visitor, $plain ) {
			$chosen = $navigation_menu( 'Chosen by location', array_slice( $plain, 0, 1 ) );
			$navigation_menu( 'Newer menu', array_slice( $plain, 1 ) );
			set_theme_mod( 'nav_menu_locations', array( 'primary' => $chosen ) );
			$navigation_visitor();
		},
		'action' => $navigation_get,
	),
	'navigation-fallback-primary-slug'     => array(
		'setup'  => function () use ( $navigation_menu, $navigation_visitor, $plain ) {
			$navigation_menu( 'Primary', array_slice( $plain, 0, 1 ) );
			$navigation_menu( 'Newer menu', array_slice( $plain, 1 ) );
			$navigation_visitor();
		},
		'action' => $navigation_get,
	),
);

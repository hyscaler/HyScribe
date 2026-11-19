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
			$item + array(
				'menu-item-type'      => 'custom',
				'menu-item-status'    => 'publish',
				'menu-item-position'  => $position + 1,
				'menu-item-parent-id' => $parent,
			)
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

// WP_Navigation_Fallback::get_fallback(), as `block_core_navigation_get_fallback_blocks()` calls it,
// in the request a page view is: its host, its URI and its main query.
$navigation_get = function ( $uri = '/', $query = array() ) {
	return function () use ( $uri, $query ) {
		// A query that needs what the setup made is a function.
		$query = is_callable( $query ) ? $query() : $query;
		$_SERVER['HTTP_HOST']   = '127.0.0.1:8081';
		$_SERVER['REQUEST_URI'] = $uri;
		$GLOBALS['wp_query']    = new WP_Query();
		$GLOBALS['wp_query']->query( $query );
		WP_Navigation_Fallback::get_fallback();
	};
};

// Categories with IDs of their own, so the menus' blocks name no ID that
// differs by run: a parent and its child, both on the first post.
$navigation_categories = function ( $post_categories = array( 1, 9002 ) ) {
	global $wpdb;
	foreach ( array( array( 9001, 'Parent Category', 'parent-category', 0 ), array( 9002, 'Child Category', 'child-category', 9001 ) ) as list( $id, $name, $slug, $parent ) ) {
		$wpdb->insert( $wpdb->terms, array( 'term_id' => $id, 'name' => $name, 'slug' => $slug ) );
		$wpdb->insert( $wpdb->term_taxonomy, array( 'term_taxonomy_id' => $id, 'term_id' => $id, 'taxonomy' => 'category', 'parent' => $parent, 'count' => 0 ) );
	}
	delete_option( 'category_children' );
	clean_term_cache( array( 9001, 9002 ), 'category' );
	wp_set_object_terms( 1, $post_categories, 'category' );
	// The child category's term is also a tag: a term shared by two taxonomies, which IDs no longer tell apart.
	$wpdb->insert( $wpdb->term_taxonomy, array( 'term_taxonomy_id' => 9003, 'term_id' => 9002, 'taxonomy' => 'post_tag', 'parent' => 0, 'count' => 0 ) );
	clean_term_cache( array( 9002 ), 'post_tag' );
};

$navigation_attachment = 0;

$navigation_page = function ( $id, $args ) {
	wp_update_post( array( 'ID' => $id ) + $args );
};

// The REST route the site editor asks (WP_REST_Navigation_Fallback_Controller),
// as the administrator, with the post php.sh's first page view wrote taken out:
// the same post, by the current user.
$navigation_rest = function () {
	global $wpdb;
	$wpdb->query( "DELETE FROM {$wpdb->posts} WHERE post_type = 'wp_navigation'" );
	wp_cache_flush();
};

$navigation_rest_get = function () {
	$request = new WP_REST_Request( 'GET', '/wp-block-editor/v1/navigation-fallback' );
	$request->set_query_params( array( '_embed' => '1' ) );
	$response = rest_do_request( $request );
	$response = rest_get_server()->response_to_data( $response, true );
	if ( ! isset( $response['id'] ) ) {
		fwrite( STDERR, wp_json_encode( $response ) . "\n" );
		exit( 1 );
	}
};

$object = function ( $type, $object, $id, $extra = array() ) {
	return array(
		'menu-item-type'      => $type,
		'menu-item-object'    => $object,
		'menu-item-object-id' => $id,
	) + $extra;
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
		'action' => $navigation_get(),
	),
	'navigation-fallback-escapes'          => array(
		'setup'  => function () use ( $navigation_menu, $navigation_visitor, $plain ) {
			$navigation_menu( 'Escapes', $plain );
			$navigation_visitor();
		},
		'action' => $navigation_get(),
	),
	'navigation-fallback-slug'             => array(
		'setup'  => function () use ( $navigation_menu, $navigation_visitor, $plain ) {
			$menu_id = $navigation_menu( 'Odd Name', array_slice( $plain, 0, 1 ) );
			wp_update_term( $menu_id, 'nav_menu', array( 'slug' => 'a-slug-of-its-own' ) );
			$navigation_visitor();
		},
		'action' => $navigation_get(),
	),
	'navigation-fallback-primary-location' => array(
		'setup'  => function () use ( $navigation_menu, $navigation_visitor, $plain ) {
			$chosen = $navigation_menu( 'Chosen by location', array_slice( $plain, 0, 1 ) );
			$navigation_menu( 'Newer menu', array_slice( $plain, 1 ) );
			set_theme_mod( 'nav_menu_locations', array( 'primary' => $chosen ) );
			$navigation_visitor();
		},
		'action' => $navigation_get(),
	),
	'navigation-fallback-primary-slug'     => array(
		'setup'  => function () use ( $navigation_menu, $navigation_visitor, $plain ) {
			$navigation_menu( 'Primary', array_slice( $plain, 0, 1 ) );
			$navigation_menu( 'Newer menu', array_slice( $plain, 1 ) );
			$navigation_visitor();
		},
		'action' => $navigation_get(),
	),
	// Without a classic menu, or with a menu that converts to nothing, the
	// default fallback: a page list.
	'navigation-fallback-default'          => array(
		'setup'  => function () use ( $navigation_visitor ) {
			$navigation_visitor();
		},
		'action' => $navigation_get(),
	),
	'navigation-fallback-empty-menu'       => array(
		'setup'  => function () use ( $navigation_menu, $navigation_visitor ) {
			$navigation_menu( 'No items', array() );
			$navigation_visitor();
		},
		'action' => $navigation_get(),
	),
	'navigation-fallback-invalid-only'     => array(
		'setup'  => function () use ( $navigation_menu, $navigation_visitor, $navigation_page, $object ) {
			$trashed = wp_insert_post( array( 'post_title' => 'Gone', 'post_type' => 'page', 'post_status' => 'publish' ) );
			wp_trash_post( $trashed );
			$navigation_menu(
				'Only invalid',
				array(
					$object( 'post_type', 'page', $trashed ),
					$object( 'post_type', 'page', 9999 ),
					$object( 'post_type', 'ghost', 1 ),
					$object( 'taxonomy', 'category', 9999 ),
					$object( 'taxonomy', 'ghost', 1 ),
					$object( 'post_type_archive', 'ghost', 0 ),
				)
			);
			$navigation_visitor();
		},
		'action' => $navigation_get(),
	),
	'navigation-fallback-invalid-mixed'    => array(
		'setup'  => function () use ( $navigation_menu, $navigation_visitor, $object, $plain ) {
			$trashed = wp_insert_post( array( 'post_title' => 'Gone', 'post_type' => 'page', 'post_status' => 'publish' ) );
			wp_trash_post( $trashed );
			$navigation_menu(
				'Mixed validity',
				array(
					$object( 'post_type', 'page', $trashed ),
					$plain[0],
					$object( 'post_type', 'page', 9999 ),
					$object( 'post_type', 'page', 2 ),
					$object( 'taxonomy', 'category', 9999 ),
				)
			);
			$navigation_visitor();
		},
		'action' => $navigation_get(),
	),
	// Items of a post, a page, a term and an archive, on the home page.
	'navigation-fallback-objects'          => array(
		'setup'  => function () use ( $navigation_menu, $navigation_visitor, $navigation_page, $navigation_categories, $object ) {
			$navigation_categories();
			$navigation_page( 1, array( 'post_title' => 'Hello "world" -- & <b>more</b>' ) );
			$navigation_page( 3, array( 'post_status' => 'draft' ) );
			update_option( 'page_for_posts', 2 );
			$navigation_menu(
				'Objects',
				array(
					$object( 'post_type', 'page', 2 ),
					$object( 'post_type', 'page', 2, array( 'menu-item-title' => 'Renamed "page"', 'menu-item-target' => '_blank', 'menu-item-classes' => 'x y', 'menu-item-xfn' => 'friend' ) ),
					$object( 'post_type', 'post', 1, array( 'menu-item-description' => 'A post', 'menu-item-attr-title' => 'Tip' ) ),
					$object( 'taxonomy', 'category', 1, array( 'parent' => 2 ) ),
					$object( 'post_type_archive', 'post', 0, array( 'parent' => 2 ) ),
					$object( 'post_type', 'page', 3 ),
					$object( 'taxonomy', 'category', 9002, array( 'parent' => 5 ) ),
					$object( 'post_type_archive', 'page', 0 ),
					$object( 'post_type_archive', 'post', 0, array( 'menu-item-title' => 'Blog' ) ),
				)
			);
			$navigation_visitor();
		},
		'action' => $navigation_get(),
	),
	'navigation-fallback-pretty'           => array(
		'setup'  => function () use ( $navigation_menu, $navigation_visitor, $navigation_categories, $object ) {
			global $wp_rewrite;
			$navigation_categories();
			$wp_rewrite->set_permalink_structure( '/%postname%/' );
			// The permastruct a visitor's request has, as the structure was set before WordPress started.
			get_taxonomy( 'category' )->add_rewrite_rules();
			$navigation_menu(
				'Pretty',
				array(
					$object( 'post_type', 'page', 2 ),
					$object( 'post_type', 'post', 1 ),
					$object( 'taxonomy', 'category', 9002 ),
					$object( 'post_type_archive', 'post', 0 ),
				)
			);
			$navigation_visitor();
		},
		'action' => $navigation_get(),
	),
	'navigation-fallback-posts-page'       => array(
		'setup'  => function () use ( $navigation_menu, $navigation_visitor, $navigation_page, $object ) {
			global $wpdb;
			$navigation_page( 3, array( 'post_status' => 'publish' ) );
			update_option( 'show_on_front', 'page' );
			update_option( 'page_on_front', 3 );
			update_option( 'page_for_posts', 2 );
			// A category of the posts page's ID, which is no reason for it to be current.
			$wpdb->insert( $wpdb->terms, array( 'term_id' => 2, 'name' => 'Same ID', 'slug' => 'same-id' ) );
			$wpdb->insert( $wpdb->term_taxonomy, array( 'term_taxonomy_id' => 2, 'term_id' => 2, 'taxonomy' => 'category', 'parent' => 0, 'count' => 0 ) );
			$navigation_menu(
				'Posts page',
				array(
					$object( 'post_type', 'page', 2 ),
					$object( 'post_type', 'page', 3 ),
					$object( 'taxonomy', 'category', 2 ),
					$object( 'post_type_archive', 'post', 0 ),
					array(
						'menu-item-title' => 'Home',
						'menu-item-url'   => 'http://127.0.0.1:8081/',
					),
				)
			);
			$navigation_visitor();
		},
		'action' => $navigation_get( '/?page_id=2', array( 'page_id' => 2 ) ),
	),
	'navigation-fallback-front-only'       => array(
		'setup'  => function () use ( $navigation_menu, $navigation_visitor, $navigation_page, $object ) {
			$navigation_page( 3, array( 'post_status' => 'publish' ) );
			update_option( 'show_on_front', 'page' );
			update_option( 'page_on_front', 3 );
			$navigation_menu(
				'Front only',
				array(
					$object( 'post_type', 'page', 3 ),
					$object( 'post_type_archive', 'post', 0 ),
				)
			);
			$navigation_visitor();
		},
		'action' => $navigation_get( '/', array( 'page_id' => 3 ) ),
	),
	'navigation-fallback-front-page'       => array(
		'setup'  => function () use ( $navigation_menu, $navigation_visitor, $navigation_page, $object ) {
			$navigation_page( 3, array( 'post_status' => 'publish' ) );
			update_option( 'show_on_front', 'page' );
			update_option( 'page_on_front', 3 );
			update_option( 'page_for_posts', 2 );
			$navigation_menu(
				'Front page',
				array(
					$object( 'post_type', 'page', 2 ),
					$object( 'post_type', 'page', 3 ),
					$object( 'post_type_archive', 'post', 0 ),
				)
			);
			$navigation_visitor();
		},
		'action' => $navigation_get( '/', array( 'page_id' => 3 ) ),
	),
	// The current page, and the items above it.
	'navigation-fallback-current-page'     => array(
		'setup'  => function () use ( $navigation_menu, $navigation_visitor, $object ) {
			$navigation_menu(
				'Current page',
				array(
					array(
						'menu-item-title' => 'Top',
						'menu-item-url'   => 'https://example.test/',
					),
					$object( 'post_type', 'post', 1, array( 'parent' => 0 ) ),
					$object( 'post_type', 'page', 2, array( 'parent' => 1 ) ),
					array(
						'menu-item-title' => 'Below',
						'menu-item-url'   => 'https://example.test/below',
						'parent'          => 2,
					),
				)
			);
			$navigation_visitor();
		},
		'action' => $navigation_get( '/?page_id=2', array( 'page_id' => 2 ) ),
	),
	'navigation-fallback-page-ancestor'    => array(
		'setup'  => function () use ( $navigation_menu, $navigation_visitor, $navigation_page, $object ) {
			$navigation_page( 3, array( 'post_status' => 'publish', 'post_parent' => 2 ) );
			$navigation_menu(
				'Page ancestor',
				array(
					$object( 'post_type', 'page', 2 ),
					$object( 'post_type', 'page', 3, array( 'parent' => 0 ) ),
				)
			);
			$navigation_visitor();
		},
		'action' => $navigation_get( '/?page_id=3', array( 'page_id' => 3 ) ),
	),
	'navigation-fallback-single-post'      => array(
		'setup'  => function () use ( $navigation_menu, $navigation_visitor, $navigation_categories, $object ) {
			$navigation_categories();
			$navigation_menu(
				'Single post',
				array(
					$object( 'post_type', 'post', 1 ),
					$object( 'taxonomy', 'category', 9001 ),
					$object( 'taxonomy', 'category', 9002, array( 'parent' => 1 ) ),
					$object( 'taxonomy', 'category', 1 ),
					$object( 'post_type', 'page', 2 ),
					$object( 'taxonomy', 'post_tag', 9002 ),
				)
			);
			$navigation_visitor();
		},
		'action' => $navigation_get( '/?p=1', array( 'p' => 1 ) ),
	),
	// An attachment is singular, and in no category of its own.
	'navigation-fallback-attachment'       => array(
		'setup'  => function () use ( $navigation_menu, $navigation_visitor, $navigation_categories, $object, &$navigation_attachment ) {
			$navigation_categories();
			$navigation_attachment = wp_insert_attachment(
				array(
					'post_title'     => 'Picture',
					'post_mime_type' => 'image/jpeg',
					'post_status'    => 'inherit',
				),
				'picture.jpg',
				1
			);
			$navigation_menu(
				'Attachment',
				array(
					$object( 'taxonomy', 'category', 1 ),
					$object( 'taxonomy', 'category', 9002 ),
					$object( 'post_type', 'post', 1 ),
				)
			);
			$navigation_visitor();
		},
		'action' => $navigation_get( '/', function () use ( &$navigation_attachment ) {
			return array( 'attachment_id' => $navigation_attachment );
		} ),
	),
	'navigation-fallback-single-other'     => array(
		'setup'  => function () use ( $navigation_menu, $navigation_visitor, $navigation_categories, $object ) {
			$navigation_categories( array( 9002 ) );
			$navigation_menu(
				'Single other',
				array(
					$object( 'post_type', 'post', 1 ),
					$object( 'taxonomy', 'category', 1 ),
					$object( 'taxonomy', 'category', 9002 ),
				)
			);
			$navigation_visitor();
		},
		'action' => $navigation_get( '/?p=1', array( 'p' => 1 ) ),
	),
	'navigation-fallback-category'         => array(
		'setup'  => function () use ( $navigation_menu, $navigation_visitor, $navigation_categories, $object ) {
			$navigation_categories();
			$navigation_menu(
				'Category archive',
				array(
					$object( 'taxonomy', 'category', 9001 ),
					$object( 'taxonomy', 'category', 9002, array( 'parent' => 0 ) ),
					$object( 'taxonomy', 'category', 1 ),
					$object( 'post_type', 'post', 1 ),
					$object( 'taxonomy', 'post_tag', 9002 ),
				)
			);
			$navigation_visitor();
		},
		'action' => $navigation_get( '/?cat=9002', array( 'cat' => 9002 ) ),
	),
	// A custom link to the page asked for, its ancestors, and the home link.
	'navigation-fallback-current-link'     => array(
		'setup'  => function () use ( $navigation_menu, $navigation_visitor ) {
			$navigation_menu(
				'Current link',
				array(
					array(
						'menu-item-title' => 'Top',
						'menu-item-url'   => 'https://example.test/',
					),
					array(
						'menu-item-title' => 'Middle',
						'menu-item-url'   => 'https://example.test/middle',
						'parent'          => 0,
					),
					array(
						'menu-item-title'   => 'Here',
						'menu-item-url'     => 'http://127.0.0.1:8081/?page_id=2#top',
						'menu-item-classes' => 'here',
						'parent'            => 1,
					),
					array(
						'menu-item-title' => 'Elsewhere',
						'menu-item-url'   => 'http://127.0.0.1:8081/?page_id=3',
					),
				)
			);
			$navigation_visitor();
		},
		'action' => $navigation_get( '/?page_id=2', array( 'page_id' => 2 ) ),
	),
	'navigation-fallback-home-link'        => array(
		'setup'  => function () use ( $navigation_menu, $navigation_visitor ) {
			$navigation_menu(
				'Home link',
				array(
					array(
						'menu-item-title' => 'Home',
						'menu-item-url'   => 'http://127.0.0.1:8081/',
					),
					array(
						'menu-item-title' => 'Home again',
						'menu-item-url'   => 'https://127.0.0.1:8081',
					),
					array(
						'menu-item-title' => 'Elsewhere',
						'menu-item-url'   => 'https://example.test/',
					),
					array(
						'menu-item-title' => 'Relative',
						'menu-item-url'   => '/',
					),
				)
			);
			$navigation_visitor();
		},
		'action' => $navigation_get( '/', array() ),
	),
	'navigation-fallback-home-paged'       => array(
		'setup'  => function () use ( $navigation_menu, $navigation_visitor ) {
			$navigation_menu(
				'Home paged',
				array(
					array(
						'menu-item-title' => 'Home',
						'menu-item-url'   => 'http://127.0.0.1:8081/',
					),
					array(
						'menu-item-title' => 'Elsewhere',
						'menu-item-url'   => 'https://example.test/',
					),
				)
			);
			$navigation_visitor();
		},
		'action' => $navigation_get( '/?paged=2', array( 'paged' => 2 ) ),
	),
	// The REST route makes the post as the page view does, by the user who asks.
	'rest-navigation-fallback-default'     => array(
		'setup'  => $navigation_rest,
		'action' => $navigation_rest_get,
	),
	'rest-navigation-fallback-classic'     => array(
		'setup'  => function () use ( $navigation_menu, $navigation_rest, $plain ) {
			$navigation_menu( 'An older menu', $plain );
			$navigation_menu( 'Oracle Menu', array( $plain[0] ) );
			$navigation_rest();
		},
		'action' => $navigation_rest_get,
	),
);

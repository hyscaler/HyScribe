<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints wp_nav_menu() over classic menus made in a transaction that is
// rolled back, for Hello Elementor's calls and the other arguments. First
// what the menus are, as the database holds them, then the cases:
//   menu<TAB>term ID<TAB>json(name)<TAB>json(slug)
//   item<TAB>menu ID<TAB>json(the post's fields and its _menu_item_* meta)
//   option<TAB>name<TAB>json(value)
//   case<TAB>json(arguments)<TAB>page asked for<TAB>hex(markup) or false
// The page asked for is front, post:<ID> or page:<ID>. wp_nav_menu()'s list
// of printed IDs is kept from case to case, as in one request.

require '/var/www/html/wp-load.php';

global $wpdb;
$wpdb->query( 'START TRANSACTION' );
wp_set_current_user( 1 );
// A published privacy policy page, which links get rel="privacy-policy" to.
wp_update_post( array( 'ID' => 3, 'post_status' => 'publish' ) );

$menus = array();
$make  = function ( $name, $items ) use ( &$menus ) {
	$menu_id = wp_create_nav_menu( $name );
	$ids     = array();
	foreach ( $items as $item ) {
		$parent = isset( $item['parent'] ) ? $ids[ $item['parent'] ] : 0;
		unset( $item['parent'] );
		$ids[] = wp_update_nav_menu_item(
			$menu_id,
			0,
			$item + array(
				'menu-item-type'      => 'custom',
				'menu-item-status'    => 'publish',
				'menu-item-parent-id' => $parent,
			)
		);
	}
	$menus[] = $menu_id;
	return $menu_id;
};

$main   = $make(
	'Main',
	array(
		array( 'menu-item-title' => 'Home', 'menu-item-url' => home_url( '/' ), 'menu-item-position' => 1 ),
		array( 'menu-item-type' => 'post_type', 'menu-item-object' => 'page', 'menu-item-object-id' => 2, 'menu-item-position' => 2 ),
		array( 'menu-item-title' => 'Child & "kin"', 'menu-item-url' => 'https://example.test/child?a=1&b=2', 'menu-item-classes' => 'one two', 'parent' => 1, 'menu-item-position' => 3 ),
		array( 'menu-item-title' => 'Grandchild', 'menu-item-url' => '#grand', 'menu-item-attr-title' => 'grandchild ', 'parent' => 2, 'menu-item-position' => 4 ),
		array( 'menu-item-type' => 'post_type', 'menu-item-object' => 'post', 'menu-item-object-id' => 1, 'menu-item-attr-title' => 'The <first> post', 'menu-item-target' => '_blank', 'menu-item-xfn' => 'friend met', 'menu-item-position' => 5 ),
		array( 'menu-item-type' => 'post_type', 'menu-item-object' => 'page', 'menu-item-object-id' => 3, 'menu-item-xfn' => 'me', 'menu-item-position' => 6 ),
		array( 'menu-item-type' => 'taxonomy', 'menu-item-object' => 'category', 'menu-item-object-id' => 1, 'menu-item-position' => 7 ),
		array( 'menu-item-title' => 'Same order, first', 'menu-item-url' => 'https://example.test/first', 'menu-item-position' => 8 ),
		array( 'menu-item-title' => 'Same order, second', 'menu-item-url' => 'https://example.test/second', 'menu-item-position' => 8 ),
		array( 'menu-item-title' => '0', 'menu-item-url' => '0', 'menu-item-classes' => '0', 'menu-item-position' => 9 ),
		array( 'menu-item-title' => 'Say "hi"', 'menu-item-url' => '/hi', 'menu-item-attr-title' => 'say "hi"', 'menu-item-position' => 10 ),
	)
);
$footer = $make(
	'Footer links',
	array(
		array( 'menu-item-title' => 'Privacy', 'menu-item-url' => get_permalink( 3 ), 'menu-item-position' => 1 ),
		array( 'menu-item-title' => 'Contact', 'menu-item-url' => 'mailto:hi@example.test', 'menu-item-position' => 2 ),
	)
);
$empty  = $make( 'Empty', array() );
// Items whose parent is no item of the menu: printed after the others, and
// at the top when none is.
$parented = function ( $menu_id, $parents ) {
	$items = get_objects_in_term( $menu_id, 'nav_menu' );
	sort( $items );
	foreach ( $parents as $index => $parent ) {
		update_post_meta( $items[ $index ], '_menu_item_menu_item_parent', 'self' === $parent ? (string) $items[ $index ] : $parent );
	}
};
$letters  = function ( $letters ) {
	$items = array();
	foreach ( $letters as $position => $letter ) {
		$items[] = array( 'menu-item-title' => $letter, 'menu-item-url' => "/$letter", 'menu-item-position' => $position + 1 );
	}
	return $items;
};
$parented( $make( 'Orphans', $letters( array( 'A', 'B', 'C' ) ) ), array( 1 => '999', 2 => '999' ) );
$parented( $make( 'Rootless', $letters( array( 'D', 'E' ) ) ), array( '998', '998' ) );
// An item of its own parent is at the top.
$parented( $make( 'Itself', $letters( array( 'F', 'G' ) ) ), array( 'self', '997' ) );
set_theme_mod( 'nav_menu_locations', array( 'menu-1' => $main, 'menu-2' => $footer, 'menu-3' => $empty ) );
wp_cache_flush();

foreach ( $menus as $menu_id ) {
	$term = get_term( $menu_id, 'nav_menu' );
	echo "menu\t$menu_id\t" . wp_json_encode( $term->name ) . "\t" . wp_json_encode( $term->slug ) . "\n";
	foreach ( get_objects_in_term( $menu_id, 'nav_menu' ) as $item ) {
		$post   = get_post( $item );
		$fields = array(
			'ID'           => $post->ID,
			'post_title'   => $post->post_title,
			'post_excerpt' => $post->post_excerpt,
			'post_content' => $post->post_content,
			'post_parent'  => $post->post_parent,
			'menu_order'   => $post->menu_order,
			'post_status'  => $post->post_status,
		);
		foreach ( get_post_meta( $item ) as $key => $values ) {
			$fields[ $key ] = $values[0];
		}
		echo "item\t$menu_id\t" . wp_json_encode( $fields ) . "\n";
	}
}
foreach ( array( 'stylesheet', 'theme_mods_twentytwentyfive', 'wp_page_for_privacy_policy', 'permalink_structure' ) as $name ) {
	echo "option\t$name\t" . wp_json_encode( get_option( $name ) ) . "\n";
}

$hello = array( 'fallback_cb' => false, 'container' => false, 'echo' => false );
$cases = array(
	array( array( 'theme_location' => 'menu-1' ) + $hello, 'front' ),
	array( array( 'theme_location' => 'menu-2' ) + $hello, 'front' ),
	array( array( 'theme_location' => 'menu-1' ) + $hello, 'post:1' ),
	array( array( 'theme_location' => 'menu-1' ) + $hello, 'page:2' ),
	array( array( 'theme_location' => 'menu-3' ) + $hello, 'front' ),
	array( array( 'theme_location' => 'menu-9' ) + $hello, 'front' ),
	array( array( 'theme_location' => '0', 'menu' => 'Empty' ) + $hello, 'front' ),
	array( array( 'menu' => 'Main', 'fallback_cb' => false, 'echo' => false ), 'front' ),
	array( array( 'menu' => 'Main', 'fallback_cb' => false, 'echo' => false ), 'front' ),
	array( array( 'menu' => $footer, 'fallback_cb' => false, 'echo' => false, 'menu_id' => 'menu-main-2' ), 'front' ),
	array( array( 'menu' => 'main', 'fallback_cb' => false, 'echo' => false ), 'front' ),
	array( array( 'menu' => 'footer-links', 'fallback_cb' => false, 'echo' => false, 'container' => 'nav', 'container_class' => 'c "x"', 'container_id' => 'cid', 'container_aria_label' => 'Footer <menu>', 'menu_class' => '', 'menu_id' => 'f<id>' ), 'front' ),
	array( array( 'menu' => 'Footer links', 'fallback_cb' => false, 'echo' => false, 'container' => 'nav', 'container_class' => '0', 'container_id' => '0', 'menu_class' => '0', 'menu_id' => '0' ), 'front' ),
	array( array( 'menu' => 'Footer links', 'fallback_cb' => false, 'echo' => false, 'container' => 'div', 'container_aria_label' => 'Not on a div' ), 'front' ),
	array( array( 'menu' => 'Footer links', 'fallback_cb' => false, 'echo' => false, 'container' => 'section' ), 'front' ),
	array( array( 'theme_location' => 'menu-1', 'before' => '<span>', 'after' => '</span>', 'link_before' => '<em>', 'link_after' => '</em>' ) + $hello, 'front' ),
	array( array( 'theme_location' => 'menu-1', 'item_spacing' => 'discard' ) + $hello, 'post:1' ),
	array( array( 'theme_location' => 'menu-1', 'item_spacing' => 'other' ) + $hello, 'front' ),
	array( array( 'theme_location' => 'menu-1', 'depth' => 1 ) + $hello, 'front' ),
	array( array( 'theme_location' => 'menu-1', 'depth' => 2 ) + $hello, 'front' ),
	array( array( 'theme_location' => 'menu-1', 'depth' => -1 ) + $hello, 'front' ),
	array( array( 'theme_location' => 'menu-1', 'depth' => -2 ) + $hello, 'front' ),
	array( array( 'theme_location' => 'menu-2', 'items_wrap' => '<ol data-id="%1$s" class="%2$s">%3$s</ol> 100%%' ) + $hello, 'front' ),
	array( array( 'theme_location' => 'menu-2', 'items_wrap' => '<ol>%s|%s|%s</ol>' ) + $hello, 'front' ),
	array( array( 'menu' => 'Orphans' ) + $hello, 'front' ),
	array( array( 'menu' => 'Orphans', 'depth' => 1 ) + $hello, 'front' ),
	array( array( 'menu' => 'Rootless' ) + $hello, 'front' ),
	array( array( 'menu' => 'Rootless', 'depth' => 1 ) + $hello, 'front' ),
	array( array( 'menu' => 'Footer links', 'theme_location' => 'menu-1' ) + $hello, 'front' ),
	array( array( 'menu' => 'Itself' ) + $hello, 'front' ),
	array( array( 'menu' => 'Empty' ) + $hello, 'front' ),
	array( $hello, 'front' ),
);

foreach ( $cases as list( $args, $page ) ) {
	$_SERVER['HTTP_HOST'] = '127.0.0.1:8081';
	list( $kind, $id )    = array_pad( explode( ':', $page ), 2, '' );
	$query                = array( 'front' => '', 'post' => "p=$id", 'page' => "page_id=$id" )[ $kind ];
	$_SERVER['REQUEST_URI'] = '' === $query ? '/' : "/?$query";
	query_posts( $query );
	$markup = wp_nav_menu( $args );
	$out    = false === $markup ? 'false' : bin2hex( $markup );
	echo "case\t" . wp_json_encode( $args ) . "\t$page\t$out\n";
}

$wpdb->query( 'ROLLBACK' );

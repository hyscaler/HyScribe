<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Must-use plugin for reference/posts-list/golden.sh, through filters only:
// posts 304 and 306 are sticky, the hidden variant hides the tags and comments
// columns and lists three posts and three pages a page, and the front variant
// shows pages 316 and 321 as the front and posts pages. The h9home variant's
// home URL is HTTPS, its site URL not.
add_filter( 'pre_option_sticky_posts', fn() => array( 304, 306 ) );
if ( 'hidden' === ( $_COOKIE['hyscribe_variant'] ?? '' ) ) {
	add_filter( 'get_user_option_manageedit-postcolumnshidden', fn() => array( 'tags', 'comments' ) );
	add_filter( 'get_user_option_edit_post_per_page', fn() => '3' );
	add_filter( 'get_user_option_edit_page_per_page', fn() => '3' );
}
if ( 'front' === ( $_COOKIE['hyscribe_variant'] ?? '' ) ) {
	add_filter( 'pre_option_show_on_front', fn() => 'page' );
	add_filter( 'pre_option_page_on_front', fn() => '316' );
	add_filter( 'pre_option_page_for_posts', fn() => '321' );
}
if ( 'h9home' === ( $_COOKIE['hyscribe_variant'] ?? '' ) ) {
	add_filter( 'pre_option_home', fn() => 'https://127.0.0.1:8081' );
}

<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Must-use plugin for reference/posts-list/golden.sh, through filters only:
// posts 304 and 306 are sticky, and the hidden variant hides the tags and comments
// columns and lists three posts a page.
add_filter( 'pre_option_sticky_posts', fn() => array( 304, 306 ) );
if ( 'hidden' === ( $_COOKIE['hyscribe_variant'] ?? '' ) ) {
	add_filter( 'get_user_option_manageedit-postcolumnshidden', fn() => array( 'tags', 'comments' ) );
	add_filter( 'get_user_option_edit_post_per_page', fn() => '3' );
}

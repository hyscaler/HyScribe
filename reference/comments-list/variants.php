<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Must-use plugin for reference/comments-list/golden.sh, through filters only:
// the paged variant lists two comments a page, hidecols hides the response
// column and noavatars turns avatars off.
$hyscribe_variant = $_COOKIE['hyscribe_variant'] ?? '';
if ( 'paged' === ( $_COOKIE['hyscribe_variant'] ?? '' ) ) {
	add_filter( 'get_user_option_edit_comments_per_page', fn() => '2' );
}
if ( 'noavatars' === ( $_COOKIE['hyscribe_variant'] ?? '' ) ) {
	add_filter( 'pre_option_show_avatars', fn() => '0' );
}
if ( 'hidecols' === ( $_COOKIE['hyscribe_variant'] ?? '' ) ) {
	add_filter( 'get_user_option_manageedit-commentscolumnshidden', fn() => array( 'response' ) );
}
// The g9 variants page comments one a page (g9pages, g9cpages), the oldest
// page the default one (g9coldest), and turn keyboard shortcuts on (g9keys).
if ( in_array( $hyscribe_variant, array( 'g9pages', 'g9cpages', 'g9coldest' ), true ) ) {
	add_filter( 'pre_option_page_comments', fn() => '1' );
	add_filter( 'pre_option_comments_per_page', fn() => '1' );
}
if ( 'g9coldest' === $hyscribe_variant ) {
	add_filter( 'pre_option_default_comments_page', fn() => 'oldest' );
}
if ( 'g9keys' === $hyscribe_variant ) {
	add_filter( 'get_user_option_comment_shortcuts', fn() => 'true' );
}

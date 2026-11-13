<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Must-use plugin for reference/comments-list/golden.sh, through filters only:
// the paged variant lists two comments a page, hidecols hides the response
// column and noavatars turns avatars off.
if ( 'paged' === ( $_COOKIE['hyscribe_variant'] ?? '' ) ) {
	add_filter( 'get_user_option_edit_comments_per_page', fn() => '2' );
}
if ( 'noavatars' === ( $_COOKIE['hyscribe_variant'] ?? '' ) ) {
	add_filter( 'pre_option_show_avatars', fn() => '0' );
}
if ( 'hidecols' === ( $_COOKIE['hyscribe_variant'] ?? '' ) ) {
	add_filter( 'get_user_option_manageedit-commentscolumnshidden', fn() => array( 'response' ) );
}

<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Must-use plugin for reference/users-list/golden.sh, through filters only:
// the paged variant lists two users a page, hidecols hides the email column
// and noavatars turns avatars off.
if ( 'paged' === ( $_COOKIE['hyscribe_variant'] ?? '' ) ) {
	add_filter( 'get_user_option_users_per_page', fn() => '2' );
}
if ( 'noavatars' === ( $_COOKIE['hyscribe_variant'] ?? '' ) ) {
	add_filter( 'pre_option_show_avatars', fn() => '0' );
}
if ( 'hidecols' === ( $_COOKIE['hyscribe_variant'] ?? '' ) ) {
	add_filter( 'get_user_option_manageuserscolumnshidden', fn() => array( 'email' ) );
}

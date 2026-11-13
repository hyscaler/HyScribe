<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Must-use plugin for reference/terms-list/golden.sh, through filters only:
// the paged variant lists three terms a page and hides the description column.
if ( 'paged' === ( $_COOKIE['hyscribe_variant'] ?? '' ) ) {
	foreach ( array( 'category', 'post_tag' ) as $taxonomy ) {
		add_filter( "get_user_option_edit_{$taxonomy}_per_page", fn() => '3' );
		add_filter( "get_user_option_manageedit-{$taxonomy}columnshidden", fn() => array( 'description' ) );
	}
}

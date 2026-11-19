<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Must-use plugin for reference/register/golden.sh: anyone may register when
// the hyscribe_variant cookie is "register", through a filter only, and every
// mail is taken as sent, as HyScribe answers until its transport sends them.
if ( 'register' === ( $_COOKIE['hyscribe_variant'] ?? '' ) ) {
	add_filter( 'pre_option_users_can_register', fn() => '1' );
}
add_filter( 'pre_wp_mail', '__return_true' );

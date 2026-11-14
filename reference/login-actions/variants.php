<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Must-use plugin for reference/login-actions/golden.sh: anyone may register
// when the hyscribe_variant cookie is "register", through a filter only.
if ( 'register' === ( $_COOKIE['hyscribe_variant'] ?? '' ) ) {
	add_filter( 'pre_option_users_can_register', fn() => '1' );
}

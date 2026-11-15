<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A must-use plugin for reference/comment-post/golden.sh: the request is
// the user's in the hyscribe_user cookie (none for a visitor), on a site
// with HyScribe's constants: no unfiltered HTML and no cron.
define( 'DISALLOW_UNFILTERED_HTML', true );
define( 'DISABLE_WP_CRON', true );

add_filter( 'determine_current_user', function () {
	return isset( $_COOKIE['hyscribe_user'] ) ? (int) $_COOKIE['hyscribe_user'] : 0;
}, 99 );

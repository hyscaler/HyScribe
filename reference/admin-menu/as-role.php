<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A must-use plugin for reference/admin-menu/golden.sh: the request is user
// 1's, with the capabilities of the role in the hyscribe_role cookie (set
// before core's own user_has_cap filters add to them), on a
// site with HyScribe's constants: no file changes from the browser
// (DISALLOW_FILE_MODS) and no unfiltered HTML. It writes nothing.
define( 'DISALLOW_FILE_MODS', true );
define( 'DISALLOW_UNFILTERED_HTML', true );

function auth_redirect() {}

add_filter( 'determine_current_user', function () {
	return 1;
}, 99 );

add_filter( 'user_has_cap', function ( $allcaps ) {
	$role = get_role( isset( $_COOKIE['hyscribe_role'] ) ? $_COOKIE['hyscribe_role'] : 'administrator' );
	return $role ? $role->capabilities : array();
}, 0 );

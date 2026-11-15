<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Must-use plugin for reference/privacy-requests/golden.sh, through filters
// only: j9pp lists two requests a page, j9hidden hides the Status and Next
// steps columns.
$hyscribe_variant = $_COOKIE['hyscribe_variant'] ?? '';
if ( 'j9pp' === $hyscribe_variant ) {
	foreach ( array( 'export', 'remove' ) as $hyscribe_kind ) {
		add_filter( "get_user_option_{$hyscribe_kind}_personal_data_requests_per_page", fn() => '2' );
	}
}
if ( 'j9hidden' === $hyscribe_variant ) {
	foreach ( array( 'export', 'erase' ) as $hyscribe_kind ) {
		add_filter( "get_user_option_manage{$hyscribe_kind}-personal-datacolumnshidden", fn() => array( 'status', 'next_steps' ) );
	}
}

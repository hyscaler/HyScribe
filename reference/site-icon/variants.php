<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Must-use plugin for reference/site-icon/golden.sh, through filters only:
// the h9icon variant's site icon is attachment 951, h9iconbare's 952,
// h9iconpdf's 953 and h9iconnone's 954.
$hyscribe_icons = array(
	'h9icon'     => '951',
	'h9iconbare' => '952',
	'h9iconpdf'  => '953',
	'h9iconnone' => '954',
);
$hyscribe_variant = $_COOKIE['hyscribe_variant'] ?? '';
if ( isset( $hyscribe_icons[ $hyscribe_variant ] ) ) {
	add_filter( 'pre_option_site_icon', fn() => $hyscribe_icons[ $hyscribe_variant ] );
}

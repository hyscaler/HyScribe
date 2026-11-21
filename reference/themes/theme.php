<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A must-use plugin for reference/themes/export.sh: PHP run with the
// environment variable HYSCRIBE_THEME set runs with that theme active
// (its `stylesheet` option, and `template` as its style.css names it,
// which for a child theme is its parent), whatever the database holds.
// Web requests have no such variable and are not affected.
$hyscribe_theme = getenv( 'HYSCRIBE_THEME' );
if ( $hyscribe_theme ) {
	$hyscribe_header   = get_file_data( WP_CONTENT_DIR . '/themes/' . $hyscribe_theme . '/style.css', array( 'template' => 'Template' ) );
	$hyscribe_template = $hyscribe_header['template'] ? $hyscribe_header['template'] : $hyscribe_theme;
	add_filter( 'pre_option_template', fn() => $hyscribe_template );
	add_filter( 'pre_option_stylesheet', fn() => $hyscribe_theme );
}

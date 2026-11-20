<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A must-use plugin for reference/themes/export.sh: PHP run with the
// environment variable HYSCRIBE_THEME set runs with that theme active
// (its `template` and `stylesheet` options), whatever the database holds.
// Web requests have no such variable and are not affected.
$hyscribe_theme = getenv( 'HYSCRIBE_THEME' );
if ( $hyscribe_theme ) {
	add_filter( 'pre_option_template', fn() => $hyscribe_theme );
	add_filter( 'pre_option_stylesheet', fn() => $hyscribe_theme );
}

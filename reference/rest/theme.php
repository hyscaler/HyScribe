<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Must-use plugin of reference/rest/theme.list: a request carrying
// X-HyScribe-Theme: <stylesheet> is served as on a site whose active theme
// is that one (an option filter, so nothing is written), and the registered
// patterns come alone, as in block-patterns.php.
add_filter( 'should_load_remote_block_patterns', '__return_false' );
if ( ! empty( $_SERVER['HTTP_X_HYSCRIBE_THEME'] ) ) {
	$hyscribe_theme = $_SERVER['HTTP_X_HYSCRIBE_THEME'];
	add_filter( 'pre_option_template', fn() => $hyscribe_theme );
	add_filter( 'pre_option_stylesheet', fn() => $hyscribe_theme );
}

<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A must-use plugin for reference/classic-head/golden.sh: where the template
// loader has picked the classic theme's template, the template of
// hyscribe-head/page.php is rendered instead, which prints wp_head() and
// wp_footer() only, so the theme's own callbacks run and WordPress 6.9's
// template output buffer hoists the late styles as it does on any page. A
// request whose options carry _fatal=1 renders hyscribe-head/fatal.php,
// which calls a function that does not exist, for PHP's critical error page.
add_filter(
	'template_include',
	function () {
		$options = array();
		if ( isset( $_SERVER['HTTP_X_HYSCRIBE_OPTIONS'] ) ) {
			parse_str( $_SERVER['HTTP_X_HYSCRIBE_OPTIONS'], $options );
		}
		$page = empty( $options['_fatal'] ) ? 'page.php' : 'fatal.php';
		return __DIR__ . '/hyscribe-head/' . $page;
	},
	PHP_INT_MAX
);

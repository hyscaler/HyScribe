<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A must-use plugin for reference/themes/hierarchy-golden.sh: the template
// file the template loader picked goes out in the header
// X-HyScribe-Template, as a path inside the theme, or inside wp-includes
// for core's theme-compat files. It only reads.
add_filter(
	'template_include',
	function ( $template ) {
		$roots = array( get_stylesheet_directory() . '/', get_template_directory() . '/', ABSPATH . 'wp-includes/' );
		$path  = (string) $template;
		foreach ( $roots as $index => $root ) {
			if ( str_starts_with( $path, $root ) ) {
				$path = ( 2 === $index ? 'wp-includes/' : '' ) . substr( $path, strlen( $root ) );
				break;
			}
		}
		header( 'X-HyScribe-Template: ' . $path );
		return $template;
	},
	PHP_INT_MAX
);

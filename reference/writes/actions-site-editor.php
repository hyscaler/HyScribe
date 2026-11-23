<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The write oracle's site editor view (MVP 7.1), loaded beside actions.php:
// what a view of site-editor.php writes, through the functions that write
// it, in the page's order. The theme's pattern cache and the admin's update
// checks are left out.

return array(
	// The first view, without the theme mods php.sh's first page view wrote:
	// the custom CSS lookup of the editor's settings, then the theme's
	// global styles post its preloads name.
	// A themes read whose links name the global styles post creates it, and nothing else.
	'rest-themes-global-styles' => array(
		'action' => function () {
			$request = new WP_REST_Request( 'GET', '/wp/v2/themes' );
			$request->set_query_params( array( 'status' => 'active' ) );
			$response = rest_do_request( $request );
			if ( $response->is_error() ) {
				fwrite( STDERR, wp_json_encode( $response->get_data() ) . "\n" );
				exit( 1 );
			}
		},
	),
	'admin-site-editor' => array(
		'setup'  => function () {
			delete_option( 'theme_mods_' . get_stylesheet() );
		},
		'action' => function () {
			wp_get_custom_css();
			WP_Theme_JSON_Resolver::get_user_global_styles_post_id();
		},
	),
);

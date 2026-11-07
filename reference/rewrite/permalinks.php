<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A must-use plugin for the rewrite oracles: a request carrying
// X-HyScribe-Permalinks: structure|category base|tag base|page on front|page for posts
// is served as on a site with those settings. They come from option
// filters, and the rewrite rules are built without being saved, so nothing
// is written to the database. With X-HyScribe-Parse, the request ends after
// WP::parse_request() with what it found, as JSON.
if ( isset( $_SERVER['HTTP_X_HYSCRIBE_PERMALINKS'] ) ) {
	list( $structure, $category, $tag, $front, $posts ) = array_pad( explode( '|', $_SERVER['HTTP_X_HYSCRIBE_PERMALINKS'] ), 5, '' );
	$options = array(
		'permalink_structure' => $structure,
		'category_base'       => $category,
		'tag_base'            => $tag,
	);
	if ( '' !== $front || '' !== $posts ) {
		$options['page_on_front']  = $front;
		$options['page_for_posts'] = $posts;
		$options['show_on_front']  = 'page';
	}
	foreach ( $options as $name => $value ) {
		add_filter( "pre_option_$name", fn() => $value );
	}
	add_filter(
		'pre_option_rewrite_rules',
		function () {
			$GLOBALS['wp_rewrite']->matches = 'matches';
			return $GLOBALS['wp_rewrite']->rewrite_rules();
		}
	);
}
if ( isset( $_SERVER['HTTP_X_HYSCRIBE_PARSE'] ) ) {
	add_action(
		'parse_request',
		function ( $wp ) {
			$vars = $wp->query_vars;
			ksort( $vars );
			header( 'Content-Type: application/json' );
			echo wp_json_encode(
				array(
					'rule'  => $wp->matched_rule,
					'query' => $wp->matched_query,
					'vars'  => $vars,
				),
				JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE
			);
			exit;
		},
		-1000
	);
}

<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The write oracle's block editor views (MVP 5.1), loaded beside
// actions.php: the writes a view of post-new.php or post.php?action=edit
// makes, through the functions that make them, in the page's order. The
// theme's pattern cache and the admin's update checks are left out.

// What the editor's page writes once its post is known: the theme's global
// styles post (its preloads), the edit lock, the custom CSS lookup.
$editor_view = function ( $id ) {
	WP_Theme_JSON_Resolver::get_user_global_styles_post_id();
	wp_set_post_lock( $id );
	wp_get_custom_css();
};

// The writes of a site whose editor was viewed once already.
$editor_viewed = function () {
	WP_Theme_JSON_Resolver::get_user_global_styles_post_id();
	wp_get_custom_css();
};

$editor_new = function ( $type ) use ( $editor_view ) {
	return function () use ( $type, $editor_view ) {
		$editor_view( get_default_post_to_edit( $type, true )->ID );
	};
};

// The theme mods php.sh's first page view wrote, which the fixture lacks.
$editor_fresh = function () {
	delete_option( 'theme_mods_' . get_stylesheet() );
};

// The write oracle's block editor saves (MVP 5.2): the requests of
// reference/rest/editor-saves.list that write, each dispatched as user 1
// through rest_do_request() over the state of reference/rest/editor-saves.sql.

$editor_state = function () {
	global $wpdb;
	$sql = file_get_contents( __DIR__ . '/editor-saves.sql' );
	$sql = strstr( $sql, "\n-- cleanup\n", true );
	foreach ( explode( "\n", $sql ) as $line ) {
		if ( preg_match( '/^(INSERT|UPDATE|DELETE) /', $line ) ) {
			$wpdb->query( $line );
		}
	}
};

$editor_save = function ( $route, $body ) use ( $editor_state ) {
	return array(
		'setup'  => $editor_state,
		'action' => function () use ( $route, $body ) {
			$request = new WP_REST_Request( 'PUT', $route );
			$request->set_header( 'Content-Type', 'application/json' );
			$request->set_body( wp_json_encode( $body ) );
			$response = rest_do_request( $request );
			if ( $response->is_error() ) {
				fwrite( STDERR, wp_json_encode( $response->get_data() ) . "\n" );
				exit( 1 );
			}
		},
	);
};

$para = fn( $text ) => "<!-- wp:paragraph -->\n<p>$text</p>\n<!-- /wp:paragraph -->";
$hello = $para( 'Welcome to WordPress. This is your first post. Edit or delete it, then start writing!' );
$drafted = $para( 'Drafted in the editor.' );

return array(
	// post-new.php's first view.
	'admin-editor-new'                 => array(
		'setup'  => $editor_fresh,
		'action' => $editor_new( 'post' ),
	),
	// The same with unfiltered HTML, which keeps the global styles as written.
	'admin-editor-new-unfiltered-html' => array(
		'setup'  => $editor_fresh,
		'action' => $editor_new( 'post' ),
	),
	// post-new.php?post_type=page once the editor was viewed.
	'admin-editor-new-page'            => array(
		'setup'  => $editor_viewed,
		'action' => $editor_new( 'page' ),
	),
	// post.php?post=1&action=edit once the editor was viewed.
	'admin-editor-edit'                => array(
		'setup'  => $editor_viewed,
		'action' => function () use ( $editor_view ) {
			$editor_view( 1 );
		},
	),
	'editor-save-draft'     => $editor_save( '/wp/v2/posts/9601', array( 'id' => 9601, 'title' => 'First draft', 'content' => $drafted, 'status' => 'draft' ) ),
	'editor-publish'        => $editor_save( '/wp/v2/posts/9601', array( 'id' => 9601, 'title' => 'Published from the editor', 'content' => $drafted, 'status' => 'publish' ) ),
	'editor-schedule'       => $editor_save( '/wp/v2/posts/9601', array( 'id' => 9601, 'title' => 'Scheduled', 'content' => $drafted, 'date' => '2030-06-01T09:30:00', 'status' => 'future' ) ),
	'editor-update'         => $editor_save( '/wp/v2/posts/1', array( 'id' => 1, 'title' => 'Hello edited', 'content' => $para( 'Edited in the editor.' ) ) ),
	'editor-unschedule'     => $editor_save( '/wp/v2/posts/9604', array( 'id' => 9604, 'status' => 'draft', 'date' => null, 'content' => $para( 'Coming up.' ) ) ),
	'editor-private'        => $editor_save( '/wp/v2/posts/9606', array( 'id' => 9606, 'status' => 'private', 'password' => '', 'content' => $para( 'For friends.' ) ) ),
	'editor-featured-image' => $editor_save( '/wp/v2/posts/1', array( 'id' => 1, 'featured_media' => 9605, 'content' => $hello ) ),
	'editor-format'         => $editor_save( '/wp/v2/posts/1', array( 'id' => 1, 'format' => 'gallery', 'content' => $hello ) ),
	'editor-format-standard' => $editor_save( '/wp/v2/posts/9606', array( 'id' => 9606, 'format' => 'standard', 'content' => $para( 'For friends.' ) ) ),
	'editor-terms'          => $editor_save( '/wp/v2/posts/1', array( 'id' => 1, 'categories' => array( 1, 9602 ), 'tags' => array( 9601 ), 'content' => $hello ) ),
	'editor-page-template'  => $editor_save( '/wp/v2/pages/2', array( 'id' => 2, 'template' => 'page-no-title', 'parent' => 9607 ) ),
);

<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
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

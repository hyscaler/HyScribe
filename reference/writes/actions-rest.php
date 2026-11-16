<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The write oracle's REST writes (MVP 4.3): the posts and pages
// controller's create_item(), update_item() and delete_item(), each a JSON
// request dispatched as user 1 through rest_do_request(), which runs the
// same controller as a request over HTTP. A new post is never sticky here:
// its ID would end up in sticky_posts, which no mask reaches.

$rest_send = function ( $method, $route, $body = array(), $query = array() ) {
	$request = new WP_REST_Request( $method, $route );
	$request->set_query_params( $query );
	if ( $body ) {
		$request->set_header( 'Content-Type', 'application/json' );
		$request->set_body( wp_json_encode( $body ) );
	}
	$response = rest_do_request( $request );
	if ( $response->is_error() ) {
		fwrite( STDERR, wp_json_encode( $response->get_data() ) . "\n" );
		exit( 1 );
	}
};

return array(
	'rest-posts-create'         => array(
		'action' => function () use ( $rest_send ) {
			$rest_send(
				'POST',
				'/wp/v2/posts',
				array(
					'title'          => '<b>Bold</b> <script>x()</script>title',
					'content'        => '<p onclick="y()">Para</p><script>bad()</script>',
					'excerpt'        => '<em>Kept</em><iframe src="https://example.test/"></iframe>',
					'status'         => 'publish',
					'slug'           => 'rest slug',
					'comment_status' => 'closed',
					'ping_status'    => 'closed',
					'categories'     => array( 1 ),
					'tags'           => array(),
					'date'           => '2026-01-02T03:04:05',
					'featured_media' => 0,
					'template'       => '',
				)
			);
		},
	),
	'rest-posts-create-draft'   => array(
		'action' => function () use ( $rest_send ) {
			$rest_send(
				'POST',
				'/wp/v2/posts',
				array(
					'title'  => 'A REST draft',
					'status' => 'draft',
					'slug'   => 'hello-world',
				)
			);
		},
	),
	'rest-pages-create'         => array(
		'action' => function () use ( $rest_send ) {
			$rest_send(
				'POST',
				'/wp/v2/pages',
				array(
					'title'      => 'A REST page',
					'content'    => '<!-- wp:template-part {"slug":"header"} /-->',
					'parent'     => 2,
					'menu_order' => 3,
					'status'     => 'publish',
				)
			);
		},
	),
	'rest-posts-update'         => array(
		'action' => function () use ( $rest_send ) {
			$rest_send(
				'POST',
				'/wp/v2/posts/1',
				array(
					'title'      => 'Hello again',
					'content'    => "<!-- wp:paragraph -->\n<p>Changed</p>\n<!-- /wp:paragraph -->\n\n<!-- wp:core/template-part {\"slug\":\"footer\"}   /-->",
					'sticky'     => true,
					'categories' => array( 1, 999 ),
					'tags'       => array(),
					'date_gmt'   => '2026-02-03T04:05:06Z',
				)
			);
		},
	),
	'rest-posts-update-draft'   => array(
		'action' => function () use ( $rest_send ) {
			$rest_send(
				'PATCH',
				'/wp/v2/posts/1',
				array(
					'status' => 'draft',
					'slug'   => 'sample-page',
					'date'   => null,
				)
			);
		},
	),
	'rest-pages-update'         => array(
		'action' => function () use ( $rest_send ) {
			$rest_send(
				'PUT',
				'/wp/v2/pages/2',
				array(
					'title'          => 'Sample renamed',
					'parent'         => 0,
					'menu_order'     => 5,
					'status'         => 'private',
					'password'       => 'dropped',
					'featured_media' => 1,
				)
			);
		},
	),
	'rest-posts-trash'          => array(
		'action' => function () use ( $rest_send ) {
			$rest_send( 'DELETE', '/wp/v2/posts/1' );
		},
	),
	'rest-posts-delete'         => array(
		'action' => function () use ( $rest_send ) {
			$rest_send( 'DELETE', '/wp/v2/posts/1', array(), array( 'force' => 'true' ) );
		},
	),
	'rest-pages-delete'         => array(
		'action' => function () use ( $rest_send ) {
			$rest_send( 'DELETE', '/wp/v2/pages/2', array(), array( 'force' => '1' ) );
		},
	),
);

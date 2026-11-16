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
	'rest-categories-create'    => array(
		'action' => function () use ( $rest_send ) {
			$rest_send(
				'POST',
				'/wp/v2/categories',
				array(
					'name'        => 'News',
					'description' => '<b>Fresh</b> <script>x()</script>',
					'slug'        => 'the news',
					'parent'      => 1,
				)
			);
		},
	),
	'rest-categories-update'    => array(
		'action' => function () use ( $rest_send ) {
			$rest_send(
				'PUT',
				'/wp/v2/categories/1',
				array(
					'name'        => 'Renamed',
					'description' => 'Desc',
					'slug'        => 'renamed',
				)
			);
		},
	),
	'rest-tags-delete'          => array(
		'setup'  => function () {
			$tag = wp_insert_term( 'Tagged', 'post_tag' );
			wp_set_object_terms( 1, array( $tag['term_id'] ), 'post_tag' );
			return $tag['term_id'];
		},
		'action' => function ( $tag ) use ( $rest_send ) {
			$rest_send( 'DELETE', "/wp/v2/tags/$tag", array(), array( 'force' => 'true' ) );
		},
	),
	'rest-settings-update'      => array(
		'action' => function () use ( $rest_send ) {
			$rest_send(
				'POST',
				'/wp/v2/settings',
				array(
					'title'          => 'New <b>title</b> & co',
					'posts_per_page' => null,
					'use_smilies'    => false,
					'start_of_week'  => '3',
					'timezone'       => 'Europe/Paris',
					'site_logo'      => 5,
					'language'       => '',
				)
			);
		},
	),
	'rest-comments-create'      => array(
		'action' => function () use ( $rest_send ) {
			$rest_send(
				'POST',
				'/wp/v2/comments',
				array(
					'post'         => 1,
					'parent'       => 1,
					'content'      => '<p onclick="x()">Hi</p> <a href="https://example.test/">link</a>',
					'author_name'  => 'Named <b>one</b>',
					'author_email' => 'named@example.test',
					'author_url'   => 'https://named.example.test/',
					'date'         => '2026-01-02T03:04:05',
					'status'       => 'hold',
				)
			);
		},
	),
	'rest-comments-update'      => array(
		'action' => function () use ( $rest_send ) {
			$rest_send(
				'POST',
				'/wp/v2/comments/1',
				array(
					'content'     => 'Edited <script>x()</script>',
					'author_name' => 'Renamed',
					'status'      => 'spam',
					'date_gmt'    => '2026-02-03T04:05:06Z',
				)
			);
		},
	),
	'rest-comments-trash'       => array(
		'action' => function () use ( $rest_send ) {
			$rest_send( 'DELETE', '/wp/v2/comments/1' );
		},
	),
	'rest-comments-delete'      => array(
		'action' => function () use ( $rest_send ) {
			$rest_send( 'DELETE', '/wp/v2/comments/1', array(), array( 'force' => 'true' ) );
		},
	),
	'rest-users-create'         => array(
		'action' => function () use ( $rest_send ) {
			remove_action( 'user_register', array( 'WP_Internal_Pointers', 'dismiss_pointers_for_new_users' ) );
			$rest_send(
				'POST',
				'/wp/v2/users',
				array(
					'username'    => 'newbie',
					'email'       => 'newbie@example.test',
					'password'    => 'secret-pass',
					'name'        => 'New Bie',
					'first_name'  => 'New',
					'description' => '<b>Hi</b><script>x()</script>',
					'slug'        => 'New Bie',
					'roles'       => array( 'editor', 'author' ),
				)
			);
		},
	),
	'rest-users-update'         => array(
		'setup'  => function () use ( $oracle_user ) {
			return $oracle_user( 'oracle-author', array( 'role' => 'author' ) );
		},
		'action' => function ( $id ) use ( $rest_send ) {
			$rest_send(
				'POST',
				"/wp/v2/users/$id",
				array(
					'name'        => 'Author Renamed',
					'nickname'    => 'Nick',
					'email'       => 'renamed@example.test',
					'url'         => 'https://a.example.test',
					'description' => 'Bio <em>here</em>',
					'password'    => 'new-secret',
					'roles'       => array( 'subscriber' ),
				)
			);
		},
	),
	'rest-users-delete'         => array(
		'setup'  => function () use ( $oracle_user ) {
			$id = $oracle_user( 'oracle-author', array( 'role' => 'author' ) );
			wp_update_post(
				array(
					'ID'          => 1,
					'post_author' => $id,
				)
			);
			return $id;
		},
		'action' => function ( $id ) use ( $rest_send ) {
			$rest_send(
				'DELETE',
				"/wp/v2/users/$id",
				array(),
				array(
					'force'    => 'true',
					'reassign' => '1',
				)
			);
		},
	),
);

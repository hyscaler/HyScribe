<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The write oracle's REST writes (MVP 4.3): the posts controller's
// create_item(), update_item() and delete_item() for posts, pages,
// navigation menus and synced patterns, and the terms, comments, users and
// settings controllers' writes, each a JSON request dispatched as user 1 through rest_do_request(), which runs the
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

// The navigation menus and synced patterns of reference/rest/site-writes.sql,
// which the tests insert as they are.
$rest_site_posts = function () {
	global $wpdb;
	$sql = file_get_contents( __DIR__ . '/site-writes.sql' );
	preg_match( '/^INSERT INTO `wp_posts` .*;$/m', $sql, $insert );
	$wpdb->query( $insert[0] );
};

// The rows of a state beside the list it is recorded with: each INSERT of
// reference/rest/<file>, which the tests insert as they are.
$rest_state = function ( $file ) {
	return function () use ( $file ) {
		global $wpdb;
		$sql = file_get_contents( __DIR__ . '/' . $file );
		preg_match_all( '/^INSERT INTO .*;$/m', $sql, $inserts );
		foreach ( $inserts[0] as $insert ) {
			$wpdb->query( $insert );
		}
	};
};

$paragraph = fn( $text ) => "<!-- wp:paragraph -->\n<p>$text</p>\n<!-- /wp:paragraph -->";

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
	'rest-navigation-create'    => array(
		'action' => function () use ( $rest_send ) {
			$rest_send(
				'POST',
				'/wp/v2/navigation',
				array(
					'title'   => 'Footer',
					'content' => '<!-- wp:navigation-link {"label":"Home","url":"http://127.0.0.1:8081/","kind":"custom"} /-->',
					'status'  => 'publish',
				)
			);
		},
	),
	'rest-navigation-update'    => array(
		'setup'  => $rest_site_posts,
		'action' => function () use ( $rest_send ) {
			$rest_send(
				'POST',
				'/wp/v2/navigation/851',
				array(
					'title'   => 'Header renamed',
					'content' => '<!-- wp:navigation-link {"label":"Only","url":"https://example.test/","kind":"custom"} /-->',
				)
			);
		},
	),
	'rest-navigation-publish'   => array(
		'setup'  => $rest_site_posts,
		'action' => function () use ( $rest_send ) {
			$rest_send(
				'PUT',
				'/wp/v2/navigation/852',
				array(
					'status' => 'publish',
					'slug'   => 'header',
				)
			);
		},
	),
	'rest-navigation-trash'     => array(
		'setup'  => $rest_site_posts,
		'action' => function () use ( $rest_send ) {
			$rest_send( 'DELETE', '/wp/v2/navigation/851' );
		},
	),
	'rest-navigation-delete'    => array(
		'setup'  => $rest_site_posts,
		'action' => function () use ( $rest_send ) {
			$rest_send( 'DELETE', '/wp/v2/navigation/852', array(), array( 'force' => 'true' ) );
		},
	),
	'rest-blocks-create'        => array(
		'action' => function () use ( $rest_send ) {
			$rest_send(
				'POST',
				'/wp/v2/blocks',
				array(
					'title'   => 'A pattern',
					'content' => "<!-- wp:paragraph -->\n<p>Reused</p>\n<!-- /wp:paragraph -->",
					'status'  => 'publish',
					'date'    => '2026-01-02T03:04:05',
				)
			);
		},
	),
	'rest-blocks-update'        => array(
		'setup'  => $rest_site_posts,
		'action' => function () use ( $rest_send ) {
			$rest_send(
				'POST',
				'/wp/v2/blocks/853',
				array(
					'title'   => 'Renamed pattern',
					'content' => "<!-- wp:paragraph -->\n<p>Changed</p>\n<!-- /wp:paragraph -->",
				)
			);
		},
	),
	'rest-blocks-publish'       => array(
		'setup'  => $rest_site_posts,
		'action' => function () use ( $rest_send ) {
			$rest_send( 'PATCH', '/wp/v2/blocks/854', array( 'status' => 'publish' ) );
		},
	),
	'rest-blocks-trash'         => array(
		'setup'  => $rest_site_posts,
		'action' => function () use ( $rest_send ) {
			$rest_send( 'DELETE', '/wp/v2/blocks/853' );
		},
	),
	'rest-blocks-delete'        => array(
		'setup'  => $rest_site_posts,
		'action' => function () use ( $rest_send ) {
			$rest_send( 'DELETE', '/wp/v2/blocks/854', array(), array( 'force' => '1' ) );
		},
	),
	'rest-blocks-create-unsynced'     => array(
		'action' => function () use ( $rest_send ) {
			$rest_send(
				'POST',
				'/wp/v2/blocks',
				array(
					'title' => 'Unsynced',
					'meta'  => array( 'wp_pattern_sync_status' => 'unsynced' ),
				)
			);
		},
	),
	'rest-blocks-create-categorised'  => array(
		'setup'  => $rest_state( 'site-writes.sql' ),
		'action' => function () use ( $rest_send, $paragraph ) {
			$rest_send(
				'POST',
				'/wp/v2/blocks',
				array(
					'title'               => 'In two',
					'content'             => $paragraph( 'Two' ),
					'status'              => 'publish',
					'wp_pattern_category' => array( 861, 862, 999 ),
					'meta'                => array(
						'other'                  => 'x',
						'wp_pattern_sync_status' => 'partial',
					),
				)
			);
		},
	),
	'rest-blocks-categories'          => array(
		'setup'  => $rest_state( 'site-writes.sql' ),
		'action' => function () use ( $rest_send ) {
			$rest_send( 'POST', '/wp/v2/blocks/853', array( 'wp_pattern_category' => array( 862 ) ) );
		},
	),
	'rest-blocks-draft-meta'          => array(
		'setup'  => $rest_state( 'site-writes.sql' ),
		'action' => function () use ( $rest_send ) {
			$rest_send(
				'POST',
				'/wp/v2/blocks/854',
				array(
					'meta'                => array( 'wp_pattern_sync_status' => 'partial' ),
					'wp_pattern_category' => array( 861 ),
				)
			);
		},
	),
	'rest-blocks-delete-categorised'  => array(
		'setup'  => $rest_state( 'site-writes.sql' ),
		'action' => function () use ( $rest_send ) {
			$rest_send( 'DELETE', '/wp/v2/blocks/853', array(), array( 'force' => 'true' ) );
		},
	),
	'rest-templates-create'           => array(
		'setup'  => $rest_state( 'template-writes.sql' ),
		'action' => function () use ( $rest_send, $paragraph ) {
			$rest_send(
				'POST',
				'/wp/v2/templates',
				array(
					'slug'        => 'my-template',
					'title'       => 'My template',
					'content'     => $paragraph( 'Mine' ),
					'description' => 'Made here',
				)
			);
		},
	),
	'rest-templates-create-suggested' => array(
		'setup'  => $rest_state( 'template-writes.sql' ),
		'action' => function () use ( $rest_send ) {
			$rest_send(
				'POST',
				'/wp/v2/templates',
				array(
					'slug'             => 'page-about',
					'title'            => array( 'raw' => 'About' ),
					'content'          => array( 'raw' => '<!-- wp:post-content /-->' ),
					'author'           => 1,
					'is_wp_suggestion' => true,
				)
			);
		},
	),
	'rest-templates-create-taken'     => array(
		'setup'  => $rest_state( 'template-writes.sql' ),
		'action' => function () use ( $rest_send ) {
			$rest_send(
				'POST',
				'/wp/v2/templates',
				array(
					'slug'    => 'my-custom',
					'title'   => 'Again',
					'content' => '<!-- wp:post-title /-->',
				)
			);
		},
	),
	'rest-template-parts-create'      => array(
		'setup'  => $rest_state( 'template-writes.sql' ),
		'action' => function () use ( $rest_send, $paragraph ) {
			$rest_send(
				'POST',
				'/wp/v2/template-parts',
				array(
					'slug'    => 'my-sidebar',
					'title'   => 'My sidebar',
					'area'    => 'header',
					'content' => $paragraph( 'Side' ),
				)
			);
		},
	),
	'rest-template-parts-create-area' => array(
		'setup'  => $rest_state( 'template-writes.sql' ),
		'action' => function () use ( $rest_send ) {
			$rest_send(
				'POST',
				'/wp/v2/template-parts',
				array(
					'slug'  => 'odd-part',
					'title' => 'Odd',
					'area'  => 'bogus',
				)
			);
		},
	),
	'rest-templates-customise'        => array(
		'setup'  => $rest_state( 'template-writes.sql' ),
		'action' => function () use ( $rest_send, $paragraph ) {
			$rest_send( 'POST', '/wp/v2/templates/twentytwentyfive//page', array( 'content' => $paragraph( 'Customised' ) ) );
		},
	),
	'rest-template-parts-customise'   => array(
		'setup'  => $rest_state( 'template-writes.sql' ),
		'action' => function () use ( $rest_send, $paragraph ) {
			$rest_send( 'PUT', '/wp/v2/template-parts/twentytwentyfive//footer', array( 'content' => $paragraph( 'Foot' ) ) );
		},
	),
	'rest-templates-update'           => array(
		'setup'  => $rest_state( 'template-writes.sql' ),
		'action' => function () use ( $rest_send, $paragraph ) {
			$rest_send(
				'POST',
				'/wp/v2/templates/twentytwentyfive//index',
				array(
					'title'   => 'Index again',
					'content' => $paragraph( 'Changed' ),
				)
			);
		},
	),
	'rest-template-parts-update'      => array(
		'setup'  => $rest_state( 'template-writes.sql' ),
		'action' => function () use ( $rest_send ) {
			$rest_send(
				'PATCH',
				'/wp/v2/template-parts/twentytwentyfive//header',
				array(
					'area'        => 'footer',
					'description' => 'Moved',
				)
			);
		},
	),
	'rest-templates-revert'           => array(
		'setup'  => $rest_state( 'template-writes.sql' ),
		'action' => function () use ( $rest_send ) {
			$rest_send( 'POST', '/wp/v2/templates/twentytwentyfive//page-no-title', array( 'source' => 'theme' ) );
		},
	),
	'rest-templates-trash'            => array(
		'setup'  => $rest_state( 'template-writes.sql' ),
		'action' => function () use ( $rest_send ) {
			$rest_send( 'DELETE', '/wp/v2/templates/twentytwentyfive//my-custom' );
		},
	),
	'rest-templates-delete'           => array(
		'setup'  => $rest_state( 'template-writes.sql' ),
		'action' => function () use ( $rest_send ) {
			$rest_send( 'DELETE', '/wp/v2/templates/twentytwentyfive//my-custom', array(), array( 'force' => 'true' ) );
		},
	),
	'rest-template-parts-delete'      => array(
		'setup'  => $rest_state( 'template-writes.sql' ),
		'action' => function () use ( $rest_send ) {
			$rest_send( 'DELETE', '/wp/v2/template-parts/twentytwentyfive//header', array(), array( 'force' => '1' ) );
		},
	),
	'rest-autosaves-draft'            => array(
		'setup'  => $rest_state( 'autosaves.sql' ),
		'action' => function () use ( $rest_send, $paragraph ) {
			$rest_send(
				'POST',
				'/wp/v2/pages/863/autosaves',
				array(
					'title'   => 'Page autosaved',
					'content' => $paragraph( 'Page words' ),
				)
			);
		},
	),
	'rest-autosaves-new'              => array(
		'setup'  => $rest_state( 'autosaves.sql' ),
		'action' => function () use ( $rest_send, $paragraph ) {
			$rest_send(
				'POST',
				'/wp/v2/posts/866/autosaves',
				array(
					'title'   => 'Held, autosaved',
					'content' => $paragraph( 'Held' ),
				)
			);
		},
	),
	'rest-autosaves-other'            => array(
		'setup'  => $rest_state( 'autosaves.sql' ),
		'action' => function () use ( $rest_send, $paragraph ) {
			$rest_send(
				'POST',
				'/wp/v2/posts/861/autosaves',
				array(
					'title'   => 'Draft autosaved',
					'content' => $paragraph( 'Later words' ),
					'excerpt' => '',
				)
			);
		},
	),
	'rest-autosaves-again'            => array(
		'setup'  => $rest_state( 'autosaves.sql' ),
		'action' => function () use ( $rest_send, $paragraph ) {
			$rest_send(
				'POST',
				'/wp/v2/posts/862/autosaves',
				array(
					'title'   => 'Published',
					'content' => $paragraph( 'Out there, changed' ),
					'excerpt' => '',
				)
			);
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

<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The write oracle's post actions (MVP 2.4): wp_insert_post(),
// wp_update_post(), wp_publish_post(), wp_trash_post(), wp_untrash_post() and
// wp_delete_post(). A post whose ID ends up inside a value no mask reaches (a
// cron key, sticky_posts, page_on_front) is inserted with an import_id, and
// comments with a fixed ID, so both sides write the same values.

$posts_insert = function ( $args ) {
	$args += array(
		'post_title'   => 'A write oracle post',
		'post_content' => 'Its content.',
		'post_status'  => 'publish',
		'post_author'  => 1,
	);
	return wp_insert_post( wp_slash( $args ), true );
};

$posts_update = function ( $args ) {
	return wp_update_post( wp_slash( $args ), true );
};

$posts_future = function () use ( $posts_insert ) {
	return $posts_insert(
		array(
			'import_id' => 900,
			'post_date' => '2030-01-01 10:00:00',
		)
	);
};

// A comment written raw, so the HyScribe side can write the same row.
$posts_comment = function ( $post_id, $comment_id, $approved, $parent = 0 ) {
	global $wpdb;
	$wpdb->insert(
		$wpdb->comments,
		array(
			'comment_ID'       => $comment_id,
			'comment_post_ID'  => $post_id,
			'comment_parent'   => $parent,
			'comment_author'   => 'A reader',
			'comment_date'     => '2026-01-01 00:00:00',
			'comment_date_gmt' => '2026-01-01 00:00:00',
			'comment_content'  => 'A comment.',
			'comment_approved' => $approved,
		)
	);
	$wpdb->insert(
		$wpdb->commentmeta,
		array(
			'comment_id' => $comment_id,
			'meta_key'   => 'oracle',
			'meta_value' => $approved,
		)
	);
};

$posts_publish = function () use ( $posts_insert ) {
	return $posts_insert( array() );
};

$posts_trash_raw = function ( $id ) {
	global $wpdb;
	$wpdb->update( $wpdb->posts, array( 'post_status' => 'trash' ), array( 'ID' => $id ) );
	clean_post_cache( $id );
	return $id;
};

$posts_with_comments = function () use ( $posts_insert, $posts_comment ) {
	$id = $posts_insert( array() );
	$posts_comment( $id, 901, '1' );
	$posts_comment( $id, 902, '0' );
	$posts_comment( $id, 903, '1' );
	return $id;
};

// A setup that sets the permalink structure as Settings > Permalinks does.
$posts_permalinks = function ( $structure ) {
	return function () use ( $structure ) {
		global $wp_rewrite;
		$wp_rewrite->set_permalink_structure( $structure );
	};
};

// Scheduled, published and private posts and pages, and a revision of each,
// dated so their GUIDs never name the day of the run.
$posts_every_status = function () use ( $posts_insert, $posts_update ) {
	foreach ( array( 'post', 'page' ) as $n => $type ) {
		$posts_insert(
			array(
				'import_id'  => 910 + $n,
				'post_type'  => $type,
				'post_title' => 'A scheduled post',
				'post_date'  => '2030-01-01 10:00:00',
			)
		);
	}
	foreach ( array( 'post', 'page' ) as $type ) {
		$id = $posts_insert(
			array(
				'post_type' => $type,
				'post_date' => '2026-03-07 10:00:00',
			)
		);
		$posts_insert(
			array(
				'post_type'   => $type,
				'post_status' => 'private',
				'post_title'  => 'A private post',
				'post_date'   => '2026-03-07 10:00:00',
			)
		);
		$posts_update(
			array(
				'ID'         => $id,
				'post_title' => 'A renamed post',
			)
		);
	}
};

return array(
	'insert-colliding-slugs'                 => array(
		'action' => function () use ( $posts_insert ) {
			$posts_insert( array( 'post_title' => 'Hello world!' ) );
			$posts_insert( array( 'post_title' => 'Hello world!' ) );
			$posts_insert( array( 'post_title' => 'Feed' ) );
			$posts_insert( array( 'post_title' => '' ) );
			$posts_insert(
				array(
					'post_title' => 'Sample Page',
					'post_type'  => 'page',
				)
			);
			$posts_insert(
				array(
					'post_title' => 'page2',
					'post_type'  => 'page',
				)
			);
		},
	),
	'insert-future-post'                     => array(
		'action' => function () use ( $posts_future ) {
			$posts_future();
		},
	),
	'publish-future-post'                    => array(
		'setup'  => $posts_future,
		'action' => function ( $id ) {
			wp_publish_post( $id );
		},
	),
	'trash-future-post'                      => array(
		'setup'  => $posts_future,
		'action' => function ( $id ) {
			wp_trash_post( $id );
		},
	),
	'insert-private-post'                    => array(
		'action' => function () use ( $posts_insert ) {
			$posts_insert(
				array(
					'post_title'    => 'A private post',
					'post_status'   => 'private',
					'post_password' => 'secret',
				)
			);
		},
	),
	'insert-pages-with-parent'               => array(
		'action' => function () use ( $posts_insert ) {
			foreach ( array( 2, 2, 0 ) as $parent ) {
				$posts_insert(
					array(
						'post_title'  => 'Child',
						'post_type'   => 'page',
						'post_parent' => $parent,
						'menu_order'  => 3,
					)
				);
			}
		},
	),
	'update-post-to-draft'                   => array(
		'setup'  => function () use ( $posts_insert ) {
			return $posts_insert( array() );
		},
		'action' => function ( $id ) use ( $posts_update ) {
			$posts_update(
				array(
					'ID'          => $id,
					'post_status' => 'draft',
				)
			);
		},
	),
	'update-post-slug-and-date'              => array(
		// A fixed date, as `_wp_old_date` keeps its day.
		'setup'  => function () use ( $posts_insert ) {
			return $posts_insert( array( 'post_date' => '2025-06-07 08:09:10' ) );
		},
		'action' => function ( $id ) use ( $posts_update ) {
			$posts_update(
				array(
					'ID'        => $id,
					'post_name' => 'A new slug',
					'post_date' => '2026-01-02 03:04:05',
				)
			);
		},
	),
	'insert-pending-post-without-capability' => array(
		'action' => function () use ( $posts_insert ) {
			wp_set_current_user( 0 );
			$posts_insert(
				array(
					'post_status' => 'pending',
					'post_name'   => 'chosen',
				)
			);
		},
	),
	'insert-sanitized-post'                  => array(
		'action' => function () use ( $posts_insert ) {
			$posts_insert(
				array(
					'post_title'     => "  <b>Bold</b> <script>alert(1)</script>title\t ",
					'post_content'   => '<p onclick="x()">Para</p><script>bad()</script><a href="javascript:x()">link</a>',
					'post_excerpt'   => '<em>ok</em><iframe src="https://example.test/"></iframe>',
					'post_status'    => 'Publish',
					'comment_status' => 'Closed',
					'ping_status'    => 'CLOSED',
					'post_password'  => 'pw',
					'to_ping'        => "http://a.test/one ftp://b.test\nhttps://c.test/two not-a-url",
					'menu_order'     => '5',
				)
			);
		},
	),
	'insert-reclaims-trashed-slug'           => array(
		'setup'  => function () use ( $posts_insert ) {
			global $wpdb;
			$id = $posts_insert( array( 'post_title' => 'Taken' ) );
			$wpdb->update( $wpdb->posts, array( 'post_status' => 'trash' ), array( 'ID' => $id ) );
			clean_post_cache( $id );
			return $id;
		},
		'action' => function () use ( $posts_insert ) {
			$posts_insert( array( 'post_title' => 'Taken' ) );
		},
	),
	'update-post-with-footnotes'             => array(
		'setup'  => function () use ( $posts_insert ) {
			$id = $posts_insert( array() );
			add_post_meta( $id, 'footnotes', '[{"id":"n1","content":"A note."}]' );
			return $id;
		},
		'action' => function ( $id ) use ( $posts_update ) {
			$posts_update(
				array(
					'ID'         => $id,
					'post_title' => 'A renamed post',
				)
			);
		},
	),
	'trash-post-with-comments'               => array(
		'setup'  => $posts_with_comments,
		'action' => function ( $id ) {
			wp_trash_post( $id );
		},
	),
	'untrash-post-with-comments'             => array(
		'setup'  => function () use ( $posts_with_comments ) {
			$id = $posts_with_comments();
			wp_trash_post( $id );
			return $id;
		},
		'action' => function ( $id ) {
			wp_untrash_post( $id );
		},
	),
	'delete-post-with-comments'              => array(
		'setup'  => function () use ( $posts_with_comments, $posts_update ) {
			$id = $posts_with_comments();
			$posts_update(
				array(
					'ID'         => $id,
					'post_title' => 'A renamed post',
				)
			);
			return $id;
		},
		'action' => function ( $id ) {
			wp_delete_post( $id, true );
		},
	),
	'delete-post-to-trash'                   => array(
		'setup'  => function () use ( $posts_insert ) {
			return $posts_insert( array() );
		},
		'action' => function ( $id ) {
			wp_delete_post( $id );
		},
	),
	'delete-front-page-with-children'        => array(
		'setup'  => function () use ( $posts_insert ) {
			$id = $posts_insert(
				array(
					'import_id'  => 910,
					'post_title' => 'Parent',
					'post_type'  => 'page',
				)
			);
			$posts_insert(
				array(
					'post_title'  => 'Kid',
					'post_type'   => 'page',
					'post_parent' => $id,
				)
			);
			update_option( 'show_on_front', 'page' );
			update_option( 'page_on_front', $id );
			return $id;
		},
		'action' => function ( $id ) {
			wp_delete_post( $id, true );
		},
	),
	'trash-sticky-post'                      => array(
		'setup'  => function () use ( $posts_insert ) {
			$id = $posts_insert( array( 'import_id' => 930 ) );
			stick_post( $id );
			return $id;
		},
		'action' => function ( $id ) {
			wp_trash_post( $id );
		},
	),
	'insert-over-existing-post'              => array(
		'setup'  => $posts_publish,
		'action' => function ( $id ) {
			wp_insert_post(
				array(
					'ID'           => $id,
					'post_title'   => 'Rewritten',
					'post_content' => 'New.',
					'post_status'  => 'publish',
				)
			);
		},
	),
	'update-post-category'                   => array(
		'setup'  => function () use ( $posts_publish ) {
			global $wpdb;
			$wpdb->query( "INSERT INTO $wpdb->terms (term_id, name, slug, term_group) VALUES (950, 'Oracle', 'oracle', 0)" );
			$wpdb->query( "INSERT INTO $wpdb->term_taxonomy (term_taxonomy_id, term_id, taxonomy, description, parent, count) VALUES (950, 950, 'category', '', 0, 0)" );
			return $posts_publish();
		},
		'action' => function ( $id ) use ( $posts_update ) {
			$posts_update(
				array(
					'ID'            => $id,
					'post_category' => array( 950, HYSCRIBE_MISSING_ID, 950 ),
				)
			);
		},
	),
	'update-page-parents'                    => array(
		'setup'  => function () use ( $posts_insert ) {
			$page = function ( $title, $parent ) use ( $posts_insert ) {
				return $posts_insert(
					array(
						'post_title'  => $title,
						'post_type'   => 'page',
						'post_parent' => $parent,
					)
				);
			};
			$a = $page( 'A', 0 );
			return array( $a, $page( 'B', $a ), $page( 'C', 0 ) );
		},
		'action' => function ( $pages ) use ( $posts_update ) {
			list( $a, $b, $c ) = $pages;
			foreach ( array( array( $a, $b ), array( $c, $a ), array( $b, $b ) ) as list( $id, $parent ) ) {
				$posts_update(
					array(
						'ID'          => $id,
						'post_parent' => $parent,
					)
				);
			}
		},
	),
	'publish-uncategorized-draft'            => array(
		'setup'  => function () use ( $posts_insert ) {
			global $wpdb;
			$id = $posts_insert( array( 'post_status' => 'draft' ) );
			$wpdb->delete( $wpdb->term_relationships, array( 'object_id' => $id ) );
			clean_object_term_cache( $id, 'post' );
			return $id;
		},
		'action' => function ( $id ) {
			wp_publish_post( $id );
		},
	),
	'insert-dated-posts'                     => array(
		'action' => function () use ( $posts_insert ) {
			$posts_insert(
				array(
					'post_status' => 'future',
					'post_date'   => '2020-01-02 03:04:05',
				)
			);
			$posts_insert( array( 'post_date_gmt' => '2024-05-06 07:08:09' ) );
			$posts_insert( array( 'post_status' => '' ) );
			$posts_insert(
				array(
					'post_status' => 'draft',
					'post_date'   => '2023-01-01 00:00:00',
				)
			);
			$posts_insert(
				array(
					'post_title'   => 'Only a title',
					'post_content' => '',
				)
			);
		},
	),
	'delete-privacy-policy-page'             => array(
		'action' => function () {
			wp_delete_post( 3, true );
		},
	),
	'trash-posts-page'                       => array(
		'setup'  => function () use ( $posts_insert ) {
			$id = $posts_insert(
				array(
					'import_id'  => 940,
					'post_title' => 'Blog',
					'post_type'  => 'page',
				)
			);
			update_option( 'page_for_posts', $id );
			return $id;
		},
		'action' => function ( $id ) {
			wp_trash_post( $id );
		},
	),
	'delete-post-with-comment-replies'       => array(
		'setup'  => function () use ( $posts_publish, $posts_comment ) {
			$id = $posts_publish();
			$posts_comment( $id, 901, '1', 902 );
			$posts_comment( $id, 902, '1' );
			$posts_comment( 1, 903, '1', 901 );
			return $id;
		},
		'action' => function ( $id ) {
			wp_delete_post( $id, true );
		},
	),
	'trash-post-named-trashed'               => array(
		'setup'  => function () use ( $posts_insert ) {
			return $posts_insert( array( 'post_title' => 'Gone__trashed' ) );
		},
		'action' => function ( $id ) {
			wp_trash_post( $id );
		},
	),
	'update-numeric-slug-under-pretty-permalinks' => array(
		// The title's update leaves a revision, so the action makes none.
		'setup'  => function () use ( $posts_publish, $posts_update ) {
			$id = $posts_publish();
			$posts_update(
				array(
					'ID'         => $id,
					'post_title' => 'A renamed post',
				)
			);
			update_option( 'permalink_structure', '/%postname%/' );
			return $id;
		},
		'action' => function ( $id ) use ( $posts_update ) {
			// Only digits read as dates; "00" names no day.
			foreach ( array( '12', 'plain-words', '00' ) as $name ) {
				$posts_update(
					array(
						'ID'        => $id,
						'post_name' => $name,
					)
				);
			}
		},
	),
	'update-post-slug-back'                  => array(
		'setup'  => function () use ( $posts_publish, $posts_update ) {
			$id = $posts_publish();
			$posts_update(
				array(
					'ID'        => $id,
					'post_name' => 'second',
				)
			);
			return $id;
		},
		'action' => function ( $id ) use ( $posts_update ) {
			$posts_update(
				array(
					'ID'        => $id,
					'post_name' => 'a-write-oracle-post',
				)
			);
		},
	),
	'insert-long-colliding-slugs'            => array(
		'action' => function () use ( $posts_insert ) {
			$posts_insert( array( 'post_title' => str_repeat( 'a-', 100 ) ) );
			$posts_insert( array( 'post_title' => str_repeat( 'a-', 100 ) ) );
		},
	),
	'untrash-post-without-desired-slug'      => array(
		'setup'  => function () use ( $posts_publish, $posts_trash_raw ) {
			return $posts_trash_raw( $posts_publish() );
		},
		'action' => function ( $id ) {
			wp_untrash_post( $id );
		},
	),
	'insert-post-without-pingbacks'          => array(
		'setup'  => function () {
			update_option( 'default_pingback_flag', '0' );
		},
		'action' => function () use ( $posts_publish ) {
			$posts_publish();
		},
	),
	'publish-post-without-guid'              => array(
		'setup'  => function () use ( $posts_insert ) {
			global $wpdb;
			$id = $posts_insert( array( 'post_status' => 'draft' ) );
			$wpdb->update( $wpdb->posts, array( 'guid' => '' ), array( 'ID' => $id ) );
			clean_post_cache( $id );
			return $id;
		},
		'action' => function ( $id ) use ( $posts_update ) {
			$posts_update(
				array(
					'ID'          => $id,
					'post_status' => 'publish',
				)
			);
		},
	),
	'update-auto-draft'                      => array(
		'setup'  => function () use ( $posts_insert ) {
			return $posts_insert( array( 'post_status' => 'auto-draft' ) );
		},
		'action' => function ( $id ) use ( $posts_update ) {
			$posts_update(
				array(
					'ID'         => $id,
					'post_title' => 'A renamed post',
				)
			);
		},
	),
	'update-post-with-autosave'              => array(
		'setup'  => function () use ( $posts_publish ) {
			$id = $posts_publish();
			_wp_put_post_revision( get_post( $id ), true );
			return $id;
		},
		'action' => function ( $id ) use ( $posts_update ) {
			$posts_update(
				array(
					'ID'         => $id,
					'post_title' => 'A renamed post',
				)
			);
		},
	),
	'trash-hello-world'                      => array(
		'action' => function () {
			wp_trash_post( 1 );
		},
	),
	'trash-last-published-post'              => array(
		'setup'  => function () use ( $posts_publish ) {
			$id = $posts_publish();
			wp_trash_post( 1 );
			return $id;
		},
		'action' => function ( $id ) {
			wp_trash_post( $id );
		},
	),
	'user-delete-with-posts'                 => array(
		'setup'  => function () use ( $posts_insert ) {
			require_once ABSPATH . 'wp-admin/includes/user.php';
			$id = wp_insert_user(
				array(
					'user_login' => 'oracle-poster',
					'user_pass'  => 'A pass phrase',
					'user_email' => 'oracle-poster@example.test',
				)
			);
			$posts_insert( array( 'post_author' => $id ) );
			$posts_insert(
				array(
					'post_author' => $id,
					'post_title'  => 'A page',
					'post_type'   => 'page',
				)
			);
			return $id;
		},
		'action' => function ( $id ) {
			wp_delete_user( $id );
		},
	),
	'update-categorized-post'                => array(
		'setup'  => function () use ( $posts_insert ) {
			global $wpdb;
			$wpdb->query( "INSERT INTO $wpdb->terms (term_id, name, slug, term_group) VALUES (950, 'Oracle', 'oracle', 0)" );
			$wpdb->query( "INSERT INTO $wpdb->term_taxonomy (term_taxonomy_id, term_id, taxonomy, description, parent, count) VALUES (950, 950, 'category', '', 0, 0)" );
			return $posts_insert( array( 'post_category' => array( 950 ) ) );
		},
		'action' => function ( $id ) use ( $posts_update ) {
			$posts_update(
				array(
					'ID'         => $id,
					'post_title' => 'A renamed post',
				)
			);
			wp_insert_post(
				array(
					'ID'           => $id,
					'post_title'   => 'Rewritten',
					'post_content' => 'New.',
					'post_status'  => 'publish',
				)
			);
		},
	),
	'publish-dated-drafts'                   => array(
		'setup'  => function () use ( $posts_insert ) {
			$draft = array(
				'post_status' => 'draft',
				'post_date'   => '2025-01-01 00:00:00',
			);
			return array( $posts_insert( $draft ), $posts_insert( $draft ) );
		},
		'action' => function ( $ids ) use ( $posts_update ) {
			$posts_update(
				array(
					'ID'          => $ids[0],
					'post_status' => 'publish',
				)
			);
			$posts_update(
				array(
					'ID'          => $ids[1],
					'post_status' => 'publish',
					'edit_date'   => true,
				)
			);
		},
	),
	'update-unpublished-post-keeps-date'     => array(
		'setup'  => function () use ( $posts_insert, $posts_update ) {
			$id = $posts_insert( array( 'post_date' => '2025-02-03 10:00:00' ) );
			$posts_update(
				array(
					'ID'          => $id,
					'post_status' => 'draft',
				)
			);
			return $id;
		},
		'action' => function ( $id ) use ( $posts_update ) {
			$posts_update(
				array(
					'ID'         => $id,
					'post_title' => 'A renamed post',
				)
			);
		},
	),
	'publish-draft-page'                     => array(
		'setup'  => function () use ( $posts_insert ) {
			return $posts_insert(
				array(
					'post_status' => 'draft',
					'post_type'   => 'page',
				)
			);
		},
		'action' => function ( $id ) {
			wp_publish_post( $id );
		},
	),
	'insert-pending-post'                    => array(
		'action' => function () use ( $posts_insert ) {
			$posts_insert(
				array(
					'post_status' => 'pending',
					'post_name'   => 'chosen',
				)
			);
		},
	),
	'update-post-with-stray-desired-slug'    => array(
		'setup'  => function () use ( $posts_publish ) {
			$id = $posts_publish();
			add_post_meta( $id, '_wp_desired_post_slug', 'elsewhere' );
			return $id;
		},
		'action' => function ( $id ) use ( $posts_update ) {
			$posts_update(
				array(
					'ID'         => $id,
					'post_title' => 'A renamed post',
				)
			);
		},
	),
	'trash-post-beside-trashed-namesake'     => array(
		'setup'  => function () use ( $posts_insert ) {
			global $wpdb;
			$id      = $posts_insert( array( 'post_title' => 'Taken' ) );
			$trashed = $posts_insert( array( 'post_title' => 'Other' ) );
			$wpdb->update(
				$wpdb->posts,
				array(
					'post_status' => 'trash',
					'post_name'   => 'taken',
				),
				array( 'ID' => $trashed )
			);
			clean_post_cache( $trashed );
			return $id;
		},
		'action' => function ( $id ) {
			wp_trash_post( $id );
		},
	),
	'publish-post-with-custom-guid'          => array(
		'setup'  => function () use ( $posts_insert ) {
			global $wpdb;
			$id = $posts_insert( array( 'post_status' => 'draft' ) );
			$wpdb->update( $wpdb->posts, array( 'guid' => 'urn:oracle:custom' ), array( 'ID' => $id ) );
			clean_post_cache( $id );
			return $id;
		},
		'action' => function ( $id ) use ( $posts_update ) {
			$posts_update(
				array(
					'ID'          => $id,
					'post_status' => 'publish',
				)
			);
		},
	),
	'update-post-name-with-autosave'         => array(
		'setup'  => function () use ( $posts_publish ) {
			$id = $posts_publish();
			_wp_put_post_revision( get_post( $id ), true );
			return $id;
		},
		'action' => function ( $id ) use ( $posts_update ) {
			$posts_update(
				array(
					'ID'        => $id,
					'post_name' => 'renamed',
				)
			);
		},
	),
	'update-post-twice'                      => array(
		'setup'  => function () use ( $posts_publish, $posts_update ) {
			$id = $posts_publish();
			$posts_update(
				array(
					'ID'         => $id,
					'post_title' => 'A renamed post',
				)
			);
			return $id;
		},
		'action' => function ( $id ) use ( $posts_update ) {
			$posts_update(
				array(
					'ID'         => $id,
					'post_title' => 'Renamed again',
				)
			);
		},
	),
	'delete-tagged-post'                     => array(
		'setup'  => function () use ( $posts_publish ) {
			global $wpdb;
			$wpdb->query( "INSERT INTO $wpdb->terms (term_id, name, slug, term_group) VALUES (960, 'Tagged', 'tagged', 0)" );
			$wpdb->query( "INSERT INTO $wpdb->term_taxonomy (term_taxonomy_id, term_id, taxonomy, description, parent, count) VALUES (960, 960, 'post_tag', '', 0, 1)" );
			$id = $posts_publish();
			$wpdb->insert(
				$wpdb->term_relationships,
				array(
					'object_id'        => $id,
					'term_taxonomy_id' => 960,
				)
			);
			clean_object_term_cache( $id, 'post' );
			return $id;
		},
		'action' => function ( $id ) {
			wp_delete_post( $id, true );
		},
	),
	'insert-inherit-post'                    => array(
		'action' => function () use ( $posts_insert ) {
			$posts_insert(
				array(
					'post_title'  => 'Hello world!',
					'post_status' => 'inherit',
				)
			);
		},
	),
	'delete-draft-post'                      => array(
		'setup'  => function () use ( $posts_insert ) {
			return $posts_insert( array( 'post_status' => 'draft' ) );
		},
		'action' => function ( $id ) {
			wp_delete_post( $id, true );
		},
	),
	// The trashed notice's Undo: the status the post was trashed from.
	'untrash-post-to-previous-status'        => array(
		'setup'  => function () use ( $posts_insert ) {
			$id = $posts_insert( array() );
			wp_trash_post( $id );
			return $id;
		},
		'action' => function ( $id ) {
			add_filter( 'wp_untrash_post_status', 'wp_untrash_post_set_previous_status', 10, 3 );
			wp_untrash_post( $id );
		},
	),
	'stick-and-unstick-posts'                => array(
		'setup'  => function () use ( $posts_insert ) {
			return $posts_insert( array( 'import_id' => 970 ) );
		},
		'action' => function ( $id ) {
			stick_post( $id );
			stick_post( $id );
			stick_post( 1 );
			unstick_post( HYSCRIBE_MISSING_ID );
		},
	),
	'update-page-template'                   => array(
		'setup'  => function () use ( $posts_insert ) {
			return $posts_insert( array( 'post_type' => 'page' ) );
		},
		'action' => function ( $id ) use ( $posts_update ) {
			$posts_update(
				array(
					'ID'            => $id,
					'page_template' => 'default',
				)
			);
		},
	),
	'insert-posts-under-day-and-name-permalinks' => array(
		'setup'  => $posts_permalinks( '/%year%/%monthnum%/%day%/%postname%/' ),
		'action' => $posts_every_status,
	),
	'insert-posts-under-month-and-name-permalinks' => array(
		'setup'  => $posts_permalinks( '/%year%/%monthnum%/%postname%/' ),
		'action' => $posts_every_status,
	),
	'insert-posts-under-numeric-permalinks'  => array(
		'setup'  => $posts_permalinks( '/archives/%post_id%' ),
		'action' => $posts_every_status,
	),
	'insert-posts-under-post-name-permalinks' => array(
		'setup'  => $posts_permalinks( '/%postname%/' ),
		'action' => $posts_every_status,
	),
	'insert-posts-under-custom-permalinks'   => array(
		'setup'  => $posts_permalinks( '/%category%/%author%/%postname%/' ),
		'action' => $posts_every_status,
	),
	'insert-posts-in-categories-under-permalinks' => array(
		'setup'  => $posts_permalinks( '/%category%/%author%/%postname%/' ),
		'action' => function () use ( $posts_insert ) {
			$parent = wp_insert_term( 'Parent', 'category' );
			$child  = wp_insert_term( 'Child', 'category', array( 'parent' => $parent['term_id'] ) );
			// The lowest ID names the link; a missing category leaves the default.
			foreach ( array( array( $child['term_id'] ), array( $child['term_id'], 1 ), array( 1 << 40 ) ) as $categories ) {
				$posts_insert( array( 'post_category' => $categories ) );
			}
			$posts_insert(
				array(
					'post_type'   => 'page',
					'post_parent' => 2,
				)
			);
			// A private post the current user may not read keeps its plain link.
			wp_set_current_user( 0 );
			$posts_insert( array( 'post_status' => 'private' ) );
		},
	),
	'insert-posts-in-odd-categories-under-permalinks' => array(
		'setup'  => function () use ( $posts_permalinks ) {
			global $wpdb;
			$posts_permalinks( '/%category%/%author%/%postname%/' )();
			// 970 and 971 are each other's parents; 972's parent is missing.
			foreach ( array( 970 => 971, 971 => 970, 972 => 1 << 40 ) as $id => $parent ) {
				$wpdb->query( $wpdb->prepare( "INSERT INTO $wpdb->terms (term_id, name, slug, term_group) VALUES (%d, %s, %s, 0)", $id, "Odd $id", "odd-$id" ) );
				$wpdb->query( $wpdb->prepare( "INSERT INTO $wpdb->term_taxonomy (term_taxonomy_id, term_id, taxonomy, description, parent, count) VALUES (%d, %d, 'category', '', %d, 0)", $id, $id, $parent ) );
			}
			update_option( 'default_category', 1 << 40 );
		},
		'action' => function () use ( $posts_insert ) {
			$posts_insert( array( 'post_category' => array( 971 ) ) );
			$posts_insert( array( 'post_category' => array( 972 ) ) );
			$posts_insert( array( 'post_author' => 1 << 40 ) );
		},
	),
);

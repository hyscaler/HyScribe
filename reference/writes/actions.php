<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The write oracle's actions (reference/writes/golden.sh), one golden each.
// `setup` makes the state the action starts from and returns what the action
// needs; only `action`'s rows are recorded, though rows `setup` added are
// new too (their IDs are masked). Both run as user 1 in one transaction.

$new_post = function ( $status ) {
	return wp_insert_post(
		array(
			'post_title'   => 'A write oracle post',
			'post_content' => 'Its content.',
			'post_status'  => $status,
			'post_author'  => 1,
		),
		true
	);
};

$new_comment = function ( $approved ) {
	return wp_insert_comment(
		array(
			'comment_post_ID'      => 1,
			'comment_author'       => 'A reader',
			'comment_author_email' => 'reader@example.test',
			'comment_author_url'   => 'https://reader.example.test',
			'comment_author_IP'    => '192.0.2.1',
			'comment_content'      => 'A comment.',
			'comment_agent'        => 'Oracle',
			'comment_approved'     => $approved,
		)
	);
};

$actions = array(
	'insert-draft-post'  => array(
		'action' => function () use ( $new_post ) {
			$new_post( 'draft' );
		},
	),
	'publish-post'       => array(
		'setup'  => function () use ( $new_post ) {
			return $new_post( 'draft' );
		},
		'action' => function ( $id ) {
			wp_update_post(
				array(
					'ID'          => $id,
					'post_status' => 'publish',
				)
			);
		},
	),
	'update-post-title'  => array(
		'setup'  => function () use ( $new_post ) {
			return $new_post( 'publish' );
		},
		'action' => function ( $id ) {
			wp_update_post(
				array(
					'ID'         => $id,
					'post_title' => 'A renamed post',
				)
			);
		},
	),
	'trash-post'         => array(
		'setup'  => function () use ( $new_post ) {
			return $new_post( 'publish' );
		},
		'action' => function ( $id ) {
			wp_trash_post( $id );
		},
	),
	'untrash-post'       => array(
		'setup'  => function () use ( $new_post ) {
			$id = $new_post( 'publish' );
			wp_trash_post( $id );
			return $id;
		},
		'action' => function ( $id ) {
			wp_untrash_post( $id );
		},
	),
	'insert-category'    => array(
		'action' => function () {
			wp_insert_term( 'News', 'category', array( 'description' => 'What is new.' ) );
		},
	),
	'insert-tag-on-post' => array(
		'action' => function () {
			$term = wp_insert_term( 'Oracle', 'post_tag' );
			wp_set_post_terms( 1, array( $term['term_id'] ), 'post_tag' );
		},
	),
	'insert-comment'     => array(
		'action' => function () use ( $new_comment ) {
			$new_comment( 1 );
		},
	),
	'approve-comment'    => array(
		'setup'  => function () use ( $new_comment ) {
			return $new_comment( 0 );
		},
		'action' => function ( $id ) {
			wp_set_comment_status( $id, 'approve' );
		},
	),
	'update-option'      => array(
		'action' => function () {
			update_option( 'blogname', 'A renamed site' );
		},
	),
);

// Each area's actions live in their own file beside this one (actions-posts.php,
// actions-terms.php and so on), so they can be added side by side.
foreach ( glob( __DIR__ . '/actions-*.php' ) as $more ) {
	$actions += require $more;
}

return $actions;

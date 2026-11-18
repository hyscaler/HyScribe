<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The write oracle's media library (MVP 6.4): the Ajax saves and deletes of
// the grid and the media modal, upload.php's bulk actions and post.php's
// trash, restore and delete of an attachment, from wp-admin's own code over
// reference/attachment-edit/content.sql (golden.sh copies it beside these
// files as attachment-edit.sql), as user 1. An action named
// `*-media-trash` runs with MEDIA_TRASH, as run.php defines it. The files
// an attachment names do not exist, so none is deleted.

// The attachments, their posts, users and comment.
$media_content = function () {
	global $wpdb;
	$sql = preg_replace( '/^--.*$/m', '', file_get_contents( __DIR__ . '/attachment-edit.sql' ) );
	foreach ( array_filter( array_map( 'trim', explode( ";\n", $sql ) ) ) as $statement ) {
		$wpdb->query( rtrim( $statement, ';' ) );
	}
};

// admin-ajax.php's `$action` over `$fields`, as the media views send them.
$media_ajax = function ( $action, $fields ) use ( $admin_run ) {
	$_GET     = array();
	$_POST    = wp_slash( $fields + array( 'action' => $action ) );
	$_REQUEST = $_POST;
	$handler  = 'wp_ajax_' . str_replace( '-', '_', $action );
	$admin_run( fn() => $handler( '' ), true );
};

// save-attachment of the attachment `$id` with `$changes`.
$media_save = function ( $id, $changes ) use ( $media_content, $media_ajax ) {
	return array(
		'setup'  => $media_content,
		'action' => function () use ( $media_ajax, $id, $changes ) {
			$media_ajax(
				'save-attachment',
				array(
					'id'      => (string) $id,
					'changes' => $changes,
					'nonce'   => wp_create_nonce( "update-post_$id" ),
				)
			);
		},
	);
};

// save-attachment-compat of the attachment `$id` with its fields `$fields`.
$media_compat = function ( $id, $fields ) use ( $media_content, $media_ajax ) {
	return array(
		'setup'  => $media_content,
		'action' => function () use ( $media_ajax, $id, $fields ) {
			$media_ajax(
				'save-attachment-compat',
				array(
					'id'          => (string) $id,
					'attachments' => array( $id => $fields ),
					'nonce'       => wp_create_nonce( "update-post_$id" ),
				)
			);
		},
	);
};

// delete-post of the attachment `$id`, as the grid's Delete sends it.
$media_delete = function ( $id ) use ( $media_content, $media_ajax ) {
	return array(
		'setup'  => $media_content,
		'action' => function () use ( $media_ajax, $id ) {
			$media_ajax(
				'delete-post',
				array(
					'id'          => (string) $id,
					'_ajax_nonce' => wp_create_nonce( "delete-post_$id" ),
				)
			);
		},
	);
};

// upload.php's bulk action `$doaction` on `$ids`, with the request's `$more`.
$media_bulk = function ( $doaction, $ids, $more = array() ) use ( $media_content, $admin_code, $admin_run ) {
	return array(
		'setup'  => $media_content,
		'action' => function () use ( $admin_code, $admin_run, $doaction, $ids, $more ) {
			$_GET     = $more + array(
				'action'   => $doaction,
				'media'    => array_map( 'strval', $ids ),
				'_wpnonce' => wp_create_nonce( 'bulk-media' ),
			);
			$_POST    = array();
			$_REQUEST = $_GET;
			$code     = $admin_code( 'upload.php', "/\nif \( \\\$doaction \) \{\n(.*?)\n\} elseif/s" );
			$admin_run(
				function () use ( $code, $doaction ) {
					global $wpdb;
					eval( $code );
				}
			);
		},
	);
};

// post.php's case `$action` on the attachment `$id`, with post.php's globals.
$media_post = function ( $id, $action ) use ( $media_content, $admin_block, $admin_run ) {
	return array(
		'setup'  => $media_content,
		'action' => function () use ( $admin_block, $admin_run, $id, $action ) {
			$_GET     = array(
				'post'     => (string) $id,
				'action'   => $action,
				'_wpnonce' => wp_create_nonce( "$action-post_$id" ),
			);
			$_POST    = array();
			$_REQUEST = $_GET;
			$code     = $admin_block( 'post.php', "\n\tcase '$action':\n", "\n\tcase '" );
			$admin_run(
				function () use ( $id, $code ) {
					global $post_type, $post_type_object, $post;
					$post_id          = $id;
					$post             = get_post( $id );
					$post_type        = $post->post_type;
					$post_type_object = get_post_type_object( $post_type );
					$sendback         = admin_url( 'upload.php' );
					eval( $code );
				}
			);
		},
	);
};

// post.php's case editpost on the attachment `$id` over `$fields`, as its
// edit screen posts them (the nonce, the Update button and the ID are added).
$media_edit = function ( $id, $fields ) use ( $media_content, $admin_block, $admin_run ) {
	return array(
		'setup'  => $media_content,
		'action' => function () use ( $admin_block, $admin_run, $id, $fields ) {
			$_GET     = array();
			$_POST    = wp_slash(
				array_filter(
					$fields + array(
						'_wpnonce'  => wp_create_nonce( "update-post_$id" ),
						'action'    => 'editpost',
						'post_ID'   => (string) $id,
						'post_type' => 'attachment',
						'save'      => 'Update',
					),
					fn( $value ) => null !== $value
				)
			);
			$_REQUEST = $_POST;
			$code     = $admin_block( 'post.php', "\n\tcase 'editpost':\n", "\n\tcase '" );
			$admin_run(
				function () use ( $id, $code ) {
					global $post_type, $post_type_object, $post;
					$post_id          = $id;
					$post             = get_post( $id );
					$post_type        = $post->post_type;
					$post_type_object = get_post_type_object( $post_type );
					$sendback         = admin_url( 'upload.php' );
					eval( $code );
				}
			);
		},
	);
};

return array(
	'media-save-attachment'                    => $media_save(
		801,
		array(
			'title'       => 'New "title" &amp; more',
			'caption'     => 'A <em>caption</em>',
			'description' => 'Some <b>description</b> with a \\ backslash',
			'alt'         => '<b>Alt</b> text',
		)
	),
	'media-save-attachment-parent'             => $media_save(
		802,
		array(
			'parent'    => '2',
			'alt'       => 'Same',
			'menuOrder' => '3',
		)
	),
	'media-save-attachment-same-alt'           => $media_save( 801, array( 'alt' => 'Alt "text" &amp; more' ) ),
	'media-save-attachment-audio'              => $media_save(
		817,
		array(
			'artist' => 'New <b>artist</b>',
			'album'  => 'Second',
			'title'  => 'Song',
		)
	),
	'media-save-attachment-audio-unread'       => $media_save( 820, array( 'artist' => 'Someone' ) ),
	'media-save-attachment-not-audio'          => $media_save( 802, array( 'artist' => 'Nobody' ) ),
	'media-save-attachment-status'             => $media_save(
		801,
		array(
			'status' => 'trash',
			'title'  => 'Kept',
		)
	),
	'media-save-attachment-trash-media-trash'  => $media_save(
		801,
		array(
			'status' => 'trash',
			'title'  => 'Dropped',
			'alt'    => 'Changed',
		)
	),
	'media-save-attachment-inherit-media-trash' => $media_save( 810, array( 'status' => 'inherit' ) ),
	'media-save-attachment-private-media-trash' => $media_save( 802, array( 'status' => 'private' ) ),
	'media-save-attachment-compat'             => $media_compat( 801, array( 'a' => 'b' ) ),
	'media-save-attachment-compat-untitled'    => $media_compat( 805, array( 'a' => 'b' ) ),
	'media-delete-post'                        => $media_delete( 801 ),
	'media-delete-post-song'                   => $media_delete( 817 ),
	'media-delete-post-media-trash'            => $media_delete( 801 ),
	'media-delete-post-trashed-media-trash'    => $media_delete( 810 ),
	'media-bulk-delete'                        => $media_bulk( 'delete', array( 801, 802 ) ),
	'media-bulk-delete-media-trash'            => $media_bulk( 'delete', array( 801, 810 ) ),
	'media-bulk-trash-media-trash'             => $media_bulk( 'trash', array( 801, 802 ) ),
	'media-bulk-untrash-media-trash'           => $media_bulk( 'untrash', array( 810 ) ),
	'media-bulk-attach'                        => $media_bulk( 'attach', array( 802, 803, 99999 ), array( 'found_post_id' => '2' ) ),
	'media-bulk-attach-same-parent'            => $media_bulk( 'attach', array( 801, 803 ), array( 'found_post_id' => '1' ) ),
	'media-bulk-detach'                        => $media_bulk( 'detach', array( 801, 802, 807 ), array( 'parent_post_id' => '1' ) ),
	'media-edit-image'                         => $media_edit(
		801,
		array(
			'post_author'              => '2',
			'post_author_override'     => '1',
			'post_title'               => 'New "title" \\ & more',
			'excerpt'                  => 'A <em>caption</em>',
			'content'                  => 'Some <b>description</b> with a \\ backslash',
			'_wp_attachment_image_alt' => '<b>Alt</b> text',
			'comment_status'           => 'open',
			'post_name'                => 'new-slug',
			'advanced_view'            => '1',
		)
	),
	'media-edit-same-alt'                      => $media_edit(
		801,
		array(
			'post_author'              => '2',
			'post_title'               => 'Sunset "dusk" &amp; sea',
			'_wp_attachment_image_alt' => 'Alt "text" &amp; more',
			'post_name'                => 'sunset-dusk-sea',
		)
	),
	'media-edit-pdf'                           => $media_edit(
		802,
		array(
			'post_author'    => '1',
			'post_title'     => 'Report 2026',
			'comment_status' => 'open',
			'ping_status'    => 'open',
			'post_name'      => 'report',
		)
	),
	'media-edit-gone-author'                   => $media_edit( 805, array( 'post_author' => '99' ) ),
	'media-edit-private-parent'                => $media_edit(
		803,
		array(
			'post_author' => '3',
			'post_title'  => 'Notes',
		)
	),
	'media-edit-pending-parent'                => $media_edit( 808, array( 'post_author' => '1' ) ),
	'media-edit-trashed-parent'                => $media_edit( 813, array( 'post_author' => '1' ) ),
	'media-edit-private-attachment'            => $media_edit( 811, array( 'post_author' => '1' ) ),
	'media-edit-trashed'                       => $media_edit( 810, array( 'post_author' => '1' ) ),
	'media-edit-song'                          => $media_edit(
		817,
		array(
			'post_author'    => '2',
			'post_title'     => 'Song',
			'id3_artist'     => 'New <b>artist</b>',
			'id3_album'      => 'Second',
			'id3_genre'      => 'Ignored',
			'_thumbnail_id'  => '-1',
			'comment_status' => 'open',
		)
	),
	'media-edit-song-untouched'                => $media_edit(
		817,
		array(
			'post_author'   => '2',
			'_thumbnail_id' => '801',
		)
	),
	'media-edit-video'                         => $media_edit(
		818,
		array(
			'post_author'   => '1',
			'id3_artist'    => 'Director',
			'_thumbnail_id' => '-1',
		)
	),
	'media-edit-no-save-button'                => $media_edit(
		802,
		array(
			'post_author' => '1',
			'save'        => null,
		)
	),
	'media-post-delete'                        => $media_post( 801, 'delete' ),
	'media-post-delete-media-trash'            => $media_post( 810, 'delete' ),
	'media-post-trash-media-trash'             => $media_post( 801, 'trash' ),
	'media-post-untrash-media-trash'           => $media_post( 810, 'untrash' ),
);

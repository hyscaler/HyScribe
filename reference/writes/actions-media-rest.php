<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The write oracle's media writes over REST: the attachments controller's
// update_item() and delete_item(), each a JSON request dispatched as user 1
// through rest_do_request(), on an attachment inserted as the tests insert
// it (no file on disk, so a delete leaves the files alone: what it deletes
// is tested on files of its own). The attachment is the first image of
// 2026/11, a child of post 1 with an alt text, a caption, a description,
// two sizes and a backup of the original, with the ID 9301 (a slug taken by
// another has 9300), as the tests give theirs: an ID is the value of a
// featured image's meta, which the oracle's mask leaves alone.

$media_attachment = function ( $args = array(), $parent = 1 ) {
	$id = wp_insert_attachment(
		$args + array(
			'import_id'      => 9301,
			'post_title'     => 'A write oracle picture',
			'post_content'   => 'Its description',
			'post_excerpt'   => 'Its caption',
			'post_mime_type' => 'image/jpeg',
			'post_author'    => 1,
		),
		'2026/11/oracle.jpg',
		$parent
	);
	$size = fn( $name, $width, $height, $bytes ) => array(
		'file'      => "oracle-{$width}x{$height}.jpg",
		'width'     => $width,
		'height'    => $height,
		'mime-type' => 'image/jpeg',
		'filesize'  => $bytes,
	);
	wp_update_attachment_metadata(
		$id,
		array(
			'width'      => 1200,
			'height'     => 800,
			'file'       => '2026/11/oracle.jpg',
			'filesize'   => 345678,
			'sizes'      => array(
				'medium'    => $size( 'medium', 300, 200, 12345 ),
				'thumbnail' => $size( 'thumbnail', 150, 150, 5432 ),
			),
			'image_meta' => array(
				'aperture'          => '0',
				'credit'            => '',
				'camera'            => '',
				'caption'           => '',
				'created_timestamp' => '0',
				'copyright'         => '',
				'focal_length'      => '0',
				'iso'               => '0',
				'shutter_speed'     => '0',
				'title'             => '',
				'orientation'       => '0',
				'keywords'          => array(),
			),
		)
	);
	update_post_meta( $id, '_wp_attachment_image_alt', 'Old alt' );
	update_post_meta(
		$id,
		'_wp_attachment_backup_sizes',
		array(
			'full-orig' => array(
				'width'  => 1200,
				'height' => 800,
				'file'   => 'oracle-orig.jpg',
			),
		)
	);
	return $id;
};

// What a delete takes with the attachment: a comment on it, and the image
// as post 1's featured image.
$media_used = function () use ( $media_attachment ) {
	global $wpdb;
	$id = $media_attachment();
	$wpdb->insert(
		$wpdb->comments,
		array(
			'comment_post_ID'  => $id,
			'comment_author'   => 'A reader',
			'comment_date'     => '2026-01-01 00:00:00',
			'comment_date_gmt' => '2026-01-01 00:00:00',
			'comment_content'  => 'A comment.',
			'comment_approved' => '1',
		)
	);
	add_post_meta( 1, '_thumbnail_id', $id );
	return $id;
};

$media_send = function ( $method, $route, $body = array(), $query = array() ) {
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
	'rest-media-update'            => array(
		'setup'  => $media_attachment,
		'action' => function ( $id ) use ( $media_send ) {
			$media_send(
				'POST',
				"/wp/v2/media/$id",
				array(
					'title'          => '<b>New</b> <script>x()</script>title',
					'caption'        => 'A <em>caption</em><iframe src="https://example.test/"></iframe>',
					'description'    => array( 'raw' => '<p onclick="y()">Described</p>' ),
					'alt_text'       => "  A <b>new</b>\talt  ",
					'post'           => 2,
					'slug'           => 'renamed picture',
					'comment_status' => 'closed',
					'ping_status'    => 'closed',
					'template'       => '',
				)
			);
		},
	),
	'rest-media-update-private'    => array(
		'setup'  => $media_attachment,
		'action' => function ( $id ) use ( $media_send ) {
			$media_send(
				'PATCH',
				"/wp/v2/media/$id",
				array(
					'status' => 'private',
					'post'   => 0,
					'date'   => '2026-01-02T03:04:05',
				)
			);
		},
	),
	'rest-media-update-alt'        => array(
		'setup'  => $media_attachment,
		'action' => function ( $id ) use ( $media_send ) {
			$media_send( 'PUT', "/wp/v2/media/$id", array( 'alt_text' => '' ) );
		},
	),
	'rest-media-update-slug'       => array(
		'setup'  => function () use ( $media_attachment ) {
			$media_attachment(
				array(
					'import_id' => 9300,
					'post_name' => 'taken-slug',
				)
			);
			return $media_attachment();
		},
		'action' => function ( $id ) use ( $media_send ) {
			$media_send( 'POST', "/wp/v2/media/$id", array( 'slug' => 'taken-slug' ) );
		},
	),
	'rest-media-update-draft'      => array(
		'setup'  => $media_attachment,
		'action' => function ( $id ) use ( $media_send ) {
			$media_send(
				'POST',
				"/wp/v2/media/$id",
				array(
					'status' => 'draft',
					'slug'   => 'a-draft-slug',
					'author' => 1,
				)
			);
		},
	),
	'rest-media-update-trashed'    => array(
		'setup'  => function () use ( $media_attachment ) {
			global $wpdb;
			$id = $media_attachment();
			$wpdb->update( $wpdb->posts, array( 'post_status' => 'trash' ), array( 'ID' => $id ) );
			clean_post_cache( $id );
			return $id;
		},
		'action' => function ( $id ) use ( $media_send ) {
			$media_send( 'POST', "/wp/v2/media/$id", array( 'title' => 'Renamed in the trash' ) );
		},
	),
	'rest-media-delete'            => array(
		'setup'  => $media_used,
		'action' => function ( $id ) use ( $media_send ) {
			$media_send( 'DELETE', "/wp/v2/media/$id", array(), array( 'force' => 'true' ) );
		},
	),
	'rest-media-delete-trashed'    => array(
		'setup'  => function () use ( $media_used ) {
			global $wpdb;
			$id = $media_used();
			add_post_meta( $id, '_wp_trash_meta_status', 'inherit' );
			add_post_meta( $id, '_wp_trash_meta_time', time() );
			$wpdb->update( $wpdb->posts, array( 'post_status' => 'trash' ), array( 'ID' => $id ) );
			clean_post_cache( $id );
			return $id;
		},
		'action' => function ( $id ) use ( $media_send ) {
			$media_send( 'DELETE', "/wp/v2/media/$id", array(), array( 'force' => '1' ) );
		},
	),
	'rest-media-delete-unattached' => array(
		'setup'  => function () use ( $media_attachment ) {
			return $media_attachment( array(), 0 );
		},
		'action' => function ( $id ) use ( $media_send ) {
			$media_send( 'DELETE', "/wp/v2/media/$id", array(), array( 'force' => 'true' ) );
		},
	),
);

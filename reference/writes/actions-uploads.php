<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The write oracle's uploads (MVP 6.2): media_handle_upload() as
// async-upload.php and media-new.php call it, and the attachments
// controller's create_item() over a raw body and over a multipart form,
// each on files of the image corpus (golden.sh copies it beside these
// files), as user 1. The files written under the uploads folder are
// deleted after, as the rows are rolled back. The upload itself is copied,
// not moved from PHP's own upload: the rows are the same.

$uploads_corpus = __DIR__ . '/corpus/';

// Every file under the uploads folder.
$uploads_list = function () {
	$dir = wp_upload_dir()['basedir'];
	wp_mkdir_p( $dir );
	$all = array();
	foreach ( new RecursiveIteratorIterator( new RecursiveDirectoryIterator( $dir, FilesystemIterator::SKIP_DOTS ) ) as $file ) {
		$all[] = $file->getPathname();
	}
	return $all;
};

// $_FILES' entry for the corpus file $source sent as $name.
$uploads_file = function ( $source, $name ) use ( $uploads_corpus ) {
	$tmp = wp_tempnam( $name );
	copy( $uploads_corpus . $source, $tmp );
	return array(
		'name'     => $name,
		'type'     => wp_check_filetype( $name )['type'],
		'tmp_name' => $tmp,
		'error'    => 0,
		'size'     => filesize( $tmp ),
	);
};

// An action that runs $upload, which gives an error or nothing, and
// deletes every file the case wrote; $setup runs first, as the case's.
$uploads_case = function ( $upload, $setup = null ) use ( $uploads_list ) {
	return array(
		'setup'  => function () use ( $uploads_list, $setup ) {
			$kept = $uploads_list();
			if ( $setup ) {
				$setup();
			}
			return $kept;
		},
		'action' => function ( $kept ) use ( $uploads_list, $upload ) {
			$error = $upload();
			foreach ( array_diff( $uploads_list(), $kept ) as $made ) {
				unlink( $made );
			}
			if ( $error ) {
				fwrite( STDERR, $error . "\n" );
				exit( 1 );
			}
		},
	);
};

// media_handle_upload( 'async-upload', $post_id ) of $source sent as $name.
$uploads_handle = function ( $source, $name, $post_id = 0 ) use ( $uploads_file ) {
	return function () use ( $uploads_file, $source, $name, $post_id ) {
		$_FILES['async-upload'] = $uploads_file( $source, $name );
		$id                     = media_handle_upload(
			'async-upload',
			$post_id,
			array(),
			array(
				'test_form' => false,
				'action'    => 'wp_handle_sideload',
			)
		);
		return is_wp_error( $id ) ? $id->get_error_message() : null;
	};
};

// The attachments controller's create_item(): the file $source sent as
// $name in a raw body, or as a multipart form's file with $form.
$uploads_rest = function ( $source, $name, $params, $form ) use ( $uploads_corpus, $uploads_file ) {
	return function () use ( $uploads_corpus, $uploads_file, $source, $name, $params, $form ) {
		$request = new WP_REST_Request( 'POST', '/wp/v2/media' );
		if ( $form ) {
			// As tests/phpunit uploads: copied rather than moved from PHP's own upload.
			define( 'DIR_TESTDATA', __DIR__ );
			$request->set_file_params( array( 'file' => $uploads_file( $source, $name ) ) );
			$request->set_body_params( $params );
		} else {
			$request->set_header( 'Content-Type', wp_check_filetype( $name )['type'] );
			$request->set_header( 'Content-Disposition', "attachment; filename=$name" );
			$request->set_body( file_get_contents( $uploads_corpus . $source ) );
			$request->set_query_params( $params );
		}
		$response = rest_do_request( $request );
		return $response->is_error() ? wp_json_encode( $response->get_data() ) : null;
	};
};

return array(
	'upload-async'          => $uploads_case( $uploads_handle( 'photo.jpg', 'photo.jpg' ) ),
	'upload-async-parent'   => $uploads_case( $uploads_handle( 'small.jpg', 'small.jpg', 1 ) ),
	'upload-async-page'     => $uploads_case( $uploads_handle( 'small.jpg', 'small.jpg', 2 ) ),
	'upload-async-name'     => $uploads_case( $uploads_handle( 'rgba.png', 'My Picture (1).PNG' ) ),
	'upload-async-again'    => $uploads_case( $uploads_handle( 'small.jpg', 'small.jpg' ), $uploads_handle( 'small.jpg', 'small.jpg' ) ),
	'upload-async-scaled'   => $uploads_case( $uploads_handle( 'big.jpg', 'big.jpg' ) ),
	'upload-async-rotated'  => $uploads_case( $uploads_handle( 'orient-6.jpg', 'orient-6.jpg' ) ),
	'upload-async-iptc'     => $uploads_case( $uploads_handle( 'iptc.jpg', 'iptc.jpg' ) ),
	'upload-async-gif'      => $uploads_case( $uploads_handle( 'anim.gif', 'anim.gif' ) ),
	'upload-async-webp'     => $uploads_case( $uploads_handle( 'lossy.webp', 'lossy.webp' ) ),
	'upload-async-avif'     => $uploads_case( $uploads_handle( 'photo.avif', 'photo.avif' ) ),
	'upload-rest-raw'       => $uploads_case( $uploads_rest( 'photo.jpg', 'photo.jpg', array(), false ) ),
	'upload-rest-raw-given' => $uploads_case(
		$uploads_rest(
			'small.jpg',
			'small.jpg',
			array(
				'title'       => 'Raw title',
				'alt_text'    => 'An alt',
				'caption'     => 'A caption',
				'description' => 'Described',
				'post'        => 1,
			),
			false
		)
	),
	'upload-rest-form'      => $uploads_case(
		$uploads_rest(
			'rgba.png',
			'rgba.png',
			array(
				'title'    => 'A form upload',
				'alt_text' => 'Alt',
				'post'     => 1,
			),
			true
		)
	),
	'upload-rest-form-name' => $uploads_case( $uploads_rest( 'small.jpg', 'small.jpg', array(), true ) ),
);

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

// The must-use plugin that leaves a PDF without an image editor.
$uploads_no_preview = function () {
	require __DIR__ . '/no-pdf-preview.php';
};

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
	'upload-async-scaled-rotated' => $uploads_case( $uploads_handle( 'big-orient-6.jpg', 'big-orient-6.jpg' ) ),
	'upload-async-scaled-png' => $uploads_case( $uploads_handle( 'big.png', 'big.png' ) ),
	'upload-async-edge'     => $uploads_case( $uploads_handle( 'edge-2560.jpg', 'edge-2560.jpg' ) ),
	'upload-async-iptc'     => $uploads_case( $uploads_handle( 'iptc.jpg', 'iptc.jpg' ) ),
	'upload-async-gif'      => $uploads_case( $uploads_handle( 'anim.gif', 'anim.gif' ) ),
	'upload-async-webp'     => $uploads_case( $uploads_handle( 'lossy.webp', 'lossy.webp' ) ),
	'upload-async-avif'     => $uploads_case( $uploads_handle( 'photo.avif', 'photo.avif' ) ),
	'upload-rest-raw'       => $uploads_case( $uploads_rest( 'photo.jpg', 'photo.jpg', array(), false ) ),
	'upload-rest-scaled'    => $uploads_case( $uploads_rest( 'big.jpg', 'big.jpg', array(), false ) ),
	'upload-rest-rotated'   => $uploads_case( $uploads_rest( 'orient-6.jpg', 'orient-6.jpg', array(), false ) ),
	'upload-rest-scaled-rotated' => $uploads_case( $uploads_rest( 'big-orient-3.jpg', 'big-orient-3.jpg', array(), false ) ),
	'upload-rest-form-scaled' => $uploads_case( $uploads_rest( 'big-rgba.png', 'big-rgba.png', array( 'title' => 'Big form' ), true ) ),
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
	// Files that are not images: their rows hold the file size alone.
	'upload-async-text'     => $uploads_case( $uploads_handle( 'plain.txt', 'plain.txt' ) ),
	'upload-async-csv'      => $uploads_case( $uploads_handle( 'words.csv', 'words.csv', 1 ) ),
	'upload-async-vtt'      => $uploads_case( $uploads_handle( 'captions.vtt', 'captions.vtt' ) ),
	'upload-async-docx'     => $uploads_case( $uploads_handle( 'sample.docx', 'sample.docx' ) ),
	'upload-async-odt'      => $uploads_case( $uploads_handle( 'sample.odt', 'My Notes (1).odt' ) ),
	'upload-async-zip'      => $uploads_case( $uploads_handle( 'sample.zip', 'sample.docx' ) ),
	'upload-async-rtf'      => $uploads_case( $uploads_handle( 'sample.rtf', 'sample.rtf' ) ),
	// Word, Excel and PowerPoint 97 to 2003 files: Compound File Binary
	// files, typed by libmagic from their summary information.
	'upload-async-doc'      => $uploads_case( $uploads_handle( 'sample.doc', 'sample.doc' ) ),
	'upload-async-xls'      => $uploads_case( $uploads_handle( 'sample.xls', 'My Sheet.xls', 1 ) ),
	'upload-async-ppt'      => $uploads_case( $uploads_handle( 'sample.ppt', 'sample.ppt' ) ),
	'upload-async-ppt-pps'  => $uploads_case( $uploads_handle( 'sample.ppt', 'slides.pps' ) ),
	'upload-async-xls-xlt'  => $uploads_case( $uploads_handle( 'sample.xls', 'sheet.xlt' ) ),
	'upload-rest-raw-doc'   => $uploads_case( $uploads_rest( 'sample.doc', 'sample.doc', array( 'title' => 'A Word file', 'post' => 1 ), false ) ),
	'upload-rest-form-xls'  => $uploads_case( $uploads_rest( 'sample.xls', 'sample.xls', array(), true ) ),
	// A PDF with no image editor, as on a host without Imagick: no preview.
	'upload-async-pdf'      => $uploads_case( $uploads_handle( 'sample.pdf', 'sample.pdf' ), $uploads_no_preview ),
	'upload-rest-raw-pdf'   => $uploads_case( $uploads_rest( 'sample.pdf', 'sample.pdf', array( 'post' => 1 ), false ), $uploads_no_preview ),
	'upload-async-text-again' => $uploads_case( $uploads_handle( 'plain.txt', 'plain.txt' ), $uploads_handle( 'plain.txt', 'plain.txt' ) ),
	'upload-rest-raw-text'  => $uploads_case(
		$uploads_rest(
			'plain.txt',
			'plain.txt',
			array(
				'title'       => 'Notes',
				'alt_text'    => 'An alt',
				'caption'     => 'A caption',
				'description' => 'Described',
				'post'        => 1,
			),
			false
		)
	),
	'upload-rest-form-docx' => $uploads_case( $uploads_rest( 'sample.docx', 'sample.docx', array( 'title' => 'A form document' ), true ) ),
	'upload-rest-raw-xlsx'  => $uploads_case( $uploads_rest( 'sample.xlsx', 'sample.xlsx', array(), false ) ),
	// Audio and video getID3 reads: a WAVE file's metadata, and the size and
	// type alone of a format WordPress's getID3 has no module for.
	'upload-async-wav'      => $uploads_case( $uploads_handle( 'tone.wav', 'tone.wav' ) ),
	'upload-async-aac'      => $uploads_case( $uploads_handle( 'tone.aac', 'tone.aac', 1 ) ),
	'upload-rest-raw-wav'   => $uploads_case( $uploads_rest( 'tone.wav', 'tone.wav', array( 'title' => 'A tone', 'post' => 1 ), false ) ),
	'upload-rest-form-aac'  => $uploads_case( $uploads_rest( 'tone.aac', 'tone.aac', array(), true ) ),
);

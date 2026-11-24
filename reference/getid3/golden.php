<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Run by golden.sh inside the PHP container: for each file of the folder
// $argv[1], name<TAB>hex( serialize( the metadata WordPress makes of it ) ),
// as wp_generate_attachment_metadata() makes it: wp_read_video_metadata()
// for a type of video, else wp_read_audio_metadata(), by the file's name.
// Reads the files only; nothing is written to the database.

define( 'WP_USE_THEMES', false );
require '/var/www/html/wp-load.php';
require_once ABSPATH . 'wp-admin/includes/image.php';
require_once ABSPATH . 'wp-admin/includes/media.php';

$names = array_diff( scandir( $argv[1] ), array( '.', '..' ) );
foreach ( $names as $name ) {
	$file = $argv[1] . '/' . $name;
	$type = wp_check_filetype( preg_replace( '/^(corpus|sample|isom)-/', '', $name ) )['type'];
	// A fatal error of getID3 on a corrupt file is recorded as false.
	try {
		$meta = str_starts_with( (string) $type, 'video/' ) ? wp_read_video_metadata( $file ) : wp_read_audio_metadata( $file );
	} catch ( Throwable $e ) {
		$meta = false;
	}
	echo $name, "\t", bin2hex( serialize( $meta ) ), "\n";
}

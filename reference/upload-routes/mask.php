<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Masks an answer of reference/upload-routes/golden.sh, as the Rust test
// masks HyScribe's (crates/hyscribe-http/src/uploads/tests.rs, `masked`):
//   php mask.php <body file> <start> <end> <rest nonce> <media-form nonce>
list( , $file, $start, $end, $rest, $form ) = $argv;
$start = (int) $start;
$end   = (int) $end;
$body  = file_get_contents( $file );
$body  = str_replace( array( $rest, $form ), '%nonce%', $body );
$body  = preg_replace( '/"(update|delete|edit)":"[0-9a-f]{10}"/', '"$1":"%nonce%"', $body );
$body  = preg_replace( '/(dismiss|error-description)-[0-9a-f]{13}/', '$1-%uniqid%', $body );
$body  = preg_replace_callback(
	'/\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d/',
	fn( $m ) => strtotime( $m[0] . 'Z' ) >= $start && strtotime( $m[0] . 'Z' ) <= $end ? '%now%' : $m[0],
	$body
);
$body = preg_replace_callback(
	'/\b\d{13}\b/',
	fn( $m ) => (int) $m[0] >= $start * 1000 && (int) $m[0] <= $end * 1000 + 999 ? '%nowms%' : $m[0],
	$body
);
$body = str_replace( gmdate( 'F j, Y', $start ), '%today%', $body );
$body = str_replace( gmdate( 'Y', $start ) . '\/' . gmdate( 'm', $start ) . '\/', '%ym%\/', $body );
$body = str_replace( gmdate( 'Y/m/', $start ), '%ym%/', $body );
$body = preg_replace( '/"(filesize|filesizeInBytes)":\d+/', '"$1":%filesize%', $body );
$body = preg_replace( '/"filesizeHumanReadable":"[^"]*"/', '"filesizeHumanReadable":"%filesize%"', $body );
echo $body;

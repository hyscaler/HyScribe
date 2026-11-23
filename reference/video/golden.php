<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints do_blocks() for video block markup naming attachments of the
// Theme Test Data fixture, whose stored sizes the block reads. Lines hold
// a hash, as for every golden over the fixture:
//   hex(markup)<TAB>sha256(html)
// With RAW=1 the html itself is printed instead. Nothing is written.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

$GLOBALS['wp'] = new WP();
wp( '' );

$raw    = getenv( 'RAW' );
$videos = array(
	'<figure class="wp-block-video"><video controls src="v.mp4"></video></figure>',
	'<figure class="wp-block-video"><video controls src="v.mp4" style="color:red"></video></figure>',
	'<figure class="wp-block-video"><video style></video></figure>',
	'<figure class="wp-block-video"><VIDEO src="v.mp4"></VIDEO></figure>',
	'<figure class="wp-block-video"><p>no video</p><!-- <video --></figure>',
);
$ids    = array( '611', '1', '999999', '"611"', '0', '-611', '611.0', 'null' );
foreach ( $videos as $video ) {
	foreach ( $ids as $id ) {
		$markup = '<!-- wp:video {"id":' . $id . '} -->' . $video . '<!-- /wp:video -->';
		$html   = do_blocks( $markup );
		echo bin2hex( $markup ), "\t", $raw ? $html : hash( 'sha256', $html ), "\n";
	}
}

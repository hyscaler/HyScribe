<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints wp_filter_content_tags( $content, 'the_content' ) for one post of
// the Theme Test Data fixture: POST is its ID, and LOOP=1 runs it as the
// main query's post in the loop, where the first images are not lazy. A
// fresh PHP runs each case, as the media count lasts the request. The
// fixture's content is not ours to commit, so the line holds a hash:
//   post ID or hex(content)<TAB>loop<TAB>sha256(html)
// With RAW=1 the html itself is printed instead. Nothing is written.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

$id   = (int) getenv( 'POST' );
$loop = getenv( 'LOOP' ) ? 1 : 0;
$GLOBALS['wp'] = new WP();
wp( 'p=' . $id . '&post_type=any' );
if ( $loop ) {
	the_post();
}
// CONTENT (hex) filters that instead of the post's content, keyed by itself.
$given   = getenv( 'CONTENT' );
$content = $given ? hex2bin( $given ) : get_post( $id )->post_content;
$html    = wp_filter_content_tags( $content, 'the_content' );
echo $given ? $given : $id, "\t", $loop, "\t", getenv( 'RAW' ) ? $html : hash( 'sha256', $html ), "\n";

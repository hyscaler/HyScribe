<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints wp_get_attachment_image() for every attachment of the Theme Test
// Data fixture, outside the loop, in each size and with a few attribute
// sets. The fixture's content is not ours to commit, so lines hold a hash:
//   id<TAB>size<TAB>hex(attributes JSON)<TAB>sha256(html)
// With RAW=1 the html itself is printed instead, for comparing by eye.
// Nothing is written.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

$GLOBALS['wp'] = new WP();
wp( '' );

$raw   = getenv( 'RAW' );
$ids   = get_posts( array( 'post_type' => 'attachment', 'post_status' => 'any', 'numberposts' => -1, 'orderby' => 'ID', 'order' => 'ASC', 'fields' => 'ids' ) );
$ids[] = 1;
$ids[] = 999999;
$sizes = array( 'thumbnail', 'medium', 'medium_large', 'large', 'full', '1536x1536', '2048x2048', 'post-thumbnail', 'bogus' );
$attrs = array(
	array(),
	array( 'alt' => 'A "quoted" <b>alt</b>', 'class' => 'extra', 'style' => 'width:100%' ),
	array( 'srcset' => 'given.jpg 100w', 'sizes' => '50vw', 'data-x' => '1' ),
);

foreach ( $ids as $id ) {
	foreach ( $sizes as $size ) {
		foreach ( $attrs as $attr ) {
			$html = wp_get_attachment_image( $id, $size, false, $attr );
			echo $id, "\t", $size, "\t", bin2hex( wp_json_encode( (object) $attr ) ), "\t", $raw ? $html : hash( 'sha256', $html ), "\n";
		}
	}
}

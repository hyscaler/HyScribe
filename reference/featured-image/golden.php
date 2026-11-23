<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints the featured image block for posts of the Theme Test Data fixture,
// rendered with each post as its postId context, outside the loop. The
// fixture's content is not ours to commit, so lines hold a hash:
//   post ID<TAB>hex(markup)<TAB>sha256(html)
// With RAW=1 the html itself is printed instead. Nothing is written.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

$GLOBALS['wp'] = new WP();
wp( '' );

$raw     = getenv( 'RAW' );
$ids     = get_posts( array( 'post_type' => 'any', 'post_status' => 'any', 'numberposts' => -1, 'orderby' => 'ID', 'order' => 'ASC', 'fields' => 'ids', 'meta_key' => '_thumbnail_id' ) );
$ids     = array_merge( $ids, array( 21, 1788, 1, 999999 ) );
$markups = array(
	'<!-- wp:post-featured-image /-->',
	'<!-- wp:post-featured-image {"isLink":true} /-->',
	'<!-- wp:post-featured-image {"isLink":true,"rel":"nofollow","linkTarget":"_blank","sizeSlug":"medium"} /-->',
	'<!-- wp:post-featured-image {"aspectRatio":"3/2"} /-->',
	'<!-- wp:post-featured-image {"aspectRatio":"auto","width":"200px","height":"","scale":"contain"} /-->',
	'<!-- wp:post-featured-image {"height":"300px","scale":""} /-->',
	'<!-- wp:post-featured-image {"width":"50%"} /-->',
	'<!-- wp:post-featured-image {"dimRatio":50,"overlayColor":"contrast","gradient":"g1","customGradient":"linear-gradient(red,blue)","customOverlayColor":"#f00","style":{"border":{"radius":"8px","width":"2px","color":"#000"}}} /-->',
	'<!-- wp:post-featured-image {"borderColor":"accent-1","style":{"border":{"top":{"width":"1px","style":"solid"}},"shadow":"var:preset|shadow|natural"}} /-->',
	'<!-- wp:post-featured-image {"useFirstImageFromPost":true,"aspectRatio":"1"} /-->',
	'<!-- wp:post-featured-image {"useFirstImageFromPost":true} /-->',
	'<!-- wp:post-featured-image {"sizeSlug":"thumbnail","className":"x","align":"wide"} /-->',
	'<!-- wp:post-featured-image {"isLink":true,"dimRatio":0} /-->',
);

foreach ( $ids as $id ) {
	foreach ( $markups as $markup ) {
		$parsed = parse_blocks( $markup )[0];
		$html   = ( new WP_Block( $parsed, array( 'postId' => $id, 'postType' => get_post_type( $id ) ) ) )->render();
		echo $id, "\t", bin2hex( $markup ), "\t", $raw ? $html : hash( 'sha256', $html ), "\n";
	}
}

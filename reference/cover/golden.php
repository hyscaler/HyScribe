<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints the cover block over posts of the Theme Test Data fixture, each
// the global post as a post template sets it, outside the loop. The
// featured images are the fixture's, so lines hold a hash:
//   post ID<TAB>hex(markup)<TAB>sha256(html)
// With RAW=1 the html itself is printed instead. Nothing is written.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

$GLOBALS['wp'] = new WP();
wp( '' );

$raw   = getenv( 'RAW' );
$ids   = get_posts( array( 'post_type' => 'any', 'post_status' => 'any', 'numberposts' => -1, 'orderby' => 'ID', 'order' => 'ASC', 'fields' => 'ids', 'meta_key' => '_thumbnail_id' ) );
$ids   = array_merge( $ids, array( 21, 999999 ) );
$inner = '<span aria-hidden="true" class="wp-block-cover__background has-background-dim"></span><div class="wp-block-cover__inner-container"><!-- wp:paragraph --><p>Text</p><!-- /wp:paragraph --></div>';
$cases = array(
	array( '{"useFeaturedImage":true}', $inner ),
	array( '{"useFeaturedImage":true,"focalPoint":{"x":0.25,"y":0.333}}', $inner ),
	array( '{"useFeaturedImage":true,"hasParallax":true}', $inner ),
	array( '{"useFeaturedImage":true,"isRepeated":true,"focalPoint":{"x":0.5,"y":1}}', $inner ),
	array( '{"useFeaturedImage":true,"hasParallax":true,"isRepeated":true,"sizeSlug":"thumbnail"}', $inner ),
	array( '{"useFeaturedImage":true,"sizeSlug":"medium"}', $inner ),
	array( '{}', $inner ),
	array( '{"useFeaturedImage":0}', $inner ),
	array( '{"useFeaturedImage":true,"backgroundType":"video"}', $inner ),
	array( '{"useFeaturedImage":true,"backgroundType":"embed-video","url":"https://example.test/v"}', $inner ),
	array( '{"useFeaturedImage":true}', '<div  class="x wp-block-cover__inner-container"><p>a</p></div>' ),
	array( '{"useFeaturedImage":true}', '<div class=\'wp-block-cover__inner-container\'><p>a</p></div>' ),
	array( '{"useFeaturedImage":true}', '<divx class="wp-block-cover__inner-container"></divx><div class="wp-block-cover__inner-container-not"></div>' ),
	array( '{"useFeaturedImage":true}', '<p>no container</p>' ),
);

foreach ( $ids as $id ) {
	global $post;
	$post = get_post( $id );
	if ( $post ) {
		setup_postdata( $post );
	}
	foreach ( $cases as list( $attrs, $content ) ) {
		$markup = '<!-- wp:cover ' . $attrs . ' --><div class="wp-block-cover">' . $content . '</div><!-- /wp:cover -->';
		$html   = do_blocks( $markup );
		echo $id, "\t", bin2hex( $markup ), "\t", $raw ? $html : hash( 'sha256', $html ), "\n";
	}
}

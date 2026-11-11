<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints the media and text block over posts of the Theme Test Data
// fixture, each the global post. The featured images are the fixture's,
// so lines hold a hash:
//   counter<TAB>post ID<TAB>hex(markup)<TAB>sha256(html)
// where counter is the last number wp_unique_id() gave before the case.
// With RAW=1 the html itself is printed instead. Nothing is written.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

$GLOBALS['wp'] = new WP();
wp( '' );

$raw     = getenv( 'RAW' );
$ids     = get_posts( array( 'post_type' => 'any', 'post_status' => 'any', 'numberposts' => -1, 'orderby' => 'ID', 'order' => 'ASC', 'fields' => 'ids', 'meta_key' => '_thumbnail_id' ) );
$ids     = array_merge( $ids, array( 21, 999999 ) );
$figure  = '<figure class="wp-block-media-text__media"></figure>';
$text    = '<div class="wp-block-media-text__content"><!-- wp:paragraph --><p>T</p><!-- /wp:paragraph --></div>';
$wrap    = function ( $class, $inner ) {
	return '<div class="wp-block-media-text ' . $class . '">' . $inner . '</div>';
};
$cases   = array(
	array( '{"useFeaturedImage":true}', $wrap( 'is-stacked-on-mobile', $figure . $text ) ),
	array( '{"useFeaturedImage":true,"mediaPosition":"right"}', $wrap( 'has-media-on-the-right', $text . $figure ) ),
	array( '{"useFeaturedImage":true,"mediaPosition":"right"}', $wrap( 'x', $figure . $text . $figure ) ),
	array( '{"useFeaturedImage":true,"imageFill":true,"focalPoint":{"x":0.2,"y":0.75}}', $wrap( 'is-image-fill', $figure . $text ) ),
	array( '{"useFeaturedImage":true,"imageFill":true}', $wrap( 'is-image-fill', $figure . $text ) . $wrap( 'is-image-fill', '' ) ),
	array( '{"useFeaturedImage":true,"mediaSizeSlug":"medium"}', $wrap( '', $figure . $text ) ),
	array( '{}', $wrap( '', $figure . $text ) ),
	array( '{"useFeaturedImage":0}', $wrap( '', $figure . $text ) ),
	array( '{"useFeaturedImage":true}', $wrap( '', '<figure class="wp-block-media-text__media extra"></figure>' . $text ) ),
	array( '{"useFeaturedImage":true}', $wrap( '', '<p>none</p>' ) ),
	array( '{"useFeaturedImage":true}', '<figure class="other"></figure>' . $wrap( '', $figure ) ),
);

foreach ( $ids as $id ) {
	global $post;
	$post = get_post( $id );
	if ( $post ) {
		setup_postdata( $post );
	}
	foreach ( $cases as list( $attrs, $content ) ) {
		$markup  = '<!-- wp:media-text ' . $attrs . ' -->' . $content . '<!-- /wp:media-text -->';
		$counter = (int) wp_unique_id();
		$html    = do_blocks( $markup );
		echo $counter, "\t", $id, "\t", bin2hex( $markup ), "\t", $raw ? $html : hash( 'sha256', $html ), "\n";
	}
}

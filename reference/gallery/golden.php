<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints do_blocks() for gallery block markup on the home page, with the
// block supports stylesheet each case leaves. Lines are
//   counter<TAB>hex(markup)<TAB>hex(html)<TAB>hex(css)
// where counter is the last number wp_unique_id() gave before the case.
// Nothing is written.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

$GLOBALS['wp'] = new WP();
wp( '' );

$image = function ( $attrs, $class ) {
	return '<!-- wp:image ' . $attrs . ' --><figure class="wp-block-image size-large"><img src="a.jpg" alt="" class="' . $class . '"/></figure><!-- /wp:image -->';
};
$images = array(
	$image( '{"id":5,"sizeSlug":"large"}', 'wp-image-5' ),
	$image( '{"id":6}', 'wp-image-6' ) . $image( '{"id":7,"data-id":"70"}', 'wp-image-7' ) . $image( '', '' ),
	'',
	'<p>no images</p>',
);
$attrs = array(
	'',
	'{"linkTo":"none","columns":3,"imageCrop":false,"className":"extra"}',
	'{"style":{"spacing":{"blockGap":"1em"}}}',
	'{"style":{"spacing":{"blockGap":"0"}}}',
	'{"style":{"spacing":{"blockGap":""}}}',
	'{"style":{"spacing":{"blockGap":null}}}',
	'{"style":{"spacing":{"blockGap":5}}}',
	'{"style":{"spacing":{"blockGap":"var:preset|spacing|40"}}}',
	'{"style":{"spacing":{"blockGap":"var:preset|spacing|Big Gap"}}}',
	'{"style":{"spacing":{"blockGap":{"left":"2em","top":"1em"}}}}',
	'{"style":{"spacing":{"blockGap":{"top":"1em"}}}}',
	'{"style":{"spacing":{"blockGap":{"left":null}}}}',
	'{"style":{"spacing":{"blockGap":{"left":3}}}}',
	'{"style":{"spacing":{"blockGap":"calc(1px)"}}}',
	'{"style":{"spacing":{"blockGap":"a/*b"}}}',
	'{"style":{"spacing":{"blockGap":"a}b"}}}',
	'{"style":{"spacing":{"padding":"1em"}}}',
	'{"style":{"spacing":"x"}}',
	'{"style":"x"}',
);
foreach ( $images as $inner ) {
	foreach ( $attrs as $attr ) {
		$markup = '<!-- wp:gallery ' . $attr . ' --><figure class="wp-block-gallery has-nested-images columns-default is-cropped">' . $inner . '</figure><!-- /wp:gallery -->';
		WP_Style_Engine_CSS_Rules_Store::remove_all_stores();
		$counter = (int) wp_unique_id();
		$html    = do_blocks( $markup );
		$css  = wp_style_engine_get_stylesheet_from_context( 'block-supports', array( 'prettify' => false ) );
		echo $counter, "\t", bin2hex( $markup ), "\t", bin2hex( $html ), "\t", bin2hex( $css ), "\n";
	}
}

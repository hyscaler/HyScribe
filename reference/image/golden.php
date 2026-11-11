<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints do_blocks() for image block markup on the home page. Lines are
//   hex(markup)<TAB>hex(html)
// Nothing is written.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

$GLOBALS['wp'] = new WP();
wp( '' );

$img     = '<img src="https://example.test/a.jpg" alt="A" class="wp-image-5"/>';
$caption = '<figcaption class="wp-element-caption">Words</figcaption>';
$empty   = '<figcaption class="wp-element-caption"></figcaption>';
$figures = array(
	"<figure class=\"wp-block-image\">$img</figure>",
	"<figure class=\"wp-block-image size-large\">$img$caption</figure>",
	"<figure class=\"wp-block-image\">$img$empty</figure>",
	"<figure class=\"wp-block-image\">$img<figcaption><!-- note --><!-- more --></figcaption></figure>",
	"<figure class=\"wp-block-image\">$img<figcaption> </figcaption></figure>",
	"<figure class=\"wp-block-image\">$img<figcaption><?pi?></figcaption></figure>",
	"<figure class=\"wp-block-image\">$img<figcaption><?xml?></figcaption></figure>",
	"<figure class=\"wp-block-image\">$img<figcaption><?></figcaption></figure>",
	"<figure class=\"wp-block-image\">$img<figcaption><? x></figcaption></figure>",
	"<figure class=\"wp-block-image\">$img<figcaption><?1></figcaption></figure>",
	"<figure class=\"wp-block-image\">$img<figcaption><?a*b></figcaption></figure>",
	"<figure class=\"wp-block-image\">$img<figcaption><?Xml-Stylesheet x?></figcaption></figure>",
	"<figure class=\"wp-block-image\">$img<figcaption><!DOCTYPE html></figcaption></figure>",
	"<figure class=\"wp-block-image\">$img<figcaption><!x></figcaption></figure>",
	"<figure class=\"wp-block-image\">$img<figcaption></ x></figcaption></figure>",
	"<figure class=\"wp-block-image\">$img<figcaption><span></span></figcaption></figure>",
	"<figure class=\"wp-block-image\">$img<figcaption></span></figcaption></figure>",
	"<figure class=\"wp-block-image\">$img<figcaption><figcaption></figcaption></figure>",
	"<figure class=\"wp-block-image\">$img<figcaption>",
	'<figure class="wp-block-image"><IMG SRC="x.jpg"></figure>',
	'<figure class="wp-block-image"><img alt="no source"></figure>',
	'<figure class="wp-block-image"><img src="" alt="empty"></figure>',
	'<figure class="wp-block-image"><img src="0"></figure>',
	'<figure class="wp-block-image"><img src></figure>',
	'<figure class="wp-block-image"><p>no image</p></figure>',
	'<figure class="wp-block-image"><imgx src="a.jpg"></figure>',
);
$attrs = array(
	'',
	'{"caption":"Given"}',
	'{"data-id":"7"}',
	'{"data-id":12}',
	'{"data-id":true}',
	'{"data-id":false}',
	'{"id":9,"metadata":{"bindings":{"id":{"source":"core/post-meta"}}}}',
	'{"id":5,"metadata":{"bindings":{"id":{"source":"core/post-meta"}}}}',
	'{"id":9,"data-id":"3","metadata":{"bindings":{"id":{"source":"core/post-meta"}}}}',
	'{"id":9,"metadata":{"bindings":{"url":{"source":"core/post-meta"}}}}',
	'{"className":"extra","align":"wide","sizeSlug":"large","linkDestination":"none"}',
);

foreach ( $figures as $figure ) {
	foreach ( $attrs as $attr ) {
		$markup = '<!-- wp:image ' . $attr . ' -->' . $figure . '<!-- /wp:image -->';
		echo bin2hex( $markup ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
	}
}
// The class to rename when the image's own is not the bound one.
foreach ( array( 'wp-image-5 wp-image-6', 'wp-image- wp-image-5', 'other', '' ) as $class ) {
	$markup = '<!-- wp:image {"id":9,"metadata":{"bindings":{"id":{"source":"core/post-meta"}}}} --><figure class="wp-block-image"><img src="a.jpg" class="' . $class . '"/>' . $empty . '</figure><!-- /wp:image -->';
	echo bin2hex( $markup ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
}

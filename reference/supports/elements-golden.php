<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints do_blocks() for blocks with element styles (style.elements) on
// Twenty Twenty-Five. Lines are
//   hex(markup)<TAB>wp-elements- counter<TAB>hex(html)<TAB>hex(block-supports CSS)
// Only pure rendering runs; the database is only read.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

$link = '{"style":{"elements":{"link":{"color":{"text":"var:preset|color|accent-1"}}}}}';
$cases = array(
	'<!-- wp:paragraph ' . $link . ' --><p class="has-link-color">x <a href="#">y</a></p><!-- /wp:paragraph -->',
	'<!-- wp:paragraph {"className":"lead","style":{"elements":{"link":{"color":{"text":"#ff0000"},":hover":{"color":{"text":"#00ff00"}},"typography":{"fontWeight":"700"}}}}} --><p class="lead has-link-color">x</p><!-- /wp:paragraph -->',
	'<!-- wp:paragraph {"style":{"elements":{"link":{":hover":{"color":{"text":"#00ff00"}}}}}} --><p>x</p><!-- /wp:paragraph -->',
	'<!-- wp:group {"style":{"elements":{"heading":{"color":{"text":"#111111","background":"#eeeeee"}},"h2":{"color":{"text":"#222222"}},"h3":{"color":{"gradient":"var:preset|gradient|vivid"}},"button":{"color":{"text":"#333333","background":"#444444"}},"link":{"color":{"text":"#555555"}}}},"layout":{"type":"constrained"}} --><div class="wp-block-group"><!-- wp:paragraph ' . $link . ' --><p class="has-link-color">in</p><!-- /wp:paragraph --></div><!-- /wp:group -->',
	'<!-- wp:group {"style":{"elements":{"button":{"color":{"background":"#444444"}}}}} --><div class="wp-block-group"><p>buttons only</p></div><!-- /wp:group -->',
	'<!-- wp:paragraph ' . $link . ' --><p>a</p><!-- /wp:paragraph --><!-- wp:paragraph ' . $link . ' --><p>b</p><!-- /wp:paragraph -->',
	'<!-- wp:paragraph {"style":{"elements":{"link":{"typography":{"fontWeight":"700"}}}}} --><p>no colour</p><!-- /wp:paragraph -->',
	'<!-- wp:paragraph {"style":{"elements":{"link":{"color":{"text":""}}}}} --><p>empty</p><!-- /wp:paragraph -->',
	'<!-- wp:paragraph {"style":{"elements":[]}} --><p>none</p><!-- /wp:paragraph -->',
	'<!-- wp:paragraph {"style":{"elements":"x"}} --><p>string</p><!-- /wp:paragraph -->',
	'<!-- wp:paragraph {"style":{"elements":{"link":"x"}}} --><p>string link</p><!-- /wp:paragraph -->',
	'<!-- wp:paragraph {"style":{"elements":{"h1":{"color":{"text":"#123456"}}}}} --><p>h1 on a paragraph</p><!-- /wp:paragraph -->',
	'<!-- wp:button {"style":{"elements":{"link":{"color":{"text":"#ff0000"}}}}} --><div class="wp-block-button"><a class="wp-block-button__link wp-element-button">B</a></div><!-- /wp:button -->',
	'<!-- wp:site-title {"style":{"elements":{"link":{"color":{"text":"#ff0000"}}}}} /-->',
	'<!-- wp:paragraph {"className":"x wp-elements-mine"} --><p>own class</p><!-- /wp:paragraph -->',
	'<!-- wp:paragraph {"className":"wp-elements-"} -->text first<!-- /wp:paragraph -->',
	'<!-- wp:hs/unknown {"style":{"elements":{"link":{"color":{"text":"#ff0000"}}}}} --><p>unregistered</p><!-- /wp:hs/unknown -->',
	'<!-- wp:paragraph {"style":{"elements":{"link":{"color":{"text":"#ff0000"}}}}} --><!-- /wp:paragraph -->',
	'<!-- wp:paragraph {"className":5,"style":{"elements":{"link":{"color":{"text":"#ff0000"}}}}} --><p>number class</p><!-- /wp:paragraph -->',
);

foreach ( $cases as $markup ) {
	WP_Style_Engine_CSS_Rules_Store::remove_all_stores();
	$counter = (int) substr( wp_unique_prefixed_id( 'wp-elements-' ), 12 );
	$html    = do_blocks( $markup );
	$css     = wp_style_engine_get_stylesheet_from_context( 'block-supports', array( 'prettify' => false ) );
	echo bin2hex( $markup ), "\t", $counter, "\t", bin2hex( $html ), "\t", bin2hex( $css ), "\n";
}

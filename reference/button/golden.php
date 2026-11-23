<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints do_blocks() for button block markup on the home page. Lines are
//   hex(markup)<TAB>hex(html)
// Nothing is written.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

$GLOBALS['wp'] = new WP();
wp( '' );

$link  = '<a class="wp-block-button__link wp-element-button">Go</a>';
$cases = array(
	'<div class="wp-block-button">' . $link . '</div>',
	'<div class="wp-block-button"><a class="wp-block-button__link wp-element-button"></a></div>',
	'<div class="wp-block-button"><a class="wp-block-button__link wp-element-button"><!-- note --></a></div>',
	'<div class="wp-block-button"><a class="wp-block-button__link wp-element-button"><!-- open</a></div>',
	'<div class="wp-block-button"><a class="wp-block-button__link wp-element-button"> </a></div>',
	'<div class="wp-block-button"><a class="wp-block-button__link wp-element-button"><img src="x.png" alt=""></a></div>',
	'<div class="wp-block-button"><A class="wp-block-button__link">Up</A></div>',
	'<div class="wp-block-button"><a class="wp-block-button__link"><a>nested</a></a></div>',
	'<div class="wp-block-button"><a class="wp-block-button__link"></abbr>x</a></div>',
	'<div class="wp-block-button"><a class="wp-block-button__link">',
	'<div class="wp-block-button"><button type="button" class="wp-block-button__link wp-element-button">Press</button></div>',
	'<div class="wp-block-button"><button type="button" class="wp-block-button__link wp-element-button"></button></div>',
	'<div class="wp-block-button"><span>no link</span></div>',
	'just text',
);
$widths = array( '50%', '33.3%', '100%', '10rem', 'var:preset|dimension|50', 'var:preset|dimension|huge', 'var:preset|dimension|Big Size' );

$markups = array();
foreach ( $cases as $html ) {
	$markups[] = "<!-- wp:button -->$html<!-- /wp:button -->";
}
foreach ( $widths as $width ) {
	$attrs     = wp_json_encode( array( 'style' => array( 'dimensions' => array( 'width' => $width ) ) ), JSON_UNESCAPED_SLASHES );
	$markups[] = "<!-- wp:button $attrs --><div class=\"wp-block-button\">$link</div><!-- /wp:button -->";
	$markups[] = "<!-- wp:button $attrs --><div class=\"wp-block-button\" style=\"margin:0\">$link</div><!-- /wp:button -->";
}
$markups[] = '<!-- wp:button {"style":{"dimensions":{"width":"50%"}}} --><div class="other">' . $link . '</div><!-- /wp:button -->';
$markups[] = '<!-- wp:button {"style":{"dimensions":{"width":"50%"}}} --><div class="wp-block-button" style>' . $link . '</div><!-- /wp:button -->';
$markups[] = '<!-- wp:button {"style":{"dimensions":{"width":""}}} --><div class="wp-block-button">' . $link . '</div><!-- /wp:button -->';
$markups[] = '<!-- wp:buttons --><div class="wp-block-buttons"><!-- wp:button --><div class="wp-block-button">' . $link . '</div><!-- /wp:button --><!-- wp:button --><div class="wp-block-button"><a class="wp-block-button__link wp-element-button"></a></div><!-- /wp:button --></div><!-- /wp:buttons -->';

foreach ( $markups as $markup ) {
	echo bin2hex( $markup ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
}

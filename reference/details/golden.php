<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints do_blocks() for details block markup on the home page. Lines are
//   hex(markup)<TAB>hex(html)
// Nothing is written.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

$GLOBALS['wp'] = new WP();
wp( '' );

$inner = '<summary>More</summary><!-- wp:paragraph --><p>Text <img src="a.png" alt=""> and <IMG src="b.png" fetchpriority="high"></p><!-- /wp:paragraph --><!-- wp:html --><figure><img src="c.png"></figure><!-- /wp:html -->';
$cases = array( '', ' {"showContent":true}', ' {"showContent":false}', ' {"showContent":"0"}', ' {"showContent":"yes"}', ' {"showContent":0}' );
foreach ( $cases as $attrs ) {
	$open   = str_contains( $attrs, 'true' ) || str_contains( $attrs, 'yes' ) ? ' open' : '';
	$markup = "<!-- wp:details$attrs --><details class=\"wp-block-details\"$open>$inner</details><!-- /wp:details -->";
	echo bin2hex( $markup ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
}
$markup = '<!-- wp:details --><details class="wp-block-details"><summary>No images</summary><!-- wp:paragraph --><p>Just text</p><!-- /wp:paragraph --></details><!-- /wp:details -->';
echo bin2hex( $markup ), "\t", bin2hex( do_blocks( $markup ) ), "\n";

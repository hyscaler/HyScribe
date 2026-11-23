<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints do_blocks() for widget group markup outside a classic sidebar.
// Lines are
//   hex(markup)<TAB>hex(html)
// Nothing is written.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

$GLOBALS['wp'] = new WP();
wp( '' );

$para  = '<!-- wp:paragraph --><p>Inside</p><!-- /wp:paragraph -->';
$cases = array(
	'<!-- wp:widget-group {"title":"Links"} -->' . "\n" . $para . "\n\n" . $para . "\n" . '<!-- /wp:widget-group -->',
	'<!-- wp:widget-group -->' . $para . '<!-- /wp:widget-group -->',
	'<!-- wp:widget-group {"title":"A & <b>B</b>"} --><div class="saved">ignored</div><!-- /wp:widget-group -->',
	'<!-- wp:widget-group {"title":"0"} -->' . $para . '<!-- /wp:widget-group -->',
	'<!-- wp:widget-group {"title":""} /-->',
	'<!-- wp:widget-group {"title":"Outer"} --><!-- wp:widget-group {"title":"Inner"} -->' . $para . '<!-- /wp:widget-group --><!-- /wp:widget-group -->',
);
foreach ( $cases as $markup ) {
	echo bin2hex( $markup ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
}

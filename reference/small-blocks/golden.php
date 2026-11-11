<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints small dynamic blocks rendered on their own. Lines are
//   hex(markup)<TAB>hex(html)
// Nothing is written.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

$markups = array(
	'<!-- wp:shortcode -->[gallery ids="1,2"]<!-- /wp:shortcode -->',
	"<!-- wp:shortcode -->\n[audio src=\"a.mp3\"]\n\nSecond line\nthird -- \"x\"\n<!-- /wp:shortcode -->",
	'<!-- wp:shortcode /-->',
	'<!-- wp:shortcode --><div>[x]</div><!-- /wp:shortcode -->',
	'<!-- wp:navigation-overlay-close /-->',
	'<!-- wp:navigation-overlay-close {"displayMode":"text"} /-->',
	'<!-- wp:navigation-overlay-close {"displayMode":"both","text":"Shut <b>it</b><script>x()</script>"} /-->',
	'<!-- wp:navigation-overlay-close {"displayMode":"other","text":""} /-->',
	'<!-- wp:navigation-overlay-close {"displayMode":"text","text":"0"} /-->',
	'<!-- wp:navigation-overlay-close {"displayMode":"icon","text":"Hide","backgroundColor":"accent-1","fontSize":"small","style":{"spacing":{"padding":{"top":"4px"}},"typography":{"letterSpacing":"1px"}}} /-->',
);
foreach ( $markups as $markup ) {
	echo bin2hex( $markup ), "\t", bin2hex( render_block( parse_blocks( $markup )[0] ) ), "\n";
}

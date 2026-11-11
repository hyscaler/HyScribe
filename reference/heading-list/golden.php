<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints do_blocks() for heading and list block markup on the home page.
// Lines are
//   hex(markup)<TAB>hex(html)
// Nothing is written.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

$GLOBALS['wp'] = new WP();
wp( '' );

$cases = array(
	array( 'heading', '<h2>Old</h2>' ),
	array( 'heading', '<h2 class="wp-block-heading">New</h2>' ),
	array( 'heading', '<H3 class="x">Upper</H3>' ),
	array( 'heading', '<div><p>a</p><h6 id="i">Deep</h6><h2>Second</h2></div>' ),
	array( 'heading', '<p>No heading</p>' ),
	array( 'heading', '<h7>Not one</h7>' ),
	array( 'heading', '0' ),
	array( 'heading', '' ),
	array( 'heading', '<h1 class>Empty class</h1>' ),
	array( 'list', '<ul><li>a</li></ul>' ),
	array( 'list', '<ol class="wp-block-list" start="3"><li>b</li></ol>' ),
	array( 'list', '<div><li>x</li><ul class="y"></ul></div>' ),
	array( 'list', '<dl><dt>t</dt></dl>' ),
	array( 'list', '0' ),
	array( 'list', '<UL>' ),
);
foreach ( $cases as list( $name, $html ) ) {
	foreach ( array( '', ' {"className":"extra","fontSize":"large"}' ) as $attrs ) {
		$markup = "<!-- wp:$name$attrs -->$html<!-- /wp:$name -->";
		echo bin2hex( $markup ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
	}
}

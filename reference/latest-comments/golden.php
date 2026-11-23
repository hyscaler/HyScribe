<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints do_blocks() for latest comments block markup over the default
// install, on its home page, without the elements support, which is
// checked on its own. Lines are
//   hex(markup)<TAB>hex(html)
// The database is only read.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

remove_filter( 'render_block', 'wp_render_elements_class_name', 10 );
remove_filter( 'render_block_data', 'wp_render_elements_support_styles', 10 );

function latest( $attrs ) {
	$json = $attrs ? ' ' . wp_json_encode( $attrs, JSON_UNESCAPED_SLASHES ) : '';
	return "<!-- wp:latest-comments$json /-->";
}

$cases = array(
	latest( array() ),
	latest( array( 'displayAvatar' => false ) ),
	latest( array( 'displayDate' => false ) ),
	latest( array( 'displayContent' => 'none' ) ),
	latest( array( 'displayContent' => 'full' ) ),
	latest( array( 'displayContent' => 'other' ) ),
	latest( array( 'displayExcerpt' => true, 'displayContent' => 'none' ) ),
	latest( array( 'displayExcerpt' => false, 'displayContent' => 'full' ) ),
	latest( array( 'commentsToShow' => 1, 'displayAvatar' => false, 'displayDate' => false, 'displayContent' => 'none' ) ),
	latest( array( 'align' => 'wide', 'className' => 'extra', 'fontSize' => 'small' ) ),
	latest( array( 'commentsToShow' => 0, 'displayAvatar' => false ) ),
	latest( array( 'commentsToShow' => -2, 'displayAvatar' => false ) ),
);

$GLOBALS['wp'] = new WP();
wp( '' );
foreach ( $cases as $markup ) {
	echo bin2hex( $markup ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
}

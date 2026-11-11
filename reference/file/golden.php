<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints do_blocks() for file block markup on the home page. Lines are
//   hex(markup)<TAB>hex(html)
// Nothing is written.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

$GLOBALS['wp'] = new WP();
wp( '' );

$object = function ( $label ) {
	return '<object class="wp-block-file__embed" data="https://example.test/a.pdf" type="application/pdf" style="width:100%;height:600px"' . $label . '></object>';
};
$link   = '<a id="wp-block-file--media-1" href="https://example.test/a.pdf">Doc</a>';
$bodies = array(
	$object( ' aria-label="Embed of Doc."' ) . $link,
	$object( ' aria-label="PDF embed"' ) . $link,
	$object( ' aria-label=""' ) . $link,
	$object( ' aria-label="0"' ) . $link,
	$object( ' aria-label' ) . $link,
	$object( '' ) . $link,
	$link,
);
$attrs  = array( '{"href":"https://example.test/a.pdf","displayPreview":true}', '{"href":"https://example.test/a.pdf"}', '{"displayPreview":false}', '{"displayPreview":"0"}' );
foreach ( $bodies as $body ) {
	foreach ( $attrs as $attr ) {
		$markup = '<!-- wp:file ' . $attr . ' --><div class="wp-block-file">' . $body . '</div><!-- /wp:file -->';
		echo bin2hex( $markup ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
	}
}
$markup = '<!-- wp:file {"displayPreview":true} --><p>no wrapper</p><!-- /wp:file -->';
echo bin2hex( $markup ), "\t", bin2hex( do_blocks( $markup ) ), "\n";

<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints do_blocks() for login/out block markup as a visitor, who is
// logged out, on several request URIs. Lines are
//   hex(request URI)<TAB>hex(markup)<TAB>hex(html)
// Nothing is written.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

$GLOBALS['wp'] = new WP();
wp( '' );

$_SERVER['HTTP_HOST'] = '127.0.0.1:8081';
$uris                 = array( '/', '/?p=1', '/?s=a+b&x=%3Cscript%3E', '/?page_id=2&q="x"&r=\'y\'', '/path with space/?a=1#frag' );
$markups              = array(
	'<!-- wp:loginout /-->',
	'<!-- wp:loginout {"redirectToCurrent":false} /-->',
	'<!-- wp:loginout {"displayLoginAsForm":true} /-->',
	'<!-- wp:loginout {"displayLoginAsForm":true,"redirectToCurrent":false} /-->',
	'<!-- wp:loginout {"displayLoginAsForm":"yes","redirectToCurrent":0} /-->',
	'<!-- wp:loginout {"className":"extra","anchor":"in","fontSize":"small","style":{"spacing":{"padding":{"top":"1em"}},"border":{"radius":"4px"},"elements":{"link":{"color":{"text":"#f00"}}}}} /-->',
	'<!-- wp:loginout {"displayLoginAsForm":true,"backgroundColor":"accent-1","style":{"typography":{"textTransform":"uppercase"}}} /-->',
);

foreach ( $uris as $uri ) {
	$_SERVER['REQUEST_URI'] = $uri;
	foreach ( $markups as $i => $markup ) {
		// wp_unique_id() counts across the run: number the link color once.
		if ( 5 === $i && '/' !== $uri ) {
			continue;
		}
		echo bin2hex( $uri ), "\t", bin2hex( $markup ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
	}
}

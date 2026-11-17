<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The test site of reference/url-details/golden.sh, served by PHP's built in
// server inside the oracle container as http://pages.test:8080, from
// routes.tsv beside it; hyscribe-testkit's page server serves the same.

$path   = parse_url( $_SERVER['REQUEST_URI'], PHP_URL_PATH );
$routes = array();
foreach ( file( __DIR__ . '/routes.tsv', FILE_IGNORE_NEW_LINES ) as $line ) {
	if ( '' !== $line && '#' !== $line[0] ) {
		$route                = explode( "\t", $line );
		$routes[ $route[0] ] = $route;
	}
}
header_remove( 'X-Powered-By' );
if ( ! isset( $routes[ $path ] ) ) {
	http_response_code( 404 );
	echo 'Not Found';
	return true;
}
list( , $status, $what ) = $routes[ $path ];
http_response_code( (int) $status );
header( 'Content-Type: text/html' );
if ( $status >= 300 && $status < 400 ) {
	header( 'Location: ' . $what );
} elseif ( '-' !== $what ) {
	echo str_replace( '%agent%', $_SERVER['HTTP_USER_AGENT'] ?? '', file_get_contents( __DIR__ . '/pages/' . $what ) );
}
return true;

<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// For reference/media/golden.sh: prints the page in argv[1] with the
// nonces of argv[2] and the time masked, hex encoded. With argv[3] set,
// a Theme Test Data page, whose content is not ours to commit, it prints
// hashes instead, of the page under hs-admin's URLs (as hs_admin_page() of
// hyscribe-testkit puts it, the oracle showing no update notice) with every
// nonce as %nonce%: sha256 of its screen meta and content, a tab, and
// sha256 of all of it. A list sorted
// by author or parent, whose ties the database breaks as it goes, is
// hashed without its rows (argv[4] is the screen).
$page = file_get_contents( $argv[1] );
foreach ( file( $argv[2], FILE_IGNORE_NEW_LINES ) as $line ) {
	list( $nonce, $action ) = explode( "\t", $line );
	$page = str_replace( $nonce, "%nonce:$action%", $page );
}
$page = preg_replace( '/"time":"[0-9]+"/', '"time":"%time%"', $page );
// Refusals and redirects hold none of the fixture's content.
if ( empty( $argv[3] ) || preg_match( "/^(302|403)\t/", $page ) ) {
	echo bin2hex( $page );
	exit;
}

/** `admin_paths()`: each path after $from under $to, without `.php`. */
function hyscribe_paths( $page, $from, $to, $stops ) {
	$out = '';
	while ( false !== ( $at = strpos( $page, $from ) ) ) {
		$out .= substr( $page, 0, $at ) . $to;
		$tail = substr( $page, $at + strlen( $from ) );
		$end  = strcspn( $tail, $stops );
		$file = substr( $tail, 0, $end );
		$out .= str_ends_with( $file, '.php' ) ? substr( $file, 0, -4 ) : $file;
		$page = substr( $tail, $end );
	}
	return $out . $page;
}

$page = hyscribe_paths( $page, '/wp-admin/', '/hs-admin/', "'?\"" );
$page = hyscribe_paths( $page, '%2Fwp-admin%2F', '%2Fhs-admin%2F', "%'\"&" );
$page = str_replace( '/wp-login.php', '/hs-login', $page );
foreach ( array( "href='", 'href="', '"url":"' ) as $prefix ) {
	$out = '';
	while ( false !== ( $at = strpos( $page, $prefix ) ) ) {
		$out .= substr( $page, 0, $at + strlen( $prefix ) );
		$tail = substr( $page, $at + strlen( $prefix ) );
		$end  = strcspn( $tail, "'?\"" );
		$file = substr( $tail, 0, $end );
		$out .= str_ends_with( $file, '.php' ) && ! str_contains( $file, '/' ) ? substr( $file, 0, -4 ) : $file;
		$page = substr( $tail, $end );
	}
	$page = $out . $page;
}
$page = preg_replace( '/%nonce:[^%]*%/', '%nonce%', $page );
if ( preg_match( '/orderby=(author|parent)/', $argv[4] ) ) {
	$start = strpos( $page, '<tbody id="the-list"' );
	$page  = substr( $page, 0, $start ) . substr( $page, strpos( $page, "\t</tbody>" ) );
}
$start = strpos( $page, '<div id="screen-meta"' );
$own   = false === $start ? $page : substr( $page, $start, strpos( $page, '<!-- wpbody-content -->' ) - $start );
echo hash( 'sha256', $own ), "\t", hash( 'sha256', $page );

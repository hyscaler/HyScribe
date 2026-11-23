<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for hyscribe-http's wp_validate_redirect() and wp_sanitize_redirect(),
// as the login screen asks them, with REQUEST_URI /wp-login.php. One case per
// line: hex(location)<TAB>hex(wp_sanitize_redirect)<TAB>hex(wp_validate_redirect
// with the fallback "FALLBACK"). Runs in the default install's PHP container
// (reference/redirect/golden.sh); it only reads.
define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';
$_SERVER['REQUEST_URI'] = '/wp-login.php';

mt_srand( 20261112 );
$home  = home_url();
$cases = array(
	'', '0', '/', '/x', 'x', '0/x', '//x', '///x', "$home/wp-admin/", "$home/wp-admin/edit.php?post_type=page&x=1",
	'http://example.test/', 'https://127.0.0.1:8081/x', 'http://127.0.0.1/x', 'HTTP://127.0.0.1:8081/', 'http://127.0.0.1:9/',
	'ftp://127.0.0.1/', 'javascript:alert(1)', 'mailto:a@b', 'http:x', 'http:/x', 'http://u@127.0.0.1/', 'http://u:p@127.0.0.1/',
	'http://127.0.0.1:8081@example.test/', 'http://example.test\\@127.0.0.1/', 'http://127.0.0.1%2F@example.test/', ' /x ',
	"\t/x\n", "\x00/x", "/x\x08", '/a b', '/é', "/\xc3", "/\xe2\x82\xac/x", '/' . str_repeat( 'é', 45 ), "/x%0d%0aSet-Cookie:x",
	'/x%0%0dd%0a', '/x%0D%0A', "/x\r\nY", '/x<script>', '/x"y\'z', '/x\\y', '/x?y=http://example.test/', '/x#y', '?x=1', '#x',
	'x?y', 'x/y?z', './x', '../x', ':80', '//:80', 'http://:80/', 'http://[::1]/', 'http://example.test:80/', 'http://localhost/',
	'HTTPS://127.0.0.1/', 'http://127.0.0.1./', 'http://127.0.0.1:8081', '//127.0.0.1/x', '//example.test/x', 'http://127.0.0.1?x',
	// Every lead byte's ranges, at their edges and truncated.
	"/\xe0\xa0\x80", "/\xe0\x9f\xbf", "/\xed\x9f\xbf", "/\xed\xa0\x80", "/\xf0\x90\x80\x80", "/\xf0\x8f\xbf\xbf",
	"/\xf4\x8f\xbf\xbf", "/\xf4\x90\x80\x80", "/\xf1\x80\x80\x80", "/\xe1\x80", "/\xc2", "/\xc1\xbf", "/\xf5\x80\x80\x80", '/日本',
);
$atoms = array( 'http', 'https', ':', '//', '/', '?', '#', '@', '127.0.0.1', 'example.test', ':8081', '%0d', '%0a', '%0', 'd', ' ', "\t",
	"\n", "\0", 'é', "\xff", "\xc3", 'x', '0', '.', '..', '\\', '[', ']', '<', '"', '%2F', 'wp-admin', '&', '=', 'A' );
for ( $i = 0; $i < 1500; $i++ ) {
	$case = '';
	$n    = mt_rand( 1, 10 );
	for ( $j = 0; $j < $n; $j++ ) {
		$case .= $atoms[ mt_rand( 0, count( $atoms ) - 1 ) ];
	}
	$cases[] = $case;
}
foreach ( $cases as $case ) {
	echo bin2hex( $case ), "\t", bin2hex( wp_sanitize_redirect( $case ) ), "\t", bin2hex( wp_validate_redirect( $case, 'FALLBACK' ) ), "\n";
}

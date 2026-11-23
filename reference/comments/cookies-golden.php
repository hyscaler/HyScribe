<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints what sanitize_comment_cookies() leaves of each commenter cookie
// value, as wp_get_current_commenter() reads it, one line per value:
//   kind<TAB>hex(cookie value)<TAB>hex(commenter's value)
// where kind is author, email or url. The values are slashed as
// wp_magic_quotes() slashes what PHP reads. Nothing is written.

require '/var/www/html/wp-load.php';

$values = array(
	'author' => array(
		'', '0', 'Ann', ' Ann ', "\tAnn\n", '<b>Ann</b>', 'A & B', 'A &amp; B', "O'Neil", '"quoted"', 'back\\slash',
		'double\\\\slash', "line\nbreak", 'tab	inside', '%41', '+plus+', 'Élodie', '<script>x</script>y', '<3 you', 'a < b > c',
		'&#8364;', '&#x41;', 'x&y;', "bad\xffbyte", "nul\0byte", str_repeat( 'a', 300 ), '<a href="http://x.test/">link</a>',
		'<!-- c -->d', '%0A', 'a%20b',
	),
	'email' => array(
		'', '0', 'a@b.test', ' A@B.test ', 'a b@c.test', 'a@b', '<a@b.test>', "a'b@c.test", 'ünï@c.test', 'a@b..test',
		'A+tag@Example.COM', "x@y.zz\n", 'a@b.test<b>', 'a"b@c.test', 'a\\b@c.test', 'a@b.test,c@d.test', 'nobody',
		'a@-b.test', 'a@b-.test', 'a@.b.test', '@b.test', 'a@b.t', "bad\xff@b.test", 'a%40b.test', 'a&amp;b@c.test',
	),
	'url' => array(
		'', '0', 'http://x.test/', 'javascript:alert(1)', ' http://x.test/a b ', 'http://x.test/?a=1&b=2',
		'http://x.test/?a=1&amp;b=2', 'http://x.test/<b>', 'www.x.test', '//x.test', "http://x.test/'q'", 'ftp://x.test/',
		'HTTP://X.TEST', 'http://x.test/é', 'http://x.test/a\\b', 'http://x.test/"q"', 'mailto:a@b.test',
		'http://x.test/%41', 'http://x.test/a+b', "http://x.test/\n", "bad\xff://x.test", 'data:text/html,x', 'x.test/path',
		'http://[::1]/', 'http://user:pw@x.test/', str_repeat( 'a', 300 ),
	),
);
$names = array(
	'author' => 'comment_author_',
	'email'  => 'comment_author_email_',
	'url'    => 'comment_author_url_',
);
foreach ( $values as $kind => $list ) {
	foreach ( $list as $value ) {
		foreach ( $names as $prefix ) {
			unset( $_COOKIE[ $prefix . COOKIEHASH ] );
		}
		$_COOKIE[ $names[ $kind ] . COOKIEHASH ] = wp_slash( $value );
		sanitize_comment_cookies();
		$commenter = wp_get_current_commenter();
		$field     = 'author' === $kind ? 'comment_author' : "comment_author_$kind";
		echo $kind, "\t", bin2hex( $value ), "\t", bin2hex( $commenter[ $field ] ), "\n";
	}
}

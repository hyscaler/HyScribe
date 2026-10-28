<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for hyscribe-phpser's add_query_arg(), remove_query_arg() and
// build_query(). Each line is
//   hex(uri)<TAB>hex(remove)<TAB>hex(add one)<TAB>hex(add several)
// where remove drops replytocom, unapproved and moderation-hash, as
// get_cancel_comment_reply_link() does. Only pure functions run.
// Random cases use a fixed seed, so the output is stable.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

mt_srand( 20261028 );

$uris = array(
	'', '/', '/?p=1', '/?p=1&replytocom=1', '/?replytocom=1&p=1#respond', '/?p=1&replytocom=1&unapproved=5&moderation-hash=abc',
	'http://example.test/?p=1', 'HTTP://Example.test/a?replytocom=2', 'https://example.test', 'https://example.test/?', 'HtTpS://x?a=1#f',
	'ftp://x/?a=1', '//x/?a=1', 'a=1', 'a=1&b=2', 'x', '?', '??', '?a=1?b=2', '/?a=1&&b=2', '/?a', '/?a=', '/?a=&b', '/?=1', '/?&', '#',
	'/#a?b=1', '/?a=1#', '/?a=1##b', '/?a[]=1&a[]=2', '/?a[x]=1&a[y][z]=2', '/?0[x]=1', '/?0=a&1=b', '/?a=b%26c&d=e+f', '/?a=%E2%82%AC',
	'/?a=%zz', '/?a%5Bb%5D=1', '/?a b=1&c.d=2', '/?a=1&a=2', '/?replytocom[]=1', '/?x=1?', '/?x=%0A', "/?x%0A=", "/?x\n=", '/?x=1&x%0A',
	'/path/?s=hello+world&cat=1', '/?p=1&replytocom=1&x[]=', '/?a[][]=1', 'mailto:a@b?subject=x', '/?a=1&b=' . str_repeat( 'x', 300 ),
	'?replytocom=1', 'replytocom=1', '/?' . implode( '&', array_map( fn( $i ) => "v$i=$i", range( 1, 1005 ) ) ),
);
$atoms = array( '/', '?', '#', '&', '=', 'a', 'b', 'p', '1', 'replytocom', 'unapproved', '[', ']', '%', '%20', '+', ' ', 'http://', 'https://', 'x.y', '%5B', '0', "\n" );
for ( $i = 0; $i < 2500; $i++ ) {
	$uri = '';
	$n   = mt_rand( 1, 12 );
	for ( $j = 0; $j < $n; $j++ ) {
		$uri .= $atoms[ mt_rand( 0, count( $atoms ) - 1 ) ];
	}
	$uris[] = $uri;
}
foreach ( $uris as $uri ) {
	echo bin2hex( $uri ), "\t",
		bin2hex( remove_query_arg( array( 'replytocom', 'unapproved', 'moderation-hash' ), $uri ) ), "\t",
		bin2hex( add_query_arg( 'x', '1', $uri ) ), "\t",
		bin2hex( add_query_arg( array( 'a' => '1 2', 'b' => false, '0' => 'z', 'p' => '9' ), $uri ) ), "\n";
}

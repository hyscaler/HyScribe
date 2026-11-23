<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Writes crates/hyscribe-phpser/tests/golden/parse-str.tsv: parse_str(),
// the oracle for hyscribe-phpser's parse_str. Each line is
//   hex(query)<TAB>hex(serialize(result))
// Random cases use a fixed seed, so the output is stable.
//
// Usage: php reference/phpser/query-golden.php > crates/hyscribe-phpser/tests/golden/parse-str.tsv

mt_srand( 20261028 );

$cases = array(
	'', '&', '&&', '=', '=x', 'a', 'a=', 'a=1', 'a=1&a=2', 'a=1&b=2&c', 'a==b', 'a=b=c', '+a=1', ' a=1', '%20a=1', '  a b.c=1',
	'a+b=c+d', 'a%2Bb=c%2Bd', 'a%=1', 'a%4=1', 'a%41=%4', '%zz=%zz', '%41%42=%43', 'a%00b=1', 'a=x%00y', "a=x\0y&b=2", '%00=1',
	'a[]=1&a[]=2', 'a[x]=1&a[y]=2', 'a[x][y]=1', 'a[x][]=1&a[x][]=2', 'a[=1', 'a[b=1', 'a[b.c d=1', 'a[b[c=1', 'a.b[c.d]=1', 'a b[c d]=1',
	'a[b][c=1', 'a[b]c=1', 'a[b]c[d]=1', 'a[b][c]d=1', 'a]=1', 'a]b[c]=1', '[a]=1', '[]=1', ' []=1', 'a[ ]=1', 'a[ ][x]=1&b[ ][]=2&c[ ][ ]=3', 'a[  ]=1', 'a[ b]=1', 'a[b ]=1',
	'a[[b]]=1', 'a[]]=1', 'a[]x=1', 'a[][]=1&a[][]=2', 'a[][x]=1&a[][x]=2', 'a[0]=x&a[]=y', 'a[5]=x&a[]=y', 'a[-5]=x&a[]=y', 'a[05]=x&a[]=y',
	'a[-0]=x', 'a[ 1]=x', 'a[1 ]=x', 'a[9223372036854775807]=x&a[]=y', 'a[9223372036854775808]=x&a[]=y', 'a[-9223372036854775808]=x&a[]=y',
	'a=1&a[]=2', 'a[]=1&a=2', 'a[x]=1&a[x][y]=2', 'a[x][y]=1&a[x]=2', '0=a&1=b&=c', '5=a&05=b&-1=c', 'a.b=1&a_b=2', 'a%5Bb%5D=1', 'a%5B=1',
	'p=1&replytocom=1', 'replytocom[]=1', 'replytocom=1x', 'replytocom=+1', 's=hello+world&cat=1', 'x=%E2%82%AC&%E2%82%AC=y',
	str_repeat( 'a[]=1&', 1001 ), str_repeat( 'k', 5000 ) . '=v', 'a' . str_repeat( '[x]', 63 ) . '=1', 'a' . str_repeat( '[x]', 64 ) . '=1',
	'a' . str_repeat( '[]', 64 ) . '=1', 'a' . str_repeat( '[]', 65 ) . '=1&b=2', implode( '&', array_map( fn( $i ) => "v$i=$i", range( 1, 1005 ) ) ),
	str_repeat( '&', 1500 ) . 'a=1', implode( '&', array_fill( 0, 999, '=x' ) ) . '&a=1&b=2',
);
$atoms = array( 'a', 'b', '0', '1', '-', ' ', '.', '+', '[', ']', '=', '&', '%', '%5B', '%5D', '%20', '%00', '%2', '%zz', 'x', '_', '[]', '[0]', '[a]' );
for ( $i = 0; $i < 3000; $i++ ) {
	$q = '';
	$n = mt_rand( 1, 14 );
	for ( $j = 0; $j < $n; $j++ ) {
		$q .= $atoms[ mt_rand( 0, count( $atoms ) - 1 ) ];
	}
	$cases[] = $q;
}
foreach ( $cases as $case ) {
	parse_str( $case, $result );
	echo bin2hex( $case ), "\t", bin2hex( serialize( $result ) ), "\n";
}

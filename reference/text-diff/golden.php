<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints wp_text_diff()'s table, with the revisions screen's arguments, for
// a set of text pairs: hex(left)<TAB>hex(right)<TAB>hex(table), the table
// FATAL where PHP stops with an error, and a long one as sha256:<its hash>.
// Run in the oracle's image, whose PCRE decides what \w matches
// (reference/text-diff/golden.sh).
define( 'ABSPATH', '/usr/src/wordpress/' );
define( 'WPINC', 'wp-includes' );
function __( $text ) {
	return $text;
}
function apply_filters( $hook, $value ) {
	return $value;
}
function wp_parse_args( $args, $defaults ) {
	return array_merge( $defaults, $args );
}
require ABSPATH . WPINC . '/formatting.php';
require ABSPATH . WPINC . '/pluggable.php';

$cases = array(
	array( '', '' ),
	array( 'Same', 'Same' ),
	array( '', 'Added text' ),
	array( 'Removed text', '' ),
	array( 'Hello world', 'Hello there world' ),
	array( "One\nTwo\nThree", "One\nTwo and a half\nThree" ),
	array( "One\nTwo\nThree", "One\nThree" ),
	array( "One\nThree", "One\nTwo\nThree" ),
	array( "Alpha\nBeta", "Gamma\nDelta\nEpsilon" ),
	array( "Completely different line here\nSecond", "Nothing alike at all, zzz\nSecond" ),
	array( "a b c\nd e f\ng h i", "g h i\na b c\nd e f" ),
	array( "<p>Tags & \"quotes\" 'single'</p>", "<p>Tags &amp; \"quotes\" 'double'</p>" ),
	array( "  Padded  \r\n\r\n\tTabs\t\tand  spaces  ", "Padded\nTabs and spaces" ),
	array( "Line\r\nEnd", "Line\rEnd\r" ),
	array( "1e3\n2\n3", "1000\n2\n4" ),
	array( "1.0\nx\n1", "1\ny\n1.0" ),
	array( "0\na", "0\nb" ),
	array( '0', '1' ),
	array( 'café crème', 'café brûlée' ),
	array( "a\u{0301}b c", "a\u{0301}b d" ),
	array( "x\u{203F}y z", "x\u{203F}y w" ),
	array( 'Ⅻ roman', 'Ⅻ numeral' ),
	array( '日本語のテキスト', '日本語の文章' ),
	array( 'emoji 😀 here', 'emoji 😃 here' ),
	array( "bad \xff byte\nsame", "bad \xff byte\nchanged" ),
	array( "keep\nbad \xff one", "keep\nbad \xfe two" ),
	array( "a \xc3 b\nx", "a \xc3 b\ny" ),
	array( "bad \xe2\x82 cut \xf0\x9f\x98 here\nq", "bad \xe2\x82 cut \xf0\x9f\x98 here\nr" ),
	array( '  lead space', ' lead space' ),
	array( 'a  b', 'a b c' ),
	array( 'x', "x\n\n\ny" ),
	array( "<!-- wp:paragraph -->\n<p>First paragraph.</p>\n<!-- /wp:paragraph -->\n\n<!-- wp:paragraph -->\n<p>Second.</p>\n<!-- /wp:paragraph -->", "<!-- wp:paragraph -->\n<p>First paragraph, edited.</p>\n<!-- /wp:paragraph -->\n\n<!-- wp:heading -->\n<h2 class=\"wp-block-heading\">New heading</h2>\n<!-- /wp:heading -->\n\n<!-- wp:paragraph -->\n<p>Second.</p>\n<!-- /wp:paragraph -->" ),
	array( "a\nb\nc\nd\ne\nf\ng\nh\ni\nj", "a\nB\nc\nD\ne\nF\ng\nH\ni\nJ" ),
	array( "p\nq\np\nq\np", "q\np\nq\np\nq" ),
	array( "same\nsame\nsame", "same\nsame" ),
	array( "x y\nx y\nz", "x y\nz\nx y" ),
	array( 'The quick brown fox jumps over the lazy dog', 'The quick red fox leaps over the lazy cat' ),
	array( "short\nlonger line of text", "longer line of text!\nshort?" ),
	array( '_under_score', '_under-score' ),
	array( ' a ! ', '  !' ),
	array( '!a  !', 'a a ! ' ),
	array( "x\na! b a", "x\n !a a!" ),
	array( "keep\ngood one", "keep\nbad \xfe two" ),
	array( "99999999999999999999\n+99999999999999999999", "+99999999999999999999\n99999999999999999999" ),
	array( "9223372036854775808\nz", "9223372036854775808.0\nz" ),
	// More unchanged lines than the 10000 of context each side keeps.
	array( "a\n" . str_repeat( "x\n", 20001 ) . 'b', "c\n" . str_repeat( "x\n", 20001 ) . 'd' ),
);
// Random pairs over a few lines and words, for the engine's edge cases.
mt_srand( 713 );
$pieces = array( 'a', 'b', 'c', 'a b', 'b a', '1', '1.0', '01', ' ', '', 'x y z', 'é', '0', '1e0', ' a' );
for ( $n = 0; $n < 600; $n++ ) {
	$pair = array();
	foreach ( array( 0, 1 ) as $side ) {
		$lines = array();
		for ( $i = mt_rand( 0, 12 ); $i > 0; $i-- ) {
			$lines[] = $pieces[ mt_rand( 0, count( $pieces ) - 1 ) ];
		}
		$pair[] = implode( "\n", $lines );
	}
	$cases[] = $pair;
}
foreach ( $cases as list( $left, $right ) ) {
	try {
		$diff = wp_text_diff(
			$left,
			$right,
			array(
				'show_split_view' => true,
				'title_left'      => 'Removed',
				'title_right'     => 'Added',
			)
		);
	} catch ( Throwable $e ) {
		$diff = 'FATAL';
	}
	// A long table is kept as its hash.
	if ( strlen( $diff ) > 100000 ) {
		$diff = 'sha256:' . hash( 'sha256', $diff );
	}
	echo bin2hex( $left ), "\t", bin2hex( $right ), "\t", bin2hex( $diff ), "\n";
}

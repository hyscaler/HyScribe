<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for hyscribe-html's TagProcessor. Prints one case per line:
//   hex(html) <TAB> operations <TAB> results
//   php golden.php <wordpress> edge          > crates/hyscribe-html/tests/golden/edge.tsv
//   php golden.php <wordpress> theme <theme> > crates/hyscribe-html/tests/golden/theme.tsv
//   php golden.php <wordpress> random        > crates/hyscribe-html/tests/golden/random.tsv
// Run through reference/html/golden.sh. Random cases use a fixed seed.
//
// Operations are space-separated, arguments hex and colon-separated, `~`
// for a missing one; each gives one space-separated result:
//   N:tag:class  next_tag()                      1 or 0
//   S:name:value set_attribute( name, value )    1 or 0
//   B:name       set_attribute( name, true )     1 or 0
//   R:name       remove_attribute()              1 or 0
//   A:class      add_class()                     1 or 0
//   D:class      remove_class()                  1 or 0
//   G:name       get_attribute()                 n, t, or s<hex>
//   H:class      has_class()                     n, 1 or 0
//   L            class_list()                    l<hex>,<hex>...
//   T            get_tag()                       n or s<hex>
//   W            next_tag() and get_tag() until the end: w<hex>,<hex>...
//   U            get_updated_html()              s<hex>
//   K:name       set_bookmark()                  1 or 0
//   X:name       release_bookmark()              1 or 0
//   J:name       seek()                          1 or 0
//   Y:name       has_bookmark()                  1 or 0
//   Z:at:n:text  replace n bytes at at with text 1

ini_set( 'display_errors', 'stderr' );
error_reporting( E_ALL & ~E_WARNING );
define( 'ABSPATH', rtrim( $argv[1], '/' ) . '/' );
define( 'WPINC', 'wp-includes' );
define( 'WP_DEBUG', false );
function __( $text ) {
	return $text;
}
foreach ( array(
	'plugin',
	'functions',
	'formatting',
	'kses',
	'http',
	'utf8',
	'class-wp-token-map',
	'html-api/html5-named-character-references',
	'html-api/class-wp-html-attribute-token',
	'html-api/class-wp-html-span',
	'html-api/class-wp-html-text-replacement',
	'html-api/class-wp-html-decoder',
	'html-api/class-wp-html-tag-processor',
) as $file ) {
	require_once ABSPATH . WPINC . "/$file.php";
}

mt_srand( 20261027 );

// A replacement of a range of the input, as HyScribe's replace_range() makes.
class Golden_Processor extends WP_HTML_Tag_Processor {
	public function replace( $at, $length, $text ) {
		$this->lexical_updates[] = new WP_HTML_Text_Replacement( $at, $length, $text );
	}

	// Whether a bookmark has moved to a negative offset, where PHP's strpos()
	// counts from the end.
	public function has_negative_bookmark() {
		foreach ( $this->bookmarks as $bookmark ) {
			if ( $bookmark->start < 0 ) {
				return true;
			}
		}
		return false;
	}
}

function arg( $value ) {
	return null === $value ? '~' : bin2hex( $value );
}

function op( $code, ...$args ) {
	return array( $code, $args );
}

function encode_op( $op ) {
	list( $code, $args ) = $op;
	return implode( ':', array_merge( array( $code ), array_map( 'arg', $args ) ) );
}

function bit( $value ) {
	return $value ? '1' : '0';
}

function run_op( $p, $op ) {
	list( $code, $args ) = $op;
	switch ( $code ) {
		case 'N':
			$query = array();
			if ( null !== $args[0] ) {
				$query['tag_name'] = $args[0];
			}
			if ( null !== $args[1] ) {
				$query['class_name'] = $args[1];
			}
			return bit( $p->next_tag( $query ) );
		case 'S':
			return bit( $p->set_attribute( $args[0], $args[1] ) );
		case 'B':
			return bit( $p->set_attribute( $args[0], true ) );
		case 'R':
			return bit( $p->remove_attribute( $args[0] ) );
		case 'A':
			return bit( $p->add_class( $args[0] ) );
		case 'D':
			return bit( $p->remove_class( $args[0] ) );
		case 'G':
			$value = $p->get_attribute( $args[0] );
			return null === $value ? 'n' : ( true === $value ? 't' : 's' . bin2hex( $value ) );
		case 'H':
			$has = $p->has_class( $args[0] );
			return null === $has ? 'n' : bit( $has );
		case 'L':
			$names = array();
			foreach ( $p->class_list() as $name ) {
				$names[] = bin2hex( (string) $name );
			}
			return 'l' . implode( ',', $names );
		case 'T':
			$tag = $p->get_tag();
			return null === $tag ? 'n' : 's' . bin2hex( $tag );
		case 'W':
			$tags = array();
			while ( $p->next_tag() ) {
				$tags[] = bin2hex( $p->get_tag() );
			}
			return 'w' . implode( ',', $tags );
		case 'U':
			return 's' . bin2hex( $p->get_updated_html() );
		case 'K':
			return bit( $p->set_bookmark( $args[0] ) );
		case 'X':
			return bit( $p->release_bookmark( $args[0] ) );
		case 'J':
			return bit( $p->seek( $args[0] ) );
		case 'Y':
			return bit( $p->has_bookmark( $args[0] ) );
		case 'Z':
			$p->replace( (int) $args[0], (int) $args[1], $args[2] );
			return '1';
	}
	throw new Exception( "unknown operation $code" );
}

function emit( $html, $ops ) {
	$ops[]   = op( 'U' );
	$p       = new Golden_Processor( $html );
	$results = array();
	// A case PHP fails on, or that moves a bookmark to a negative offset, is
	// left out: no edit WordPress makes reaches either.
	try {
		foreach ( $ops as $op ) {
			$results[] = run_op( $p, $op );
			if ( $p->has_negative_bookmark() ) {
				return;
			}
		}
	} catch ( Throwable $e ) {
		return;
	}
	echo bin2hex( $html ), "\t", implode( ' ', array_map( 'encode_op', $ops ) ), "\t", implode( ' ', $results ), "\n";
}

function pick( $list ) {
	return $list[ mt_rand( 0, count( $list ) - 1 ) ];
}

// Hand-written cases: each is HTML and a list of operations.
function edge_cases() {
	$walk   = array( op( 'W' ) );
	$first  = array( op( 'N', null, null ), op( 'T' ) );
	$cases  = array();
	$tokens = array(
		'', 'plain', '<', '<<', '<>', '< div>', '<3', '</', '</>', '</ >', '</3>', '</div', '</div>', '</div x=1>',
		'<div', '<div ', '<div>', '<DiV>', '<div/>', '<div/ >', '<div / >', '<div x>', '<div x', '<div x=', '<div x="',
		'<div x="1"', '<div x="1">', "<div x='1'>", '<div x=1>', '<div x=1', '<div x = 1 >', "<div\tx\n=\f'1'\r>",
		'<div x="a>b">', '<div x=a"b>', '<div =x>', '<div ==x>', '<div x==y>', '<div "x"=1>', '<div x/y=1>',
		'<div x=1/>', '<div x=/>', '<div a b c>', '<div a="1" A="2" a=3>', "<div a\0b=1>", "<d\0v>", '<a1-b.c>',
		'<!---->', '<!-->', '<!--->', '<!-- x -->', '<!-- x --!>', '<!-- x -- >', '<!-- x', '<!--', '<!-', '<!',
		'<!-- a -- b -->', '<!-- --->', '<!-- ----->', '<!DOCTYPE html>', '<!doctype html', '<!DOCTYPE>',
		'<![CDATA[ x ]]>', '<![CDATA[ x > ]]>', '<![CDATA[', '<?xml version="1.0"?>', '<?php echo 1; ?>', '<?',
		'<?x', '<?>', '</?x>', '</!x>', '</ x>', '</ x', '<a><b></b></a>', 'x<a>y</a>z', 'x <y', 'x < y',
		'<script>a</script>', '<script>a</script', '<script>a</scriptx></script>', '<script>a</SCRIPT >',
		'<script>a</script/>', '<script>a</script x=">">', '<script><!--a--></script>', '<script><!-- <script> </script> --></script>',
		'<script><!-- <script> </script> </script>', '<script><!-- </script>x', '<script><!--></script>', '<script><!---></script>',
		'<script>--></script>', '<script> -- </script>', '<script><!-- <SCRIPT>a</script>b</script>c</script>',
		'<script></script>', '<script>', '<script x=1>a</script><p>', '<script>a</script>tail',
		'<script><!-- <scriptx> </script>', '<script>a<</script>', '<script>a</scrip</script>',
		'<style>a</style>', '<style>a</style', '<style>a</stylex></style>', '<style>a</STYLE>', '<style>a</style/>',
		'<style>a</style x="</style>">b', '<style>a</style/ x>', '<style>a</style ', '<style></style>',
		'<textarea><b></textarea>', '<textarea>a</textarea', '<title><p>t</title>', '<title>a</titl</title>',
		'<iframe>x</iframe>', '<noembed>x</noembed>', '<noframes>x</noframes>', '<xmp><b></xmp>', '<listing>x',
		'<pre>x</pre>', '<textarea>a</textarea x>', '<textarea>a</textarea =>', '<title>a</title\0>',
		'<title>a</title x=1>b</title>', '<style>a</style x', '<style>a</style x=', '<style>a</style =x',
		'<style>x</style', "<style>x</style\t>", '<style>x</st', '<script>x</sc', '<script>x</script ',
		'<script>12345678</script>', '<script>12345678</script abc', '<script>12345678</script abc>x<b>', '<script><script></script><b>', '<script>1234567</script>', '<script>-->', '<script><!--',
		'<div class="a b">', '<div class=" a  b ">', '<div class>', '<div class="">', '<div CLASS="A">',
		'<div class="a" class="b">', '<p>&amp;</p>', '<div x="&amp;&lt;&#65;&#x42;&notin;&ampx">',
	);
	foreach ( $tokens as $html ) {
		$cases[] = array( $html, $walk );
		$cases[] = array( $html, $first );
		$cases[] = array(
			$html,
			array( op( 'N', null, null ), op( 'S', 'data-x', '1' ), op( 'A', 'k' ), op( 'G', 'x' ), op( 'N', null, null ), op( 'S', 'y', '2' ) ),
		);
	}
	$attrs = '<div id=a ID="b" class="x  y x" data-v=\'&lt;q&gt;\' hidden Hidden=1 =eq data-e="" title="a&#10;b">';
	$names = array( 'id', 'ID', 'class', 'data-v', 'hidden', 'HIDDEN', '=eq', 'data-e', 'title', 'missing', '', "a\0" );
	foreach ( $names as $name ) {
		$cases[] = array( $attrs, array( op( 'N', null, null ), op( 'G', $name ) ) );
		$cases[] = array( $attrs, array( op( 'N', null, null ), op( 'R', $name ), op( 'G', $name ) ) );
		$cases[] = array( $attrs, array( op( 'N', null, null ), op( 'S', $name, 'new"<&>\'' ), op( 'G', $name ) ) );
		$cases[] = array( $attrs, array( op( 'N', null, null ), op( 'B', $name ), op( 'G', $name ) ) );
	}
	$values = array( "\r\n\r", "a\0b", '&#0;', '&#128;&#159;', '&#xD800;', '&#1114112;', '&#x110000;', '&#99999999;', '&#x', '&#;', '&;', '&am', '&amp', '&ampx', '&amp=', '&amp;x', '&lt', '&ltx', '&notit;', '&notin', '&CounterClockwiseContourIntegral;', '&#00000065', '&#x0000041', '&#x00000000041' );
	foreach ( $values as $value ) {
		$cases[] = array( '<div x="' . $value . '">', array( op( 'N', null, null ), op( 'G', 'x' ) ) );
	}
	$invalid = array( '', 'a b', 'a"', "a'", 'a>', 'a&', 'a<', 'a/', 'a=', "a\x01", "a\x1f", "a\x7f", "a\xef\xb7\x90", "a\xef\xbf\xbe", "\xf0\x9f\xbf\xbf", "\xf4\x8f\xbf\xbe", "\xf3\xaf\xbf\xbf", 'é', 'ünï' );
	foreach ( $invalid as $name ) {
		$cases[] = array( '<p>', array( op( 'N', null, null ), op( 'S', $name, 'v' ), op( 'B', $name ) ) );
	}
	$classes = array(
		array( '<p>', array( 'A:a', 'A:b', 'A:a' ) ),
		array( '<p class="a b">', array( 'D:a', 'A:c' ) ),
		array( '<p class="a b">', array( 'D:a', 'D:b' ) ),
		array( '<p class="a b">', array( 'D:z' ) ),
		array( '<p class="a b">', array( 'A:a' ) ),
		array( '<p class=" a  b	c a ">', array( 'A:d' ) ),
		array( '<p class=" a  b	c a ">', array( 'D:b' ) ),
		array( '<p class>', array( 'A:x' ) ),
		array( '<p class="x" class="y">', array( 'D:x' ) ),
		array( '<p class="x" class="y">', array( 'A:z' ) ),
		array( '<p class="&lt;a&gt;">', array( 'A:b' ) ),
		array( '<p>', array( 'A:a"b' ) ),
		array( '<p class="1 2">', array( 'A:1', 'D:2', 'A:3' ) ),
		array( '<p class="A">', array( 'D:a', 'A:a' ) ),
		array( '<p>', array( 'A:a', 'D:a' ) ),
		array( '<p class="a">', array( 'A:b', 'R:class' ) ),
		array( '<p class="a">', array( 'A:b', 'S:class:c' ) ),
		array( '<p class="a">', array( 'S:class:c', 'A:b' ) ),
		array( '<p class="a">', array( 'R:class', 'A:b' ) ),
		array( '<p>', array( 'B:class', 'A:b' ) ),
		array( '<p class="a">', array( 'A:b', 'G:class', 'A:c' ) ),
		array( '<p class="a">', array( 'A:b', 'L', 'H:b', 'H:a', 'H:A' ) ),
		array( '</p>', array( 'A:b', 'D:b', 'L', 'H:b', 'R:x', 'S:x:1' ) ),
	);
	foreach ( $classes as $case ) {
		list( $html, $codes ) = $case;
		$ops = array( op( 'N', null, null ) );
		foreach ( $codes as $code ) {
			$parts = explode( ':', $code );
			$ops[] = op( ...$parts );
		}
		$ops[]   = op( 'L' );
		$cases[] = array( $html, $ops );
	}
	// Duplicates, insert order, numeric names, repeated removals.
	$cases[] = array( '<div a=1 b a=2 A=3>x</div><p>', array( op( 'N', null, null ), op( 'R', 'a' ), op( 'R', 'a' ) ) );
	$cases[] = array( '<div a=1 b a=2 A=3>x</div><p>', array( op( 'N', null, null ), op( 'R', 'a' ), op( 'R', 'a' ), op( 'G', 'b' ), op( 'N', null, null ), op( 'T' ) ) );
	$cases[] = array( '<div a=1 b a=2 A=3>x</div><p>', array( op( 'N', null, null ), op( 'S', 'a', 'z' ) ) );
	$cases[] = array( '<div a=1 a=2>', array( op( 'N', null, null ), op( 'R', 'a' ), op( 'S', 'a', 'z' ) ) );
	$cases[] = array( '<div>x</div>', array( op( 'N', null, null ), op( 'S', 'zz', 'z' ), op( 'S', 'b', '2' ), op( 'B', 'a' ), op( 'A', 'k' ), op( 'S', 'b', '1' ) ) );
	$cases[] = array( '<div>', array( op( 'N', null, null ), op( 'S', 'b', '1' ), op( 'S', 'B', '2' ) ) );
	$cases[] = array( '<div b=0>', array( op( 'N', null, null ), op( 'R', 'b' ), op( 'G', 'b' ), op( 'R', 'b' ), op( 'R', 'c' ) ) );
	$cases[] = array( '<div>', array( op( 'N', null, null ), op( 'S', 'c', '1' ), op( 'R', 'c' ), op( 'G', 'c' ) ) );
	$cases[] = array( '<a 0=x 1=y>', array( op( 'N', null, null ), op( 'G', '0' ), op( 'R', '0' ), op( 'S', '1', 'z' ), op( 'G', '1' ) ) );
	$cases[] = array( '<a x=1 x=2><b y>', array( op( 'N', null, null ), op( 'R', 'x' ), op( 'N', null, null ), op( 'S', '0', 'q' ), op( 'S', '05', 'q' ), op( 'S', '-1', 'q' ), op( 'S', '-0', 'q' ) ) );
	$cases[] = array( '<a x=1 x=2 x=3><b y>', array( op( 'N', null, null ), op( 'R', 'x' ), op( 'N', null, null ), op( 'R', '1' ), op( 'G', '0' ), op( 'G', '1' ) ) );
	$cases[] = array( '<a s=1><b><c>', array( op( 'N', null, null ), op( 'S', 's', '2' ), op( 'S', 't', '3' ), op( 'N', null, null ), op( 'S', '0', 'q' ), op( 'N', null, null ), op( 'R', '1' ) ) );
	$cases[] = array( '<a>', array( op( 'N', null, null ), op( 'S', '9223372036854775806', 'q' ), op( 'S', '9223372036854775808', 'q' ), op( 'N', null, null ) ) );
	$cases[] = array( '<a><b x=1 x=2>', array( op( 'N', null, null ), op( 'S', '-5', 'q' ), op( 'N', null, null ), op( 'R', 'x' ), op( 'G', '-4' ) ) );
	$cases[] = array( '<a b=1111111111><p>', array( op( 'N', null, null ), op( 'R', 'b' ), op( 'N', null, null ), op( 'S', 'c', '1' ), op( 'U' ), op( 'T' ), op( 'G', 'c' ) ) );
	$cases[] = array( '<a x=1 x=2><p><i>', array( op( 'N', null, null ), op( 'R', 'x' ), op( 'R', 'x' ), op( 'N', null, null ), op( 'S', 'c', '1' ), op( 'U' ), op( 'T' ), op( 'N', null, null ), op( 'T' ) ) );
	// Queries.
	$doc = '<div class="a"><P class="b c"></p><p class="c"><br><SPAN class=c></div>';
	foreach ( array( array( 'p', null ), array( 'P', null ), array( null, 'c' ), array( 'p', 'c' ), array( 'span', 'c' ), array( 'div', 'b' ), array( 'x', null ), array( null, 'C' ), array( '', null ), array( null, '' ) ) as $query ) {
		$cases[] = array( $doc, array( op( 'N', $query[0], $query[1] ), op( 'T' ), op( 'N', $query[0], $query[1] ), op( 'T' ), op( 'N', $query[0], $query[1] ), op( 'T' ) ) );
	}
	$cases[] = array( "<d\0v>", array( op( 'N', "d\0v", null ), op( 'N', null, null ) ) );
	$cases[] = array( "<d\0v>", array( op( 'N', "d\u{FFFD}v", null ) ) );
	// Edits before, at and after the end.
	$cases[] = array( '<p><i>', array( op( 'N', null, null ), op( 'S', 'a', '1' ), op( 'N', null, null ), op( 'N', null, null ), op( 'S', 'b', '1' ), op( 'G', 'a' ), op( 'T' ) ) );
	$cases[] = array( '<p><i', array( op( 'N', null, null ), op( 'A', 'x' ), op( 'N', null, null ), op( 'T' ), op( 'U' ), op( 'N', null, null ) ) );
	$cases[] = array( '<p>', array( op( 'N', null, null ), op( 'A', 'x' ), op( 'U' ), op( 'A', 'y' ), op( 'U' ), op( 'L' ), op( 'N', null, null ), op( 'A', 'z' ) ) );
	$cases[] = array( '<p><script>a</script><i>', array( op( 'N', null, null ), op( 'S', 'a', '1' ), op( 'N', null, null ), op( 'S', 'b', '1' ), op( 'U' ), op( 'T' ), op( 'N', null, null ), op( 'T' ) ) );
	// URLs.
	foreach ( url_cases() as $url ) {
		$cases[] = array( '<a href="x">', array( op( 'N', null, null ), op( 'S', 'href', $url ), op( 'G', 'href' ) ) );
	}
	foreach ( array( 'action', 'archive', 'background', 'cite', 'classid', 'codebase', 'data', 'formaction', 'href', 'HREF', 'icon', 'longdesc', 'manifest', 'poster', 'profile', 'src', 'usemap', 'xmlns', 'data-href', 'style' ) as $name ) {
		$cases[] = array( '<a>', array( op( 'N', null, null ), op( 'S', $name, "javascript:x'&" ), op( 'S', $name, "a b'&" ) ) );
	}
	// Many pending edits.
	foreach ( array( 999, 1000, 1001, 1002 ) as $count ) {
		$html = str_repeat( '<a>', $count ) . '<b><c><script>x</script><d>';
		$ops  = array();
		for ( $i = 0; $i < $count; $i++ ) {
			$ops[] = op( 'N', null, null );
			$ops[] = op( 'S', 'x', (string) $i );
		}
		foreach ( array( '3', '1000', '1001' ) as $name ) {
			$ops[] = op( 'N', null, null );
			$ops[] = op( 'S', $name, 'q' );
		}
		$ops[]   = op( 'N', null, null );
		$ops[]   = op( 'S', 'z', 'q' );
		$cases[] = array( $html, $ops );
	}
	// Bookmarks: set on tags and other tokens, moved by edits before and inside
	// them, released, sought back to after edits, and their limits.
	$n = op( 'N', null, null );
	$t = op( 'T' );
	$bookmarks = array(
		array( '<a><b><c>', array( $n, op( 'K', 'a' ), $n, op( 'K', 'b' ), $n, op( 'J', 'a' ), $t, op( 'J', 'b' ), $t, op( 'Y', 'a' ), op( 'X', 'a' ), op( 'Y', 'a' ), op( 'X', 'a' ), op( 'J', 'a' ), $t ) ),
		array( '<a><b><c>', array( $n, op( 'K', 'a' ), op( 'S', 'x', 'long value' ), $n, op( 'K', 'b' ), op( 'S', 'y', '1' ), $n, op( 'J', 'a' ), $t, op( 'G', 'x' ), op( 'J', 'b' ), op( 'G', 'y' ), $n, $t ) ),
		array( '<a x="1"><b y="2">', array( $n, op( 'R', 'x' ), $n, op( 'K', 'b' ), op( 'R', 'y' ), op( 'J', 'b' ), op( 'G', 'y' ) ) ),
		array( '<a><b>', array( $n, op( 'J', 'nope' ), op( 'K', 'a' ), op( 'J', 'a' ), $t ) ),
		array( '<a><b>', array( $n, $n, $n, op( 'K', 'end' ), op( 'Y', 'end' ) ) ),
		array( '<a class="x"><b>', array( $n, op( 'K', 'a' ), op( 'A', 'yy' ), op( 'D', 'x' ), $n, op( 'J', 'a' ), op( 'L' ), $n, $t ) ),
		array( '<a><b><c><d><e><f><g><h><i><j><k><l>', array( $n, op( 'K', '1' ), op( 'K', '2' ), op( 'K', '3' ), op( 'K', '4' ), op( 'K', '5' ), op( 'K', '6' ), op( 'K', '7' ), op( 'K', '8' ), op( 'K', '9' ), op( 'K', '10' ), op( 'K', '11' ), op( 'K', '1' ), op( 'X', '5' ), op( 'K', '11' ) ) ),
		array( '<a>x<script>y</script><!--z--><b>', array( $n, op( 'K', 'a' ), op( 'N', 'b', null ), op( 'K', 'b' ), op( 'J', 'a' ), op( 'N', 'b', null ), op( 'J', 'a' ), op( 'S', 'q', 'w' ), op( 'J', 'b' ), $t ) ),
	);
	// Replacements around a bookmark on <b> while the cursor is on <d>, as edits
	// make them: whole tokens and the spans between them, and edits inside a
	// tag; before, over, around and after it, then again after a flush.
	$ranges = array( array( 0, 3, '<aa>' ), array( 0, 0, 'x' ), array( 3, 3, '' ), array( 0, 6, '' ), array( 3, 6, 'x' ), array( 4, 1, 'q' ), array( 5, 0, ' y' ), array( 6, 3, '<cc>' ), array( 6, 0, '<z>' ), array( 3, 0, '<y>' ), array( 9, 3, '' ) );
	foreach ( $ranges as $first ) {
		foreach ( $ranges as $second ) {
			$cases[] = array(
				'<a><b><c><d>',
				array( $n, $n, op( 'K', 'b' ), $n, $n, op( 'Z', (string) $first[0], (string) $first[1], $first[2] ), op( 'U' ), op( 'Y', 'b' ), op( 'Z', (string) $second[0], (string) $second[1], $second[2] ), op( 'J', 'b' ), $t, op( 'Y', 'b' ) ),
			);
		}
	}
	$many = array( $n, op( 'K', 'a' ), $n, op( 'K', 'b' ) );
	for ( $i = 0; $i < 1002; $i++ ) {
		$many[] = op( 'J', 0 === $i % 2 ? 'a' : 'b' );
	}
	$many[]      = $t;
	$bookmarks[] = array( '<a><b>', $many );
	foreach ( $bookmarks as $case ) {
		$cases[] = $case;
	}
	return $cases;
}

function url_cases() {
	$urls  = array(
		'', ' ', 'http://example.com/', 'https://example.com/a?b=c&d=e', ' http://x', "\t\n http://x", 'example.com', 'example.com/path',
		'index.php', 'wp-login.php?x', 'a-b.PHP', '.php', 'a.phpx', 'a_b.php', '/relative', '//proto.rel/x', '#frag', '?query',
		'javascript:alert(1)', 'JavaScript:x', 'java script:x', 'jav&#x09;ascript:x', '&#106;avascript:x', 'mailto:a@b.c',
		'MAILTO:a%0a@b', 'mailto:x%0D%0A', 'http://x/%0a%0A%0d%0D', 'http://x/%%0a0a', '%0a', 'http://x/a b', 'http://x/<b>',
		'http://x/"q"', "http://x/'q'", 'http://x/?a[1]=2', 'http://x/?a[]=1&b[]=2', 'http://[::1]/a[b]', 'http://[::1]:8080/a',
		'http://u:p@host:80/a[1]', 'http://u@host/[x]', 'http://@host/[x]', '//host:80/[x]', '/[x]', '[x]', '/a[b]/c',
		'http://host:65536/[x]', 'http://host:0/[x]', 'http://host:+8/[x]', 'http://host:-0/[x]', 'http://host:-1/[x]',
		'http://host:8a/[x]', 'http://host:123456/[x]', 'http://host:/[x]', 'http://:80/[x]', 'a:80/[x]', 'a:65536/[x]', ':80/[x]',
		'x:/[y]', 'x://[y]', 'file:///[c]', 'file:///c:/[d]', 'FILE:///[c]', 'a+b.c-d:x[1]', 'a_b:x[1]', 'a_b:[1]', 'a_b?:x[1]',
		'//[x]', '//', 'a:', 'a:[', 'http:[x]', 'http:/[x]', 'h[t]tp://x', '[]:80', 'x]y', 'tel:+1-555', 'sms:1', 'feed:http://x',
		'feed:feed:http://x', 'feed:feed:feed:http://x', 'feed:javascript:x', 'feed:0', 'feed:', 'urn:x', 'svn+ssh://x', 'ftp://x',
		'data:text/html,x', 'x&#58y', 'x&#058;y', 'x&#x3a;y', 'x&#X3A;y', 'x&#x03ay', 'x&#58;y', 'x&colon;y', 'x&COLON;y', 'x&#5;8',
		'x&#58', 'x&#x3a', 'x&#x3aa:y', 'x&#585:y', 'x/?a:b', 'a/?:b', '&#x6a;avascript:x', '&#0000106;avascript:x', 'h&#116;tp:x',
		'&#9223372036854775807;:x', '&#9223372036854775808;:x', '&#x7fffffffffffffff;:x',
		'http://x/&amp;', 'http://x/&nbsp;', 'http://x/&nosuch;', 'http://x/&#39;', 'http://x/&#039;', 'http://x/&#0;',
		'http://x/&#1;', 'http://x/&#9;', 'http://x/&#65;', 'http://x/&#00065;', 'http://x/&#1234567;', 'http://x/&#12345678;',
		'http://x/&#55296;', 'http://x/&#x41;', 'http://x/&#X41;', 'http://x/&#x0041;', 'http://x/&#xD800;', 'http://x/&#x110000;',
		'http://x/&#x10FFFF;', 'http://x/&#xFFFFFFF;', 'http://x/&ab;', 'http://x/&abcdefgh;', 'http://x/&abcdefghi;',
		'http://x/&sup2;', 'http://x/&sup23;', 'http://x/&a12;', 'http://x/&a123;', 'http://x/&;', 'http://x/&#;', 'http://x/&#x;',
		'http://x/é', "http://x/\x80\xff", 'http://x/\\0', "a\\0:b", "&#92;0x:y", 'h&#0;ttp:x', 'ht&#32;tp:x', 'ht&#10;tp:x',
		"http\x00://x", 'http;//x', 'x;//y', 'mailto;//x', '\\\\x', 'a|b', '$x', '!x', '*x', '(x)', ',x', '~x', '+x', '@x',
		'%', 'http://x?a#b[c]', 'https:[x]', 'https://x[1]', 'HTTP://X/[1]', 'irc6://[x]', 'webcal:x', 'xmpp:x', 'gopher:x',
	);
	$atoms = array(
		'http', 'https', 'feed', 'javascript', 'mailto', 'x', ':', '://', '//', '/', '?', '#', '[', ']', '&', '&amp;', '&#58;',
		'&#x3a', '&colon;', '&#106;', ';', ' ', "\t", '%0a', '%0D', '@', 'u:p@', 'host', ':80', ':65536', '.php', "'", '"', '<',
		'\\', '0', "\x01", 'é', '-', '+', '&nbsp;', '&#039;', '&#', 'x', '=', '1',
	);
	for ( $i = 0; $i < 1500; $i++ ) {
		$url = '';
		$n   = mt_rand( 1, 8 );
		for ( $j = 0; $j < $n; $j++ ) {
			$url .= pick( $atoms );
		}
		$urls[] = $url;
	}
	return $urls;
}

function theme_cases( $theme ) {
	$documents = array();
	foreach ( json_decode( file_get_contents( "$theme/patterns.json" ), true ) as $pattern ) {
		$documents[] = $pattern['content'];
	}
	foreach ( array( 'templates', 'parts' ) as $folder ) {
		foreach ( glob( "$theme/$folder/*.html" ) as $file ) {
			$documents[] = file_get_contents( $file );
		}
	}
	$cases = array();
	foreach ( $documents as $html ) {
		$cases[] = array( $html, array( op( 'W' ) ) );
		// What the layout and spacing supports do to a block's wrapper.
		$cases[] = array(
			$html,
			array(
				op( 'N', null, null ),
				op( 'T' ),
				op( 'A', 'is-layout-constrained' ),
				op( 'A', 'wp-container-core-group-is-layout-1' ),
				op( 'S', 'style', 'margin-top:0;padding:var(--wp--preset--spacing--50)' ),
				op( 'G', 'class' ),
				op( 'L' ),
				op( 'N', null, 'wp-block-button__link' ),
				op( 'G', 'href' ),
				op( 'S', 'href', 'https://example.com/?a[0]=1&b=2' ),
				op( 'N', 'img', null ),
				op( 'G', 'src' ),
				op( 'R', 'alt' ),
			),
		);
	}
	return $cases;
}

function random_cases() {
	$atoms = array(
		'<div>', '<div class="a b">', '<p class=c>', '<P CLASS="A">', '<span id=x data-v=\'1\'>', '</div>', '</p>', '<br/>',
		'<img src=x alt="">', '<a href="/x" a=1 A=2>', '<i =x>', '<b c d c>', '<!-- c -->', '<!---->', '<!DOCTYPE html>', '<?x y?>',
		'</ x>', '<script>s</script>', '<style>a</style>', '<title>t</title>', '<textarea>t</textarea>', 'text', ' ', "\n", '<',
		'>', '"', "'", '=', '/', '&amp;', '&#65;', '<![CDATA[x]]>', '<div x="&lt;">', '<em class="a  a b">', "<u\0>", '<x-y z>',
	);
	$names   = array( 'class', 'id', 'ID', 'data-v', 'href', 'src', 'style', 'a', 'A', 'c', 'x', '0', '1', '5', 'z', '', 'a b', "\x01" );
	$classes = array( 'a', 'b', 'c', 'A', 'B', 'x', '1', '0', 'wp-block', 'a b', '', '"' );
	$values  = array( '', '1', 'v', 'a b', '<&>"\'', '&amp;', 'javascript:x', 'http://x/?a[1]', '/y', '0', 'é' );
	$cases   = array();
	for ( $i = 0; $i < 3000; $i++ ) {
		$html = '';
		$n    = mt_rand( 1, 10 );
		for ( $j = 0; $j < $n; $j++ ) {
			$html .= pick( $atoms );
		}
		$ops = array();
		$n   = mt_rand( 1, 14 );
		for ( $j = 0; $j < $n; $j++ ) {
			switch ( mt_rand( 0, 13 ) ) {
				case 0:
				case 1:
				case 2:
					$ops[] = op( 'N', null, null );
					break;
				case 3:
					$ops[] = op( 'N', mt_rand( 0, 1 ) ? pick( array( 'div', 'P', 'span', 'x', 'script' ) ) : null, mt_rand( 0, 1 ) ? pick( $classes ) : null );
					break;
				case 4:
				case 5:
					$ops[] = op( 'S', pick( $names ), pick( $values ) );
					break;
				case 6:
					$ops[] = op( 'B', pick( $names ) );
					break;
				case 7:
					$ops[] = op( 'R', pick( $names ) );
					break;
				case 8:
					$ops[] = op( 'A', pick( $classes ) );
					break;
				case 9:
					$ops[] = op( 'D', pick( $classes ) );
					break;
				case 10:
					$ops[] = op( 'G', pick( $names ) );
					break;
				case 11:
					$ops[] = mt_rand( 0, 1 ) ? op( 'H', pick( $classes ) ) : op( 'L' );
					break;
				case 12:
					$ops[] = op( 'T' );
					break;
				default:
					$ops[] = op( 'U' );
			}
		}
		$cases[] = array( $html, $ops );
	}
	return $cases;
}

switch ( $argv[2] ) {
	case 'edge':
		$cases = edge_cases();
		break;
	case 'theme':
		$cases = theme_cases( $argv[3] );
		break;
	default:
		$cases = random_cases();
}
foreach ( $cases as $case ) {
	emit( $case[0], $case[1] );
}

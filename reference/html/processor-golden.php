<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for hyscribe-html's HtmlProcessor: WP_HTML_Processor::create_fragment()
// walked with next_token(). One case per line:
//   hex(html) <TAB> tokens <TAB> error
// Tokens are comma-separated, each dot-separated:
//   type name closer virtual namespace depth breadcrumbs text
// with type, name, namespace, breadcrumbs (joined by /) and text in hex, ~ for
// null. The error is ~, or the last error and the unsupported message in hex. A
// case over 4096 bytes is its HTML's sha256 hash, count:<tokens> and its error.
//   php processor-golden.php <wordpress> > crates/hyscribe-html/tests/golden/processor.tsv

ini_set( 'display_errors', 'stderr' );
error_reporting( E_ALL & ~E_WARNING & ~E_NOTICE );
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
	'html-api/class-wp-html-doctype-info',
	'html-api/class-wp-html-open-elements',
	'html-api/class-wp-html-token',
	'html-api/class-wp-html-stack-event',
	'html-api/class-wp-html-processor-state',
	'html-api/class-wp-html-unsupported-exception',
	'html-api/class-wp-html-active-formatting-elements',
	'html-api/class-wp-html-processor',
) as $file ) {
	require_once ABSPATH . WPINC . "/$file.php";
}

function arg( $value ) {
	return null === $value ? '~' : bin2hex( $value );
}

$is_virtual = new ReflectionMethod( 'WP_HTML_Processor', 'is_virtual' );

function walk( $html ) {
	global $is_virtual;
	$p      = WP_HTML_Processor::create_fragment( $html );
	$tokens = array();
	// A long case counts its tokens: what it checks is where the bookmarks run out.
	if ( strlen( $html ) > 4096 ) {
		$count = 0;
		while ( $p->next_token() ) {
			++$count;
		}
		$error = $p->get_last_error();
		echo 'sha256:', hash( 'sha256', $html ), "\tcount:", $count, "\t", $error ? $error . ':' : '~', "\n";
		return;
	}
	while ( $p->next_token() ) {
		$tokens[] = implode(
			'.',
			array(
				arg( $p->get_token_type() ),
				arg( $p->get_token_name() ),
				$p->is_tag_closer() ? '1' : '0',
				$is_virtual->invoke( $p ) ? '1' : '0',
				arg( $p->get_namespace() ),
				$p->get_current_depth(),
				arg( implode( '/', $p->get_breadcrumbs() ) ),
				arg( $p->get_modifiable_text() ),
			)
		);
	}
	$error = $p->get_last_error();
	if ( null !== $error ) {
		$exception = $p->get_unsupported_exception();
		$error    .= ':' . bin2hex( $exception ? $exception->getMessage() : '' );
	}
	echo bin2hex( $html ), "\t", implode( ',', $tokens ), "\t", $error ?? '~', "\n";
}

$cases = array(
	'', 'text', "  \n ", '<p>a', '<p>a</p>b', '<p><p>x', '<div><p>a</div>b', '</p>', '</div>', '</x>', '<x-y>a</x-y>',
	'<b>bold</b>', '<b><i>x</i></b>', '<b><i>x</b></i>', '<p><b>x</p>y', '<a href=1>x<a>y</a>', '<b>x', '<nobr>a<nobr>b',
	'<ul><li>a<li>b</ul>', '<dl><dt>a<dd>b<dt>c</dl>', '<li>a<div><li>b', '<li>a<p><li>b', '</li>', '</dd>',
	'<h1>a<h2>b</h1>c', '</h3>', '<h1><p>x</h1>', "<pre>\nx</pre>", '<listing>a</listing>',
	'<form><form>x</form></form>', '<form><div></form>x', '</form>', '<button><button>x', '<button><p>x</button>y',
	'<br>', '<br/>x', '<img src=a><hr><wbr>', '<input type=hidden><input>', '<param><source><track>', '<embed><area><keygen>',
	"<textarea>\nx</textarea>y", '<title>t</title>', '<script>s</script>x', '<style>s</style>', '<xmp>x</xmp>', '<iframe>i</iframe>', '<noembed>n</noembed>',
	'<select><option>a<option>b</select>', '<select><optgroup><option>a</select>', '<option>a<option>b', '<optgroup>a<optgroup>b',
	'<ruby>a<rb>b<rt>c<rp>d<rtc>e</ruby>', '<rt>x', '<object><b>x</object>', '<marquee>m</marquee>', '<applet>a</applet>',
	'<!-- c -->x', '<?php x ?>', '</3>', '</>', '<!DOCTYPE html>x', '<html lang=x><body class=y>z', '<body><body>x', '</body>x', '</html>x', '</body><!--c-->', '</body> ', '</html> ', '</html><!--x-->',
	'<frameset>', '<div><frameset>', '<plaintext>x', '<image src=a>', '<template>t</template>', '<head>x', '<meta charset=x>a', '<link><base><basefont><bgsound>',
	'<noscript>x</noscript>', '<caption>x', '<td>x', '<tr><th>x', '<col><colgroup><frame>',
	'<b><p>x</b>y', '<a><div></a>', '<b></b></b>', '<i>a</b>b', '<em><strong>x</em>', "a\x00b", "\x00", "<p>\x00x",
	'<div><span>x</div>y', '<span><div>x</span>y', '<a><b><c>x</a>', '<summary>s</summary>', '<search>x</search>',
	'<details><summary>a</summary>b</details>', '<p><table>', '<math>m</math>', '<svg/>x', '<math/>x',
);
$cases = array_merge(
	$cases,
	array(
		'<table><tr><td>a</td></tr></table>', '<table><td>a<td>b</table>x', '<table><tr><th>h</table>', '<table><caption>c</caption><tr><td>x</table>',
		'<table><colgroup><col><col></colgroup></table>', '<table><col><tr></table>', '<table><thead><tr><td>a</thead><tbody><tr><td>b</tbody><tfoot></table>',
		'<table>x</table>', '<table> </table>', "<table>\x00</table>", '<table><!--c--></table>', '<table><input type=hidden><input type=x></table>',
		'<table><form>x</table>', '<table><style>s</style><script>t</script></table>', '<table><template>t</template></table>', '<table><table>', '</table>',
		'<table><tbody></tr></td></th></caption></col></colgroup></body></html></tbody></table>', '<table><tr><td><table><tr><td>x</table>y</table>',
		'<table><td><b>x</td>y', '<table><caption><b>c</table>', '<table><caption><p>x</caption>y', '<table><caption></body>x</table>',
		'<table><colgroup>x</colgroup>', '<table><colgroup><template></template></colgroup>', '<table><colgroup></col></colgroup>', '<table><colgroup></colgroup></colgroup>',
		'<table><tr><td>a</tr>b</table>', '<table><tr></tbody></table>', '<table><thead><caption></thead>', '<table><tr><caption>', '<table><td></tbody></table>',
		'<table><td></table>x', '<table><td><th>x', '<table><td></tr>', '<table><td></th>', '<table><td></body>', '<td>x</td>', '<tr><td>x',
		'<template><td>x</td></template>', '<template><tr><td>a</template>', '<template><col></template>', '<template><caption>c</template>', '<template><div>d</div></template>',
		'<template><template>x</template></template>', '</template>', '<template>', '<template><!--c--></template>', '<template></div></template>', '<template><style></style></template>',
		'<div><template><p></div></template>x', '<form><template><form></template></form>', '<template><form></form></template>',
		'<svg><circle/><g><rect></rect></g></svg>', '<svg><title>t</title><desc><b>x</b></desc></svg>', '<svg><foreignObject><p>x</foreignObject></svg>y',
		'<svg><p>x</svg>', '<svg><b>x</b></svg>', '<svg><font color=red>x</font></svg>', '<svg><font>x</font></svg>', '<svg></br></svg>', '<svg></p></svg>',
		'<svg><![CDATA[x]]></svg>', '<svg><![CDATA[ ]]></svg>', '<svg><!--c--><?pi?></svg>', '<svg><script>s</script></svg>', '<svg></svg></svg>', '<svg><g></G></svg>',
		'<svg><image href=a/><image></image></svg>', '<svg><style>a</style></svg>', '<svg><textarea>x</textarea></svg>', '<svg><title><svg><path/></svg></title></svg>',
		'<math><mi>x</mi><mo>+</mo><mn>1</mn></math>', '<math><mi><b>x</b></mi></math>', '<math><mtext><mglyph/><malignmark/></mtext></math>',
		'<math><annotation-xml encoding="text/html"><p>x</annotation-xml></math>', '<math><annotation-xml encoding="application/xhtml+xml"><div>d</div></annotation-xml></math>',
		'<math><annotation-xml><svg><path/></svg></annotation-xml></math>', '<math><annotation-xml encoding=x><p>x</p></annotation-xml></math>', '<math><ms>s</ms><mi>i</math>',
		'<math></math></math>', '<table><svg><g></svg></table>', '<p><svg><p>x', '<svg><table>', '<div><svg><div>x</div></svg></div>', '<b><svg></b></svg>',
		'<a><p><a>x', '<b>1<p>2</b>3', '<i><b>x</i>y', '<b><b><b>x</b></b></b>', '<u><div></u>x', '<a>1<div>2</a>3', '<nobr><nobr>x', '<font><p>x</font>',
		'<b><table><td></b>x</table>', '<em>x<em>y</em>z</em>', '<b><template><b>x</b></template></b>', '<s>a<p>b</s>c',
		'<p><b></p><nobr>', '<nobr>a<nobr>b</nobr>', '<p><b></p><object>', '<p><b></p><xmp>', '<p><b></p><select>', '<p><b></p><svg>', '<b><svg><foreignObject></b>',
		'<b><b><b><b><b>x</b>', '<a><svg><foreignObject><a></a>', '<table><!DOCTYPE html>', '<template><col></table>', '<template><tr></table>', '<template><td></table>',
		'<table><form><form>', '<template><table><form>', '<table><caption><template><td>', '<table><caption><svg><foreignObject><table></table></caption>',
		'<table><colgroup><!--x-->', '<table><colgroup><!DOCTYPE html>', '<table><colgroup></col>', '<table><colgroup><html>', '<template><col></colgroup>', '<template><col><div>',
		'<table><tbody></thead>', '<template><tr></tr><caption>', '<table><tbody><tr></thead>', '<template><td></td></tr>', '<template><td></td><caption>', '<table><tr></td>',
		'<table><tr><td></thead>', '<table><tr><td><div></tr>', '<table><tr><td><svg></tr>', '<table><caption><template></template><td>', '<table><colgroup><template><td>',
		'<table><tr><td><svg><p>', '<table><caption><svg><p>', '<table><tr><td><table></table>x', '<table><tbody><template></template>', '<table><tr><template></template>',
		'<template><tbody></table>', '<template><caption></caption></table>', '<template><colgroup></table>', '<p><b></p><xmp>x</xmp>', '<nobr><div><nobr>', '<nobr><b><nobr>', '<svg><!DOCTYPE html>',
		'<p></p><div>x', '<p><button></button><div>x', '<dd><ul></dd>x', '<li><button></li>x', '<p><b></p><param>', '<p><b></p><textarea>x</textarea>', '<p><b></p><iframe>x</iframe>',
		'<p>a<xmp>x</xmp>', '<b><object></b>x', '<table><tr><template></template><td>x', '<table><tbody><template></template><tr>', '<table><template></template><tr>', '<template><col><template></template><col>',
	)
);
foreach ( $cases as $html ) {
	walk( $html );
}
// The tag processor's edge strings.
require __DIR__ . '/processor-edge-strings.php';
foreach ( processor_edge_strings() as $html ) {
	walk( $html );
}
// Random fragments from atoms, with a fixed seed.
mt_srand( 20261112 );
$atoms = array(
	'<p>', '</p>', '<div>', '</div>', '<b>', '</b>', '<i>', '</i>', '<a href=x>', '</a>', '<table>', '</table>', '<tr>', '</tr>', '<td>', '</td>', '<th>',
	'<tbody>', '</tbody>', '<caption>', '</caption>', '<col>', '<colgroup>', '<template>', '</template>', '<li>', '</li>', '<ul>', '</ul>', '<dd>', '<dt>',
	'<h1>', '</h2>', '<span>', '</span>', '<svg>', '</svg>', '<math>', '</math>', '<mi>', '<foreignObject>', '<desc>', '<g>', '</g>', '<br>', '</br>',
	'<img>', '<hr>', '<input>', '<select>', '<option>', '</select>', '<form>', '</form>', '<button>', '</button>', '<pre>', '<textarea>t</textarea>',
	'<!--c-->', 'text', ' ', "\x00", '<![CDATA[c]]>', '<font color=1>', '<nobr>', '</nobr>', '<ruby>', '<rt>', '<body>', '</body>', '</html>', '<html>',
	'<frameset>', '<head>', '<style>s</style>', '<script>s</script>', '<title>t</title>', '<x-y>', '</x-y>', '&amp;', '<?pi?>', '</3>', '<object>', '</object>',
);
for ( $i = 0; $i < 2000; $i++ ) {
	$html = '';
	$n    = mt_rand( 1, 12 );
	for ( $j = 0; $j < $n; $j++ ) {
		$html .= $atoms[ mt_rand( 0, count( $atoms ) - 1 ) ];
	}
	walk( $html );
}
// More open elements than bookmarks.
walk( str_repeat( '<span>', 10001 ) );
walk( str_repeat( '<span>x</span>', 6000 ) );
// Virtual table sections, which keep their bookmarks.
walk( str_repeat( '<table><td>', 10001 ) );
walk( str_repeat( '<table><colgroup><td></td>', 10001 ) );
walk( str_repeat( '<table><tbody><col>', 10001 ) );

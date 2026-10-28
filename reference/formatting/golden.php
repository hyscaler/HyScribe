<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for hyscribe-render's wptexturize() and convert_smilies(). Prints
// "hex(input)<TAB>hex(output)" lines:
//   php golden.php texturize > crates/hyscribe-render/tests/golden/wptexturize.tsv
//   php golden.php smilies   > crates/hyscribe-render/tests/golden/convert-smilies.tsv
// and likewise balance, autop and clickable (golden.sh lists the files).
// Runs inside the default install's PHP container (reference/formatting/golden.sh),
// so the shortcodes, options and post contents are the fixture's. Only pure
// functions run and the database is only read. Random cases use a fixed seed,
// so the output is stable.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

mt_srand( 20261027 );

// The fixture registers embed, wp_caption, caption, gallery, playlist, audio
// and video. A `code` shortcode is added so that the no-texturize shortcode
// default is exercised too; it is never rendered.
add_shortcode( 'code', '__return_empty_string' );

function emit( $input, $output ) {
	echo bin2hex( $input ), "\t", bin2hex( $output ), "\n";
}

function random_cases( $atoms, $count, $max ) {
	$cases = array();
	for ( $i = 0; $i < $count; $i++ ) {
		$doc = '';
		$n   = mt_rand( 1, $max );
		for ( $j = 0; $j < $n; $j++ ) {
			$doc .= $atoms[ mt_rand( 0, count( $atoms ) - 1 ) ];
		}
		$cases[] = $doc;
	}
	return $cases;
}

function post_contents() {
	global $wpdb;
	return $wpdb->get_col( "SELECT post_content FROM $wpdb->posts ORDER BY ID" );
}

function texturize_cases() {
	$cases = array(
		'', '0', ' ', "\n", 'plain text',
		// Quotes and apostrophes.
		'"Hello," she said.', "'Single,' he said.", "It's John's.", "'cause 'tis 'twas 'em 'emma",
		"'tain't 'twere 'twill 'til 'bout 'nuff 'round", "don't 'quote' me", "rock 'n' roll",
		'"quoted"', "'quoted'", '"a" "b"', "'a' 'b'", '("paren")', '["bracket"]', '{"brace"}',
		"-'dash'", '-"dash"', '&lt;"lt"&gt;', "&lt;'lt'&gt;", "x'", "'x", "'", '"', "''", '``', "``a''",
		"'99 '99s '99's '9 '99% '999 '99.0 '99,5 '99.", "'99' '99\" '99'. '99\"!", "'42' '0.42' \"42\" \"4,2.\"",
		"in '99 and '07", "the '90s", "x '12' y", 'x "12" y', "'12'x", '"12"x',
		"9' 9\" 9'' 5'10\" 6' tall", "\"It's 9' long,\" he said.", "'It's 9' long,' he said.",
		"\"a 9\" b 7\".\"", "\"5\" and 6\"", "\"x\" y\" z\".", "'a' b' c'.", "\"a\" b\"", "\"one 2\" three 4\"",
		"'one 2' three 4'.", "\"a 2\" b 3\" c\"", "\"x\ny\"", "'x\ny'", "\"x\"\n", "'x'\n",
		"\"\xC2\xA0x\"", "'\xC2\xA0x'", "&nbsp;'x'", '&nbsp;"x"', "a&nbsp;'b", "\t'x'\t", "\r\"x\"\r",
		"\"x\"&gt;", "'x'&gt;", "'x'&lt;", "\"x\"]", "'x')", "'x'-", "'x'?", "\"x\"!", "\"x\";",
		"'x\"y'", "\"x'y\"", "\"'x'\"", "'\"x\"'", "\"\"x\"\"", "a\"b\"c", "a'b'c", "a' b' c'",
		// Dashes, ellipses, trademark, times.
		'a - b', 'a -- b', 'a --- b', 'a--b', 'a---b', 'a----b', 'a-----b', 'xn--abc', 'xn---a', '--', '-', '---',
		"-\n", "\n-", "--\n", " --", "-- ", "\xC2\xA0-\xC2\xA0", '&nbsp;--&nbsp;', 'a-b', '-a', 'a-',
		'Wait...', '....', 'Name (tm)', 'Name(tm)', ' (tm) (tm)',
		'9x9', '10x10', '1.5x2.5', '0x9999', '0x1', '00x1', '1x2x3', 'a9x9', '9x9a', '9x9.', '9.x9', '1.x2',
		'1.x2 and 3x4', '1,x2', '9x9,5', '9x.9', 'x9', '9x', '9 x 9', '12x34x56', '0.5x0.5', '0,0x1', '9x9_',
		'_9x9', '9x9x', 'x9x9', '9X9', '1x2.,3', '1x2.,', '1x2,.a', '01.x2',
		// Ampersands.
		'a & b', '&amp;', '&AMP;', '&#038;', '&#x26;', '&#X26;', '&#xZZ;', '&#;', '&#x;', '&abcdefgh;',
		'&abcdefghi;', '&a1;', '&a5;', '&;', '&', '&&amp;', '<a href="x&y">&</a>', '<a title="&amp;&x">',
		// HTML.
		'<p>"a"</p>', '<p class="x">\'b\'</p>', '<a href="it\'s">it\'s</a>', '<img alt="\'x\'" />',
		'<!-- "comment" -->"after"', '<!-- unclosed "x"', '<!-->"x"', '<!--->"x"', '<!---->"x"',
		'<!-- a -- b -->"x"', '<!-- a --->"x"', '<!x>"y"', '<![CDATA["x"]]>"y"', '<', '<"x"', 'a < b',
		'a > b', '"a" > "b"', '<p', '<p "x"', '< p>"x"', '</p>"x"', '<>"x"', '</>"x"', '<<p>>"x"',
		// No-texturize elements.
		'<pre>"x"</pre>"y"', '<code>"x"</code>"y"', '<kbd>"x"</kbd>', '<style>"x"</style>', '<script>"x"</script>',
		'<tt>"x"</tt>', '<pre class="a">"x"</pre>"y"', '<PRE>"x"</PRE>', '<prex>"x"</prex>', '<pr>"x"</pr>',
		'<pre>"x"', '</pre>"x"', '<pre><code>"x"</code>"y"</pre>"z"', '<pre>"a"</code>"b"</pre>"c"',
		'<pre><pre>"x"</pre>"y"</pre>"z"', '<code>"a"</pre>"b"</code>"c"', "<pre\n>\"x\"</pre>",
		'<pre>"x"</pre >"y"', '<pre>"x"</ pre>"y"', '<pre/>"x"', '<pre />"x"', '<pre', '<pre "x"',
		'<code>"x"<code>"y"</code>"z"</code>"w"', '<pre>&</pre>', '<pre><a href="&">x</a></pre>',
		// Shortcodes.
		'[caption]"x"[/caption]', '[caption id="1"]"x"[/caption]', '[gallery ids="1,2"]', '["x"]',
		'[[caption]]"x"', '[[caption]"x"[/caption]]', '[caption]]"x"', '[caption/]"x"', '[caption /]"x"',
		'[captionx]"x"', '[caption=1]"x"', '[caption <b>"x"</b>]"y"', '[caption "x" <b]"y"', '[caption',
		'[caption "x"', '[/caption]"x"', '[code]"x"[/code]"y"', '[code lang="php"]"x"[/code]"y"',
		'[code]"a"[code]"b"[/code]"c"[/code]"d"', '[[code]]"x"', '[code]]"x"[/code]', '[/code]"x"',
		'[code]<pre>"a"[/code]"b"</pre>"c"', '<pre>[code]"a"</pre>"b"[/code]"c"', '[code "x"]"y"',
		"[code\t]\"x\"[/code]", "[code\n]\"x\"[/code]", "[code\x0b]\"x\"[/code]", '[audio src="a.mp3"]',
		'[video]x[/video]', '[playlist]', '[embed]https://example.com[/embed]', '[wp_caption]"x"[/wp_caption]',
		'[unknown]"x"[/unknown]', '[caption>]"x"', '[caption [b]]"x"', '[caption <b>]', '[caption <b]',
		'[caption <b<c>]"x"', '[]"x"', '[/]"x"', '[[]]', '[&]', '[/caption"x"', '[//caption]"x"',
		'[[/caption]]"x"', '[/[caption]"x"', '[code/]"x"', '[/code "x"]"y"', '[code]"x"[/code ]"y"',
	);
	foreach ( post_contents() as $content ) {
		$cases[] = $content;
	}
	$atoms = array(
		"'", '"', "''", '``', '...', '-', '--', '---', ' ', "\n", "\t", "\xC2\xA0", '&nbsp;', '9', '0', '42',
		'x', 'xn', '.', ',', '(', ')', '[', ']', '{', '}', '!', '?', '%', '&', '&amp;', '&lt;', '&gt;', '&#8217;',
		'&#x27;', ' (tm)', "'tis", "'em", 'a', 'word', '<p>', '</p>', '<pre>', '</pre>', '<code>', '</code>',
		'<script>', '</script>', '<!--', '-->', '<!-- c -->', '<', '>', '<a href="x&y">', '[caption]',
		'[/caption]', '[[caption]]', '[code]', '[/code]', '[gallery ids="1"]', '[x]', '<br />',
	);
	return array_merge( $cases, random_cases( $atoms, 600, 14 ) );
}

function smilies_cases() {
	global $wpsmiliestrans;
	$cases = array(
		'', ' ', 'plain', ':)', ' :) ', 'a:)', ':)a', ':) :)', ':):)', ":)\n:(", "\n:)\n", "\t:)\t", "\r:)\r",
		"\xC2\xA0:)\xC2\xA0", '&nbsp;:)&nbsp;', '&amp;:)', ':)&nbsp', ':mrgreen:', 'x :mrgreen: y', ':???:', ':?:',
		':?', ':??', ':!:', '8-O', '8O', '8)', ':-)', ';-)', ';)', ':P', ':p', ':D', ':x', ':X',
		'<p>:)</p>', '<p> :) </p>', '<code>:)</code> :)', '<code class="x"> :) </code> :)', '<pre> :) </pre> :)',
		'<style> :) </style> :)', '<script> :) </script> :)', '<textarea> :) </textarea> :)', '<codex> :) </codex> :)',
		'<code> :) </codex> :) </code> :)', '<code> :) </CODE> :)', '<CODE> :) </CODE> :)', '<code><pre> :) </pre> :) </code> :)',
		'<code> :) </code > :)', '<p> :) <a', '<p><a :)', '<a :)', 'x<a :)', ':) <', ':) >', '> :)', '<>:)', '<> :)',
		'<p title=":)"> :)', ':)<br>', '<br>:)', '<code', '<code :)', '</code> :)', 'a <code> :) </code> b :)',
	);
	foreach ( array_keys( $wpsmiliestrans ) as $smiley ) {
		$cases[] = "$smiley";
		$cases[] = "a $smiley b";
	}
	foreach ( post_contents() as $content ) {
		$cases[] = $content;
	}
	$atoms = array(
		':)', ':(', ':?', ':???:', ':mrgreen:', ':', ')', '8', 'O', '-', ' ', "\n", "\t", "\xC2\xA0", '&nbsp;', 'x',
		'<p>', '</p>', '<code>', '</code>', '<pre>', '</pre>', '<textarea>', '</textarea>', '<', '>', ';)', '8-O',
	);
	return array_merge( $cases, random_cases( $atoms, 400, 10 ) );
}

function autop_cases() {
	$cases = array(
		'', ' ', "\n\n", 'one line', "one\ntwo", "one\n\ntwo", "a\r\nb\r\rc", "  lead\n\n\ntrail  \n",
		"<p>already</p>", "<div>block</div>text\n\nmore", "<div>x\ny</div>", "<p>x</p>\n<p>y</p>",
		"<pre>keep\n\n  this</pre>\nafter", "a<pre>one</pre>b<pre class=\"x\">two\n</pre>c</pre>", "no pre end <pre>open",
		"<br><br>x<br/>  <br />y", "line<br>\nnext", "line <br />\nnext", "line<br /> \nnext",
		"<select>\n<option>a</option>\n <option>b</option>\n</select>",
		"<object data=\"x\">\n<param name=\"a\">\n<embed src=\"y\">\n</object>",
		"<audio>\n<source src=\"a\">\n<track src=\"b\">\n</audio>\n[video]\n<source>\n[/video]",
		"<figure><img src=x>\n<figcaption>cap\n</figcaption>\n</figure>",
		"<ul>\n<li>a</li>\n<li>b\nc</li>\n</ul>", "<li>loose</li>", "<p><li>x</li></p>",
		"<blockquote>quote\n\nmore</blockquote>", "<BLOCKQUOTE cite='a>b' class=\"q\">x</BLOCKQUOTE>", "<blockquote class=\"open>",
		"<script>a\nb</script>\n\n<style>\nc{}</style><svg>\n</svg><math>\n</math>",
		"<!-- a\ncomment -->\ntext", "<![CDATA[x\ny]]>\nz", "<a href=\"x\ny\">link</a>", "<!-- unterminated\n",
		"<hr>\n<hr/>text<hr  />", "<h1>t</h1>\n<h6 class=x>s</h6>", "<table><tr><td>c\nd</td></tr></table>",
		"<address>a\nb</address>", "<form>f</form>", "<p>x</div>", "<span>\n\n</span>", "<path d=\"x\">\n</path>",
		"text \x0b\n\x0c\n end", "\t\n\t\nx", "<p>\n</p>", "<br />\n\n<br />",
		"<blockquote class=\"open>x", "<pre>a</pre>literal <pre wp-pre-tag-0></pre> end", "<pre wp-pre-tag-1></pre><pre>a</pre><pre>b</pre>", "<pre>a</pre><pre>b<pre wp-pre-tag-0></pre>c</pre><pre wp-pre-tag-1></pre>", "<pre>x <pre wp-pre-tag-1></pre></pre><pre>y</pre>", "<pre wp-pre-tag-01></pre><pre>x</pre><pre wp-pre-tag-9></pre><pre wp-pre-tag-0>", "<pre wp-pre-tag-x></pre><pre>z</pre>", "<pre>a<pre wp-pre-tag-0></pre>", "<p><li>x</p>\ny", "<li>x\ny", "<br /><p<br /><div>", "<script</script>\nx", "[audio [video]x<source>", "<source>\n</object><param name", "<source>[audio src", "a<br /> <p", "<track>\n</audiox>\n[/videoy]\n[/audio]",
	);
	foreach ( post_contents() as $content ) {
		$cases[] = $content;
	}
	$atoms = array( "\n", "\n\n", "\r\n", "\r", ' ', "\t", 'text', '<p>', '</p>', '<div>', '</div>', '<br>', '<br />', '<br/>', '<pre>', '</pre>', '<option>', '</option>', '<object>', '</object>', '<param>', '<embed>', '<audio>', '</audio>', '[video]', '[/video]', '<source>', '<track>', '<figcaption>', '</figcaption>', '<li>', '</li>', '<ul>', '</ul>', '<blockquote>', "<blockquote class='a>b'>", '</blockquote>', '<script>', '</script>', '<style>', '</style>', '<svg>', '</svg>', '<!-- c -->', '<!--', '-->', '<![CDATA[', ']]>', '<hr>', '<hr />', '<h2>', '</h2>', '<td>', '<span>', '</span>', '<a href="x">', '<address>', '</address>', '<form>', '</form>', '<path>', '<', '>', '"', "'" );
	return array_merge( $cases, random_cases( $atoms, 3000, 16 ) );
}

function balance_cases() {
	$cases = array(
		'', 'plain', '<b>open', '</b>stray', '<b><i>x</b>', '<p>a<p>b', '<div><div>nested</div>', '<span><span>x',
		'<br>', '<br/>', '<BR >', '<img src="x">', '<img src="x"/>', '<img src="x" / >', '<hr  >', '<input type=text>',
		'<p/>', '<div / >', '<x-custom attr>y', '<my-el.v2>z</my-el.v2>', '<my-elX>', '<a-1>b', '<ns:tag>c</ns:tag>', '<_u>d',
		'<B>Upper</b>', '<b >sp</b >', "<b\n>nl</b>", '<b class="x">y', '< !-- not a comment -->', '<!-- c --><b>', '<3 love <3',
		'<1>', '<a href="x>y">z</a>', '<a <b>', '<<b>>', '<b', '</', '</>', '<b/c>', '<table><tr><td>x</table>', '<ul><li>a<li>b</ul>',
		'<q><q>x</q></q>', '<p class="x"/>', '<div id=a / >', '<my.el-x>a', '<a1_b-c>d', '<a.b-c_d.e>f</a.b-c_d.e>', '<my->', '<my-->', '<a-->', '<blockquote><blockquote>q', '<em><strong>x</em></strong>', '</div></div><div>', "<p>\x0bv</p>",
	);
	foreach ( post_contents() as $content ) {
		$cases[] = $content;
	}
	$atoms = array( '<b>', '</b>', '<i>', '</i>', '<p>', '</p>', '<div>', '</div>', '<span>', '</span>', '<br>', '<br />', '<img src=x>', '<x-el>', '</x-el>', '<a href="q">', '</a>', '<ul>', '<li>', '</li>', '<q>', '</q>', 'text', ' ', "\n", '<', '>', '/', '"', '< !--', '<!--', '-->', '<3', '<B>', '</B>', '<b class="c">', '<hr/>', '<p/>', '<em>', '</em>' );
	return array_merge( $cases, random_cases( $atoms, 3000, 14 ) );
}

function clickable_cases() {
	$home  = home_url();
	$host  = wp_parse_url( $home, PHP_URL_HOST );
	$cases = array(
		'', '0', ' ', 'plain text', 'http://example.com', 'see http://example.com.', 'x https://example.com/a?b=c&d=e#f y',
		"$home/about", 'go ' . strtoupper( $home ) . '/x', "http://$host:9/y", "https://$host/z", "ftp://$host/f", 'HTTP://Example.COM/',
		'(http://example.com)', '(http://example.com/a_(b))', 'http://example.com/a_(b)c)', 'http://example.com/a))', '((http://example.com/(a)))',
		'http://example.com/x).html', 'http://example.com).html', 'http://example.com/a.b.c.d', 'http://example.com/p.html.', 'http://e.com/f.abcdefgh',
		'http://e.com/x.y z', 'http://e.com/a,b;c:d!e?f', "http://e.com/it's", 'http://e.com/?q=1!', 'http://e.com/a?', 'http://e.com/a..b',
		',http://e.com', ';http://e.com', ':http://e.com', '!http://e.com', '?http://e.com', '.http://e.com', '<http://e.com>', '<http://e.com x',
		'xhttp://e.com', '_http://e.com', 'a://b', 'abcdefghijklmnopqrst://e.com', 'abcdefghijklmnopqrstu://e.com', 'http:/e.com', 'http://',
		'http:// e.com', 'javascript://e.com/x', 'data://x', 'irc://irc.example.com/c', 'ssh://h', 'feed://e.com', 'news://n', 'tel://1',
		'http://e.com/' . str_repeat( 'a', 1990 ), 'http://e.com/' . str_repeat( 'a', 1993 ) . ' x', 'http://e.com/' . str_repeat( 'a', 1994 ) . ' x', 'http://e.com/' . str_repeat( 'a', 1995 ) . ' x', 'http://e.com/' . str_repeat( 'a', 2100 ) . ' x',
		'http://e.com/a))(', 'http://e.com/a)b)c(d', 'xxxxxx<a bc><a c>d</a></a>', '<a><a >z</a></a>', '<a><a z', 'x <a href="q"><a y', 'xyzw <a\tb><a c>d</a></a>',
		str_repeat( 'a', 2200 ) . '(http://e.com ' . str_repeat( 'b ', 3893 ), str_repeat( 'a', 2200 ) . '(http://e.com ' . str_repeat( 'b ', 3893 ) . 'c',
		"http://e.com/\xC3\xA9t\xC3\xA9", "http://e.com/\x80\xff", 'http://e.com/<b>', 'http://e.com/"q"', 'http://e.com/[1]*+=&$-~%#@/',
		'www.example.com', 'see www.example.com.', 'WWW.Example.com/a', 'ftp.example.com,', 'www.', 'www.e.com)', 'xwww.e.com', '>www.e.com',
		"www.$host/a", 'www.e.com/a;b:c', 'ftp.e.com/a?b=c', 'www.e.com/"x', 'www.e.com/(x)',
		'me@example.com', 'mail me@example.com.', 'a.b+c-d_e@sub.example.co.uk', 'me@localhost', 'me@e.c', 'me@e.co', 'me@a.b.c',
		'me@a-b.c-d.ef', 'me@a.b-c', 'me@.com', '@e.com', 'x@e.com@f.com', 'ME@EXAMPLE.COM', 'me@e.com1', 'me@e.c1-d.ab', 'x me@e..com',
		'<p>http://e.com</p>', '<a href="http://e.com">http://e.com</a>', '<a href="x">www.e.com</a>', '<a>me@e.com</a>',
		'<code>http://e.com</code> http://f.com', '<pre class="x">www.e.com</pre>', '<CODE>http://e.com</CODE>', '<code>x<code>y</code>http://e.com</code>http://f.com',
		'<script>http://e.com</script>', '<style>a{}</style>www.e.com', '<codex>http://e.com</codex>', '<code\n>http://e.com</code>', '</code>http://e.com',
		'<pre>a</PRE>http://e.com', '<img src="http://e.com/i.png"> http://e.com', '<http://e.com/a b>', '< http://e.com>', '<b>http://e.com</b>',
		"a\nhttp://e.com\nb", "a\thttp://e.com\tb", "a\x0bhttp://e.com\x0cb", "a\rwww.e.com\rb", "x\x00http://e.com", '<<http://e.com', '<>http://e.com',
		'<a href="x"><a href="y">z</a></a>', '<A HREF="x"><a href="y">z</a></A>', '<a><a href="y">z</a></a>', "<a\thref='x'><a href='y'>z</a></a>",
		'<a x><a y></a></a>', '<a ><a y>z</a></a>', '<a href="x"><a  y>z</a></a>', '<ab><a y>z</a></a>', '<a href="x"><a y>z</a></a></a>',
		str_repeat( 'word http://e.com ', 700 ), str_repeat( 'x', 10001 ), str_repeat( 'x', 2200 ) . ' http://e.com ' . str_repeat( 'y', 9000 ),
		str_repeat( 'a ', 5001 ) . 'http://e.com', str_repeat( "a\x00", 5001 ) . 'www.e.com', str_repeat( 'http://e.com/' . str_repeat( 'q', 2090 ) . ' ', 6 ),
		str_repeat( 'a', 2101 ) . ' ' . str_repeat( 'b', 9000 ) . ' me@e.com', str_repeat( 'a', 9999 ) . ' 0', str_repeat( ' ', 10001 ) . '0',
		'<b>' . str_repeat( 'me@e.com ', 1200 ) . '</b>',
	);
	foreach ( post_contents() as $content ) {
		$cases[] = $content;
	}
	$atoms = array(
		'http://e.com', 'https://', "$home/", 'www.', 'ftp.', 'e', '.com', '.', ',', ';', ':', '!', '?', ')', '(', "'", '"', '/', '@', '-', '_',
		' ', "\n", '<a href="x">', '</a>', '<a>', '<code>', '</code>', '<pre>', '</pre>', '<b>', '<', '>', 'me@', 'x', '1', '#', '&', "\xC3\xA9",
	);
	return array_merge( $cases, random_cases( $atoms, 3000, 12 ) );
}

if ( 'clickable' === $argv[1] ) {
	echo home_url(), "\n";
	remove_all_filters( 'comment_text' );
	add_filter( 'comment_text', 'make_clickable' );
	foreach ( clickable_cases() as $case ) {
		echo bin2hex( $case ), "\t0\t", bin2hex( make_clickable( $case ) ), "\n";
		echo bin2hex( $case ), "\t1\t", bin2hex( apply_filters( 'comment_text', $case ) ), "\n";
	}
	exit;
}
if ( 'balance' === $argv[1] ) {
	foreach ( balance_cases() as $case ) {
		emit( $case, force_balance_tags( $case ) );
	}
	exit;
}
if ( 'autop' === $argv[1] ) {
	foreach ( autop_cases() as $case ) {
		foreach ( array( true, false ) as $br ) {
			echo bin2hex( $case ), "\t", $br ? '1' : '0', "\t", bin2hex( wpautop( $case, $br ) ), "\n";
		}
	}
	exit;
}
if ( 'texturize' === $argv[1] ) {
	foreach ( texturize_cases() as $case ) {
		emit( $case, wptexturize( $case ) );
	}
} else {
	foreach ( smilies_cases() as $case ) {
		emit( $case, convert_smilies( $case ) );
	}
}

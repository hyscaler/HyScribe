<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for hyscribe-kses: runs WordPress's escaping and sanitizing
// functions (default filters, UTF-8 blog charset) and writes, per function,
// "hex(input)<TAB>hex(output)" lines, with "throws" where PHP throws:
//   php golden.php <wordpress> <theme folder> <output folder>
// limits.tsv records inputs too long to store as
// "function<TAB>hex(prefix)<TAB>hex(unit)<TAB>count<TAB>hex(suffix)<TAB>expected",
// expected being hex(output), "=" for the input itself, or "len:<n>".
// Run through reference/kses/golden.sh. Random cases use a fixed seed, so
// the output is stable.

require rtrim( $argv[1], '/' ) . '/wp-load.php';

mt_srand( 20261027 );

$theme = rtrim( $argv[2], '/' );
$out   = rtrim( $argv[3], '/' );

/** Every `style` and `class` attribute value in the theme's markup. */
function real_values( $theme ) {
	$documents = array();
	foreach ( json_decode( file_get_contents( "$theme/patterns.json" ), true ) as $pattern ) {
		$documents[] = $pattern['content'];
	}
	foreach ( array( 'templates', 'parts' ) as $folder ) {
		foreach ( glob( "$theme/$folder/*.html" ) as $file ) {
			$documents[] = file_get_contents( $file );
		}
	}
	$values = array();
	foreach ( $documents as $document ) {
		preg_match_all( '/\s(?:style|class)="([^"]*)"/', $document, $matches );
		foreach ( $matches[1] as $value ) {
			$values[ $value ] = true;
		}
	}
	return array_map( 'strval', array_keys( $values ) );
}

/** `$count` strings of one to `$max` atoms, then 100 of random bytes. */
function random_cases( $atoms, $count, $max ) {
	$cases = array();
	for ( $i = 0; $i < $count; $i++ ) {
		$case = '';
		$n    = mt_rand( 1, $max );
		for ( $j = 0; $j < $n; $j++ ) {
			$case .= $atoms[ mt_rand( 0, count( $atoms ) - 1 ) ];
		}
		$cases[] = $case;
	}
	for ( $i = 0; $i < 100; $i++ ) {
		$case = '';
		$n    = mt_rand( 0, 24 );
		for ( $j = 0; $j < $n; $j++ ) {
			$case .= chr( mt_rand( 0, 255 ) );
		}
		$cases[] = $case;
	}
	return $cases;
}

$entities = array(
	'', 'plain', '0', '&', '&&', '&amp;', '&amp;amp;', '&AMP;', '&lt;&gt;&quot;&#039;', '&apos;', '&hellip;',
	'&Hellip;', '&frac14;', '&frac145;', '&sup1;', '&there4;', '&thetasym;', '&thetasyms;', '&a;', '&ab;',
	'&nbsp', '&nbsp ;', '&nbsp;&nbsp;', '&#0;', '&#00;', '&#9;', '&#09;', '&#8;', '&#10;', '&#13;', '&#31;',
	'&#32;', '&#65;', '&#0065;', '&#000000065;', '&#1114111;', '&#1114112;', '&#55295;', '&#55296;',
	'&#57343;', '&#57344;', '&#65533;', '&#65534;', '&#65535;', '&#65536;', '&#12345678;', '&#0001114111;',
	'&#99999999999999999999;', '&#x41;', '&#X41;', '&#x0041;', '&#x00000041;', '&#x;', '&#x0;', '&#x9;',
	'&#x10FFFF;', '&#x110000;', '&#xD7FF;', '&#xD800;', '&#XD800;', '&#xDFFF;', '&#xE000;', '&#xFFFE;',
	'&#xfffd;', '&#x1234567;', '&#X1234567;', '&#x0001234567;', '&#X000;', '&#xG;', '&#;', '&#', '&#65',
	'&#x41', '&#38;', '&#38;#38;', '&#38;#x41;', '&#38;amp;', '&&amp;;', '&amp;&#38;', '&#XYZZY;', 'AT&T',
	'<script>alert(1)</script>', '"\'<>', 'a"b\'c', '&lt;script&gt;', '" onmouseover="alert(1)', "' onfocus='x",
	"\xc3\xa9", "\xff", "\xc3", "\xc0\x80", "\xed\xa0\x80", "\xf4\x90\x80\x80", "\xef\xbf\xbe", "\xf0\x9f\x98\x80",
	"\0", "a\0b", '&#256;', '&#321;', '&#x100;', '&#xFFFFFFFFFFFFFFFF;', '&#x7FFFFFFFFFFFFFFF;', '&#x8000000000000000;',
	'&#9223372036854775807;', '&#9223372036854775808;', '&#18446744073709551616;', '&#X41', '&#x41;&#x42;',
);
$protocols = array(
	'javascript:alert(1)', 'JavaScript:alert(1)', ' javascript:x', "java\tscript:x", "java\nscript:x",
	"java\0script:x", "java\x0bscript:x", 'java&#115;cript:x', 'java&#0000115;cript:x', 'java&#x73;cript:x',
	'java&#X73;cript:x', '&#106;&#97;&#118;&#97;&#115;&#99;&#114;&#105;&#112;&#116;&#58;alert(1)',
	'javascript&#58;x', 'javascript&#058;x', 'javascript&#0000058x', 'javascript&#x3a;x', 'javascript&#X3A;x',
	'javascript&#x003A', 'javascript&#x003a;x', 'javascript&colon;x', 'javascript&COLON;x', 'javascript&#58x',
	'javascript&#585;x', 'javascript&#x3ab;x', 'javascript&#x3aG', 'javascript&#5;8x', 'data:text/html,<script>',
	'DATA:x', 'vbscript:x', 'http://example.com', 'http://x ', 'https://a b ', 'https://x', 'HTTP://x', 'hTtPs://x', 'http:x', '//example.com',
	'/path:x', '/?a:b', 'a/?b:c', 'a/b?:c', '?x:y', '#a:b', 'mailto:a@b', 'MAILTO:a', 'feed:http://x',
	'feed:javascript:x', 'feed:feed:http://x', 'feed:feed:feed:http://x', 'feed:feed:feed:feed:x', 'feed:',
	'feed:0', 'feed: 0', 'feed:javascript:0', 'FEED:http://x', str_repeat( 'javascript:', 5 ) . 'x',
	str_repeat( 'javascript:', 6 ) . 'x', str_repeat( 'javascript:', 7 ) . 'x', 'a:b:c:d:e:f:g', 'a:b:c:d:e:f',
	'irc6://x', 'tel:+1', 'urn:isbn', 'webcal://x', 'svn+ssh://x', 'http:', ':', '::', ':x', ' :x', 'x: ', 'x:0',
	'x: 0 ', 'http: x', '&#99999999999999999999;:x', '&#9223372036854775807;:x', '&#9223372036854775808;:x',
	'&#x7FFFFFFFFFFFFFFF;:x', '&#x8000000000000000;:x', '&#x8000000000000000;x', 'x:&#99999999999999999999;:y', 'feed:&#99999999999999999999;:x',
	'&#38;#x6A;avascript:x', 'j&#38;#97;vascript:x', '\\0javascript:x', 'jav\\0ascript:x', 'java\\\\00script:x',
	'jav\\ascript:x', '&#104;ttp://x', '&#x68;ttps://x', "h\ttp://x", 'ht tp://x', "\0http://x", 'https:x',
	"https:\0x", 'http&#58;//x', 'http&colon;//x', 'mms:x', 'ftp:x', 'xmpp:x', 'sms:x', 'fax:x', 'nntp:x',
);
$nulls = array(
	'\\0', '\\\\0', '\\00', '\\a', '\\', 'a\\', '\\\\', '\\0\\0', '\\\\\\000x', "\\\x000", "\\\x00\\0",
	"\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x0e\x0f\x10\x1f\x20\x7f", '0', '00', '\\0a\\b0',
);
$tags = array(
	'<p>Hello <b>World</b></p>', '<script>alert(1)</script>x', '<SCRIPT type="a">x</script>y', '<style>a{}</style>b',
	'<style>a</STYLE >b', '<script>a', '<scriptx>a</scriptx>b', '<scripts>a</script>b', '<style a="</style>">b',
	'a<script>b</script>c<style>d</style>e', '<script><script>x</script>y', "<script>\n</script>z", 'a < b > c',
	'a <b', 'a < b', "a <\tb", 'a<', '<', '>', 'a>b', '<a href="x>y">z</a>', "<a href='x>y'>z</a>", '<a b="c\'d">e',
	'<!-- comment -->x', '<!-- a > b -->c', '<!--->x', '<!-->x', '<!---->x', '<!-- x', '<!x>y', '<!DOCTYPE html>x',
	'<!doctype html><p>x</p>', '<!DOCTYPE html "a>b">c', '<?php echo 1; ?>x', '<?php echo "?>"; ?>y', '<? (a ?> b) ?>c',
	"<?php echo 'a'; ?>z", '<?xml version="1.0"?>x', 'a<?xml version="1.0"?>x', '<?xml a>b', 'a<?xml a>b',
	'a<?XML a->b>c', 'a<?xml a-->b', '<a<b>>c', '<a <b>c', '<a<!-- -->>x', '<a"<!-- x -->y', "\0a\0", "a\0<b\0>c",
	'<<>>', '<a>>b', '<"a">b', "<a\\'>b", '<!"x>y', "<!'x>y", '<!a\\">b', '<?"\\"?>a', "<?'\\'?>b", "<?'\"'?>a<?(?>b",
	'<?)?>a', '<?((?>a', ' padded ', "\n\ttext \r\n", "a\n\n b\t\tc  d", '0', '', ' 0 ',
	'<script></script><scripts>x</SCRIPT>d', '<script>a</style>b', '<script a', '<scrip>a</scrip>', "a <\tb>",
	"<a b='c\"d'>e", '<a"<b>c', '<a<!x>>y', '<a<?x>?>>z', '<!- ->x', '<a"<!-- --> ">x', '<!x">y">z', '<!x\\">y',
	'<!doctype "a>b">c', "<!x doctype 'a>b'>c", "<!x octype 'a>b'>c", '<!DOCTYPE<a>>b', '<!OCTYPE<a>>b',
	"<?php echo '?>'; ?>y", '<? \\" ?>a', '<?"("?>a', "<?'a'?>b>c", '<?a>?>d', '<a<?b?>>c', 'a<?XmL a>b',
	'<a"<">b', '<a < b>c>d', '<a<!-- -->>x>y', '<a<b<!-- -->>>x>y', '<a<!x>q>y', '<!x->z', '<!x--a>b', '<?")"?>a',
	'a<?xml a->b>c', 'a<?yml a>b', 'a<?xnl a>b', 'a<!xml a>b', 'a<?Xxml a>b', "a\r\n \tb  c\nd",
);
$keys = array( '', 'Key', 'my_key-1', 'a b', 'ÄÖ', 'k.e/y', 'UPPER_CASE', "\0x", '0', '--', '__' );
$css = array(
	'', ' ', ';', ';;;', 'a', 'a;b', 'width', 'width:', ':', ':1px', 'width::1px', 'width:1px:2px', 'foo', '--*:red',
	' ; color:red', 'COLOR:red', 'color : red ; ;', 'a:b;;c', 'color:red;;color:blue', 'color:red}', 'color:red{',
	'x:linear-gradient(a)', 'color:red;background:url(javascript:alert(1))', 'width:expression(alert(1))',
	'expression(alert(1))', 'color:red/*x*/', 'color:/**/red', 'width:calc(1px /* */)', 'behavior:url(x.htc)',
	'-moz-binding:url(x)', 'color:red\\;', 'color:\\72 ed', 'font-family:"a"', "font-family:'a'", 'content:"x"',
	'color:red!important', 'color: red !important', '--x:1', '--x-y_z:2', '--:1', '---:1', '--a b:1', '--x:url(a)',
	'--x:url(javascript:a)', "--x: url( 'a' )", '--x:linear-gradient(red,blue)', '--x:calc(1px)', '--x:{}',
	'--x: url(javascript:x)', '--x:a url(javascript:x)', '--x:var(--y,url(javascript:alert(1)))',
	'--x:calc(url(data:text/html,x))', '--X:1', '--x:a=b', '--x:url(a) url(b)', '--x:url(a) linear-gradient(red,blue)',
	'background:linear-gradient(to right,rgb(1,2,3),#fff)', 'background:linear-gradient(to right,rgb(calc(1),2,3),#fff)',
	'background:repeating-linear-gradient(45deg,red 0 1px,blue 1px 2px)', 'background-image:conic-gradient(red,blue)',
	'background:radial-gradient(circle,red,blue), url(a.png)', 'background:url(a.png),linear-gradient(red,blue)',
	'background:linear-gradient(red,blue) url(javascript:x)', 'color:linear-gradient(red,blue)',
	'background:linear-gradient(red,blue', 'background:linear-gradient(red,(blue)', 'background:linear-gradient()',
	'background:Linear-gradient(red)', 'background:repeating-gradient(red)', 'background:xlinear-gradient(red)',
	'background:linear-gradient(red)linear-gradient(blue)', 'background:linear-gradient(a(b)c(d))',
	'background:linear-gradient(a)x linear-gradient(a)', 'background:linear-gradient(linear-gradient(a))',
	'background:calc(linear-gradient(((a))))', 'background:url(', 'background:url()', 'background:url( )',
	'background:url(0)', "background:url( '0' )", "background:url('')", 'background:url("")', "background:url('a)",
	'background:url(a b)', 'background:url( a )', 'background:url(a)b)', 'background:url(url(a))',
	'background:url(http://x/?a=b&c=d)', 'background:url(//x)', 'background:url(&#106;avascript:x)',
	'background:url(java\\script:x)', 'background:url( javascript:x)', 'background:url("javascript:x")',
	"background:url('data:image/svg+xml,<svg>')", "background-image:url(' javascript:x ')", 'background:url("a\')',
	"background:url('a' )", "background:url( 'a'b' )", 'background:url(\'a"\')', 'background:url("a" "b")',
	'background:url(a)url(b)', 'background:calc(url())', 'background:calc(url()) url(a)', 'background:URL(javascript:x)',
	'background:var(--a,url(javascript:x))', 'background:var(--a,URL(javascript:x))', 'background:URL(data:x)',
	'background:linear-gradient(red,url(javascript:x))', 'width:calc(url(javascript:alert(1)))', 'filter:url(#a)',
	'mask:url(data:image/png,x)', 'cursor:url(x),auto', 'cursor:url(javascript:x),auto', 'stroke:url(#g)',
	'list-style:url(a)', 'list-style-image:url(a)', 'marker-start:url(#m)', 'fill:url(javascript:x)',
	'transform:rotate(45deg) translateX(1px)', 'transform:matrix3d(1,2)', 'transform:rotateX(1deg)',
	'transform:scale3d(1,1,1) skewY(1deg)', 'transform:perspective(1px) translate3d(1px,2px,3px)',
	'clip-path:polygon(0 0,1px 1px)', "clip-path:path('M0 0')", 'clip-path:url(#c)', 'clip-path:xywh(1px 2px 3px 4px)',
	'clip-path:shape(from 0 0)', 'clip-path:circle(50%)', 'clip-path:ellipse(1px 2px)', 'clip-path:inset(1px)',
	'clip-path:rect(1px 2px)', 'width:min(1px,max(2px,3px))', 'width:minmax(1px,2px)', 'width:clamp(1rem,2vw,3rem)',
	'grid-template-columns:repeat(auto-fill,minmax(100px,1fr))', 'width:calc(100% - var(--x))', 'width:calc(1px',
	'width:calc(1px))', 'width:calc)', 'width:attr(x)', 'width:env(x)', 'width:Calc(1px)', 'width:calc (1px)',
	'width:CALC(1px)', 'width:xcalc(1px)', 'width:-calc(1px)', 'width:_calc(1px)', 'width:9calc(1px)',
	"width:\xc3\xa9calc(1px)", 'width:calc(1px+calc(2px))', 'width:calc(1px) calc(2px)', 'width:calc(1px)calc(2px)',
	"background:url('')x)", '--x:a url(javascript:a)x', '--x-y_Z9:1', '-x:1', '--x:linear-gradient(red,(blue))',
	'background-image:repeating-radial-gradient(red,blue)conic-gradient(red)', 'background:linear-gradient((a)',
	'background:linear-gradient((ab', 'background:linear-gradient(rgb(calc(1)))', 'width:a/b*c)', 'width:a{b<c>',
	'background:linear-linear-gradient(a)gradient(b) linear-gradient(b)', 'background:url(a) url(data:x)',
	'width:calc(1px+calc(2px))calc(3px) var(--x)', 'transform:rotateX(1deg) scale3d(1,1,1) translateZ(1px)',
	'width:calc(1px) calc((2px)', 'calc(1px)', ':1px;--*:red;foo', "\0wid\nth:1\rpx\t", 'color:red;COLOR:red;margin:0',
	'background:url( "javascript:x")', 'background:url( "" )', 'background:url("a" )', 'background:url(a',
	'width:a/calc(1)*b', 'width:calc((1px))', 'width:calc(()', 'width:calc(()))','width:1px=2', 'width:a&b', 'width:a}b', 'width:a{b',
	'width:a\\b', 'width:a<b>', 'width:"a"', 'WIDTH:1px', ' width : 1px ', 'width:1px;;height:2px', "\0width:1px",
	"wid\nth:1px", "wid\tth:1px", "width:1\rpx", 'width:\\01px', "wid\x0bth:1px", 'width:a/b', 'width:a/ *b',
	'width:a*/b', 'width:a/*', "width:1px\0", 'width:calc(1px)/*', 'margin:0;padding:0', 'd:path("M0")',
	'x:1', 'r:1', 'font:12px/1.5 "Helvetica Neue",Arial', 'color:rgb(1,2,3)', 'color:rgba(1,2,3,.5)',
	'color:var(--wp--preset--color--base)', 'margin:var(--a,var(--b,1px))', 'width:var(--a,calc(1px + var(--b)))',
);

$urls = array(
	'', ' ', 'http://example.com/', 'https://example.com/a?b=c&d=e', ' http://x', "\t\n http://x", " \t\0http://a b", 'example.com',
	'example.com/path', 'index.php', 'wp-login.php?x', 'a-b.PHP', 'a-b.PHP?x', '.php', 'a.phpx', 'a_b.php', '/relative', '/rel',
	'//proto.rel/x', '#frag', '#f', '?query', '?q', 'javascript:alert(1)', 'javascript:x', 'JavaScript:x', 'java script:x',
	'jav&#x09;ascript:x', '&#106;avascript:x', 'mailto:a@b.c', 'mailto:a%0a@b', 'MAILTO:a%0a@b', 'MAILTO:%0D', 'mailto:x%0D%0A',
	'http://x/%0a%0A%0d%0D', 'http://x/%%0a0a', '%0a', '%0a%0D', 'http://x/a b', 'http://x/<b>', 'http://x/<"q">', 'http://x/"q"',
	"http://x/'q'", "http://x/'", 'HTTP:x', 'tel:1', 'x:y', 'http://x/?a[1]=2', 'http://x/?a[]=1&b[]=2', 'http://[::1]/a[b]',
	'http://[::1]:8080/a', 'http://[::1]:8080/a[b]', 'http://u:p@h:80/[x]', 'http://u:p@host:80/a[1]', 'http://u@host/[x]',
	'http://@h/[x]', 'http://@host/[x]', '//h:80/[x]', '//host:80/[x]', '/[x]', '[x]', '/a[b]/c', 'x:/[y]', 'http://h:99999/[x]',
	'http://h:-1/[x]', 'a[1]:8/', 'http://host:65536/[x]', 'http://host:65535/[x]', 'http://host:0/[x]', 'http://host:+8/[x]',
	'http://host:-0/[x]', 'http://host:-1/[x]', 'http://host:8a/[x]', 'http://host:123456/[x]', 'http://host:/[x]',
	'http://:80/[x]', 'a:80/[x]', 'a:65536/[x]', ':80/[x]', 'x://[y]', 'file:///[c]', 'file:///c:/[d]', 'FILE:///[c]',
	'a+b.c-d:x[1]', 'a_b:x[1]', 'a_b:[1]', 'a_b?:x[1]', '//[x]', '//', 'a:', 'a:[', 'http:[x]', 'http:/[x]', 'h[t]tp://x',
	'[]:80', 'x]y', 'tel:+1-555', 'sms:1', 'feed:http://x', 'feed:feed:http://x', 'feed:feed:feed:http://x', 'feed:javascript:x',
	'feed:0', 'feed:', 'urn:x', 'svn+ssh://x', 'ftp://x', 'data:text/html,x', 'x&#58y', 'x&#058;y', 'x&#x3a;y', 'x&#X3A;y',
	'x&#x03ay', 'x&#58;y', 'x&colon;y', 'x&COLON;y', 'x&#5;8', 'x&#58', 'x&#x3a', 'x&#x3aa:y', 'x&#585:y', 'x/?a:b', 'a/?:b',
	'&#x6a;avascript:x', '&#0000106;avascript:x', 'h&#116;tp:x', 'h&#116;tp://x', '&#9223372036854775807;:x',
	'&#9223372036854775808;:x', '&#99999999999999999999;:x', '&#x7fffffffffffffff;:x', '&#x8000000000000000;:x',
	'http://x/&amp;', 'http://x/&nbsp;', 'http://x/&nosuch;', 'http://x/&#39;', 'http://x/&#039;', 'http://x/&#0;',
	'http://x/&#1;', 'http://x/&#9;', 'http://x/&#65;', 'http://x/&#00065;', 'http://x/&#1234567;', 'http://x/&#12345678;',
	'http://x/&#55296;', 'http://x/&#x41;', 'http://x/&#X41;', 'http://x/&#x0041;', 'http://x/&#X0041;', 'http://x/&#xD800;',
	'http://x/&#x110000;', 'http://x/&#x10FFFF;', 'http://x/&#xFFFFFFF;', 'http://x/&ab;', 'http://x/&abcdefgh;',
	'http://x/&abcdefghi;', 'http://x/&sup2;', 'http://x/&sup23;', 'http://x/&a12;', 'http://x/&a123;', 'http://x/&;',
	'http://x/&#;', 'http://x/&#x;', 'http://x/&amp;&nbsp;&nosuch;&ab;&abcdefghi;&sup2;&sup23;&a123;',
	'http://x/&#9;&#00065;&#1234567;&#12345678;&#55296;&#1;&#0;', 'http://x/&#x41;&#X0041;&#xD800;&#x110000;&#xFFFFFFF;&#x;',
	'http://x/é', "http://x/\x80\xff", "http://x/\x80\xff\x7f", 'http://x/\\0', "a\\0:b", "&#92;0x:y", 'h&#0;ttp:x',
	'ht&#32;tp:x', 'ht&#10;tp:x', "http\x00://x", 'http;//x', 'x;//y', 'mailto;//x', '\\\\x', 'a|b', '$x', '!x', '*x', '(x)',
	',x', '~x', '+x', '@x', '%', 'http://x?a#b[c]', 'https:[x]', 'https://x[1]', 'HTTP://X/[1]', 'irc6://[x]', 'webcal:x',
	'xmpp:x', 'gopher:x', 'http://user:pass@[2001:db8::1]:8443/p[1]?q[]=1#f[2]', '//[::1]:80/[x]', 'http://[::1/[x]',
	'http://h:80:90/[x]', '0', '00', '/', '#', '?', ':', ' 0', 'a:0', 'feed:feed:0', 'mailto: x ',
);

/** Inputs for the functions on byte strings. */
$real    = real_values( $theme );
$sets    = array(
	'esc-html'           => array( 'esc_html', array_merge( $entities, $tags, $css, $real ), array( '&', '&amp;', 'amp;', '#', '#x', '#X', 'x', '0', '9', '58', '65', ';', 'nbsp', 'hellip', 'apos', 'quot', 'lt', 'a', 'A', 'F', '<', '>', '"', "'", ' ', "\xc3\xa9", "\xff", "\0", '\\' ) ),
	'esc-attr'           => array( 'esc_attr', array_merge( $entities, $tags, $css, $real ), array( '&', '&amp;', '#', '#x', '0', '65', ';', 'apos', 'lt', 'a', '<', '>', '"', "'", ' ', "\xc3\xa9", "\xe2\x82", "\0" ) ),
	'normalize-entities' => array( 'wp_kses_normalize_entities', array_merge( $entities, $real ), array( '&', '&amp;', 'amp;', '#', '#x', '#X', 'x', 'X', '0', '1', '9', '58', '65', 'D800', 'FFFF', ';', 'nbsp', 'frac', '14', 'apos', 'abcdefgh', 'a', 'Z', ' ' ) ),
	'decode-entities'    => array( 'wp_kses_decode_entities', array_merge( $entities, $protocols ), array( '&', '&#', '#', 'x', 'X', '0', '1', '9', '65', 'ff', 'FF', ';', 'a', ' ', '&#x' ) ),
	'no-null'            => array( 'wp_kses_no_null', array_merge( $nulls, $entities, $protocols ), array( '\\', '0', "\0", "\x01", "\x0b", "\x0c", "\x0e", "\x1f", "\t", "\n", "\r", 'a', ' ' ) ),
	'bad-protocol'       => array( 'bad_protocol', array_merge( $protocols, $entities, $nulls, $real ), array( 'javascript', 'http', 'https', 'feed', 'data', ':', '//', '/', '?', '&#58;', '&#58', '&#x3a;', '&#X3A', '&colon;', '&#106;', '&#x6A;', '&#', '0', ' ', "\t", "\0", '\\0', 'a', 'mailto', 'x' ) ),
	'strip-all-tags'     => array( 'wp_strip_all_tags', array_merge( $tags, $entities, $real ), array( '<', '>', '<!--', '-->', '-', '!', '?', '<?', '?>', '<?xml', '<!DOCTYPE', 'doctype', '"', "'", '\\', '(', ')', 'a', ' ', "\n", "\t", "\0", '<script>', '</script>', '<style>', '</STYLE>', '<script', 'l', 'm', 'x', 'e', 'p' ) ),
	'strip-all-tags-breaks' => array( 'strip_breaks', array_merge( $tags, $real ), array( '<', '>', '<b>', 'a', ' ', "\n", "\r", "\t", "\0", '<script>', '</script>' ) ),
	'sanitize-key'       => array( 'sanitize_key', array_merge( $keys, $real ), array( 'A', 'z', '_', '-', ' ', "\xc3\xa9", '0', '.', 'Z' ) ),
	'safecss-filter-attr' => array( 'safecss_filter_attr', array_merge( $css, $real ), array( 'color', 'background', 'width', '--x', ':', ';', ' ', 'red', 'url(', ')', '(', 'javascript:', 'http://a', '"', "'", 'calc(', 'var(', 'linear-gradient(', 'repeating-', ',', '1px', '\\', '&', '=', '}', '/*', '*/', '0', 'data:', 'rgb(', "\n", "\0", 'expression(', '-gradient(', 'url( ', 'URL(' ) ),
	'esc-url'            => array( 'esc_url', $urls, array( 'http', 'https', 'HTTP', 'ftp', 'feed', 'feed:', 'javascript', 'javascript:', 'mailto', 'mailto:', 'data', 'tel', 'x', 'host', 'example.com', ':', '://', ';//', '//', '/', '?', '#', '&#58', '&#58;', '&#x3A;', '&#x3a', '&colon;', '&#106;', '[', ']', '[::1]', '[2001:db8::1]', ':80', ':8080', ':65536', ':0', ':-1', 'u:p@', 'user@', '@', ' ', "\t", "\n", "\0", "\x01", "\x7f", '%', '%0a', '%0D', '%5B', '&', '&amp;', '&nbsp;', '&#039;', '&#', ';', "'", '"', '<', '>', '\\', '.php', 'index', 'a-b', '0', '1', '=', 'é', "\xff" ) ),
);

function bad_protocol( $text ) {
	return wp_kses_bad_protocol( $text, wp_allowed_protocols() );
}

function strip_breaks( $text ) {
	return wp_strip_all_tags( $text, true );
}

function run( $function, $input ) {
	try {
		return bin2hex( $function( $input ) );
	} catch ( TypeError $e ) {
		return 'throws';
	}
}

foreach ( $sets as $file => list( $function, $cases, $atoms ) ) {
	$cases = array_merge( $cases, random_cases( $atoms, 1000, 12 ) );
	$lines = '';
	foreach ( $cases as $case ) {
		$lines .= bin2hex( $case ) . "\t" . run( $function, $case ) . "\n";
	}
	file_put_contents( "$out/$file.tsv", $lines );
}

// PCRE's limits, at their edges.
$limits = array(
	array( 'wp_strip_all_tags', '<script>', 'a', 999997, '</script>x' ),
	array( 'wp_strip_all_tags', '<script>', 'a', 999998, '</script>x' ),
	array( 'wp_strip_all_tags', '<style>', 'a', 999997, '</STYLE>y' ),
	array( 'wp_strip_all_tags', '<style>', 'a', 999998, '</STYLE>y' ),
	array( 'wp_strip_all_tags', '<script>', '</scrip', 142856, '</script>z' ),
	array( 'wp_strip_all_tags', '<script>', '</scrip', 142857, '</script>z' ),
	array( 'wp_strip_all_tags', '<script><script>', 'a', 999988, 'x' ),
	array( 'wp_strip_all_tags', '<script><script>', 'a', 999989, 'x' ),
	array( 'safecss_filter_attr', 'width:calc(', 'a', 8188, ')' ),
	array( 'safecss_filter_attr', 'width:calc(', 'a', 8189, ')' ),
	array( 'safecss_filter_attr', 'width:calc(', '(a)', 2047, ')' ),
	array( 'safecss_filter_attr', 'width:calc(', '(a)', 2048, ')' ),
	array( 'safecss_filter_attr', 'width:calc(', '(', 2729, str_repeat( ')', 2730 ) ),
	array( 'safecss_filter_attr', 'width:calc(', '(', 2730, str_repeat( ')', 2731 ) ),
	array( 'safecss_filter_attr', 'width:calc(rotate(', 'a', 8179, '))' ),
	array( 'safecss_filter_attr', 'width:calc(rotate(', 'a', 8180, '))' ),
	array( 'safecss_filter_attr', 'background:linear-gradient(', 'a', 8190, ')' ),
	array( 'safecss_filter_attr', 'background:linear-gradient(', 'a', 8191, ')' ),
	array( 'safecss_filter_attr', 'background:repeating-conic-gradient(', '(b)', 8189, ')' ),
	array( 'safecss_filter_attr', 'background:repeating-conic-gradient(', '(b)', 8190, ')' ),
	array( 'safecss_filter_attr', 'background:radial-gradient(', '(ab)c', 4095, ')' ),
	array( 'safecss_filter_attr', 'background:radial-gradient(', '(ab)c', 4096, ')' ),
);
$lines = '';
foreach ( $limits as list( $function, $prefix, $unit, $count, $suffix ) ) {
	$input  = $prefix . str_repeat( $unit, $count ) . $suffix;
	$output = $function( $input );
	if ( $output === $input ) {
		$expected = '=';
	} elseif ( strlen( $output ) > 64 ) {
		$expected = 'len:' . strlen( $output );
	} else {
		$expected = bin2hex( $output );
	}
	$lines .= "$function\t" . bin2hex( $prefix ) . "\t" . bin2hex( $unit ) . "\t$count\t" . bin2hex( $suffix ) . "\t$expected\n";
}
file_put_contents( "$out/limits.tsv", $lines );

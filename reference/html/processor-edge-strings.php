<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The tag processor oracle's edge strings, for the HTML processor oracle too.

function processor_edge_strings() {
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
	$tokens = array_merge(
		$tokens,
		array(
			'<?xml version="1.0"?>x', '<?php echo 1 ?>', '<?a b?>', '<?a?>', '<?a  ?>', '<?a-b_c d ?>', '<?_x?>', '<?1x?>', '<?x:y?>', '<?xml-stylesheet a?>', '<?XML x?>', '<?a', '<?a b c', '<?>x', '<??>', '<?:x y?>',
			'<!DOCTYPE html>', '<!doctype html PUBLIC "x">', '<!DOCTYPE>', '<!DOCTYPEx>', '<!doctypehtml>', '<!DOCTYP>', '<!DOC', '<!doctype',
			'<![CDATA[x]]>', '<![CDATA[]]>', '<![CDATA[x]>', '<![CDATA[x]]', '<![CDATA[a]]b]]>', '<!x>', '<!>', '<!->', '<!--a-->', '<!---->', '<!----->', '<!-- a --!>', "<!--\r\na\rb-->",
			"<pre>\nx</pre>", "<pre>\n\nx</pre>", "<pre>\r\nx</pre>", "<listing>\nx</listing>", "<PRE>\n&amp;x", "<pre><b>\nx</b>", "<div>\nx</div>",
			"<textarea>\nx</textarea>", "<textarea>\n\nx</textarea>", "<textarea>&amp;&lt;b&gt;</textarea>", "<title>a&amp;b\x00c</title>", "<script>a&amp;\x00b</script>", "<style>a&amp;\r\nb</style>",
			"<xmp>&amp;</xmp>", "<iframe>&amp;</iframe>", "<noembed>&amp;</noembed>", "<noframes>&amp;</noframes>",
			"a&amp;b&not=c&notd&#65;&#x42;\x00e\r\nf\rg", '&notin;&notit;&ampx', "x\x00y", '</3>', '</ x>', '</>', '</-x>', '</!x>', '</?x>', '</', '</x', "a\r\n<b>\r\n", '<?a b>', '</-', '<!---', '<!------', 'xxxxxx<!a>', 'xxxxxxxxxx<!-- a -->', '<?xml a>', '<?:?>', '<?:a?>', 'xxxxxx<?x y?>', 'xxxxxx<?:a?>',
		)
	);
	return $tokens;
}

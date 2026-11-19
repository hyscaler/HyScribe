<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints the core shortcodes through the `the_content` filters over the
// Theme Test Data fixture, each case with a post as the global one:
//   feed<TAB>post ID<TAB>hex(content)<TAB>sha256(html)
// where feed is 1 for the cases after `is_feed()` turns true. The counters
// of the galleries, players and unique IDs run through the whole list, so
// the cases are in order. The last line is `assets`, the scripts the
// players enqueued: head and footer, hashed. Before it, the engine alone
// over inputs of its own, in hex:
//   atts<TAB>hex(text)<TAB>hex(serialize( shortcode_parse_atts( text ) ))
//   strip<TAB>hex(content)<TAB>hex(strip_shortcodes( content ))
//   unautop<TAB>hex(text)<TAB>hex(shortcode_unautop( text ))
// With RAW=1 the HTML itself is printed instead. Nothing is written.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

$GLOBALS['wp'] = new WP();
wp( '' );

$raw = getenv( 'RAW' );
$img = '<img src="http://x.test/a.jpg" width="10" height="10" alt="A" />';
$cap = function ( $attrs, $inner = null ) use ( $img ) {
	return '[caption ' . $attrs . ']' . ( $inner ?? $img . ' A caption' ) . '[/caption]';
};

$cases = array(
	// Captions.
	array( 555, $cap( 'id="attachment_611" align="aligncenter" width="300"' ) ),
	array( 555, $cap( 'id="attachment_611" align="aligncenter" width="300"' ) ),
	array( 555, '[wp_caption width="120" caption="Short" class="extra" align="alignleft"]' . $img . '[/wp_caption]' ),
	array( 555, $cap( 'width="100" caption="Hello <b>bold</b> <script>alert(1)</script>"', $img ) ),
	array( 555, '[caption]' . $img . ' text[/caption]' ),
	array( 555, '[caption width="50"]' . $img . '[/caption]' ),
	array( 555, '[caption width="50"]just text[/caption]' ),
	array( 555, $cap( 'id="a b" caption_id="cap" class="extra" width="50"', '<a href="/x">' . $img . '</a>text' ) ),
	array( 555, $cap( 'id="x" width="50" caption_id="cap"', $img . ' t' ) ),
	array( 555, $cap( 'id="x" width="50" caption_id="cap"', $img . ' t' ) ),
	array( 555, $cap( 'id="x_y" width="50"', '<A HREF="/q" class="z">  ' . strtoupper( $img ) . ' </a> tail <i>x</i>' ) ),
	array( 555, '[[caption width=1]x[/caption]] and [[caption]]' ),
	array( 555, $cap( 'width="30" caption="x"', '[gallery columns="2"]' ) ),
	array( 555, $cap( 'width="30" caption="a&amp;b &quot;q&quot;" align="alignright"', $img ) ),
	array( 555, "[caption width='30' caption='single']" . $img . '[/caption]' ),
	array( 555, '[caption width=30 caption=word]' . $img . '[/caption]' ),
	array( 555, '[caption width="30" caption="x"]' . $img ),
	array( 555, '[caption width="30" caption="x"/]' ),
	array( 555, "[caption\u{a0}width=\"30\"\u{200b}caption=\"z\"]" . $img . '[/caption]' ),
	array( 555, "[caption width=\"30\" caption=\"\xff\"]" . $img . ' bad[/caption]' ),
	array( 555, '[caption width="30" caption="<b>x"]' . $img . '[/caption]' ),
	array( 555, '[caption width="30" caption="a\tb\x41\101\q\\\\"]' . $img . '[/caption]' ),
	array( 555, '[caption WIDTH="30" Caption="up"]' . $img . '[/caption]' ),
	array( 555, '[caption width="30" width="40" caption="dup"]' . $img . '[/caption]' ),
	array( 555, '[caption 0="1" width="30" caption="zero" "quoted" \'single\' bare]' . $img . '[/caption]' ),
	array( 555, '[caption width="0" caption="x"]' . $img . '[/caption]' ),
	array( 555, '[caption width="9"]<img src="a.png"[/caption]' ),
	array( 555, '[caption width="9"]<a href="x">text</a> <img src="a.png" />cap [i][/caption]' ),
	array( 555, '[caption width="9"]<a >x</a><a href="y">  <img src="b.png" /> </a> after[/caption]' ),
	array( 555, '[caption width="30" caption="0"]' . $img . '[/caption]' ),
	array( 555, '[captions]x[/captions] [caption-foo] [CAPTION width=1] [caption' ),
	array( 555, '[caption width="30" caption="x" ]' . $img . '[/caption ]' ),
	array( 555, '[caption width="30" caption="a/b"]' . $img . '[/caption] [caption width=3 caption=/]' . $img . '[/caption]' ),
	array( 555, '[caption width="30" caption="x"]' . $img . '[/caption]]' ),
	array( 555, '[[caption width="30" caption="x"]' . $img . '[/caption]' ),
	array( 555, "[caption width=\"30\" caption=\"x\"]\n" . $img . "\n[/caption]\n\n[caption width=\"31\" caption=\"y\"]" . $img . '[/caption]' ),
	array( 555, "Intro\n\n[caption width=\"30\" caption=\"p\"]" . $img . "[/caption]\n\nOutro" ),
	array( 555, '<p> [caption width="30" caption="p"]' . $img . '[/caption]&nbsp;</p>' ),
	array( 555, '<p>&nbsp;[caption width="30" caption="p"]' . $img . "[/caption] \xC2\xA0</p> <p>[caption width=\"30\" caption=\"p\"]</p>" ),
	array( 555, '<p>[caption width="30" caption="p"/]</p><p>[caption width="3" caption="q"]x[/caption]tail</p>' ),
	array( 555, 'a &#91;caption width=1 caption=x&#93; b [caption width="30" caption="x"]' . $img . '[/caption] &#091;caption&#093;' ),
	array( 555, '<div title="a [b] c">[caption width="30" caption="in div"]' . $img . '[/caption]</div><!-- [caption width=1 caption=c] --> <span>[caption width="40" caption="v"]' . $img . '[/caption]</span>' ),
	// Left as they are or printed as nothing.
	array( 555, 'Before [unknown a=1] after [[unknown]]' ),
	// Galleries.
	array( 555, '[gallery]' ),
	array( 555, '[gallery columns="2" size="medium"]' ),
	array( 555, '[gallery link="file"]' ),
	array( 555, '[gallery link="none" size="full"]' ),
	array( 555, '[gallery ids="755,754,756"]' ),
	array( 555, '[gallery ids="756,754" orderby="title" order="DESC"]' ),
	array( 555, '[gallery ids="754 755" columns=0]' ),
	array( 555, '[gallery itemtag="p" icontag="span" captiontag="em"]' ),
	array( 555, '[gallery itemtag="script" icontag="1" captiontag="a" ids="611"]' ),
	array( 555, '[gallery exclude="611,616,617"]' ),
	array( 555, '[gallery include="770,771" exclude="770"]' ),
	array( 555, '[gallery id="1177"]' ),
	array( 555, '[gallery id="501" size="large" link="file"]' ),
	array( 555, '[gallery id="587"]' ),
	array( 555, '[gallery id="99999"]' ),
	array( 555, '[gallery id="1168"]' ),
	array( 555, '[gallery id="1164"]' ),
	array( 555, '[gallery id="1726"]' ),
	array( 555, '[gallery id="-5"]' ),
	array( 555, '[gallery include=","]' ),
	array( 555, '[gallery ids=""]' ),
	array( 555, '[gallery ids="0"]' ),
	array( 555, '[gallery ids="754,754,755" size="nonexistent"]' ),
	array( 555, '[gallery size="thumbnail" columns="-2" ids="754"]' ),
	array( 555, '[gallery orderby="ID" order="desc" ids="754,755"]' ),
	array( 555, '[gallery order="bogus" ids="754,755"]' ),
	array( 555, '[gallery link="0" ids="754"]' ),
	array( 555, '[gallery columns="3" ids="1687,1691"]' ),
	array( 555, '[gallery size="100x100" ids="754"]' ),
	array( 1177, '[gallery]' ),
	array( 1177, "A\n\n[gallery columns=\"2\"]\n\nB" ),
	array( 501, '[gallery]' ),
	array( 0, '[gallery]' ),
	array( 0, '[gallery ids="754"]' ),
	array( 587, '[gallery]' ),
	// Audio and video.
	array( 587, '[audio]' ),
	array( 587, '[audio src="http://x.test/a.mp3"]' ),
	array( 587, '[audio src="http://x.test/a.mp3?x=1"]' ),
	array( 587, '[audio mp3="http://x.test/a.mp3" ogg="http://x.test/a.ogg"]' ),
	array( 587, '[audio src="http://x.test/a.m4a" loop autoplay muted="true" preload="auto" class="c" style="width:10px"]' ),
	array( 587, '[audio src="http://x.test/a.mp3" preload="bogus"]' ),
	array( 555, '[audio]' ),
	array( 587, '[audio src="http://x.test/a.pdf"]' ),
	array( 587, '[audio src=\'http://x.test/<b>.mp3\']' ),
	array( 587, '[audio src="http://x.test/a.MP3" loop="false" autoplay="0" muted="FALSE"]' ),
	array( 587, '[audio wav="http://x.test/x.wav" flac="http://x.test/y.flac" mp3="http://x.test/z.txt"]' ),
	array( 587, '[audio ogg="http://x.test/a.ogg"]a[/audio]' ),
	array( 587, "[audio src=\"http://x.test/a.mp3\n\"]" ),
	array( 587, '<p>[audio src="http://x.test/a.mp3"]</p>' ),
	array( 555, '[video src="http://x.test/v.mp4"]' ),
	array( 555, '[video src="http://x.test/v.mp4" width="300" height="100" poster="http://x.test/p.jpg" loop autoplay muted="1" preload="none" class="vc"]' ),
	array( 555, "[video mp4=\"http://x.test/a.mp4\" webm=\"http://x.test/a.webm\"]x\r\ny\tz [/video]" ),
	array( 555, '[video src="https://www.youtube.com/watch?v=abc&feature=share"]' ),
	array( 555, '[video src="http://youtu.be/abc"]' ),
	array( 555, '[video src="https://vimeo.com/123?x=1" loop]' ),
	array( 555, '[video src="https://vimeo.com/124" loop="1"]' ),
	array( 555, '[video src="https://a.vimeo.com/123/456"]' ),
	array( 555, '[video src="http://x.test/v.mov"]' ),
	array( 555, '[video]' ),
	array( 555, '[video src="/v.mp4"]' ),
	array( 555, '[video src="http://x.test/v.mp4"] text [/video]' ),
	array( 555, '[video src="http://x.test/v.mp4"]line1' . "\n" . 'line2' . "\t" . '3[/video]' ),
	array( 555, '[video src="http://x.test/v.mp4" width="0"]' ),
	array( 555, '[video class="" src="http://x.test/v.mp4" preload=""]' ),
	array( 555, '[video src="http://x.test/v.mp4" width="640px" height="x"]' ),
	array( 555, '[video src="http://x.test/v.mp4" width="1000" height="500"]' ),
	array( 555, '[video src="https://notvimeo.com/123"]' ),
	array( 555, "[video src=\"https://x\nvimeo.com/123\"]" ),
	array( 555, '[video src="https://x.y.vimeo.com/123" poster="javascript:alert(1)"]' ),
	array( 555, '[audio src="http://x.test/b.mp3"] and [video src="http://x.test/b.mp4"]' ),
);

// After is_feed() turns true.
$feed_cases = array(
	array( 555, '[gallery]' ),
	array( 555, '[gallery link="file" size="medium"]' ),
	array( 555, '[gallery link="none" size="thumbnail" columns="2"]' ),
	array( 555, '[gallery link="none" size="none" ids="754"]' ),
	array( 555, '[gallery id="587"]' ),
	array( 555, '[caption width="30" caption="f"]' . $img . '[/caption]' ),
);

$run = function ( $id, $content, $feed ) use ( $raw ) {
	global $post;
	$post = $id ? get_post( $id ) : null;
	if ( $post ) {
		setup_postdata( $post );
	}
	$html = apply_filters( 'the_content', $content );
	echo $feed, "\t", $id, "\t", bin2hex( $content ), "\t", $raw ? $html : hash( 'sha256', $html ), "\n";
};

foreach ( $cases as list( $id, $content ) ) {
	$run( $id, $content, 0 );
}
global $wp_query;
$wp_query->is_feed = true;
foreach ( $feed_cases as list( $id, $content ) ) {
	$run( $id, $content, 1 );
}

$atts = array(
	'', '  ', 'a="b" c=\'d\' e=f "g" \'h\' i', 'a = "b"', 'a="b"c', 'a=', '=b', '"unterminated', "a='x",
	'a="1" a="2"', 'Ab-C="x"', '1="x" 2=y z', '0="zero" k=v 5', 'a="<b>"', 'a="<b"', 'a="x<y>z<"', 'a=b"c', 'a="b" "c',
	"\xff", "a=\u{a0}b\u{200b}\u{200b}c=d", 'a="\\x41\\101\\n\\\\\\q\\x\\xg\\"', '"" \'\' x', 'a=""', "a=''", "a=\"x\"\nb=\"y\"",
	"a=1\tb=2\r\nc=3\x0bd=4\x0ce=5", '-a=b a_b=c', 'a=b=c', 'a=\'', 'a="b"\'c\' "d"e', 'x y z', 'a= b', 'a ="b" c= d',
	'"a" "a" 0 "0" 007 "1"', '-5="x" y', '-a=b -5=c d', 'a="\\a\\b\\f\\r\\v\\t\\n"', 'a="\\7\\12\\400\\18"', "a=\"<a\\x3e<\"", 'caption="a<b>c</b>d" x="<"',
);
foreach ( $atts as $text ) {
	echo "atts\t", bin2hex( $text ), "\t", bin2hex( serialize( shortcode_parse_atts( $text ) ) ), "\n";
}
$strip = array(
	'', 'no shortcode', 'a [caption width=1]x[/caption] b', '[caption]x[/caption][gallery][audio a=b/][/audio]', '[[caption]] [[gallery]x[/gallery]]',
	'[embed]u[/embed] [playlist] [video src=a]', 'a &#91;caption&#93; <b title="[caption]">[caption]</b> [caption', '[caption]unclosed',
	'[caption x/] [caption x / ] [caption /]', "[gallery\nids=1]", '[caption][gallery]x[/caption]', '[gallery ]]', '[[gallery]',
	'[captionx] [caption-x] [caption_x] [wp_caption a]b[/wp_caption] [wp_caption]',
	'<!-- [caption] --> <![CDATA[ [gallery] ]]> [caption]', '< [caption] >', '[unknown][caption]', '[audio][/audio][audio][/gallery]',
);
foreach ( $strip as $text ) {
	echo "strip\t", bin2hex( $text ), "\t", bin2hex( strip_shortcodes( $text ) ), "\n";
}
$unautop = array(
	'', '<p>[gallery]</p>', '<p> [gallery columns=2] </p>', "<p>\n\t[caption a=b]x[/caption]\n</p>", '<p>[gallery]</p><p>[audio a=b/]</p>', '<p>[gallery]text</p>',
	'<p>text [gallery]</p>', '<p>[gallery]</p', '<P>[gallery]</P>', '<p>&nbsp;[gallery]&nbsp;</p>', "<p>\xC2\xA0[gallery]\xC2\xA0</p>", '<p>[unknown]</p>',
	'<p>[caption]x[/caption] y</p>', '<p>[caption]x[/caption][/caption]</p>', '<p>[caption]x[/caption]</p> <p>[gallery /]</p>', '<p>[caption x/]</p>',
	'<p>[caption x]</p>', '<p>[caption x/] text</p>', '<p>[caption x]y</p>', '<p>[caption x] [/caption]</p>', '<p>[[gallery]]</p>', '<p>[gallery', '<p><p>[gallery]</p>',
	'<p>[wp_caption]a[/wp_caption]</p><p>[caption-x]</p><p>[captionx]</p>', '<p>[caption a/b]</p>', '<p>[caption a/ b/]c[/caption]</p>', '<p>[embed]u[/embed]</p>',
);
foreach ( $unautop as $text ) {
	echo "unautop\t", bin2hex( $text ), "\t", bin2hex( shortcode_unautop( $text ) ), "\n";
}

ob_start();
wp_print_head_scripts();
print_footer_scripts();
$assets = ob_get_clean();
echo "assets\t", $raw ? $assets : hash( 'sha256', $assets ), "\n";

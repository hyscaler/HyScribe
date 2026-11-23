<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints wp_kses_post() and serialize_blocks( parse_blocks() ) for the
// theme's markup, the posts in the database, hostile markup and random
// fragments. Lines are
//   host<TAB>hex(upload host)
//   kses<TAB>hex(input)<TAB>hex(wp_kses_post(input))
//   serialize<TAB>hex(input)<TAB>hex(serialize_blocks(parse_blocks(input)))
// Read only: nothing is written to the database.

require '/var/www/html/wp-load.php';

$upload = wp_parse_url( wp_upload_dir( null, false )['url'] );
$host   = $upload['host'] . ( isset( $upload['port'] ) ? ':' . $upload['port'] : '' );
echo "host\t", bin2hex( $host ), "\n";

$inputs = array();
$theme  = get_theme_root() . '/twentytwentyfive';
foreach ( array_merge( glob( "$theme/templates/*.html" ), glob( "$theme/parts/*.html" ), glob( "$theme/patterns/*.php" ) ) as $file ) {
	$inputs[] = file_get_contents( $file );
}
foreach ( get_posts( array( 'post_type' => 'any', 'post_status' => 'any', 'numberposts' => -1 ) ) as $post ) {
	$inputs[] = $post->post_content;
}

$pdf = "http://$host/wp-content/uploads/a.pdf";
array_push(
	$inputs,
	'',
	'plain text',
	'<script>alert(1)</script>',
	'<img src=x onerror=alert(1)>',
	'<a href="javascript:alert(1)">x</a>',
	'<a href="jav&#x09;ascript:alert(1)">x</a>',
	'<a href=" javascript:alert(1)">x</a>',
	'<a href="JaVaScRiPt:alert(1)">x</a>',
	'<a href="http://example.com/?a=1&b=2" title="t">x</a>',
	'<a href="/relative" rel="nofollow" target="_blank">x</a>',
	'<svg/onload=alert(1)>',
	'<iframe src="//evil.example"></iframe>',
	'<div style="background:url(javascript:alert(1))">x</div>',
	'<div style="color:red;behavior:url(x)">x</div>',
	'<p style="color: red">x</p>',
	'<p style="COLOR: RED; width: calc(100% - 2px)">x</p>',
	'<p style="">x</p>',
	'<p style>x</p>',
	'<p style="color:red" style="color:blue">x</p>',
	'<p style="x:&quot;y&quot;">x</p>',
	"<object data=\"$pdf\" type=\"application/pdf\"></object>",
	"<object data=\"$pdf\" type=\"APPLICATION/PDF\"></object>",
	"<object data=\"$pdf\"></object>",
	"<object type=\"application/pdf\" data=\"$pdf?x\"></object>",
	"<object type=\"application/pdf\" data=\"$pdf#x\"></object>",
	'<object type="application/pdf" data="http://evil.example/a.pdf"></object>',
	"<object data=\"$pdf\" type=\"text/html\"/>",
	"<object data=\"$pdf\" type=\"text/html\" />",
	'<a download>x</a>',
	'<a download="y">x</a>',
	'<a data-foo="1" data-Bar="2" data-="3" data-x.y="4" data-a_b-c="5">x</a>',
	'<!-- comment -->',
	'<!-- a -- b --->',
	'<!--->',
	'<!-->',
	'<!---->',
	'<!-- <script>x</script> -->',
	"<!-- line\nbreak -->",
	"<!-- unterminated\n",
	"<!-- unterminated\nmore",
	'<!-- unterminated',
	'</ script>',
	'</>',
	'</>>',
	'</ <!-- a>',
	'<!DOCTYPE html>',
	'<!doctype html>',
	'<![CDATA[x]]>',
	'<?php echo 1; ?>',
	'a < b',
	'a > b',
	"a <\n",
	"a <b\n",
	'<<a>>',
	'<a <b>',
	'<a title="x>y">z</a>',
	"<a title='x'>z</a>",
	'<a title=x y=z>z</a>',
	'<A HREF="http://example.com" TITLE="T">z</A>',
	'<br/>',
	'<br />',
	'<img src="x.png" / >',
	'<img src="x.png" alt="a" / >',
	"<p\nclass=\"x\">y</p>",
	"x\x00y\\0z\x07",
	'Tom &amp; Jerry & co &copy; &bogus; &#65; &#x110000; &#X41;',
	'<p title="&quot;&lt;&#39;&apos;">x</p>',
	'<td colspan="2" style="width:50%">x</td>',
	'<input type="text" name="q">',
	'<form action="/x"><input></form>',
	'<abbr title=\"x\">y</abbr>',
	'<abbr title=\\"x\\">y</abbr>',
	"<p>\xff</p>",
	"<p title=\"\xff\">x</p>",
	"a <\xff b",
	"<a\x00 href=x>y</a>",
	'<p class="a" class="b">x</p>',
	'<p unknown="1" class="c">x</p>',
	'<custom-element>x</custom-element>',
	'<p 1="2">x</p>',
	'<p =x>y</p>',
	'<p "x">y</p>',
	'<p class="x',
	'<p class="x" ',
	'<p/class="x">y</p>',
	'<area href="javascript:x" shape="rect">',
	'<img src="data:image/png;base64,AAAA" srcset="x 1x">',
	'<video src="http://x/v.mp4" poster="javascript:x" controls></video>',
	'<blockquote cite="javascript:x">q</blockquote>',
	'<a href="&#106;avascript:alert(1)">x</a>',
	'<a href="&#0000106avascript:alert(1)">x</a>',
	'<a href="http&#58;//example.com">x</a>',
	'<!-- wp:paragraph {"content":"<script>x</script>"} --><p>x</p><!-- /wp:paragraph -->',
	'<!-- wp:template-part {"tagName":"script","slug":"x"} /-->',
	'<!-- wp:template-part {"tagName":"header","slug":"x"} /-->',
	'<!-- wp:template-part {"tagName":"HEADER","slug":"x"} /-->',
	'<!-- wp:template-part {"tagName":"","slug":"x"} /-->',
	'<!-- wp:template-part {"tagName":7,"slug":"x"} /-->',
	'<!-- wp:template-part {"inner":{"tagName":"script"}} /-->',
	'<!-- wp:core/group {"a<b>":"c","n":[1,"<i>",{"x":"<img src=x onerror=1>"}],"f":1.5,"t":true,"z":null} --><div>a</div><!-- /wp:core/group -->',
	'<!-- wp:group {"\u003cx\u003e":"1","":"2","5":"3"} /-->',
	'<!-- wp:group {"a\u003ci\u003e":"1","a":"2"} /-->',
	'<!-- wp:group {"a":"1","a\u003ci\u003e":"2"} /-->',
	'<!-- wp:group {"1\u003cb\u003e2":"x","n":{"x":{"y":"\u003cscript\u003e"}}} /-->',
	'<!-- wp:x {"a":"--><script>"} /-->',
	'<!-- wp:p -->0<!-- /wp:p -->',
	'<!-- wp:p --><!-- /wp:p -->',
	'<!-- wp:x {"q":"\\"\\\\","u":"é\u2028/"} /-->',
	'<!-- wp:x {"q":"\\"a"} /-->',
	'<!-- wp:x {"big":1e400} /-->',
	'<!-- wp:x {"bad" /-->',
	'<!-- wp:x {"bad":} /-->',
	'<!-- wp:x {"k\u003cscript\u003e":"v"} --><p>x</p><!-- /wp:x -->',
	'<!-- wp:x {"a":"&amp; & <"} /-->',
	'<!-- wp:paragraph --->x<!-- /wp:paragraph --->',
	'<!-- a ---->',
	"<!-- a -\n-->",
	"<!-- a>b -->\nc",
	"<!-- a>\n-->x",
	"<!-- wp:p {\"a\":\"\\u003cscript\\u003e\"} --->\nx<!-- /wp:p --->",
	"<!-- wp:p {\"a\":\"\\u003cscript\\u003e\"} --->x\n<!-- /wp:p --->",
	'<p title="&gt;&lt;&amp;">x</p>',
	"<!-- a\n--->",
	'<!-- wp:group --><div><!-- wp:paragraph {"x":"<b>y</b>"} --><p>a</p><!-- /wp:paragraph --></div><!-- /wp:group -->'
);

$fragments = array( '<', '>', '</', '<!--', '-->', '--->', '<!', 'a', 'p', 'div', 'script', 'object', ' ', '=', '"', "'", '\\"', "\n", 'href', 'style', 'title', 'javascript:', 'color:red', '&', '&amp;', 'data-x', '/', 'b', '<!-- wp:paragraph ', '<!-- /wp:paragraph -->', '{"a":"<b>"}', '/-->', 'onerror', 'x', "\x00", '\\0', 'é' );
mt_srand( 7 );
for ( $i = 0; $i < 3000; $i++ ) {
	$input = '';
	$count = mt_rand( 1, 30 );
	for ( $j = 0; $j < $count; $j++ ) {
		$input .= $fragments[ mt_rand( 0, count( $fragments ) - 1 ) ];
	}
	$inputs[] = $input;
}

foreach ( $inputs as $input ) {
	echo "kses\t", bin2hex( $input ), "\t", bin2hex( wp_kses_post( $input ) ), "\n";
	echo "serialize\t", bin2hex( $input ), "\t", bin2hex( serialize_blocks( parse_blocks( $input ) ) ), "\n";
}

<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for hyscribe-rest's url-details parser: the private methods of
// WP_REST_URL_Details_Controller from a WordPress 7.1.3 tree, called by
// reflection on pages built from the snippets below, with the charset UTF-8.
// Lines are hex(page)<TAB>hex(url)<TAB>hex(title)<TAB>hex(icon)<TAB>hex(description)<TAB>hex(image).
//   php parse-golden.php <wordpress> > crates/hyscribe-rest/tests/golden/url-details-parse.tsv

$root = rtrim( $argv[1], '/' ) . '/wp-includes/';
function hs_extract( $file, $name, $indent = '' ) {
	preg_match( '/\n' . $indent . '(?:public static )?function ' . $name . '\(.*?\n' . $indent . '}\n/s', file_get_contents( $file ), $found );
	return $found[0];
}
foreach ( array( 'wp_parse_url', '_get_component_from_parsed_url_array', '_wp_translate_php_url_constant_to_key' ) as $name ) {
	eval( hs_extract( $root . 'http.php', $name ) );
}
eval( hs_extract( $root . 'formatting.php', 'wp_strip_all_tags' ) );
eval( 'class WP_Http {' . hs_extract( $root . 'class-wp-http.php', 'make_absolute_url', '\t' ) . '}' );
function get_bloginfo( $show ) {
	return 'UTF-8';
}
function __( $text ) {
	return $text;
}
class WP_REST_Controller {}
require $root . 'rest-api/endpoints/class-wp-rest-url-details-controller.php';

$controller = new WP_REST_URL_Details_Controller();
$call       = function ( $method, ...$args ) use ( $controller ) {
	$reflection = new ReflectionMethod( $controller, $method );
	return $reflection->invoke( $controller, ...$args );
};

$pages = array(
	'',
	'0',
	'<title>0</title>',
	'<title></title>',
	'<title>  spaced  </title>',
	'<TITLE x="1">Upper</TITLE>',
	'<titles>Prefixed</titles></title>',
	'<title>Unclosed',
	'<title Unclosed tag',
	"<title>a</title >< / title>",
	"<title>first< \n/\t title>",
	'<title>&amp;lt;b&amp;gt; &#39;q&#39; &apos;a&apos; &quot;d&quot; &bogus; &#0; &#xD800; &#1114112;</title>',
	'<title><b>bold</b><script>x()</script>after</title>',
	'<title>&#1;&#9;&#127;&#128;&#159;&#160;&#xFFFE;&#x1FFFF;&#xFDD0;&#xFDF0;&#xE000;&#55295;&#57344;</title>',
	"<title>\xff\xfe broken</title>",
	'<head><title>In head</title></head><title>Late</title>',
	'x<head>abcdef</head>',
	'</head><head><title>Before</title>',
	'<head><title>Body</title><body><title>No</title>',
	'<head><title>Neither</title>',
	'<link rel="icon" href="/a.ico">',
	'<link rel=icon href=/b.ico>',
	"<link\trel='shortcut icon' href='c.ico'>",
	'<link rel="icon shortcut" href="d.ico">',
	'<link rel="apple-touch-icon" href="e.ico"><link rel="ICON" HREF="F.ico">',
	'<link href="g.ico" rel="icon">',
	'<link data-rel="icon" href="h.ico">',
	'<link rel= " icon " href="i.ico">',
	'<link rel="stylesheet icon" href="j.ico">',
	'<link rel="icon" href="">',
	'<link rel="icon" href="0">',
	'<link rel="icon" href=\'k.ico" x>',
	'<link rel="icon" href="l.ico\' y="z">',
	"<link rel=\"icon\" href='m.i'co' n='o'>",
	'<link rel="icon" href=" spaced.ico">',
	'<link rel="icon" href="data:image/png;base64,AAAA">',
	'<link rel="icon" href="DATA:image/png,x">',
	'<link rel="icon" href="//cdn.test/n.ico">',
	'<link rel="icon" href="https://other.test/o.ico">',
	'<link rel="icon" href="../../p.ico">',
	'<link rel="icon">',
	'<link rel="icon" href="q.ico"',
	'<link rel="icon" <link rel="icon" href="r.ico">',
	'<linkrel="icon" href="s.ico">',
	'<link rel="icon" href="t.ico"/>',
	'<link rel=">icon" href="u.ico">',
	'<meta name="description" content="Plain">',
	'<meta content="Reversed" name="description">',
	'<meta name="og:description" content="OG">',
	'<meta name="twitter:description" content="Twitter"><meta name="description" content="Second">',
	'<meta name=description content="Unquoted name">',
	'<meta name=descriptions content="Prefix">',
	'<meta name="description" content=Unquoted>',
	'<meta name="description" content="">',
	'<meta name="description" content="0">',
	'<meta name="description" content=" padded ">',
	'<meta name="description" content="Has > inside">',
	'<meta name="description" content="Unclosed>',
	'<meta name="description" content="Unclosed>">',
	'<meta name="description" content="Unclosed>"',
	'<meta name="description" content="a"b">',
	"<meta name='description' content='it's'>",
	'<meta content="abc><meta name="description" content="real">',
	'<meta name="description" content="x"><meta name="description" content="y">',
	'<meta name=" description " content="Spaced name">',
	'<meta name="description " content="Trailing">',
	'<meta name="DESCRIPTION" CONTENT="Upper">',
	"<meta\nname=\"description\"\ncontent=\"Lines\">",
	'<meta name="description" content="&lt;b&gt;x&lt;/b&gt; &amp;amp;">',
	'<meta name="description"',
	'<meta name="description" content="none" <meta content="later" name="description">',
	'<metaname="description" content="glued">',
	'<meta property="og:image" content="/a.png">',
	'<meta property="og:image:url" content="b.png">',
	'<meta property="og:image:width" content="300"><meta property="og:image" content="c.png">',
	'<meta property=og:image:width content="300"><meta property="og:image" content="d.png">',
	'<meta property="og:image" content="">',
	'<meta property="og:image" content="https://cdn.test/e.png">',
	'<meta property="og:image" content="data:image/png,x">',
	"<meta property='og:image' content='../f.png'>",
	'<meta name="og:image" content="g.png">',
	'<meta property="og:image" content="h.png"><meta property="og:image" content="i.png">',
	'<meta property="og:imagex" content="j.png">',
	'<meta charset="utf-8"><meta property="og:image" content="k.png">',
);
$urls = array( 'http://pages.test:8080/dir/page.html', 'https://pages.test', 'pages.test/x' );
foreach ( $pages as $index => $page ) {
	$url  = $urls[ $index % count( $urls ) ];
	$head = $call( 'get_document_head', $page );
	$meta = $call( 'get_meta_with_content_elements', $head );
	$out  = array( $page, $url, $call( 'get_title', $head ), $call( 'get_icon', $head, $url ), $call( 'get_description', $meta ), $call( 'get_image', $meta, $url ) );
	echo implode( "\t", array_map( 'bin2hex', $out ) ), "\n";
}

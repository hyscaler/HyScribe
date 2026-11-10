<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints the time to read block over posts built in memory, as the home
// page's loop sets each up. Lines are
//   hex(markup)<TAB>hex(content)<TAB>hex(html)
// Nothing is written.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

$GLOBALS['wp'] = new WP();
wp( '' );

$long     = trim( str_repeat( 'word ', 400 ) );
$contents = array(
	'',
	" \n\t",
	'One',
	'One two three',
	'<p>One</p><p>two</p>',
	'<!-- wp:paragraph --><p>Block <em>marked</em> up</p><!-- /wp:paragraph -->',
	'a<!-- hidden words here -->b',
	'a <!-- never closed b c',
	'one&nbsp;two&#160;three&NBSP;four',
	'one &amp; two &#8212; three &bogus four; five',
	'one--two three—four',
	'1 2 3 ! ? ok',
	"tab\tsep\nnew\rline",
	"wide\u{3000}space\u{1680}ogham",
	"emoji \u{1F600} here",
	"\u{00D7} times \u{00F7} divide \u{2013} dash",
	"bad \xff byte",
	"bad &\xff; byte",
	'< p>not a tag</p> <3 hearts',
	'<a href="x">link</a>text',
	"teaser words<!--more-->hidden rest of words",
	"page one words<!--nextpage-->page two",
	$long,
	trim( str_repeat( 'word ', 189 ) ),
	trim( str_repeat( 'word ', 95 ) ),
	trim( str_repeat( 'word ', 1000 ) ),
);
$attrs = array(
	array(),
	array( 'displayAsRange' => false ),
	array( 'displayMode' => 'words' ),
	array( 'displayMode' => 'other' ),
	array( 'averageReadingSpeed' => 10 ),
	array( 'averageReadingSpeed' => 2.5, 'displayAsRange' => false ),
	array( 'textAlign' => 'center', 'fontSize' => 'small' ),
);

foreach ( $contents as $i => $content ) {
	$post = new WP_Post(
		(object) array(
			'ID'           => 9000 + $i,
			'post_author'  => 1,
			'post_title'   => "Case $i",
			'post_name'    => "case-$i",
			'post_content' => $content,
			'post_status'  => 'publish',
			'post_type'    => 'post',
			'filter'       => 'raw',
		)
	);
	wp_cache_add( $post->ID, $post, 'posts' );
	$GLOBALS['post'] = $post;
	setup_postdata( $post );
	foreach ( $attrs as $a ) {
		$json   = $a ? ' ' . wp_json_encode( $a, JSON_UNESCAPED_SLASHES ) : '';
		$markup = "<!-- wp:post-time-to-read$json /-->";
		$block  = parse_blocks( $markup )[0];
		$html   = ( new WP_Block( $block, array( 'postId' => $post->ID, 'postType' => 'post' ) ) )->render();
		echo bin2hex( $markup ), "\t", bin2hex( $content ), "\t", bin2hex( $html ), "\n";
	}
}

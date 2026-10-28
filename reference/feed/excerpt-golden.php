<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for the excerpt and content a feed prints, over posts that live
// only in the object cache. Lines are
//   post<TAB>hex(json {content, excerpt, password})<TAB>hex(get_the_excerpt())<TAB>hex(the_excerpt_rss)<TAB>hex(get_the_content_feed( 'rss2' ))
//   ent2ncr<TAB>hex(text)<TAB>hex(ent2ncr( text ))
//   decode<TAB>hex(text)<TAB>hex(html_entity_decode( text, ENT_COMPAT ))
// Shortcodes and emoji, which HyScribe does not port yet, are left out.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

// The elements support (wp-elements-* classes) is not ported yet.
remove_filter( 'render_block', 'wp_render_elements_class_name', 10 );
remove_filter( 'render_block_data', 'wp_render_elements_support_styles', 10 );

$GLOBALS['wp_query']->is_feed = true;

$words = static fn( $n, $word = 'word' ) => implode( ' ', array_fill( 0, $n, $word ) );
$p     = static fn( $text ) => "<!-- wp:paragraph -->\n<p>$text</p>\n<!-- /wp:paragraph -->";
$cases = array(
	array( $p( 'Welcome to WordPress. This is your first post. Edit or delete it, then start writing!' ), '' ),
	array( $p( 'Body' ), 'Hand written &amp; <b>bold</b> excerpt "quoted" -- wordpress' ),
	array( $p( 'Body' ), "  \n\t " ),
	array( $p( $words( 55 ) ), '' ),
	array( $p( $words( 56 ) ), '' ),
	array( $p( $words( 80, "one\ttwo" ) ), '' ),
	array( "Classic text, no blocks.\n\nSecond paragraph &amp; more <em>emphasis</em>.", '' ),
	array( "<!-- wp:heading -->\n<h2 class=\"wp-block-heading\">Heading</h2>\n<!-- /wp:heading -->\n\n" . $p( 'After' ), '' ),
	array( "<!-- wp:image -->\n<figure class=\"wp-block-image\"><img src=\"x.png\" alt=\"Alt text\"/><figcaption>Caption</figcaption></figure>\n<!-- /wp:image -->\n\n" . $p( 'Text' ), '' ),
	array( "<!-- wp:group -->\n<div class=\"wp-block-group\">" . $p( 'In a group' ) . "<!-- wp:image -->\n<figure class=\"wp-block-image\"><img src=\"x.png\" alt=\"\"/></figure>\n<!-- /wp:image --><!-- wp:group -->\n<div class=\"wp-block-group\">" . $p( 'Nested' ) . "</div>\n<!-- /wp:group --></div>\n<!-- /wp:group -->", '' ),
	array( "<!-- wp:columns -->\n<div class=\"wp-block-columns\"><!-- wp:column -->\n<div class=\"wp-block-column\">" . $p( 'Column one' ) . "</div>\n<!-- /wp:column --><!-- wp:column -->\n<div class=\"wp-block-column\">" . $p( 'Column two' ) . "</div>\n<!-- /wp:column --></div>\n<!-- /wp:columns -->", '' ),
	array( "<!-- wp:quote -->\n<blockquote class=\"wp-block-quote\">" . $p( 'Quoted' ) . "</blockquote>\n<!-- /wp:quote -->", '' ),
	array( "<!-- wp:quote -->\n<blockquote class=\"wp-block-quote\">" . $p( 'Quoted' ) . "<!-- wp:image -->\n<figure class=\"wp-block-image\"><img src=\"x.png\" alt=\"\"/></figure>\n<!-- /wp:image --></blockquote>\n<!-- /wp:quote -->\n\n" . $p( 'Outside' ), '' ),
	array( "<!-- wp:list -->\n<ul class=\"wp-block-list\"><!-- wp:list-item -->\n<li>One</li>\n<!-- /wp:list-item --></ul>\n<!-- /wp:list -->", '' ),
	array( "<!-- wp:html -->\n<div>Raw <script>alert(1)</script> html</div>\n<!-- /wp:html -->", '' ),
	array( "<!-- wp:paragraph {\"metadata\":{\"blockVisibility\":false}} -->\n<p>Hidden</p>\n<!-- /wp:paragraph -->\n\n" . $p( 'Shown' ), '' ),
	array( $p( 'Note<sup data-fn="a1" class="fn"><a href="#a1" id="a1-link">1</a></sup> here' ), '' ),
	array( $p( "Notes<sup data-fn=\"a1\" class=\"fn\">\n <a href=\"#a1\" id=\"a1-link\">12</a> </sup>, <sup data-fn=\"\" class=\"fn\"><a href=\"#\" id=\"x\">2</a></sup>, <sup data-fn=\"b\" class=\"fn\"><a href=\"#b\" id=\"b\">x</a></sup> and <sup data-fn=\"c\" class=\"fn\"><a href=\"#c\" id=\"c\">3</a>, <sup data-fn=\"d\" class=\"fn\"><a href=\"#d\" id=\"d\"></a></sup>" ), '' ),
	array( "<!-- wp:group -->\n<div class=\"wp-block-group\"></div>\n<!-- /wp:group -->\n\n<!-- wp:columns -->\n<div class=\"wp-block-columns\"><!-- wp:image -->\n<figure class=\"wp-block-image\"><img src=\"x.png\" alt=\"\"/></figure>\n<!-- /wp:image --><!-- wp:column {\"metadata\":{\"blockVisibility\":false}} -->\n<div class=\"wp-block-column\">" . $p( 'Hidden column' ) . "</div>\n<!-- /wp:column --></div>\n<!-- /wp:columns -->\n\n" . $p( 'Last' ), '' ),
	array( $p( 'Teaser' ) . "\n\n<!-- wp:more -->\n<!--more-->\n<!-- /wp:more -->\n\n" . $p( 'Rest' ), '' ),
	array( $p( 'Page one' ) . "\n\n<!-- wp:nextpage -->\n<!--nextpage-->\n<!-- /wp:nextpage -->\n\n" . $p( 'Page two' ), '' ),
	array( $p( 'Tom &amp; Jerry &hellip; &copy; &nbsp;space "double" \'single\' -- (c) ... 1/2 wordpress | pipe ]]> end' ), '' ),
	array( $p( "Multibyte \u{00e9}t\u{00e9} \u{5c71}\u{7530} words" ), '' ),
	array( $p( 'Secret' ), 'Secret excerpt', 'pw' ),
	array( '', '' ),
	array( $p( '' ), '' ),
);
foreach ( $cases as $n => $case ) {
	list( $content, $excerpt ) = $case;
	$post = new WP_Post(
		(object) array(
			'ID'            => 9000 + $n,
			'post_author'   => 1,
			'post_date'     => '2026-10-27 08:08:15',
			'post_date_gmt' => '2026-10-27 08:08:15',
			'post_content'  => $content,
			'post_excerpt'  => $excerpt,
			'post_password' => $case[2] ?? '',
			'post_status'   => 'publish',
			'post_type'     => 'post',
			'post_name'     => 'case-' . $n,
			'filter'        => 'raw',
		)
	);
	wp_cache_set( $post->ID, $post, 'posts' );
	$GLOBALS['post'] = $post;
	setup_postdata( $post );
	$the_excerpt = get_the_excerpt();
	ob_start();
	the_excerpt_rss();
	$rss = ob_get_clean();
	$feed = get_the_content_feed( 'rss2' );
	$input = array( 'content' => $content, 'excerpt' => $excerpt, 'password' => $case[2] ?? '' );
	echo "post\t", bin2hex( wp_json_encode( $input ) ), "\t", bin2hex( $the_excerpt ), "\t", bin2hex( $rss ), "\t", bin2hex( $feed ), "\n";
}
foreach ( array( '', 'plain', '&amp; &hellip; &nbsp; &copy;', '&Agrave;&agrave;&AGRAVE;', '&unknown; &amp &; &#38; &#x26;', 'a | b || c', '&amp;amp;', 'lamp; x&amp', '&hearts;&diams;&sube;&sub;&thetasym;', "&amp;\n&lt;tag&gt;" ) as $text ) {
	echo "ent2ncr\t", bin2hex( $text ), "\t", bin2hex( ent2ncr( $text ) ), "\n";
}
foreach ( array( 'plain', 'x &amp', 'x &amp; &lt;&gt;&quot;', '&#039; &#39; &#x27;', '&eacute;&brkbar;&nosuch;', '&#65;&#x41;&#X41;&#; &#x;', 'lamp; &amp' ) as $text ) {
	echo "decode\t", bin2hex( $text ), "\t", bin2hex( html_entity_decode( $text, ENT_COMPAT, 'UTF-8' ) ), "\n";
}

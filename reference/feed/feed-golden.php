<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for the posts feeds: the home feed through feed-rss2.php,
// feed-atom.php and feed-rdf.php, with options and post 1 changed inside a
// transaction that is rolled back. Lines are
//   hex(json state)<TAB>hex(RSS2 feed)<TAB>hex(Atom feed)<TAB>hex(RDF feed)
// where the state has the options, post 1's changed fields, its enclosure
// field, its tags and its category's name.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

// The elements support (wp-elements-* classes) is not ported yet.
remove_filter( 'render_block', 'wp_render_elements_class_name', 10 );
remove_filter( 'render_block_data', 'wp_render_elements_support_styles', 10 );

global $wpdb;
$cases = array(
	array(),
	array( 'options' => array( 'rss_use_excerpt' => '1' ) ),
	array( 'options' => array( 'blogdescription' => 'Tag <b>line</b> & "more"', 'blogname' => 'Name & <i>co</i>' ) ),
	array( 'post' => array( 'comment_status' => 'closed', 'comment_count' => 0 ) ),
	array( 'post' => array( 'comment_count' => 0 ) ),
	array( 'post' => array( 'post_content' => '', 'post_excerpt' => 'Only <em>an</em> excerpt & co' ) ),
	array( 'post' => array( 'post_title' => 'Title &amp; <b>bold</b> "q" &hellip; | pipe', 'post_content' => "<!-- wp:paragraph -->\n<p>Has ]]> inside</p>\n<!-- /wp:paragraph -->" ) ),
	array( 'post' => array( 'post_password' => 'pw' ) ),
	array( 'post' => array( 'post_content' => "<!-- wp:paragraph -->\n<p>Teaser</p>\n<!-- /wp:paragraph -->\n\n<!-- wp:more -->\n<!--more-->\n<!-- /wp:more -->\n\n<!-- wp:paragraph -->\n<p>Rest</p>\n<!-- /wp:paragraph -->" ) ),
	array( 'enclosure' => "http://example.test/a b.mp3\n 123 \naudio/mpeg extra" ),
	array( 'enclosure' => "http://example.test/a.mp3\n123" ),
	array( 'enclosure' => "javascript:alert(1)\n-5\n<b>x</b>" ),
	array( 'enclosure' => "http://example.test/b.ogg\naudio/ogg\n 1e3" ),
	array( 'enclosure' => "http://example.test/c.exe\n12\napplication/x-msdownload" ),
	array( 'tags' => array( 'Feed tag', 'Uncategorized', 'Cats &amp; Dogs' ) ),
	array( 'options' => array( 'html_type' => 'application/xhtml+xml' ) ),
	array( 'user_url' => '' ),
	array( 'category' => 'Cats & Dogs "quoted" &#039;single&#039; &#38; &eacute; &#x26; &#X41; &#x27; &#xZZ; &#; &#x; &#99999999999; &#xD800; &brkbar; &nosuch; lamp; &amp' ),
);
foreach ( $cases as $case ) {
	$wpdb->query( 'START TRANSACTION' );
	foreach ( $case['options'] ?? array() as $name => $value ) {
		add_filter( "pre_option_$name", static fn() => $value );
	}
	if ( isset( $case['post'] ) ) {
		$wpdb->update( $wpdb->posts, $case['post'], array( 'ID' => 1 ) );
	}
	if ( isset( $case['enclosure'] ) ) {
		add_post_meta( 1, 'enclosure', $case['enclosure'] );
	}
	if ( isset( $case['tags'] ) ) {
		wp_set_post_tags( 1, $case['tags'] );
	}
	if ( isset( $case['user_url'] ) ) {
		$wpdb->update( $wpdb->users, array( 'user_url' => $case['user_url'] ), array( 'ID' => 1 ) );
	}
	if ( isset( $case['category'] ) ) {
		$wpdb->update( $wpdb->terms, array( 'name' => $case['category'] ), array( 'term_id' => 1 ) );
	}
	wp_cache_flush();
	$_SERVER['REQUEST_URI'] = '/?feed=rss2';
	$GLOBALS['wp']->main( array( 'feed' => 'rss2' ) );
	ob_start();
	load_template( ABSPATH . WPINC . '/feed-rss2.php', false );
	$rss = ob_get_clean();
	$_SERVER['REQUEST_URI'] = '/?feed=atom';
	$GLOBALS['wp']->main( array( 'feed' => 'atom' ) );
	ob_start();
	load_template( ABSPATH . WPINC . '/feed-atom.php', false );
	$atom = ob_get_clean();
	$_SERVER['REQUEST_URI'] = '/?feed=rdf';
	$GLOBALS['wp']->main( array( 'feed' => 'rdf' ) );
	ob_start();
	load_template( ABSPATH . WPINC . '/feed-rdf.php', false );
	$rdf = ob_get_clean();
	$state = $case + array( 'options' => array() );
	if ( isset( $case['tags'] ) ) {
		$state['tags'] = wp_list_pluck( get_the_tags( 1 ), 'name' );
	}
	echo bin2hex( wp_json_encode( $state ) ), "\t", bin2hex( $rss ), "\t", bin2hex( $atom ), "\t", bin2hex( $rdf ), "\n";
	foreach ( $case['options'] ?? array() as $name => $value ) {
		remove_all_filters( "pre_option_$name" );
	}
	$wpdb->query( 'ROLLBACK' );
	wp_cache_flush();
}

<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for the RSS2 comment feeds through feed-rss2-comments.php, with
// comments inserted and posts changed inside a transaction that is rolled
// back. Lines are
//   hex(json state)<TAB>hex(feed)
// where the state has the query, the options, post 1's changed fields and
// the comment rows the feed lists, in its order.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

global $wpdb;
$insert = static function ( $fields ) {
	return wp_insert_comment(
		$fields + array(
			'comment_post_ID'      => 1,
			'comment_author'       => 'Reader',
			'comment_author_email' => 'reader@example.test',
			'comment_author_url'   => '',
			'comment_content'      => 'Text',
			'comment_approved'     => 1,
			'comment_date'         => '2026-10-28 09:00:00',
			'comment_date_gmt'     => '2026-10-28 09:00:00',
		)
	);
};
$setup = static function () use ( $insert ) {
	$insert( array( 'comment_author' => 'Linker', 'comment_content' => "See http://example.test/a?b=1&c=2 :) \"quoted\" -- dash\n\nSecond <b>bold & more</b> &amp; <script>x</script>", 'comment_date_gmt' => '2026-10-28 10:00:00' ) );
	$insert( array( 'comment_author' => '', 'user_id' => 1, 'comment_date_gmt' => '2026-10-28 11:00:00' ) );
	$insert( array( 'comment_author' => '', 'comment_date_gmt' => '2026-10-28 12:00:00' ) );
	$insert( array( 'comment_author' => 'Pinger', 'comment_type' => 'pingback', 'comment_date_gmt' => '2026-10-28 13:00:00' ) );
	$insert( array( 'comment_author' => 'Noter', 'comment_type' => 'note', 'comment_date_gmt' => '2026-10-28 14:00:00' ) );
	$insert( array( 'comment_author' => 'Held', 'comment_approved' => 0, 'comment_date_gmt' => '2026-10-28 15:00:00' ) );
	$insert( array( 'comment_author' => 'On the page &amp; <i>co</i>', 'comment_post_ID' => 2, 'comment_date_gmt' => '2026-10-28 16:00:00' ) );
	$draft = wp_insert_post( array( 'post_title' => 'Draft', 'post_status' => 'draft' ) );
	$insert( array( 'comment_author' => 'On a draft', 'comment_post_ID' => $draft, 'comment_date_gmt' => '2026-10-28 17:00:00' ) );
};
$cases = array(
	array( 'query' => array( 'feed' => 'comments-rss2' ) ),
	array( 'query' => array( 'feed' => 'comments-rss2' ), 'setup' => true ),
	array( 'query' => array( 'feed' => 'comments-rss2' ), 'setup' => true, 'options' => array( 'posts_per_rss' => '3' ) ),
	array( 'query' => array( 'feed' => 'comments-rss2' ), 'setup' => true, 'post' => array( 'post_password' => 'pw' ) ),
	array( 'query' => array( 'feed' => 'comments-rss2' ), 'setup' => true, 'options' => array( 'blogname' => 'Name & <i>co</i>', 'blogdescription' => 'A <b>tag</b> line' ) ),
	array( 'query' => array( 'p' => 1, 'feed' => 'rss2' ), 'setup' => true ),
	array( 'query' => array( 'p' => 1, 'feed' => 'rss2' ), 'setup' => true, 'post' => array( 'post_password' => 'pw' ) ),
	array( 'query' => array( 'p' => 1, 'feed' => 'rss2' ), 'setup' => true, 'post' => array( 'post_title' => 'Title &amp; <b>bold</b> | pipe' ) ),
	array( 'query' => array( 'page_id' => 2, 'feed' => 'rss2' ), 'setup' => true ),
	array( 'query' => array( 'page_id' => 2, 'feed' => 'rss2' ) ),
);
foreach ( $cases as $case ) {
	$wpdb->query( 'START TRANSACTION' );
	foreach ( $case['options'] ?? array() as $name => $value ) {
		add_filter( "pre_option_$name", static fn() => $value );
	}
	if ( isset( $case['setup'] ) ) {
		$setup();
	}
	if ( isset( $case['post'] ) ) {
		$wpdb->update( $wpdb->posts, $case['post'], array( 'ID' => 1 ) );
	}
	wp_cache_flush();
	$_SERVER['REQUEST_URI'] = '/?' . http_build_query( $case['query'] );
	$GLOBALS['wp']->main( $case['query'] );
	ob_start();
	load_template( ABSPATH . WPINC . '/feed-rss2-comments.php', false );
	$feed  = ob_get_clean();
	$state = array(
		'query'    => $case['query'],
		'options'  => $case['options'] ?? array(),
		'post'     => $case['post'] ?? array(),
		'comments' => array_map( 'get_object_vars', $GLOBALS['wp_query']->comments ),
		'users'    => array( 1 => get_userdata( 1 )->display_name ),
	);
	echo bin2hex( wp_json_encode( $state ) ), "\t", bin2hex( $feed ), "\n";
	foreach ( $case['options'] ?? array() as $name => $value ) {
		remove_all_filters( "pre_option_$name" );
	}
	$wpdb->query( 'ROLLBACK' );
	wp_cache_flush();
}

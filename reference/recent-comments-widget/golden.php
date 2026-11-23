<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints do_blocks() for the recent comments widget in a legacy widget
// block (signed under the fixed salt `test-salt`), over comments inserted
// for it and rolled back after. Lines are
//   hex(markup)<TAB>hex(state JSON)<TAB>hex(html)
// where the state holds the comments the widget may show, newest first (ID,
// post ID, author, author URL, user ID), their posts (ID, title, password,
// type, name, date) and the users (ID, display name). The widget's
// `static $first_instance` carries from line to line.

define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

global $wpdb;
$wpdb->query( 'START TRANSACTION' );
add_filter( 'salt', static fn() => 'test-salt' );

$add_post = static function ( $title, $extra = array() ) {
	return wp_insert_post(
		array_merge(
			array(
				'post_title'    => $title,
				'post_content'  => 'x',
				'post_status'   => 'publish',
				'post_date'     => '2026-01-01 09:00:00',
				'post_date_gmt' => '2026-01-01 09:00:00',
			),
			$extra
		)
	);
};
$secret   = $add_post( 'Secret', array( 'post_password' => 'pw' ) );
$untitled = $add_post( '' );
$fancy    = $add_post( 'Fancy "quotes" & <b>bold</b>' );
$draft    = $add_post( 'Draft', array( 'post_status' => 'draft' ) );

$minute  = 0;
$add_comment = static function ( $post_id, $extra = array() ) use ( &$minute ) {
	++$minute;
	$date = sprintf( '2026-02-01 09:%02d:00', $minute );
	return wp_insert_comment(
		array_merge(
			array(
				'comment_post_ID'      => $post_id,
				'comment_author'       => 'Visitor',
				'comment_author_email' => 'visitor@example.com',
				'comment_content'      => 'Hello',
				'comment_approved'     => 1,
				'comment_date'         => $date,
				'comment_date_gmt'     => $date,
			),
			$extra
		)
	);
};
$add_comment( 1, array( 'comment_author_url' => 'https://example.com/me' ) );
$add_comment( 1, array( 'comment_author_url' => 'http://127.0.0.1:8081/about' ) );
$add_comment( 1, array( 'comment_author_url' => 'HTTP://127.0.0.1:8081/Upper' ) );
$add_comment( 1, array( 'comment_author_url' => 'http://' ) );
// Not `http://` until esc_url() trims it.
$add_comment( 1, array( 'comment_author_url' => ' http://' ) );
$add_comment( 1, array( 'comment_author_url' => 'javascript:alert(1)' ) );
$add_comment( 1, array( 'comment_author_url' => 'example.org/bare' ) );
$add_comment( 1, array( 'comment_author_url' => '//127.0.0.1:8081/relative' ) );
$add_comment( 1, array( 'comment_author_url' => '/rooted' ) );
$add_comment( $secret );
$add_comment( $untitled );
$add_comment( $fancy, array( 'comment_author' => 'Ann & <b>Bo</b> "Q"' ) );
$add_comment( 1, array( 'comment_author' => '', 'user_id' => 1 ) );
$add_comment( 1, array( 'comment_author' => '' ) );
$add_comment( 1, array( 'comment_author' => 'Pinger', 'comment_type' => 'pingback' ) );
$add_comment( 1, array( 'comment_approved' => 0, 'comment_author' => 'Pending' ) );
$add_comment( $draft, array( 'comment_author' => 'On a draft' ) );

function widget( $instance ) {
	$serialized = serialize( $instance );
	$attrs      = array(
		'idBase'   => 'recent-comments',
		'instance' => array(
			'encoded' => base64_encode( $serialized ),
			'hash'    => wp_hash( $serialized ),
		),
	);
	return '<!-- wp:legacy-widget ' . wp_json_encode( $attrs ) . ' /-->';
}
$cases = array(
	widget( array() ),
	widget( array( 'number' => 20 ) ),
	widget( array( 'number' => 2 ) ),
	widget( array( 'number' => 0 ) ),
	widget( array( 'number' => 'abc' ) ),
	widget( array( 'number' => -3 ) ),
	widget( array( 'number' => '99999999999' ) ),
	widget( array( 'title' => '0', 'number' => 1 ) ),
	widget( array( 'title' => 'Said & <i>done</i>', 'number' => 1 ) ),
	widget( array( 'number' => 1 ) ) . widget( array( 'number' => 1 ) ),
);

$state = static function () use ( $wpdb ) {
	$comments = get_comments(
		array(
			'number'      => 100,
			'status'      => 'approve',
			'post_status' => 'publish',
		)
	);
	$ids      = array_unique( wp_list_pluck( $comments, 'comment_post_ID' ) );
	$posts    = array_map( 'get_post', $ids );
	return array(
		'comments' => array_map( static fn( $c ) => array( (int) $c->comment_ID, (int) $c->comment_post_ID, $c->comment_author, $c->comment_author_url, (int) $c->user_id ), $comments ),
		'posts'    => array_values( array_map( static fn( $p ) => array( $p->ID, $p->post_title, $p->post_password, $p->post_type, $p->post_name, $p->post_date ), $posts ) ),
		'users'    => array_map( static fn( $u ) => array( $u->ID, $u->display_name ), get_users() ),
	);
};

$GLOBALS['wp'] = new WP();
wp( '' );
$now = wp_json_encode( $state() );
foreach ( $cases as $markup ) {
	echo bin2hex( $markup ), "\t", bin2hex( $now ), "\t", bin2hex( do_blocks( $markup ) ), "\n";
}

// With no comment the list is empty.
$wpdb->query( "DELETE FROM $wpdb->comments" );
wp_cache_flush();
echo bin2hex( $cases[0] ), "\t", bin2hex( wp_json_encode( $state() ) ), "\t", bin2hex( do_blocks( $cases[0] ) ), "\n";
$wpdb->query( 'ROLLBACK' );

<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints the comment blocks over comments inserted into the fixture
// database inside a transaction that is rolled back. Lines are
//   hex(markup)<TAB>hex(json state)<TAB>hex(html)
// where the state has the options set, the post and every comment row.

require '/var/www/html/wp-load.php';

global $wpdb;
$wpdb->query( 'START TRANSACTION' );

$insert = function ( $fields ) {
	return wp_insert_comment(
		$fields + array(
			'comment_post_ID'      => 1,
			'comment_author'       => 'Reader',
			'comment_author_email' => 'reader@example.test',
			'comment_author_url'   => '',
			'comment_content'      => 'Text',
			'comment_approved'     => 1,
			'comment_date'         => '2026-10-27 09:00:00',
			'comment_date_gmt'     => '2026-10-27 09:00:00',
		)
	);
};
$a = $insert( array( 'comment_author' => 'A' ) );
$parent = $a;
foreach ( array( 'B', 'C', 'D', 'E', 'F', 'G' ) as $n => $name ) {
	$parent = $insert( array( 'comment_author' => $name, 'comment_parent' => $parent, 'comment_date_gmt' => "2026-10-27 09:1$n:00" ) );
}
$insert( array( 'comment_author' => 'H same date' ) );
$i = $insert( array( 'comment_author' => 'I unapproved', 'comment_approved' => 0, 'comment_date_gmt' => '2026-10-27 08:30:00' ) );
$insert( array( 'comment_author' => 'J spam', 'comment_approved' => 'spam', 'comment_parent' => $a ) );
$insert( array( 'comment_author' => 'K orphan', 'comment_parent' => $i ) );
$insert( array( 'comment_author' => 'L pingback', 'comment_type' => 'pingback', 'comment_date_gmt' => '2026-10-27 07:00:00' ) );
$insert( array( 'comment_author' => 'M author', 'user_id' => 1, 'comment_date_gmt' => '2026-10-27 10:00:00' ) );
$insert( array( 'comment_author' => 'N note', 'comment_type' => 'note' ) );
$insert( array( 'comment_author' => 'O trash', 'comment_approved' => 'trash' ) );
$insert( array( 'comment_author' => 'P second reply', 'comment_parent' => $a, 'comment_date_gmt' => '2026-10-27 09:05:00' ) );
$reader = wp_insert_user( array( 'user_login' => 'reader', 'user_pass' => 'x', 'user_nicename' => 'Reader Two!', 'display_name' => 'Reader' ) );
$insert( array( 'comment_author' => 'Q other user', 'user_id' => $reader, 'comment_date_gmt' => '2026-10-27 11:00:00' ) );
$legacy = $insert( array( 'comment_author' => 'R untyped', 'comment_date_gmt' => '2026-10-27 12:00:00' ) );
$wpdb->update( $wpdb->comments, array( 'comment_type' => '' ), array( 'comment_ID' => $legacy ) );
clean_comment_cache( $legacy );
$users = $wpdb->get_results( "SELECT ID, display_name, user_nicename FROM {$wpdb->users}", ARRAY_A );
$rows = $wpdb->get_results( "SELECT * FROM {$wpdb->comments} WHERE comment_post_ID = 1 ORDER BY comment_ID", ARRAY_A );

$option_sets = array(
	array(),
	array( 'thread_comments' => '0' ),
	array( 'thread_comments_depth' => '2' ),
	array( 'thread_comments_depth' => '1' ),
	array( 'comment_order' => 'desc' ),
	array( 'comment_order' => 'desc', 'thread_comments_depth' => '3' ),
);
$markups = array(
	'<!-- wp:comment-template --><p>c</p><!-- /wp:comment-template -->',
	'<!-- wp:comment-template {"className":"listed"} --><p>c</p><!-- /wp:comment-template -->',
);
$reset = function () {
	$GLOBALS['comment_alt']        = 0;
	$GLOBALS['comment_depth']      = 0;
	$GLOBALS['comment_thread_alt'] = 0;
};
foreach ( $option_sets as $options ) {
	foreach ( array( 'thread_comments', 'thread_comments_depth', 'comment_order' ) as $name ) {
		remove_all_filters( "pre_option_$name" );
	}
	foreach ( $options as $name => $value ) {
		add_filter( "pre_option_$name", static fn() => $value );
	}
	foreach ( array( 1, 2, 0 ) as $post_id ) {
		foreach ( $markups as $markup ) {
			$reset();
			$context = $post_id ? array( 'postId' => $post_id ) : array();
			$html    = ( new WP_Block( parse_blocks( $markup )[0], $context ) )->render();
			// Twice in one request, as the counters carry on.
			$html   .= ( new WP_Block( parse_blocks( $markup )[0], $context ) )->render();
			$state   = array( 'options' => $options, 'post' => $post_id, 'comments' => $rows, 'users' => $users, 'password' => false );
			echo bin2hex( $markup ), "\t", bin2hex( wp_json_encode( $state ) ), "\t", bin2hex( $html ), "\n";
		}
	}
}

// A password protected post shows no comments.
$wpdb->update( $wpdb->posts, array( 'post_password' => 'pw' ), array( 'ID' => 1 ) );
clean_post_cache( 1 );
$markup = $markups[0];
$reset();
$state = array( 'options' => array(), 'post' => 1, 'comments' => $rows, 'users' => $users, 'password' => true );
echo bin2hex( $markup ), "\t", bin2hex( wp_json_encode( $state ) ), "\t", bin2hex( ( new WP_Block( parse_blocks( $markup )[0], array( 'postId' => 1 ) ) )->render() ), "\n";

$wpdb->query( 'ROLLBACK' );

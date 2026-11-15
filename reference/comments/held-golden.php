<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints the comment blocks as the author of a held comment sees them from
// the link wp-comments-post.php sends them to (`unapproved` and
// `moderation-hash`, wp_get_unapproved_comment_author_email()), over
// comments inserted into the fixture database inside a transaction that is
// rolled back. The first line is hex(json) of every comment row; then lines are
//   hex(markup)<TAB>hex(json state)<TAB>hex(html)
// where the state has the options set, the request, the time and the
// wp_hash() of each comment date.

require '/var/www/html/wp-load.php';

global $wpdb;
$wpdb->query( 'START TRANSACTION' );

$insert = function ( $fields ) {
	return wp_insert_comment(
		$fields + array(
			'comment_post_ID'      => 1,
			'comment_author'       => 'Reader',
			'comment_author_email' => 'reader@example.test',
			'comment_content'      => 'Text',
			'comment_approved'     => 1,
			'comment_date'         => '2026-10-27 09:00:00',
			'comment_date_gmt'     => '2026-10-27 09:00:00',
		)
	);
};
$now     = gmdate( 'Y-m-d H:i:s' );
$top     = $insert( array( 'comment_author' => 'Approved' ) );
$held    = $insert( array( 'comment_author' => 'Held <b>now</b>', 'comment_author_email' => 'held@example.test', 'comment_author_url' => 'https://held.example/', 'comment_approved' => 0, 'comment_content' => "Held <b>text</b> &amp; http://example.test/\n\nMore <a href=\"https://a.example/\">linked</a>.", 'comment_date_gmt' => $now, 'comment_parent' => $top ) );
$earlier = $insert( array( 'comment_author' => 'Held before', 'comment_author_email' => 'Held@Example.test', 'comment_approved' => 0, 'comment_date_gmt' => '2026-10-27 08:00:00' ) );
$other   = $insert( array( 'comment_author' => 'Held elsewhere', 'comment_author_email' => 'other@example.test', 'comment_approved' => 0, 'comment_date_gmt' => $now ) );
$insert( array( 'comment_author' => 'Spam', 'comment_author_email' => 'held@example.test', 'comment_approved' => 'spam', 'comment_date_gmt' => $now ) );
$insert( array( 'comment_author' => 'Held note', 'comment_author_email' => 'held@example.test', 'comment_approved' => 0, 'comment_type' => 'note', 'comment_date_gmt' => $now ) );
$insert( array( 'comment_author' => 'Held on page 2', 'comment_post_ID' => 2, 'comment_author_email' => 'held@example.test', 'comment_approved' => 0, 'comment_date_gmt' => $now ) );
$old     = $insert( array( 'comment_author' => 'Held long ago', 'comment_author_email' => 'old@example.test', 'comment_approved' => 0, 'comment_date_gmt' => '2026-10-27 07:00:00' ) );
$nobody  = $insert( array( 'comment_author' => 'Held without email', 'comment_author_email' => '', 'comment_approved' => 0, 'comment_date_gmt' => $now ) );

$rows = $wpdb->get_results( "SELECT * FROM {$wpdb->comments} ORDER BY comment_ID", ARRAY_A );
echo bin2hex( wp_json_encode( array( 'comments' => $rows ) ) ), "\n";
$hashes = array();
foreach ( $rows as $row ) {
	$hashes[ $row['comment_date_gmt'] ] = wp_hash( $row['comment_date_gmt'] );
}
$hash = fn( $id ) => wp_hash( get_comment( $id )->comment_date_gmt );

$markups = array(
	'<!-- wp:comment-template --><!-- wp:comment-author-name {"isLink":true,"linkTarget":"_blank"} /--><!-- wp:comment-content /--><!-- wp:comment-reply-link /--><!-- /wp:comment-template -->',
	'<!-- wp:comments --><div class="wp-block-comments"><!-- wp:comment-template --><!-- wp:comment-author-name /--><!-- /wp:comment-template --><!-- wp:comments-pagination --><!-- wp:comments-pagination-previous /--><!-- wp:comments-pagination-numbers /--><!-- wp:comments-pagination-next /--><!-- /wp:comments-pagination --></div><!-- /wp:comments -->',
);
$uris = array(
	"/?p=1&unapproved=$held&moderation-hash=" . $hash( $held ),
	"/?p=1&unapproved=$held&moderation-hash=" . strtoupper( $hash( $held ) ),
	"/?p=1&unapproved=$held&moderation-hash=" . $hash( $old ),
	"/?p=1&unapproved=$old&moderation-hash=" . $hash( $old ),
	"/?p=1&unapproved=$other&moderation-hash=" . $hash( $other ),
	"/?p=1&unapproved=$nobody&moderation-hash=" . $hash( $nobody ),
	"/?p=1&unapproved=$held",
	"/?p=1&unapproved=0&moderation-hash=" . $hash( $held ),
	"/?p=1&unapproved=$held" . 'x&moderation-hash=' . $hash( $held ),
	'/?p=1&unapproved=99999&moderation-hash=' . $hash( $held ),
	'/?p=1',
);
$option_sets = array(
	array(),
	array( 'thread_comments' => '0' ),
	array( 'comment_order' => 'desc' ),
);
$elements = static fn() => (int) substr( wp_unique_prefixed_id( 'wp-elements-' ), 12 );
$GLOBALS['wp_query']->is_singular = true;
$GLOBALS['post']                  = get_post( 1 );
foreach ( $option_sets as $options ) {
	foreach ( array_keys( $options + array( 'thread_comments' => 1, 'comment_order' => 1 ) ) as $name ) {
		remove_all_filters( "pre_option_$name" );
	}
	foreach ( $options as $name => $value ) {
		add_filter( "pre_option_$name", static fn() => $value );
	}
	foreach ( $uris as $uri ) {
		foreach ( $markups as $markup ) {
			$_SERVER['REQUEST_URI'] = $uri;
			$_GET                   = array();
			parse_str( (string) parse_url( $uri, PHP_URL_QUERY ), $_GET );
			$_GET = wp_slash( $_GET );
			$GLOBALS['comment_alt']        = 0;
			$GLOBALS['comment_depth']      = 0;
			$GLOBALS['comment_thread_alt'] = 0;
			$counter = $elements();
			$parsed  = parse_blocks( $markup )[0];
			$parsed  = apply_filters( 'render_block_data', $parsed, $parsed, null );
			// Twice in one request, as the counters carry on.
			$html  = ( new WP_Block( $parsed, array( 'postId' => 1 ) ) )->render();
			$html .= ( new WP_Block( $parsed, array( 'postId' => 1 ) ) )->render();
			$state   = array( 'options' => $options, 'elements' => $counter, 'post' => 1, 'now' => time(), 'uri' => $uri, 'global' => 1, 'singular' => true, 'hashes' => $hashes );
			echo bin2hex( $markup ), "\t", bin2hex( wp_json_encode( $state ) ), "\t", bin2hex( $html ), "\n";
		}
	}
}

$wpdb->query( 'ROLLBACK' );

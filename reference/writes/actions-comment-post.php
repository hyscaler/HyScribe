<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The write oracle's front end comment posts (MVP 3.7), loaded beside
// actions.php: one action per case of reference/comment-post/cases.list
// that PHP answered with a redirect (crates/hyscribe-http/tests/golden/
// comment-post.tsv), as wp-comments-post.php runs it:
// wp_handle_comment_submission() over the case's body, as its user, from
// 192.0.2.1. The setup runs the case's setup SQL and first body.

$cases   = __DIR__ . '/comment-post-cases.list';
$answers = __DIR__ . '/comment-post.tsv';
if ( ! is_file( $cases ) || ! is_file( $answers ) ) {
	return array();
}
$redirected = array();
foreach ( file( $answers, FILE_IGNORE_NEW_LINES ) as $line ) {
	list( $case, $status ) = explode( "\t", $line );
	$redirected[ $case ] = '302' === $status;
}

// The case's body as wp-comments-post.php reads it, for the current user.
$submit = function ( $body, $cookie ) {
	$body = preg_replace_callback( '/%a(\d+)%/', fn( $m ) => str_repeat( 'a', (int) $m[1] ), $body );
	$body = str_replace( '%nonce%', wp_create_nonce( 'unfiltered-html-comment_1' ), $body );
	parse_str( $body, $post );
	$_COOKIE = array();
	if ( '-' !== $cookie ) {
		list( $name, $value ) = explode( '=', $cookie, 2 );
		$_COOKIE[ $name ] = urldecode( $value );
	}
	$_SERVER['REMOTE_ADDR']     = '192.0.2.1';
	$_SERVER['HTTP_USER_AGENT'] = 'HyScribe oracle';
	// `wp_unslash()` of what `wp_magic_quotes()` slashed.
	wp_handle_comment_submission( $post );
};

$actions = array();
foreach ( file( $cases, FILE_IGNORE_NEW_LINES ) as $line ) {
	if ( '' === $line || '#' === $line[0] ) {
		continue;
	}
	list( $case, $method, $user, $cookie, $setup, , $first, $body ) = explode( "\t", $line );
	if ( empty( $redirected[ $case ] ) ) {
		continue;
	}
	$actions[ "comment-post-$case" ] = array(
		'setup'  => function () use ( $submit, $user, $cookie, $setup, $first ) {
			global $wpdb;
			foreach ( '-' === $setup ? array() : explode( '; ', $setup ) as $query ) {
				$wpdb->query( $query );
			}
			wp_cache_flush();
			wp_set_current_user( (int) $user );
			if ( '-' !== $first ) {
				$submit( $first, $cookie );
			}
		},
		'action' => function () use ( $submit, $cookie, $body ) {
			$submit( $body, $cookie );
		},
	);
}
return $actions;

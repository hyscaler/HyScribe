<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Must-use plugin for reference/notices/golden.sh, through filters only: the
// n6trash, n6spam and n6pending variants give comment 1 that status while
// the comments list prints its notices (between all_admin_notices and its
// views), so ?same= reads it and the list itself does not.
$hyscribe_status = array(
	'n6trash'   => 'trash',
	'n6spam'    => 'spam',
	'n6pending' => '0',
)[ $_COOKIE['hyscribe_variant'] ?? '' ] ?? null;
if ( null !== $hyscribe_status ) {
	$hyscribe_filter = function ( $comment ) use ( $hyscribe_status ) {
		if ( 1 === (int) $comment->comment_ID ) {
			$comment                   = clone $comment;
			$comment->comment_approved = $hyscribe_status;
		}
		return $comment;
	};
	add_action( 'all_admin_notices', fn() => add_filter( 'get_comment', $hyscribe_filter ) );
	add_filter(
		'views_edit-comments',
		function ( $views ) use ( $hyscribe_filter ) {
			remove_filter( 'get_comment', $hyscribe_filter );
			return $views;
		}
	);
}

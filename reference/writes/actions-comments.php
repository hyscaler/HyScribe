<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The write oracle's comment actions (MVP 2.4), loaded beside actions.php.
// wp_new_comment() takes slashed data; its refusals write nothing, so each
// refusal is followed by a write that shows what was, or was not, done.

// A comment by $author on post 1, as wp_insert_comment() saves it.
$comment = function ( $approved, $more = array() ) {
	return wp_insert_comment(
		$more + array(
			'comment_post_ID'      => 1,
			'comment_author'       => 'A reader',
			'comment_author_email' => 'reader@example.test',
			'comment_author_url'   => 'https://reader.example.test',
			'comment_author_IP'    => '192.0.2.1',
			'comment_content'      => 'A comment.',
			'comment_agent'        => 'Oracle',
			'comment_approved'     => $approved,
		)
	);
};

// wp_new_comment() of a comment on post 1, slashed, with WP_Error on refusal.
$submit = function ( $more = array() ) {
	return wp_new_comment(
		wp_slash(
			$more + array(
				'comment_post_ID'      => 1,
				'comment_author'       => 'A reader',
				'comment_author_email' => 'reader@example.test',
				'comment_author_url'   => 'https://reader.example.test',
				'comment_author_IP'    => '192.0.2.1',
				'comment_agent'        => 'Oracle',
				'comment_content'      => 'A new comment.',
			)
		),
		true
	);
};

// The fixture's approved commenter, who needs no moderation.
$known = array(
	'comment_author'       => 'A WordPress Commenter',
	'comment_author_email' => 'wapuu@wordpress.example',
);

$html = array(
	'user_id'              => 1,
	'comment_author'       => " <b>Admin</b> & co's\t%41name ",
	'comment_author_email' => ' ADMIN<i>@</i>Example.test ',
	'comment_author_url'   => ' https://example.test/a b<b>c</b> ',
	'comment_author_IP'    => '192.0.2.1<script>, ::1',
	'comment_agent'        => str_repeat( 'Agent ', 50 ),
	'comment_content'      => "<a href=\"https://example.test\" class=\"x\" rel=\"me\">out</a> <A HREF='http://127.0.0.1:8081/about'>in</A> <a\nhref=\"https://split.test\">split</a> <span class=\"wp-note-mention user-2 user-0 evil\">@me</span> <script>alert(1)</script> &#147;quoted&#148; it's \"fine\" \\ <b>bold",
);

return array(
	'new-comment-held'          => array(
		'action' => function () use ( $submit ) {
			$submit();
		},
	),
	'new-comment-by-admin'      => array(
		'action' => function () use ( $submit, $html ) {
			$submit( $html );
		},
	),
	'new-comment-unfiltered'    => array(
		'action' => function () use ( $submit, $html ) {
			// What kses_init() does for a user who may post unfiltered HTML.
			kses_remove_filters();
			$submit( $html );
		},
	),
	'new-comment-balanced'      => array(
		'setup'  => function () {
			update_option( 'use_balanceTags', '1' );
		},
		'action' => function () use ( $submit, $known ) {
			$submit( array( 'comment_content' => '<b>Bold <i>both</b> it\'s' ) + $known );
		},
	),
	'new-comment-links'         => array(
		'action' => function () use ( $submit, $known ) {
			$submit( array( 'comment_content' => 'See https://a.test and www.b.test.' ) + $known );
			$submit( array( 'comment_content' => 'Only https://c.test here.' ) + $known );
		},
	),
	'new-comment-moderation'    => array(
		'setup'  => function () {
			update_option( 'comment_moderation', '1' );
		},
		'action' => function () use ( $submit, $known ) {
			$submit( $known );
		},
	),
	'new-comment-moderation-key' => array(
		'setup'  => function () {
			update_option( 'moderation_keys', "\n  0\nSPAM#Wörd\n" );
		},
		'action' => function () use ( $submit, $known ) {
			$submit( array( 'comment_content' => 'This has spam#wÖrd in it.' ) + $known );
			$submit( array( 'comment_agent' => 'a SPAM#WÖRD agent' ) + $known );
			$submit( array( 'comment_content' => 'This is clean.' ) + $known );
		},
	),
	'new-comment-disallowed'    => array(
		'setup'  => function () {
			update_option( 'disallowed_keys', "\nbadword\n" );
		},
		'action' => function () use ( $submit, $known ) {
			$submit( array( 'comment_content' => 'A bad<b>word</b> here.' ) + $known );
			// The author's and moderators' comments are not checked.
			$submit(
				array(
					'user_id'         => 1,
					'comment_content' => 'My own badword.',
				)
			);
		},
	),
	'new-comment-replies'       => array(
		'setup'  => function () use ( $comment ) {
			return array( $comment( 'spam' ), $comment( 0 ) );
		},
		'action' => function ( $ids ) use ( $submit ) {
			foreach ( array( 1, $ids[0], $ids[1], 999 ) as $n => $parent ) {
				$submit(
					array(
						'user_id'         => 1,
						'comment_parent'  => $parent,
						'comment_content' => "Reply $n.",
					)
				);
			}
		},
	),
	'new-comment-duplicate'     => array(
		'action' => function () use ( $submit, $known ) {
			$fixture = get_comment( 1, ARRAY_A );
			$dupe    = array( 'comment_content' => $fixture['comment_content'] ) + $known;
			if ( ! is_wp_error( $submit( $dupe ) ) ) {
				throw new Exception( 'no duplicate' );
			}
			$submit( array( 'comment_parent' => 1 ) + $dupe );
		},
	),
	'new-comment-flood'         => array(
		'setup'  => function () use ( $comment ) {
			$comment( 1, array( 'comment_author_IP' => '192.0.2.7' ) );
		},
		'action' => function () use ( $submit ) {
			wp_set_current_user( 0 );
			$submit(
				array(
					'comment_author_IP'    => '192.0.2.8',
					'comment_author_email' => 'other@example.test',
				)
			);
			if ( ! is_wp_error( $submit( array( 'comment_author_IP' => '192.0.2.7' ) ) ) ) {
				throw new Exception( 'no flood' );
			}
		},
	),
	'new-comment-anonymous'     => array(
		'action' => function () use ( $submit ) {
			kses_remove_filters();
			$submit(
				array(
					'comment_author_email' => '',
					'comment_content'      => '&#100; &#129; &#130; &#1300; <a download rel="me  me" href="/about">x</a> <a href="mailto:x@example.test">m</a>',
				)
			);
		},
	),
	'new-comment-pingback'      => array(
		'action' => function () use ( $submit, $known ) {
			$submit(
				array(
					'comment_type'       => 'pingback',
					'comment_date'       => '2026-01-02 03:04:05',
					'comment_date_gmt'   => '0',
					'comment_author_url' => 'https://elsewhere.test/post',
				) + $known
			);
		},
	),
	'new-comment-by-author'     => array(
		'action' => function () use ( $submit ) {
			global $wpdb;
			add_filter(
				'user_has_cap',
				function ( $caps ) {
					$caps['moderate_comments'] = false;
					return $caps;
				}
			);
			$submit(
				array(
					'user_id'         => 1,
					'comment_content' => 'By the author.',
				)
			);
			$wpdb->update( $wpdb->posts, array( 'post_author' => 2 ), array( 'post_author' => 1 ) );
			$submit(
				array(
					'user_id'              => 1,
					'comment_author'       => 'admin',
					'comment_author_email' => 'admin@default-install.test',
					'comment_content'      => 'Not the author.',
				)
			);
		},
	),
	'comment-missing'           => array(
		'action' => function () {
			foreach ( array( wp_trash_comment( 999 ), wp_spam_comment( 999 ), wp_untrash_comment( 999 ), wp_set_comment_status( 1, 'bogus' ), wp_delete_comment( 999 ) ) as $result ) {
				if ( false !== $result ) {
					throw new Exception( 'found' );
				}
			}
			wp_set_comment_status( 1, '0' );
		},
	),
	'update-comment-moved'      => array(
		'setup'  => function () use ( $comment ) {
			return $comment( 0 );
		},
		'action' => function ( $id ) {
			if ( 0 !== wp_update_comment( array( 'comment_ID' => $id ) ) ) {
				throw new Exception( 'changed' );
			}
			if ( false !== wp_update_comment( array( 'comment_ID' => 999 ) ) || false !== wp_update_comment( array( 'comment_ID' => $id, 'comment_post_ID' => 999 ) ) ) {
				throw new Exception( 'found' );
			}
			wp_update_comment(
				wp_slash(
					array(
						'comment_ID'           => $id,
						'comment_post_ID'      => 2,
						'comment_author_email' => 'new@example.test',
						'comment_author_url'   => 'https://new.example.test',
						'comment_author_IP'    => '192.0.2.9',
						'comment_karma'        => 3,
						'comment_approved'     => 'approve',
						'comment_agent'        => 'New agent',
						'comment_type'         => 'pingback',
						'comment_parent'       => 1,
						'user_id'              => 1,
					)
				)
			);
			wp_update_comment(
				array(
					'comment_ID'       => 1,
					'comment_post_ID'  => 0,
					'comment_approved' => 'spam',
				)
			);
		},
	),
	'update-comment'            => array(
		'action' => function () {
			wp_update_comment(
				wp_slash(
					array(
						'comment_ID'       => 1,
						'comment_author'   => 'Edited <i>name</i>',
						'comment_content'  => 'Edited <script>x</script> <a href="https://example.test">it\'s</a>',
						'comment_date'     => '2026-11-01 10:00:00',
						'comment_date_gmt' => '1999-01-01 00:00:00',
						'comment_approved' => 'hold',
					)
				)
			);
		},
	),
	'hold-comment'              => array(
		'action' => function () {
			wp_set_comment_status( 1, 'hold' );
		},
	),
	'status-unchanged'          => array(
		'setup'  => function () {
			global $wpdb;
			$wpdb->update( $wpdb->posts, array( 'comment_count' => 5 ), array( 'ID' => 1 ) );
		},
		'action' => function () use ( $comment ) {
			if ( false !== wp_set_comment_status( 1, 'approve' ) ) {
				throw new Exception( 'changed' );
			}
			$comment( 0 );
		},
	),
	'spam-comment'              => array(
		'action' => function () {
			wp_spam_comment( 1 );
		},
	),
	'unspam-comment'            => array(
		'setup'  => function () {
			wp_spam_comment( 1 );
		},
		'action' => function () {
			wp_unspam_comment( 1 );
		},
	),
	'trash-comment'             => array(
		'action' => function () {
			wp_trash_comment( 1 );
		},
	),
	'untrash-comment'           => array(
		'setup'  => function () use ( $comment ) {
			$held = $comment( 0 );
			wp_trash_comment( $held );
			return array( $held, $comment( 'trash' ) );
		},
		'action' => function ( $ids ) {
			wp_untrash_comment( $ids[0] );
			wp_untrash_comment( $ids[1] );
		},
	),
	'delete-comment'            => array(
		'setup'  => function () use ( $comment ) {
			$reply = $comment( 1, array( 'comment_parent' => 1 ) );
			$comment( 0, array( 'comment_parent' => $reply ) );
			add_comment_meta( $reply, 'oracle', 'a' );
			add_comment_meta( $reply, 'oracle', 'b' );
			return $reply;
		},
		'action' => function ( $reply ) {
			wp_delete_comment( $reply, true );
		},
	),
	'delete-comment-to-trash'   => array(
		'action' => function () {
			wp_delete_comment( 1 );
		},
	),
	'delete-trashed-comment'    => array(
		'setup'  => function () {
			wp_trash_comment( 1 );
		},
		'action' => function () {
			wp_delete_comment( 1 );
		},
	),
	'trash-note'                => array(
		'setup'  => function () use ( $comment ) {
			$note = $comment( 1, array( 'comment_type' => 'note' ) );
			foreach ( array( array( 1, '01' ), array( 0, '03' ), array( 'spam', '02' ), array( 1, '04' ) ) as $n => list( $approved, $second ) ) {
				$comment(
					$approved,
					array(
						'comment_type'     => 3 === $n ? 'comment' : 'note',
						'comment_parent'   => $note,
						'comment_date'     => "2026-01-01 00:00:$second",
						'comment_date_gmt' => "2026-01-01 00:00:$second",
					)
				);
			}
			return $note;
		},
		'action' => function ( $note ) {
			wp_trash_comment( $note );
		},
	),
);

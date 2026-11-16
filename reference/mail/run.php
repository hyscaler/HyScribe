<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Runs one case of the mail oracle (reference/mail/golden.sh) in PHP
// WordPress, inside a transaction it rolls back, and prints the wp_mail()
// calls capture.php kept: case, to, subject, message and headers, escaped
// as the write oracle escapes, with random keys, hashes, request IDs and the
// export's expiry date masked. A case that sends nothing prints `case\t-`.
//   php run.php                  the cases, one per line
//   php run.php <case> <blog>    one case, blog `plain`, `odd` or `empty`
//   php run.php decode           wp_specialchars_decode()'s cases, in hex

$args = $argv;
if ( str_ends_with( $args[1] ?? '', '-no-trash' ) ) {
	define( 'EMPTY_TRASH_DAYS', 0 );
}
define( 'DISABLE_WP_CRON', true );
require '/var/www/html/wp-load.php';

( function ( $args ) {
	if ( 'decode' === ( $args[1] ?? '' ) ) {
		$texts = array(
			'',
			'plain text',
			'&',
			'&amp;',
			'&amp;amp;',
			'&lt;&gt;&#060;&#062;&#60;&#0060;&#00062;',
			'&#38;&#038;&#0038;&#x26;&#X26;&#x026;&#X0026;',
			'&#39;&#039;&#0039;&#x27;&#X27;&#x027;&#X00027;',
			'&quot;&#34;&#034;&#0034;&#x22;&#X22;&#x0022;',
			'&apos;&#39&#x27&nbsp;&hellip;&#8217;',
			'&#0;&#x;&#;&#00;&#x0;',
			'&#x0000027;&#000000039;&#x0000000000026;',
			'a&b;c && &&amp;; &#&#39;',
			'&#x2G; &#3 9; &# 39; &#x 27;',
			"caf\u{e9} &amp; \u{fc}",
			"\xff&amp;\xfe",
			'&AMP;&LT;&QUOT;&Amp;',
			'&#038;lt; &amp;#039; &#x26;quot;',
			"line\r\n&amp;\ttab",
		);
		foreach ( $texts as $text ) {
			echo bin2hex( $text ), "\t", bin2hex( wp_specialchars_decode( $text ) ), "\t", bin2hex( wp_specialchars_decode( $text, ENT_QUOTES ) ), "\n";
		}
		return;
	}
	require_once ABSPATH . 'wp-admin/includes/admin.php';

	// A user whose login rawurlencode() changes and ends in a period, and whose
	// email address addslashes() changes.
	$odd_user = function ( $role = 'subscriber' ) {
		return wp_insert_user(
			wp_slash(
				array(
					'user_login' => 'Tom O. Neil.',
					'user_email' => "o'neil@example.test",
					'user_pass'  => 'a password',
					'role'       => $role,
				)
			)
		);
	};
	$plain_user = function ( $role ) {
		return wp_insert_user(
			array(
				'user_login' => "an-$role",
				'user_email' => "$role@example.test",
				'user_pass'  => 'a password',
				'role'       => $role,
			)
		);
	};
	// Comment 1 on post 1 as `$fields` say, with a title and text to decode.
	$comment = function ( $fields ) {
		global $wpdb;
		$wpdb->update( $wpdb->posts, array( 'post_title' => 'Hello &amp; "world" &lt;i&gt; &#039;s' ), array( 'ID' => 1 ) );
		$wpdb->update(
			$wpdb->comments,
			array_merge(
				array(
					'comment_author'       => 'A "Reader"',
					'comment_author_email' => 'reader@example.test',
					'comment_author_url'   => 'https://reader.example.test/a?b=1&c=2',
					'comment_author_IP'    => '127.0.0.1',
					'comment_content'      => "A &lt;b&gt;bold&lt;/b&gt; &amp;amp; &quot;quoted&quot; &#039;s\nline two",
				),
				$fields
			),
			array( 'comment_ID' => 1 )
		);
		clean_post_cache( 1 );
		clean_comment_cache( 1 );
	};
	$author = function ( $id ) {
		global $wpdb;
		$wpdb->update( $wpdb->posts, array( 'post_author' => $id ), array( 'ID' => 1 ) );
		clean_post_cache( 1 );
	};
	$request = function ( $action, $status = null ) use ( $odd_user ) {
		$odd_user();
		$id = wp_create_user_request( "o'neil@example.test", $action );
		if ( $status ) {
			wp_update_post(
				array(
					'ID'          => $id,
					'post_status' => $status,
				)
			);
		}
		return $id;
	};

	$cases = array(
		'retrieve-password'              => function () use ( $odd_user ) {
			$odd_user();
			$_SERVER['REMOTE_ADDR'] = '192.0.2.1';
			retrieve_password( 'Tom O. Neil.' );
		},
		'retrieve-password-logged-in'    => function () use ( $odd_user ) {
			$odd_user();
			$_SERVER['REMOTE_ADDR'] = '192.0.2.1';
			wp_set_current_user( 1 );
			retrieve_password( "o'neil@example.test" );
		},
		'retrieve-password-no-address'   => function () use ( $odd_user ) {
			$odd_user();
			$_SERVER['REMOTE_ADDR'] = '';
			retrieve_password( 'Tom O. Neil.' );
		},
		'new-user-both'                  => function () use ( $odd_user ) {
			wp_new_user_notification( $odd_user(), null, 'both' );
		},
		'new-user-admin'                 => function () use ( $odd_user ) {
			wp_new_user_notification( $odd_user(), null, 'admin' );
		},
		'new-user-user'                  => function () use ( $odd_user ) {
			wp_new_user_notification( $odd_user(), null, 'user' );
		},
		'email-change-request'           => function () use ( $odd_user ) {
			wp_set_current_user( $odd_user() );
			$_POST['email'] = 'new-address@example.test';
			send_confirmation_on_profile_email( get_current_user_id() );
		},
		'new-admin-email'                => function () {
			wp_set_current_user( 1 );
			update_option_new_admin_email( 'admin@default-install.test', 'new-admin@example.test' );
		},
		'admin-email-changed'            => function () {
			wp_site_admin_email_change_notification( 'admin@default-install.test', 'new-admin@example.test', 'admin_email' );
		},
		'admin-email-changed-default'    => function () {
			wp_site_admin_email_change_notification( 'you@example.com', 'new-admin@example.test', 'admin_email' );
		},
		'password-changed'               => function () use ( $odd_user ) {
			wp_update_user(
				array(
					'ID'        => $odd_user(),
					'user_pass' => 'another password',
				)
			);
		},
		'email-changed'                  => function () use ( $odd_user ) {
			wp_update_user(
				array(
					'ID'         => $odd_user(),
					'user_email' => 'new-address@example.test',
				)
			);
		},
		'password-and-email-changed'     => function () use ( $odd_user ) {
			wp_update_user(
				array(
					'ID'         => $odd_user(),
					'user_pass'  => 'another password',
					'user_email' => 'new-address@example.test',
				)
			);
		},
		'password-reset-admin'           => function () use ( $odd_user ) {
			wp_password_change_notification( get_userdata( $odd_user() ) );
		},
		'password-reset-admin-self'      => function () {
			wp_password_change_notification( get_userdata( 1 ) );
		},
		'postauthor-comment'             => function () use ( $comment ) {
			$comment( array() );
			wp_notify_postauthor( 1 );
		},
		'postauthor-comment-no-trash'    => function () use ( $comment ) {
			$comment( array() );
			wp_notify_postauthor( 1 );
		},
		'postauthor-reply'               => function () use ( $comment ) {
			$comment( array( 'comment_parent' => 1 ) );
			wp_notify_postauthor( 1 );
		},
		'postauthor-anonymous'           => function () use ( $comment ) {
			$comment(
				array(
					'comment_author'    => '',
					'comment_author_IP' => '',
				)
			);
			wp_notify_postauthor( 1 );
		},
		'postauthor-no-email'            => function () use ( $comment ) {
			$comment( array( 'comment_author_email' => '' ) );
			wp_notify_postauthor( 1 );
		},
		'postauthor-anonymous-no-email'  => function () use ( $comment ) {
			$comment(
				array(
					'comment_author'       => '',
					'comment_author_email' => '',
				)
			);
			wp_notify_postauthor( 1 );
		},
		'postauthor-trackback'           => function () use ( $comment ) {
			$comment( array( 'comment_type' => 'trackback' ) );
			wp_notify_postauthor( 1 );
		},
		'postauthor-pingback'            => function () use ( $comment ) {
			$comment( array( 'comment_type' => 'pingback' ) );
			wp_notify_postauthor( 1 );
		},
		'postauthor-note'                => function () use ( $comment, $author, $plain_user ) {
			$comment(
				array(
					'comment_type' => 'note',
					'user_id'      => 1,
				)
			);
			$author( $plain_user( 'editor' ) );
			wp_set_current_user( 1 );
			wp_notify_postauthor( 1 );
		},
		'postauthor-note-resolved'       => function () use ( $comment, $author, $plain_user ) {
			$comment(
				array(
					'comment_type'    => 'note',
					'comment_content' => '',
				)
			);
			$author( $plain_user( 'editor' ) );
			wp_notify_postauthor( 1 );
		},
		'postauthor-subscriber'          => function () use ( $comment, $author, $plain_user ) {
			$comment( array( 'comment_parent' => 1 ) );
			$author( $plain_user( 'subscriber' ) );
			wp_notify_postauthor( 1 );
		},
		'postauthor-own-comment'         => function () use ( $comment ) {
			$comment( array( 'user_id' => 1 ) );
			wp_notify_postauthor( 1 );
		},
		'postauthor-own-moderation'      => function () use ( $comment ) {
			$comment( array() );
			wp_set_current_user( 1 );
			wp_notify_postauthor( 1 );
		},
		'moderator-comment'              => function () use ( $comment ) {
			$comment( array( 'comment_approved' => '0' ) );
			wp_notify_moderator( 1 );
		},
		'moderator-comment-no-trash'     => function () use ( $comment ) {
			$comment( array( 'comment_approved' => '0' ) );
			wp_notify_moderator( 1 );
		},
		'moderator-reply'                => function () use ( $comment ) {
			$comment(
				array(
					'comment_approved' => '0',
					'comment_parent'   => 1,
				)
			);
			wp_notify_moderator( 1 );
		},
		'moderator-trackback'            => function () use ( $comment ) {
			$comment(
				array(
					'comment_approved' => '0',
					'comment_type'     => 'trackback',
				)
			);
			wp_notify_moderator( 1 );
		},
		'moderator-pingback'             => function () use ( $comment ) {
			$comment(
				array(
					'comment_approved'  => '0',
					'comment_type'      => 'pingback',
					'comment_author_IP' => '',
				)
			);
			wp_notify_moderator( 1 );
		},
		'moderator-editor-two-waiting'   => function () use ( $comment, $author, $plain_user ) {
			$comment( array( 'comment_approved' => '0' ) );
			$author( $plain_user( 'editor' ) );
			wp_insert_comment(
				array(
					'comment_post_ID'  => 1,
					'comment_content'  => 'Another.',
					'comment_approved' => '0',
				)
			);
			wp_notify_moderator( 1 );
		},
		'moderator-subscriber'           => function () use ( $comment, $author, $plain_user ) {
			$comment( array( 'comment_approved' => '0' ) );
			$author( $plain_user( 'subscriber' ) );
			wp_notify_moderator( 1 );
		},
		'user-request-export'            => function () use ( $request ) {
			wp_send_user_request( $request( 'export_personal_data' ) );
		},
		'user-request-erase'             => function () use ( $request ) {
			wp_send_user_request( $request( 'remove_personal_data' ) );
		},
		'request-confirmed-export'       => function () use ( $request ) {
			_wp_privacy_send_request_confirmation_notification( $request( 'export_personal_data', 'request-confirmed' ) );
		},
		'request-confirmed-erase'        => function () use ( $request ) {
			_wp_privacy_send_request_confirmation_notification( $request( 'remove_personal_data', 'request-confirmed' ) );
		},
		'erasure-fulfilled'              => function () use ( $request ) {
			_wp_privacy_send_erasure_fulfillment_notification( $request( 'remove_personal_data', 'request-completed' ) );
		},
		'erasure-fulfilled-policy'       => function () use ( $request ) {
			wp_update_post(
				array(
					'ID'          => 3,
					'post_status' => 'publish',
				)
			);
			_wp_privacy_send_erasure_fulfillment_notification( $request( 'remove_personal_data', 'request-completed' ) );
		},
		'export-ready'                   => function () use ( $request ) {
			$id = $request( 'export_personal_data', 'request-completed' );
			update_post_meta( $id, '_export_file_name', 'wp-personal-data-file-abc123.zip' );
			wp_privacy_send_personal_data_export_email( $id );
		},
	);
	if ( count( $args ) < 2 ) {
		echo implode( "\n", array_keys( $cases ) ), "\n";
		return;
	}
	list( , $name, $blog ) = $args;

	// The site title as stored: esc_html() keeps entities as they are, so an
	// odd title has every entity wp_specialchars_decode() treats.
	$titles = array(
		'odd'   => 'Tom &amp; Jerry&#039;s &lt;b&gt; &#0039; &#X26; &#x00027; &amp;lt; 100% &quot;q&quot; &#034;',
		'empty' => '',
	);
	if ( isset( $titles[ $blog ] ) ) {
		add_filter( 'option_blogname', fn() => $titles[ $blog ] );
	}

	global $wpdb;
	$GLOBALS['hyscribe_mail'] = array();
	$wpdb->query( 'START TRANSACTION' );
	$start = time();
	$cases[ $name ]();
	$end = time();
	$wpdb->query( 'ROLLBACK' );

	// The export's expiry, three days on, in the date format.
	$dates = array();
	for ( $t = $start; $t <= $end; $t++ ) {
		$dates[ date_i18n( get_option( 'date_format' ), $t + 3 * DAY_IN_SECONDS ) ] = '%date%';
	}
	$mask = function ( $value ) use ( $dates ) {
		$value = preg_replace( '/([?&](?:#038;)?(?:confirm_)?key=)[A-Za-z0-9]{20}(?![A-Za-z0-9])/', '$1%key%', $value );
		$value = preg_replace( '/((?:newuseremail|adminhash)=)[0-9a-f]{32}/', '$1%hash%', $value );
		$value = preg_replace( '/(request_id=)\d+/', '$1%request%', $value );
		return strtr( $value, $dates );
	};
	$escape = fn( $value ) => strtr( $value, array( '\\' => '\\\\', "\t" => '\t', "\n" => '\n', "\r" => '\r' ) );

	$label = "$name/$blog";
	if ( ! $GLOBALS['hyscribe_mail'] ) {
		echo "$label\t-\n";
	}
	foreach ( $GLOBALS['hyscribe_mail'] as $mail ) {
		$fields = array( $mail['to'], $mail['subject'], $mail['message'], $mail['headers'] );
		foreach ( $fields as $field ) {
			if ( ! is_string( $field ) ) {
				fwrite( STDERR, "$label: a field is not a string\n" );
				exit( 1 );
			}
		}
		echo $label, "\t", implode( "\t", array_map( fn( $f ) => $escape( $mask( $f ) ), $fields ) ), "\n";
	}
} )( $args );

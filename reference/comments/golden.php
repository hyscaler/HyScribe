<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints the comment blocks over comments inserted into the fixture
// database inside a transaction that is rolled back. Lines are
//   hex(markup)<TAB>hex(json state)<TAB>hex(html)
// where the state has the options set, the post and every comment row.

require '/var/www/html/wp-load.php';

// The elements support (wp-elements-* classes) is not ported yet.
remove_filter( 'render_block', 'wp_render_elements_class_name', 10 );
remove_filter( 'render_block_data', 'wp_render_elements_support_styles', 10 );

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
foreach ( array(
	array( '', 0, '' ),
	array( '', 1, 'http://' ),
	array( '<b>Bold</b> & co', 0, 'javascript:alert(1)' ),
	array( 'Linked', 0, 'https://example.test/a b?x=1&y=2' ),
	array( 'Plain http', 0, 'http://example.test' ),
) as $n => list( $author, $user_id, $url ) ) {
	$insert( array( 'comment_author' => $author, 'user_id' => $user_id, 'comment_author_url' => $url, 'comment_date' => "2026-10-2$n 1$n:3$n:00", 'comment_date_gmt' => "2026-10-27 13:0$n:00" ) );
}
$insert( array( 'comment_post_ID' => 2, 'comment_author' => 'On the front page' ) );
foreach ( array(
	"See http://example.test/a_(b) and www.example.org, or mail me@example.test :) \"quoted\" -- wordpress\n\nSecond <b>para\nline",
	'0',
	'',
	home_url( '/inside' ) . ' <code>http://example.test</code> <a href="http://example.test">http://example.test</a> 8-)',
	"<p>Already</p>\n<blockquote>q</blockquote>",
) as $n => $content ) {
	$insert( array( 'comment_author' => "S$n content", 'comment_content' => $content, 'comment_date_gmt' => "2026-10-27 14:0$n:00" ) );
}
// Users for the avatar's initials: no display name, no email address.
$named  = wp_insert_user( array( 'user_login' => 'named', 'user_pass' => 'x', 'user_email' => 'Named@Example.test ', 'first_name' => 'Ada', 'last_name' => 'Lovelace' ) );
$wpdb->update( $wpdb->users, array( 'display_name' => '' ), array( 'ID' => $named ) );
$login  = wp_insert_user( array( 'user_login' => 'loginonly', 'user_pass' => 'x', 'user_email' => '' ) );
$wpdb->update( $wpdb->users, array( 'display_name' => '' ), array( 'ID' => $login ) );
$first  = wp_insert_user( array( 'user_login' => 'firstonly', 'user_pass' => 'x', 'user_email' => 'first@example.test', 'first_name' => 'Grace' ) );
$wpdb->update( $wpdb->users, array( 'display_name' => '' ), array( 'ID' => $first ) );
clean_user_cache( $named );
clean_user_cache( $login );
clean_user_cache( $first );
foreach ( array(
	array( 'Ada by user', $named, 'other@example.test' ),
	array( 'Login by user', $login, 'other@example.test' ),
	array( 'First by user', $first, 'other@example.test' ),
	array( "\u{5c71}\u{7530} \u{592a}\u{90ce}", 0, 'kanji@example.test' ),
	array( 'Ann Lee Smith', 0, '' ),
	array( 'single', 0, '  MiXed@Example.TEST' ),
	array( "\u{00e9}mile \u{00c9}cole", 0, 'e@example.test' ),
	array( 'Missing user', 999, 'gone@example.test' ),
	array( "\u{1F600} Smile", 0, 'smile@example.test' ),
) as $n => list( $author, $user_id, $email ) ) {
	$insert( array( 'comment_author' => $author, 'user_id' => $user_id, 'comment_author_email' => $email, 'comment_author_url' => $n % 2 ? 'https://example.test/' . $n : '', 'comment_date_gmt' => "2026-10-27 15:0$n:00" ) );
}
$users = $wpdb->get_results( "SELECT ID, display_name, user_nicename, user_login, user_email FROM {$wpdb->users}", ARRAY_A );
foreach ( $users as &$user ) {
	$user['first_name'] = get_user_meta( $user['ID'], 'first_name', true );
	$user['last_name']  = get_user_meta( $user['ID'], 'last_name', true );
}
unset( $user );

// The front page takes comments, so its reply links show.
$wpdb->update( $wpdb->posts, array( 'comment_status' => 'open' ), array( 'ID' => 2 ) );
clean_post_cache( 2 );
$rows = $wpdb->get_results( "SELECT * FROM {$wpdb->comments} ORDER BY comment_ID", ARRAY_A );

$option_sets = array(
	array(),
	array( 'thread_comments' => '0' ),
	array( 'thread_comments_depth' => '2' ),
	array( 'thread_comments_depth' => '1' ),
	array( 'comment_order' => 'desc' ),
	array( 'comment_order' => 'desc', 'thread_comments_depth' => '3' ),
	array( 'comment_registration' => '1' ),
	array( 'date_format' => 'Y/m/d', 'thread_comments_depth' => '4' ),
	array( 'show_on_front' => 'page', 'page_on_front' => '2' ),
	array( 'use_smilies' => '0' ),
	array( 'avatar_default' => 'initials', 'avatar_rating' => 'PG' ),
	array( 'avatar_default' => 'gravatar_default', 'avatar_rating' => '' ),
	array( 'avatar_default' => 'https://example.test/a b.png?x=1', 'avatar_rating' => 'X' ),
	array( 'avatar_default' => 'mysteryman', 'show_avatars' => '' ),
	array( 'avatar_default' => '', 'avatar_rating' => '0' ),
);
$markups = array(
	'<!-- wp:comment-template --><p>c</p><!-- /wp:comment-template -->',
	'<!-- wp:comment-template {"className":"listed"} --><p>c</p><!-- /wp:comment-template -->',
	'<!-- wp:comment-template --><!-- wp:comment-date {"isLink":false} /--><!-- wp:comment-author-name {"style":{"elements":{"link":{"color":{"text":"red"}}}}} /--><!-- /wp:comment-template -->',
	'<!-- wp:comment-template --><!-- wp:comment-edit-link --><a>saved</a><!-- /wp:comment-edit-link --><!-- /wp:comment-template -->',
	'<!-- wp:comment-date /-->',
	'<!-- wp:comment-author-name /-->',
	'<!-- wp:comment-reply-link /-->',
	'<!-- wp:comment-content /-->',
	'<!-- wp:comment-template --><!-- wp:avatar /--><!-- wp:avatar {"size":40,"isLink":true,"linkTarget":"_blank","borderColor":"contrast","style":{"border":{"radius":"5px","width":"2px","top":{"color":"red","width":"1px"}}}} /--><!-- wp:avatar {"isLink":true,"style":{"border":{"color":"#123456","style":"dashed"}}} /--><!-- /wp:comment-template -->',
	'<!-- wp:avatar /-->',
	'<!-- wp:avatar {"userId":1,"isLink":true,"linkTarget":"_blank","size":300} /-->',
	'<!-- wp:avatar {"userId":99,"isLink":true} /-->',
	'<!-- wp:avatar {"userId":"1","size":0} /-->',
	'<!-- wp:avatar {"userId":-1} /-->',
	'<!-- wp:avatar {"userId":0} /-->',
	'<!-- wp:avatar {"userId":1.7,"size":-20} /-->',
	'<!-- wp:avatar {"userId":"abc"} /-->',
	'<!-- wp:comment-template --><!-- wp:comment-content /--><!-- wp:comment-content {"textAlign":"right","style":{"elements":{"link":{"color":{"text":"red"}}}}} /--><!-- /wp:comment-template -->',
	'<!-- wp:comment-template --><!-- wp:comment-date /--><!-- wp:comment-date {"format":"Y-m-d H:i","isLink":true} /--><!-- wp:comment-date {"format":"human-diff","style":{"elements":{"link":{"color":{"text":"red"}}}}} /--><!-- wp:comment-author-name /--><!-- wp:comment-author-name {"isLink":true,"linkTarget":"_blank","textAlign":"right"} /--><!-- wp:comment-author-name {"isLink":true,"linkTarget":""} /--><!-- wp:comment-edit-link /--><!-- wp:comment-reply-link /--><!-- wp:comment-reply-link {"textAlign":"center","style":{"elements":{"link":{"color":{"text":"red"}}}}} /--><!-- /wp:comment-template -->',
);
$reset = function () {
	$GLOBALS['comment_alt']        = 0;
	$GLOBALS['comment_depth']      = 0;
	$GLOBALS['comment_thread_alt'] = 0;
};
foreach ( $option_sets as $options ) {
	foreach ( array( 'thread_comments', 'thread_comments_depth', 'comment_order', 'comment_registration', 'date_format', 'show_on_front', 'page_on_front', 'use_smilies', 'avatar_default', 'avatar_rating', 'show_avatars' ) as $name ) {
		remove_all_filters( "pre_option_$name" );
	}
	foreach ( $options as $name => $value ) {
		add_filter( "pre_option_$name", static fn() => $value );
	}
	foreach ( array( 1, 2, 3, 0 ) as $post_id ) {
		foreach ( $markups as $markup ) {
			$reset();
			$context = $post_id ? array( 'postId' => $post_id ) : array();
			$html    = ( new WP_Block( parse_blocks( $markup )[0], $context ) )->render();
			// Twice in one request, as the counters carry on.
			$html   .= ( new WP_Block( parse_blocks( $markup )[0], $context ) )->render();
			$state   = array( 'options' => $options, 'post' => $post_id, 'comments' => $rows, 'users' => $users, 'password' => false, 'closed' => false, 'now' => time() );
			echo bin2hex( $markup ), "\t", bin2hex( wp_json_encode( $state + array( 'siteurl' => site_url() ) ) ), "\t", bin2hex( $html ), "\n";
		}
	}
}

// In a singular template's loop the first three images are not lazy, and
// the first large one takes the high priority.
foreach ( array( 'thread_comments', 'thread_comments_depth', 'comment_order', 'comment_registration', 'date_format', 'show_on_front', 'page_on_front', 'use_smilies', 'avatar_default', 'avatar_rating', 'show_avatars' ) as $name ) {
	remove_all_filters( "pre_option_$name" );
}
$GLOBALS['wp_query']->in_the_loop = true;
foreach ( array( current( preg_grep( '/^<!-- wp:comment-template --><!-- wp:avatar/', $markups ) ), '<!-- wp:avatar {"userId":1,"size":224} /-->', '<!-- wp:avatar {"userId":1,"size":223} /-->' ) as $markup ) {
	wp_increase_content_media_count( -wp_increase_content_media_count( 0 ) );
	wp_high_priority_element_flag( true );
	$reset();
	$html  = ( new WP_Block( parse_blocks( $markup )[0], array( 'postId' => 1 ) ) )->render();
	$html .= ( new WP_Block( parse_blocks( $markup )[0], array( 'postId' => 1 ) ) )->render();
	$state = array( 'options' => array(), 'post' => 1, 'comments' => $rows, 'users' => $users, 'password' => false, 'closed' => false, 'now' => time(), 'loop' => true );
	echo bin2hex( $markup ), "\t", bin2hex( wp_json_encode( $state + array( 'siteurl' => site_url() ) ) ), "\t", bin2hex( $html ), "\n";
}
$GLOBALS['wp_query']->in_the_loop = false;

// With comments closed there is no reply link.
foreach ( array( 'thread_comments', 'thread_comments_depth', 'comment_order', 'comment_registration', 'date_format', 'show_on_front', 'page_on_front', 'use_smilies', 'avatar_default', 'avatar_rating', 'show_avatars' ) as $name ) {
	remove_all_filters( "pre_option_$name" );
}
// Logged-out reply links go to the login page, back to the global post.
add_filter( 'pre_option_comment_registration', static fn() => '1' );
$GLOBALS['post'] = get_post( 1 );
$reset();
$markup = end( $markups );
$state  = array( 'options' => array( 'comment_registration' => '1' ), 'post' => 1, 'comments' => $rows, 'users' => $users, 'password' => false, 'closed' => false, 'now' => time(), 'global' => 1 );
$html   = ( new WP_Block( parse_blocks( $markup )[0], array( 'postId' => 1 ) ) )->render();
$html  .= ( new WP_Block( parse_blocks( $markup )[0], array( 'postId' => 1 ) ) )->render();
echo bin2hex( $markup ), "\t", bin2hex( wp_json_encode( $state + array( 'siteurl' => site_url() ) ) ), "\t", bin2hex( $html ), "\n";
remove_all_filters( 'pre_option_comment_registration' );
$GLOBALS['post'] = null;
$wpdb->update( $wpdb->posts, array( 'comment_status' => 'closed' ), array( 'ID' => 1 ) );
clean_post_cache( 1 );
$reset();
$markup = end( $markups );
$state  = array( 'options' => array(), 'post' => 1, 'comments' => $rows, 'users' => $users, 'password' => false, 'closed' => true, 'now' => time() );
$html  = ( new WP_Block( parse_blocks( $markup )[0], array( 'postId' => 1 ) ) )->render();
$html .= ( new WP_Block( parse_blocks( $markup )[0], array( 'postId' => 1 ) ) )->render();
echo bin2hex( $markup ), "\t", bin2hex( wp_json_encode( $state + array( 'siteurl' => site_url() ) ) ), "\t", bin2hex( $html ), "\n";

// A password protected post shows no comments.
$wpdb->update( $wpdb->posts, array( 'post_password' => 'pw' ), array( 'ID' => 1 ) );
clean_post_cache( 1 );
$markup = $markups[0];
$reset();
$state = array( 'options' => array(), 'post' => 1, 'comments' => $rows, 'users' => $users, 'password' => true, 'closed' => true, 'now' => time() );
$html  = ( new WP_Block( parse_blocks( $markup )[0], array( 'postId' => 1 ) ) )->render();
$html .= ( new WP_Block( parse_blocks( $markup )[0], array( 'postId' => 1 ) ) )->render();
echo bin2hex( $markup ), "\t", bin2hex( wp_json_encode( $state + array( 'siteurl' => site_url() ) ) ), "\t", bin2hex( $html ), "\n";

// A post template that inherits the main query runs its loop, and only
// there are images counted; a query of its own runs after it has ended.
$wpdb->update( $wpdb->posts, array( 'post_password' => '' ), array( 'ID' => 1 ) );
clean_post_cache( 1 );
$GLOBALS['wp_query']->query( array( 'post_type' => array( 'post', 'page' ), 'orderby' => 'ID', 'order' => 'ASC' ) );
wp_increase_content_media_count( -wp_increase_content_media_count( 0 ) );
wp_high_priority_element_flag( true );
$markup = '<!-- wp:group --><div class="wp-block-group"><!-- wp:query {"query":{"inherit":true}} --><div class="wp-block-query"><!-- wp:post-template --><!-- wp:avatar /--><!-- wp:avatar /--><!-- /wp:post-template --></div><!-- /wp:query --><!-- wp:avatar {"userId":1} /--><!-- wp:query {"query":{"inherit":false,"postType":"page","perPage":1}} --><div class="wp-block-query"><!-- wp:post-template --><!-- wp:avatar /--><!-- /wp:post-template --></div><!-- /wp:query --></div><!-- /wp:group -->';
$html  = ( new WP_Block( parse_blocks( $markup )[0] ) )->render();
$html .= ( new WP_Block( parse_blocks( $markup )[0] ) )->render();
$state = array( 'options' => array(), 'post' => 0, 'comments' => $rows, 'users' => $users, 'password' => false, 'closed' => true, 'now' => time(), 'main' => wp_list_pluck( $GLOBALS['wp_query']->posts, 'ID' ) );
echo bin2hex( $markup ), "\t", bin2hex( wp_json_encode( $state + array( 'siteurl' => site_url() ) ) ), "\t", bin2hex( $html ), "\n";

$wpdb->query( 'ROLLBACK' );

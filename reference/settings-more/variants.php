<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Must-use plugin for reference/settings-more/golden.sh, through filters and
// the object cache only: each variant gives the options it names, as
// get_option() reads them. s2trashed caches a trashed page 98.
$hyscribe_options = array(
	's2discussion' => array(
		'default_pingback_flag'        => '0',
		'default_ping_status'          => 'closed',
		'default_comment_status'       => 'closed',
		'require_name_email'           => '0',
		'comment_registration'         => '1',
		'close_comments_for_old_posts' => '1',
		'close_comments_days_old'      => '7"',
		'show_comments_cookies_opt_in' => '0',
		'thread_comments'              => '0',
		'thread_comments_depth'        => '10',
		'page_comments'                => '1',
		'comments_per_page'            => '20',
		'default_comments_page'        => 'oldest',
		'comment_order'                => 'desc',
		'comments_notify'              => '0',
		'moderation_notify'            => '0',
		'wp_notes_notify'              => '0',
		'comment_moderation'           => '1',
		'comment_previously_approved'  => '0',
		'comment_max_links'            => '5',
		'moderation_keys'              => "spam\n<b>&amp; \"x\"",
		'disallowed_keys'              => "viagra\n'y' & </textarea>",
		'avatar_rating'                => 'PG',
		'avatar_default'               => 'identicon',
		'show_avatars'                 => '2',
	),
	's2noavatars'  => array(
		'show_avatars'          => '0',
		'avatar_rating'         => 'X',
		'avatar_default'        => 'gravatar_default',
		'thread_comments_depth' => '3x',
	),
	's2media'      => array(
		'thumbnail_size_w'              => '100"',
		'thumbnail_size_h'              => '80',
		'thumbnail_crop'                => '0',
		'medium_size_w'                 => '400',
		'medium_size_h'                 => '0',
		'large_size_w'                  => '2048',
		'large_size_h'                  => '1536',
		'uploads_use_yearmonth_folders' => '0',
		'upload_path'                   => 'files/<up>',
	),
	's2urlpath'    => array(
		'upload_path'     => 'wp-content/uploads',
		'upload_url_path' => 'http://cdn.test/a"b',
	),
	's2uploads'    => array(
		'upload_path' => 'wp-content/uploads',
	),
	's2day'        => array( 'permalink_structure' => '/%year%/%monthnum%/%day%/%postname%/' ),
	's2month'      => array( 'permalink_structure' => '/%year%/%monthnum%/%postname%/' ),
	's2numeric'    => array( 'permalink_structure' => '/archives/%post_id%' ),
	's2postname'   => array( 'permalink_structure' => '/%postname%/' ),
	's2custom'     => array(
		'permalink_structure' => '/%category%/<%postname%>/',
		'category_base'       => 'topics',
		'tag_base'            => 'labels"<',
	),
	's2published'  => array( 'wp_page_for_privacy_policy' => '2' ),
	's2unset'      => array( 'wp_page_for_privacy_policy' => '0' ),
	's2missing'    => array( 'wp_page_for_privacy_policy' => '99' ),
	's2trashed'    => array( 'wp_page_for_privacy_policy' => '98' ),
);
$hyscribe_variant = $_COOKIE['hyscribe_variant'] ?? '';
foreach ( $hyscribe_options[ $hyscribe_variant ] ?? array() as $hyscribe_name => $hyscribe_value ) {
	add_filter( "pre_option_$hyscribe_name", fn() => $hyscribe_value );
}
// The Permalinks screen flushes the rewrite rules: keep .htaccess as it is.
add_filter( 'flush_rewrite_rules_hard', '__return_false' );
if ( 's2trashed' === $hyscribe_variant ) {
	wp_cache_add(
		98,
		(object) array(
			'ID'          => 98,
			'post_type'   => 'page',
			'post_status' => 'trash',
			'post_title'  => 'Old policy',
			'filter'      => 'raw',
		),
		'posts'
	);
}
// s2nopages finds no pages, s2orphan only one whose parent is not listed.
if ( in_array( $hyscribe_variant, array( 's2nopages', 's2orphan' ), true ) ) {
	$hyscribe_pages = 's2nopages' === $hyscribe_variant ? array() : array(
		new WP_Post(
			(object) array(
				'ID'          => 60,
				'post_parent' => 999,
				'post_type'   => 'page',
				'post_status' => 'publish',
				'post_title'  => 'Orphan',
				'filter'      => 'raw',
			)
		),
	);
	add_filter(
		'posts_pre_query',
		fn( $posts, $query ) => 'page' === $query->get( 'post_type' ) ? $hyscribe_pages : $posts,
		10,
		2
	);
}

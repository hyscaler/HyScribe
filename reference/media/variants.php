<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Must-use plugin for reference/media/golden.sh, through filters only: no
// screen writes the user's library mode or an edit lock, and each
// variant gives the user options or mime types it names.
add_filter(
	'update_user_metadata',
	fn( $check, $id, $key ) => str_ends_with( $key, 'media_library_mode' ) ? true : $check,
	10,
	3
);
add_filter(
	'update_post_metadata',
	fn( $check, $id, $key ) => '_edit_lock' === $key ? true : $check,
	10,
	3
);
add_filter(
	'add_post_metadata',
	fn( $check, $id, $key ) => '_edit_lock' === $key ? true : $check,
	10,
	3
);
$hyscribe_variant = $_COOKIE['hyscribe_variant'] ?? '';
if ( 'm4list' === $hyscribe_variant ) {
	add_filter( 'get_user_option_media_library_mode', fn() => 'list' );
}
if ( 'm4paged' === $hyscribe_variant ) {
	add_filter( 'get_user_option_upload_per_page', fn() => '5' );
}
if ( 'm4types' === $hyscribe_variant ) {
	add_filter(
		'pre_get_available_post_mime_types',
		fn() => array( 'image/jpeg', 'application/pdf', 'application/vnd.ms-excel', 'application/zip', 'text/plain' )
	);
}
if ( 'm4pages' === $hyscribe_variant ) {
	add_filter( 'pre_option_wp_attachment_pages_enabled', fn() => '1' );
}
if ( 'm4states' === $hyscribe_variant ) {
	add_filter( 'theme_mod_custom_logo', fn() => 1692 );
}
if ( 'm4hidden' === $hyscribe_variant ) {
	add_filter( 'get_user_option_manageuploadcolumnshidden', fn() => array( 'author' ) );
}
// The stored settings equal the cookie golden.sh sends, which
// wp_user_settings() then leaves alone.
if ( 'm4uploader' === $hyscribe_variant ) {
	add_filter( 'get_user_option_user-settings', fn() => 'uploader=1' );
}
if ( 'm4large' === $hyscribe_variant ) {
	add_filter( 'pre_option_large_size_h', fn() => '0' );
}
// The newest attachment as the user's, uploaded two hours ago, and the
// video with the image 611 as its thumbnail.
if ( 'm4recent' === $hyscribe_variant ) {
	add_filter(
		'the_posts',
		function ( $posts ) {
			foreach ( $posts as $post ) {
				if ( 1692 === $post->ID ) {
					$post->post_date   = gmdate( 'Y-m-d H:i:s', time() - 7200 );
					$post->post_author = '1';
				}
			}
			return $posts;
		}
	);
	add_filter(
		'get_post_metadata',
		fn( $value, $id, $key ) => 1690 === $id && '_thumbnail_id' === $key ? array( '611' ) : $value,
		10,
		3
	);
}
// One attachment, 611, in the Trash.
if ( 'm4trash' === $hyscribe_variant ) {
	add_filter(
		'posts_pre_query',
		function ( $posts, $query ) {
			if ( 'attachment' !== $query->get( 'post_type' ) ) {
				return $posts;
			}
			$post                 = clone get_post( 611 );
			$post->post_status    = 'trash';
			$query->found_posts   = 1;
			$query->max_num_pages = 1;
			return array( $post );
		},
		10,
		2
	);
}

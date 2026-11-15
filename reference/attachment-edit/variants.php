<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Must-use plugin for reference/attachment-edit/golden.sh, through filters
// only. In every a8lock variant user 2 (a8lock3: user 3) holds a fresh edit
// lock, so a view writes no lock of its own. a8lockcount and the a8lockref
// variants, which golden.sh sends with a referer, count the users as two,
// as the stored count of the default install with
// reference/attachment-edit/content.sql does not; a8locklayout gives the
// user screen options and sorted meta boxes; a8locknorich turns rich editing
// off; a8locknodelete takes away deleting attachment 801, a8locknoread
// reading it. The j9 variants hold user 2's lock too: j9pretty and
// j9prettynoread turn pretty permalinks on, j9prettynoread takes away
// reading attachment 801, j9noupload uploading files, and j9front makes
// attachment 802 the page on the front.
$hyscribe_variant = $_COOKIE['hyscribe_variant'] ?? '';
if ( str_starts_with( $hyscribe_variant, 'a8lock' ) || str_starts_with( $hyscribe_variant, 'j9' ) ) {
	$hyscribe_holder = 'a8lock3' === $hyscribe_variant ? 3 : 2;
	add_filter(
		'get_post_metadata',
		fn( $value, $id, $key ) => '_edit_lock' === $key ? array( time() . ":$hyscribe_holder" ) : $value,
		10,
		3
	);
}
if ( in_array( $hyscribe_variant, array( 'a8lockcount', 'a8lockref', 'a8lockrefpost' ), true ) ) {
	add_filter( 'pre_option_user_count', fn() => '2' );
}
if ( 'a8locklayout' === $hyscribe_variant ) {
	add_filter( 'get_user_option_screen_layout_attachment', fn() => '1' );
	add_filter( 'get_user_option_metaboxhidden_attachment', fn() => array( 'commentstatusdiv' ) );
	add_filter( 'get_user_option_closedpostboxes_attachment', fn() => array( 'slugdiv' ) );
	add_filter(
		'get_user_option_meta-box-order_attachment',
		fn() => array(
			'side'   => 'authordiv,submitdiv',
			'normal' => 'slugdiv',
		)
	);
}
foreach ( array( 'a8locknodelete' => 'delete_post', 'a8locknoread' => 'read_post', 'j9prettynoread' => 'read_post' ) as $hyscribe_name => $hyscribe_cap ) {
	if ( $hyscribe_name === $hyscribe_variant ) {
		add_filter(
			'map_meta_cap',
			fn( $caps, $cap, $user, $args ) => $hyscribe_cap === $cap && 801 === (int) ( $args[0] ?? 0 ) ? array( 'do_not_allow' ) : $caps,
			10,
			4
		);
	}
}
if ( 'a8locknorich' === $hyscribe_variant ) {
	add_filter( 'get_user_option_rich_editing', fn() => 'false' );
}
if ( in_array( $hyscribe_variant, array( 'j9pretty', 'j9prettynoread' ), true ) ) {
	add_filter( 'pre_option_permalink_structure', fn() => '/%postname%/' );
}
if ( 'j9front' === $hyscribe_variant ) {
	add_filter( 'pre_option_show_on_front', fn() => 'page' );
	add_filter( 'pre_option_page_on_front', fn() => '802' );
}
if ( 'j9noupload' === $hyscribe_variant ) {
	add_filter(
		'user_has_cap',
		function ( $allcaps ) {
			$allcaps['upload_files'] = false;
			return $allcaps;
		},
		1
	);
}

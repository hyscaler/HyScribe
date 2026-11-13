<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Must-use plugin for reference/profile/golden.sh, through filters only:
// each variant gives user 1 the meta it names, as get_user_meta() reads it.
$hyscribe_meta = array(
	'options'   => array(
		'rich_editing'         => 'false',
		'syntax_highlighting'  => 'false',
		'comment_shortcuts'    => 'true',
		'show_admin_bar_front' => 'false',
		'infinite_scrolling'   => 'false',
		'admin_color'          => 'ocean',
		'first_name'           => 'Ad &amp; Min',
		'last_name'            => 'Istrator',
		'nickname'             => 'boss',
		'description'          => 'Bio <b>bold</b> & more',
	),
	'pending'   => array(
		'_new_email' => array(
			'hash'     => 'x',
			'newemail' => 'new@example.test',
		),
	),
	'caps'      => array(
		'wp_capabilities' => array(
			'administrator' => true,
			'edit_posts'    => true,
			'custom'        => false,
		),
	),
	'sessions1' => array(
		'session_tokens' => array(
			'a' => array( 'expiration' => 4102444800 ),
		),
	),
	'sessions2' => array(
		'session_tokens' => array(
			'a' => array( 'expiration' => 4102444800 ),
			'b' => array( 'expiration' => 4102444800 ),
		),
	),
);
$hyscribe_variant = $_COOKIE['hyscribe_variant'] ?? '';
add_filter(
	'get_user_metadata',
	function ( $value, $id, $key ) use ( $hyscribe_meta, $hyscribe_variant ) {
		if ( 1 === (int) $id && isset( $hyscribe_meta[ $hyscribe_variant ][ $key ] ) ) {
			return array( $hyscribe_meta[ $hyscribe_variant ][ $key ] );
		}
		return $value;
	},
	10,
	3
);
if ( 'noavatars' === $hyscribe_variant ) {
	add_filter( 'pre_option_show_avatars', fn() => '0' );
}

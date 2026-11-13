<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Must-use plugin for reference/user-edit/golden.sh, through filters only:
// users other than 1 keep their own capabilities, which as-role.php would
// replace by user 1's, u3nocreate and u3nopromote deny user 1 a capability,
// and each variant gives users the meta it names, as get_user_meta() reads
// it.
$hyscribe_meta = array(
	'u3options'  => array(
		601 => array(
			'rich_editing'         => 'false',
			'syntax_highlighting'  => 'false',
			'comment_shortcuts'    => 'true',
			'show_admin_bar_front' => 'false',
			'infinite_scrolling'   => 'false',
			'admin_color'          => 'ocean',
			'nickname'             => 'eddie',
			'description'          => 'Bio <b>bold</b> & more',
		),
	),
	'u3sessions' => array(
		601 => array(
			'session_tokens' => array(
				'a' => array( 'expiration' => 4102444800 ),
			),
		),
	),
	'u3caps'     => array(
		601 => array(
			'wp_capabilities' => array(
				'editor'     => true,
				'edit_users' => true,
				'custom'     => false,
			),
		),
	),
	'u3roles'    => array(
		604 => array( 'wp_capabilities' => array( 'contributor' => true ) ),
		606 => array( 'wp_capabilities' => array( 'administrator' => true ) ),
	),
);
$hyscribe_variant = $_COOKIE['hyscribe_variant'] ?? '';
add_filter(
	'user_has_cap',
	function ( $allcaps, $caps, $args, $user ) use ( $hyscribe_variant ) {
		if ( 1 !== $user->ID ) {
			return $user->allcaps;
		}
		$denied = array(
			'u3nocreate'  => 'create_users',
			'u3nopromote' => 'promote_users',
		);
		if ( isset( $denied[ $hyscribe_variant ] ) ) {
			$allcaps[ $denied[ $hyscribe_variant ] ] = false;
		}
		return $allcaps;
	},
	1,
	4
);
add_filter(
	'get_user_metadata',
	function ( $value, $id, $key ) use ( $hyscribe_meta, $hyscribe_variant ) {
		if ( isset( $hyscribe_meta[ $hyscribe_variant ][ (int) $id ][ $key ] ) ) {
			return array( $hyscribe_meta[ $hyscribe_variant ][ (int) $id ][ $key ] );
		}
		return $value;
	},
	10,
	3
);
if ( 'u3noavatars' === $hyscribe_variant ) {
	add_filter( 'pre_option_show_avatars', fn() => '0' );
}
if ( 'u3default' === $hyscribe_variant ) {
	add_filter( 'pre_option_default_role', fn() => 'editor' );
}

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
// The u3ssl variants are served over HTTPS, which offers application
// passwords: user 601 has two, user 1 has one.
$hyscribe_meta['u3sslapps'] = array(
	601 => array(
		'_application_passwords' => array(
			array(
				'uuid'      => '55555555-5555-4555-8555-555555555555',
				'app_id'    => '',
				'name'      => 'Tablet',
				'password'  => 'x',
				'created'   => 1700000000,
				'last_used' => 1732000000,
				'last_ip'   => '2001:db8::1',
			),
			array(
				'uuid'      => '66666666-6666-4666-8666-666666666666',
				'app_id'    => '',
				'name'      => 'Script',
				'password'  => 'y',
				'created'   => 1731974400,
				'last_used' => null,
				'last_ip'   => null,
			),
		),
	),
	1   => array(
		'_application_passwords' => array(
			array(
				'uuid'      => '77777777-7777-4777-8777-777777777777',
				'app_id'    => '',
				'name'      => 'Admin app',
				'password'  => 'z',
				'created'   => 1700000000,
				'last_used' => null,
				'last_ip'   => null,
			),
		),
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

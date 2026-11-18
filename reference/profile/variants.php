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
$hyscribe_apps = array(
	array(
		'uuid'      => '11111111-1111-4111-8111-111111111111',
		'app_id'    => '',
		'name'      => 'Phone',
		'password'  => 'x',
		'created'   => 1700000000,
		'last_used' => null,
		'last_ip'   => null,
	),
);
$hyscribe_meta['sslapps1'] = array( '_application_passwords' => $hyscribe_apps );
$hyscribe_meta['sslapps3'] = array(
	'_application_passwords' => array(
		$hyscribe_apps[0],
		array(
			'uuid'      => '22222222-2222-4222-8222-222222222222',
			'app_id'    => '33333333-3333-4333-8333-333333333333',
			'name'      => 'Laptop <b>"quoted"</b> & more',
			'password'  => 'y',
			'created'   => 1731974400,
			'last_used' => 1732000000,
			'last_ip'   => '203.0.113.7',
		),
		array(
			'uuid'      => '44444444-4444-4444-8444-444444444444',
			'app_id'    => '',
			'name'      => 'Empty',
			'password'  => 'z',
			'created'   => 0,
			'last_used' => 0,
			'last_ip'   => '',
		),
	),
);
$hyscribe_meta['sslhidden']    = array(
	'_application_passwords' => $hyscribe_apps,
	'wp_manageapplication-passwords-usercolumnshidden' => array( 'last_ip', 'created' ),
);
$hyscribe_meta['sslhiddenall'] = array(
	'_application_passwords' => $hyscribe_apps,
	'wp_manageapplication-passwords-usercolumnshidden' => array( 'name', 'created', 'last_used', 'last_ip', 'revoke' ),
);
$hyscribe_meta['sslhiddenempty'] = array(
	'wp_manageapplication-passwords-usercolumnshidden' => array( 'last_ip' ),
);
$hyscribe_meta['sslsessions2'] = $hyscribe_meta['sessions2'];
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

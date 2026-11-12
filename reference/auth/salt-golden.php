<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for hyscribe-auth's wp_salt() and nonces: WordPress's own wp_salt()
// and wp_create_nonce() run once per case in a fresh PHP process, with the
// case's constants defined, its options stubbed, and for nonces the current
// user and session token stubbed. Lines are
//   salt <TAB> scheme <TAB> hex(constants JSON) <TAB> hex(options JSON) <TAB> hex(salt),
//     or ~ where WordPress would generate a salt and store it
//   nonce <TAB> hex(constants JSON) <TAB> user ID <TAB> hex(token) <TAB> hex(action) <TAB> tick <TAB> nonce
//   cookie <TAB> scheme <TAB> hex(user_login) <TAB> hex(user_pass) <TAB> expiration <TAB> hex(token) <TAB> hex(cookie)
//   php salt-golden.php <wordpress> > crates/hyscribe-auth/tests/golden/salt.tsv

if ( isset( $argv[2] ) ) {
	// One case: constants and options as JSON, then a scheme, or the user,
	// token and actions of nonces.
	define( 'ABSPATH', rtrim( $argv[1], '/' ) . '/' );
	define( 'WPINC', 'wp-includes' );
	define( 'DAY_IN_SECONDS', 86400 );
	foreach ( json_decode( $argv[2], true ) as $name => $value ) {
		define( $name, $value );
	}
	$GLOBALS['options'] = json_decode( $argv[3], true );
	function __( $text ) {
		return $text;
	}
	function get_site_option( $name ) {
		return $GLOBALS['options'][ $name ] ?? false;
	}
	function wp_prime_site_option_caches( $names ) {
	}
	function update_site_option( $name, $value ) {
		echo '~';
		exit;
	}
	function wp_generate_password() {
		return 'generated';
	}
	function wp_get_current_user() {
		return (object) array( 'ID' => (int) ( $GLOBALS['argv'][5] ?? 0 ) );
	}
	function wp_get_session_token() {
		return $GLOBALS['argv'][6] ?? '';
	}
	function get_userdata( $id ) {
		return (object) array( 'user_login' => $GLOBALS['argv'][5] ?? '', 'user_pass' => $GLOBALS['argv'][6] ?? '' );
	}
	require ABSPATH . WPINC . '/plugin.php';
	require ABSPATH . WPINC . '/pluggable.php';
	if ( 'cookie' === $argv[4] ) {
		foreach ( array( 'auth', 'secure_auth', 'logged_in' ) as $scheme ) {
			echo $scheme, ' ', bin2hex( wp_generate_auth_cookie( 1, (int) $argv[7], $scheme, $argv[8] ) ), "\n";
		}
		exit;
	}
	if ( 'nonce' === $argv[4] && isset( $argv[7] ) ) {
		foreach ( json_decode( $argv[7], true ) as $action ) {
			echo bin2hex( (string) $action ), ' ', wp_nonce_tick( $action ), ' ', wp_create_nonce( $action ), "\n";
		}
		exit;
	}
	echo bin2hex( wp_salt( $argv[4] ) );
	exit;
}

$run = static fn( ...$args ) => shell_exec( implode( ' ', array_map( 'escapeshellarg', array_merge( array( PHP_BINARY, __FILE__, $argv[1] ), $args ) ) ) );

$names = array( 'AUTH_KEY', 'SECURE_AUTH_KEY', 'LOGGED_IN_KEY', 'NONCE_KEY', 'AUTH_SALT', 'SECURE_AUTH_SALT', 'LOGGED_IN_SALT', 'NONCE_SALT' );
$all   = array_combine( $names, array_map( static fn( $n ) => strtolower( $n ) . ' value', $names ) );
$cases = array(
	array( $all, array() ),
	array( array( 'AUTH_KEY' => 'k', 'AUTH_SALT' => 's' ), array() ),
	array( array( 'AUTH_KEY' => 'put your unique phrase here' ) + $all, array( 'auth_key' => 'stored key' ) ),
	array( array( 'AUTH_KEY' => 'put your unique phrase here' ) + $all, array() ),
	array( array( 'AUTH_KEY' => 'same', 'NONCE_SALT' => 'same' ) + $all, array( 'auth_key' => 'stored key', 'nonce_salt' => 'stored nonce salt' ) ),
	array( array( 'AUTH_SALT' => 'same', 'LOGGED_IN_KEY' => 'same' ) + $all, array( 'auth_salt' => 'stored salt' ) ),
	array( array( 'AUTH_KEY' => 'same', 'AUTH_SALT' => 'same' ), array( 'auth_key' => 'k', 'auth_salt' => 's' ) ),
	array( array( 'AUTH_KEY' => '0' ) + $all, array( 'auth_key' => 'stored key' ) ),
	array( array( 'AUTH_KEY' => '' ) + $all, array( 'auth_key' => 'stored key' ) ),
	array( array( 'AUTH_SALT' => 's' ), array( 'auth_key' => 'stored key' ) ),
	array( array( 'AUTH_KEY' => 'k' ), array( 'auth_salt' => '0' ) ),
	array( array(), array( 'auth_key' => 'stored key', 'auth_salt' => 'stored salt' ) ),
	array( array(), array( 'auth_key' => 'stored key' ) ),
	array( array(), array() ),
	array( array( 'SECURE_AUTH_KEY' => 'k', 'AUTH_SALT' => 's' ), array( 'auth_key' => 'other' ) ),
	array( array( 'NONCE_KEY' => 'nk' ), array( 'nonce_salt' => 'ns', 'logged_in_key' => 'lk', 'logged_in_salt' => 'ls' ) ),
);
foreach ( $cases as list( $constants, $options ) ) {
	foreach ( array( 'auth', 'secure_auth', 'logged_in', 'nonce' ) as $scheme ) {
		$out = $run( json_encode( $constants ), json_encode( (object) $options ), $scheme );
		echo 'salt', "\t", $scheme, "\t", bin2hex( json_encode( (object) $constants ) ), "\t", bin2hex( json_encode( (object) $options ) ), "\t", $out, "\n";
	}
}
$actions = json_encode( array( -1, 'log-out', 'update-post_1', '', '0' ) );
foreach ( array( array( 0, '' ), array( 1, 'token' ), array( 42, "t|ken" ) ) as list( $user, $token ) ) {
	foreach ( explode( "\n", trim( $run( json_encode( $all ), '{}', 'nonce', (string) $user, $token, $actions ) ) ) as $line ) {
		list( $action, $tick, $nonce ) = explode( ' ', $line );
		echo 'nonce', "\t", bin2hex( json_encode( $all ) ), "\t", $user, "\t", bin2hex( $token ), "\t", $action, "\t", $tick, "\t", $nonce, "\n";
	}
}
$hashes = array( '$P$BhcDbuoXp5w5dIiVF7G1V1v1sq.Hof0', '$2y$10$8bDmo5f2V1q7kRbfY3t8U.QWl3MZp4y2XU4b6xZ4tC1qF5aQf4w1a', '$wp$2y$10$8bDmo5f2V1q7kRbfY3t8U.QWl3MZp4y2XU4b6xZ4tC1qF5aQf4w1a', '5f4dcc3b5aa765d61d8327deb882cf99', 'ab', '$P$abcdefg' );
foreach ( $hashes as $hash ) {
	foreach ( array( array( 'admin', 1893456000, 'tok' ), array( 'Jo|e', 1600000000, 'a|b' ) ) as list( $login, $expiration, $token ) ) {
		foreach ( explode( "\n", trim( $run( json_encode( $all ), '{}', 'cookie', $login, $hash, (string) $expiration, $token ) ) ) as $line ) {
			list( $scheme, $cookie ) = explode( ' ', $line );
			echo 'cookie', "\t", $scheme, "\t", bin2hex( $login ), "\t", bin2hex( $hash ), "\t", $expiration, "\t", bin2hex( $token ), "\t", $cookie, "\n";
		}
	}
}

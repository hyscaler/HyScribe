<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for hyscribe-auth's wp_salt( 'auth' ): WordPress's own wp_salt() run
// once per case in a fresh PHP process, with the case's constants defined and
// its options stubbed. One case per line:
//   hex(constants JSON) <TAB> hex(options JSON) <TAB> hex(salt), or ~ where
//   WordPress would generate a salt and store it.
//   php salt-golden.php <wordpress> > crates/hyscribe-auth/tests/golden/salt.tsv

if ( isset( $argv[2] ) ) {
	// One case: constants and options as JSON.
	define( 'ABSPATH', rtrim( $argv[1], '/' ) . '/' );
	define( 'WPINC', 'wp-includes' );
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
	require ABSPATH . WPINC . '/plugin.php';
	require ABSPATH . WPINC . '/pluggable.php';
	echo bin2hex( wp_salt( 'auth' ) );
	exit;
}

$names = array( 'AUTH_KEY', 'SECURE_AUTH_KEY', 'LOGGED_IN_KEY', 'NONCE_KEY', 'AUTH_SALT', 'SECURE_AUTH_SALT', 'LOGGED_IN_SALT', 'NONCE_SALT' );
$all   = array_combine( $names, array_map( static fn( $n ) => strtolower( $n ) . ' value', $names ) );
$cases = array(
	array( $all, array() ),
	array( array( 'AUTH_KEY' => 'k', 'AUTH_SALT' => 's' ), array() ),
	array( array( 'AUTH_KEY' => 'put your unique phrase here' ) + $all, array( 'auth_key' => 'stored key' ) ),
	array( array( 'AUTH_KEY' => 'put your unique phrase here' ) + $all, array() ),
	array( array( 'AUTH_KEY' => 'same', 'NONCE_SALT' => 'same' ) + $all, array( 'auth_key' => 'stored key' ) ),
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
);
foreach ( $cases as list( $constants, $options ) ) {
	$out = shell_exec( escapeshellarg( PHP_BINARY ) . ' ' . escapeshellarg( __FILE__ ) . ' ' . escapeshellarg( $argv[1] ) . ' ' . escapeshellarg( json_encode( $constants ) ) . ' ' . escapeshellarg( json_encode( (object) $options ) ) );
	echo bin2hex( json_encode( (object) $constants ) ), "\t", bin2hex( json_encode( (object) $options ) ), "\t", $out, "\n";
}

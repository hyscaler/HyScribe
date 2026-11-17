<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for hyscribe-auth's wp_verify_fast_hash(): WordPress's own function
// and its phpass class, from a WordPress 7.1.3 tree, over fast hashes,
// portable phpass hashes as WordPress before 6.8 stored reset keys, bcrypt
// through crypt(), hashes of neither kind and a phpass hash of a message
// up to and over 4096 bytes. Lines are
// hex(message) <TAB> hash <TAB> 1 or 0.
//   php verify-fast-hash-golden.php <wordpress> > crates/hyscribe-auth/tests/golden/verify-fast-hash.tsv

$root = rtrim( $argv[1], '/' );
define( 'ABSPATH', $root . '/' );
define( 'WPINC', 'wp-includes' );
$source = file_get_contents( $root . '/wp-includes/functions.php' );
foreach ( array( 'wp_fast_hash', 'wp_verify_fast_hash' ) as $name ) {
	preg_match( '/\nfunction ' . $name . '\(.*?\n}\n/s', $source, $found );
	eval( $found[0] );
}
require_once $root . '/wp-includes/class-phpass.php';

$key    = 'abcdefghijklmnopqrst';
$phpass = new PasswordHash( 8, true );
$p      = $phpass->HashPassword( $key );
$h      = '$H$' . substr( $p, 3 );
$cases  = array(
	array( $key, wp_fast_hash( $key ) ),
	array( 'abcdefghijklmnopqrsT', wp_fast_hash( $key ) ),
	array( '', wp_fast_hash( '' ) ),
	array( $key, wp_fast_hash( $key ) . 'x' ),
	array( $key, '$generic$' ),
	array( $key, $p ),
	array( 'other', $p ),
	array( $key, $h ),
	array( $key, '$P$' . 'Z' . substr( $p, 4 ) ),
	array( $key, substr( $p, 0, 20 ) ),
	array( $key, crypt( $key, '$2y$04$abcdefghijklmnopqrstuu' ) ),
	array( 'other', crypt( $key, '$2y$04$abcdefghijklmnopqrstuu' ) ),
	array( $key, $key ),
	array( $key, '' ),
	array( str_repeat( 'a', 4096 ), $phpass->crypt_private( str_repeat( 'a', 4096 ), $p ) ),
	array( str_repeat( 'a', 4097 ), $phpass->crypt_private( str_repeat( 'a', 4097 ), $p ) ),
);
foreach ( $cases as list( $message, $hash ) ) {
	echo bin2hex( $message ), "\t", $hash, "\t", wp_verify_fast_hash( $message, $hash ) ? 1 : 0, "\n";
}

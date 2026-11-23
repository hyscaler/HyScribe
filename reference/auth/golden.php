<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Records WordPress's password hashing as crates/hyscribe-auth/tests/golden/passwords.tsv:
//   crypt<TAB>hex key<TAB>setting<TAB>crypt( key, setting )
//   check<TAB>hex password<TAB>hex hash<TAB>wp_check_password() as 0 or 1
//   rehash<TAB>hex hash<TAB>wp_password_needs_rehash() as 0 or 1
//   hash<TAB>hex password<TAB>a hash wp_hash_password() made<TAB>wp_check_password() of it
// Run in the PHP container:
//   docker exec hyscribe-default-install-php php /dev/stdin < reference/auth/golden.php > crates/hyscribe-auth/tests/golden/passwords.tsv
// Only reads from the database.
require '/var/www/html/wp-load.php';
require_once ABSPATH . WPINC . '/class-phpass.php';

$passwords = array(
	'', 'a', 'password', ' padded ', "\ttabbed\n", "a\0b", "\0", 'héllo wörld', "\x80", "\x80\xff\x80\xff", "\xff\x00\xff",
	str_repeat( 'x', 71 ), str_repeat( 'x', 72 ), str_repeat( 'x', 73 ), str_repeat( 'é', 40 ), str_repeat( 'p', 4096 ), str_repeat( 'p', 4097 ),
	"\x80abc\x81def", 'U*U', 'U*U*', "\xa3", "\xff\xa3345", "\xff\xa3" . '34' . "\xff\xff\xff\xa3" . '345', "\xff\xff\xff\xa3", "\xa3\xff\xff\xff", "\x80ab", '0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789chars after 72 are ignored',
);
$salts = array( '/OK.fbVrR/bpIqNJ5ianF.', 'abcdefghijklmnopqrstuu', '..........................', 'ZZZZZZZZZZZZZZZZZZZZZZ', 'abcdefghijklmnopqrstuv', 'abcdefghijklmnopqrstu.', '012345678901234567890/', 'abcdefghijklmnopqrstu', 'abcdefghijklmnopqrst!u', 'abcdefghijklmnopqrstuu$extra' );
$settings = array();
foreach ( array( '2a', '2b', '2x', '2y' ) as $variant ) {
	foreach ( $salts as $salt ) {
		$settings[] = '$' . $variant . '$04$' . $salt;
	}
}
foreach ( array( '$2y$05$abcdefghijklmnopqrstuu', '$2y$4$abcdefghijklmnopqrstuu', '$2y$03$abcdefghijklmnopqrstuu', '$2y$32$abcdefghijklmnopqrstuu', '$2y$0a$abcdefghijklmnopqrstuu', '$2y$0:$abcdefghijklmnopqrstuu', '$2z$04$abcdefghijklmnopqrstuu', '$2y$04abcdefghijklmnopqrstuu', '$2$04$abcdefghijklmnopqrstuu', '$2y$', '' ) as $setting ) {
	$settings[] = $setting;
}

function out( ...$fields ) {
	echo implode( "\t", $fields ), "\n";
}

foreach ( $passwords as $password ) {
	if ( strlen( $password ) > 200 ) {
		continue;
	}
	foreach ( $settings as $setting ) {
		out( 'crypt', bin2hex( $password ), $setting, crypt( $password, $setting ) );
	}
}

$phpass = new PasswordHash( 8, true );
foreach ( $passwords as $password ) {
	$hashes = array(
		md5( $password ),
		strtoupper( md5( $password ) ),
		md5( $password ) . 'x',
		substr( md5( $password ), 0, 31 ),
		$phpass->HashPassword( $password ),
		'$P$9abcdefgh' . substr( $phpass->HashPassword( $password ), 12 ),
		'$wp' . crypt( base64_encode( hash_hmac( 'sha384', $password, 'wp-sha384', true ) ), '$2y$04$abcdefghijklmnopqrstuu' ),
		'$wp' . crypt( base64_encode( hash_hmac( 'sha384', trim( $password ), 'wp-sha384', true ) ), '$2y$04$abcdefghijklmnopqrstuu' ),
		'$wp' . crypt( $password, '$2y$04$abcdefghijklmnopqrstuu' ),
		crypt( $password, '$2y$04$abcdefghijklmnopqrstuu' ),
		crypt( $password, '$2a$04$abcdefghijklmnopqrstuu' ),
		crypt( $password, '$2x$04$abcdefghijklmnopqrstuu' ),
		crypt( $password, '$2b$04$abcdefghijklmnopqrstuu' ),
		crypt( $password, '$2y$04$abcdefghijklmnopqrstuu' ) . 'x',
		'$2y$04$abcdefghijklmnopqrstuv' . substr( crypt( $password, '$2y$04$abcdefghijklmnopqrstuu' ), 29 ),
		crypt( $password, '$1$abcdefgh$' ),
		crypt( $password, '$6$abcdefgh$' ),
		password_hash( $password, PASSWORD_ARGON2ID ),
	);
	foreach ( $hashes as $hash ) {
		out( 'check', bin2hex( $password ), bin2hex( $hash ), wp_check_password( $password, $hash ) ? 1 : 0 );
	}
	if ( strlen( $password ) <= 4096 ) {
		$hash = wp_hash_password( $password );
		out( 'hash', bin2hex( $password ), $hash, wp_check_password( $password, $hash ) ? 1 : 0 );
	}
}
foreach ( array( '', '$P$', '$P$Babcdefgh', '$P$babcdefgh0123456789012345678901', '$P$Babcdefg', '$H$9abcdefghIJZBHIGGcyLs2c3Xb5JJW/', '*0', '*1' ) as $hash ) {
	out( 'check', bin2hex( 'password' ), bin2hex( $hash ), wp_check_password( 'password', $hash ) ? 1 : 0 );
}

foreach ( array(
	'$wp$2y$12$abcdefghijklmnopqrstuuXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
	'$wp$2y$10$abcdefghijklmnopqrstuuXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
	'$wp$2y$12$abcdefghijklmnopqrstuuXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
	'$wp$2b$12$abcdefghijklmnopqrstuuXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
	'$wp$2y$+12$abcdefghijklmnopqrstuuXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
	'$wp$2y$012$abcdefghijklmnopqrstuuXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
	'$wp$2y$-12$abcdefghijklmnopqrstuuXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
	'$wp$2y$+10$abcdefghijklmnopqrstuuXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
	'$wp$2y$99999999999999999999$abcdefghijklmnopqrstuuXXXXXXXXXXXX',
	'$wp$2y$12x$abcdefghijklmnopqrstuuXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
	'$wp$2y$ 12$abcdefghijklmnopqrstuuXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
	'$wp$2y$$abcdefghijklmnopqrstuuXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
	'$wp$argon2id$v=19$m=65536,t=4,p=1$abc$def',
	'$2y$12$abcdefghijklmnopqrstuuXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
	'$P$Babcdefgh0123456789012345678901',
	'5f4dcc3b5aa765d61d8327deb882cf99',
	'$wp',
	'',
) as $hash ) {
	out( 'rehash', bin2hex( $hash ), wp_password_needs_rehash( $hash ) ? 1 : 0 );
}

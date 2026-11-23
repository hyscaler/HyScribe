<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for hyscribe-auth's wp_fast_hash(): WordPress's own function, from
// a WordPress 7.1.3 tree, over messages that cross BLAKE2b's 128 byte
// blocks. Lines are hex(message) <TAB> hash.
//   php fast-hash-golden.php <wordpress> > crates/hyscribe-auth/tests/golden/fast-hash.tsv

$source = file_get_contents( rtrim( $argv[1], '/' ) . '/wp-includes/functions.php' );
preg_match( '/\nfunction wp_fast_hash\(.*?\n}\n/s', $source, $found );
eval( $found[0] );

$messages = array( '', 'a', 'abcdefghijklmnopqrst', "\0\xff key" );
foreach ( array( 127, 128, 129, 255, 256, 257, 1000 ) as $length ) {
	$messages[] = substr( str_repeat( 'The quick brown fox jumps over the lazy dog. ', 30 ), 0, $length );
}
foreach ( $messages as $message ) {
	echo bin2hex( $message ), "\t", wp_fast_hash( $message ), "\n";
}

<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Runs one wp-config.php the way WordPress would and prints what it
// defined: one line per constant, "name<TAB>type<TAB>hex(value)", plus
// $table_prefix. Used by golden.sh; never by HyScribe.

error_reporting( E_ALL & ~E_WARNING );
$file = $argv[1];

$abspath = sys_get_temp_dir() . '/hyscribe-wpconfig-' . getmypid() . '/';
@mkdir( $abspath );
file_put_contents( $abspath . 'wp-settings.php', "<?php\n" );
define( 'ABSPATH', $abspath );

$before = get_defined_constants( true )['user'] ?? array();
ob_start();
require $file;
ob_end_clean();
$after = get_defined_constants( true )['user'] ?? array();

$out = array();
foreach ( $after as $name => $value ) {
	if ( array_key_exists( $name, $before ) ) {
		continue;
	}
	$out[] = array( $name, $value );
}
if ( isset( $table_prefix ) ) {
	$out[] = array( '$table_prefix', $table_prefix );
}
foreach ( $out as list( $name, $value ) ) {
	if ( is_string( $value ) ) {
		$type = 's';
		$hex  = bin2hex( $value );
	} elseif ( is_int( $value ) ) {
		$type = 'i';
		$hex  = bin2hex( (string) $value );
	} elseif ( is_bool( $value ) ) {
		$type = 'b';
		$hex  = $value ? '31' : '30';
	} elseif ( null === $value ) {
		$type = 'n';
		$hex  = '';
	} else {
		$type = 'x';
		$hex  = '';
	}
	echo bin2hex( $name ), "\t", $type, "\t", $hex, "\n";
}

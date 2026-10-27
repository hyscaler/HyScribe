<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prints wpdb::parse_db_host() for each DB_HOST value on stdin:
// "hex(input)<TAB>hex(host)<TAB>port<TAB>hex(socket)<TAB>ipv6", or
// "hex(input)<TAB>false". Argument: a WordPress folder.

define( 'ABSPATH', rtrim( $argv[1], '/' ) . '/' );
define( 'WPINC', 'wp-includes' );
require ABSPATH . 'wp-includes/class-wpdb.php';

// As in wp-includes/functions.php, which class-wpdb.php expects loaded.
function absint( $maybeint ) {
	return abs( (int) $maybeint );
}

$wpdb = ( new ReflectionClass( 'wpdb' ) )->newInstanceWithoutConstructor();
while ( false !== ( $line = fgets( STDIN ) ) ) {
	$input  = rtrim( $line, "\n" );
	$result = $wpdb->parse_db_host( $input );
	if ( false === $result ) {
		echo bin2hex( $input ), "\tfalse\n";
		continue;
	}
	list( $host, $port, $socket, $is_ipv6 ) = $result;
	echo bin2hex( $input ), "\t", bin2hex( (string) $host ), "\t", null === $port ? '-' : $port, "\t",
		null === $socket ? '-' : bin2hex( $socket ), "\t", $is_ipv6 ? '1' : '0', "\n";
}

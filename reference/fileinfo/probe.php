<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// What `finfo_file( FILEINFO_MIME_TYPE )` says of each file of a folder:
// name<TAB>mime, in name order (golden.sh runs it in the oracle's image).

$finfo = finfo_open( FILEINFO_MIME_TYPE );
$names = scandir( $argv[1] );
foreach ( $names as $name ) {
	if ( '.' !== $name[0] ) {
		printf( "%s\t%s\n", $name, finfo_file( $finfo, $argv[1] . '/' . $name ) );
	}
}

<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Run by reference/site-editor/page-golden.sh, with plain PHP: prints the
// golden rows of one site editor page (its path, the nonces file of the
// request's user, the case's name, and a directory to keep the masked page
// in, if any). The JSON values are kept as the page prints them, never
// decoded and encoded again.

list( , $path, $nonces, $case ) = $argv;
$page = file_get_contents( $path );
foreach ( file( $nonces, FILE_IGNORE_NEW_LINES ) as $line ) {
	list( $nonce, $action ) = explode( "\t", $line );
	$page = str_replace( $nonce, "%nonce:$action%", $page );
}
$page = preg_replace( '/"time":"\d+"/', '"time":"%time%"', $page );

// The end of the JSON value starting at $at.
function json_end( $text, $at ) {
	$depth = 0;
	for ( $i = $at, $n = strlen( $text ); $i < $n; $i++ ) {
		$c = $text[ $i ];
		if ( '"' === $c ) {
			for ( $i++; '"' !== $text[ $i ]; $i++ ) {
				if ( '\\' === $text[ $i ] ) {
					$i++;
				}
			}
		} elseif ( '[' === $c || '{' === $c ) {
			$depth++;
		} elseif ( ']' === $c || '}' === $c ) {
			if ( 0 === $depth ) {
				return $i;
			}
			if ( 0 === --$depth ) {
				return $i + 1;
			}
		} elseif ( 0 === $depth && ( ',' === $c || ')' === $c || ' ' === $c || ';' === $c || ':' === $c ) ) {
			return $i;
		}
	}
	return $n;
}

// The JSON value printed after $marker.
function json_after( $page, $marker ) {
	$at = strpos( $page, $marker );
	if ( false === $at ) {
		return null;
	}
	$at += strlen( $marker );
	return substr( $page, $at, json_end( $page, $at ) - $at );
}

// An object's members as the page prints them: key => raw JSON.
function members( $object ) {
	$out = array();
	$at  = 1;
	while ( '}' !== $object[ $at ] ) {
		$end         = json_end( $object, $at );
		$key         = json_decode( substr( $object, $at, $end - $at ) );
		$at          = $end + 1;
		$end         = json_end( $object, $at );
		$out[ $key ] = substr( $object, $at, $end - $at );
		$at          = ',' === $object[ $end ] ? $end + 1 : $end;
	}
	return $out;
}

function row( $case, $part, $value ) {
	if ( strlen( $value ) > 2000 && 'html' !== $part ) {
		$value = 'sha256:' . hash( 'sha256', $value ) . ' ' . strlen( $value );
	}
	echo "$case\t$part\t$value\n";
}

$settings = json_after( $page, 'wp.editSite.initializeEditor( "site-editor", ' );
foreach ( members( $settings ) as $key => $value ) {
	row( $case, "settings.$key", $value );
}
$parts = array(
	'categories' => 'wp.blocks.setCategories( ',
	'bindings'   => 'for ( const source of ',
	'blocks'     => 'wp.blocks.unstable__bootstrapServerSideBlockDefinitions(',
);
foreach ( $parts as $part => $marker ) {
	$parts[ $part ] = array( $marker, json_after( $page, $marker ) );
	row( $case, $part, $parts[ $part ][1] );
}

// The preloaded responses, which the REST goldens hold, by path; the page
// with them, the settings and the parts above left out.
$marker   = 'wp.apiFetch.createPreloadingMiddleware( ';
$preloads = json_after( $page, $marker );
$keys     = array();
foreach ( members( $preloads ) as $key => $value ) {
	$keys = array_merge( $keys, 'OPTIONS' === $key ? array_map( fn( $k ) => "OPTIONS $k", array_keys( members( $value ) ) ) : array( "GET $key" ) );
}
row( $case, 'preloads', implode( ' ', $keys ) );
$whole = str_replace( $marker . $preloads, $marker . '%preloads%', $page );
$whole = str_replace( $settings, '%settings%', $whole );
foreach ( $parts as $part => list( $marker, $value ) ) {
	$whole = str_replace( $marker . $value, "$marker%$part%", $whole );
}
row( $case, 'html', bin2hex( $whole ) );
if ( isset( $argv[4] ) ) {
	file_put_contents( "$argv[4]/$case.masked.html", $whole );
}

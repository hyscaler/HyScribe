<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Run by reference/post-edit/editor-golden.sh, with plain PHP: prints the
// golden rows of one block editor page (its path, the nonces file of the
// request's user, the case's name). The JSON values are kept as the page
// prints them, never decoded and encoded again.

list( , $path, $nonces, $case ) = $argv;
$page = file_get_contents( $path );
foreach ( file( $nonces, FILE_IGNORE_NEW_LINES ) as $line ) {
	list( $nonce, $action ) = explode( "\t", $line );
	$page = str_replace( $nonce, "%nonce:$action%", $page );
}
$page = preg_replace( '/"activePostLock":"\d+:/', '"activePostLock":"%time%:', $page );

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
	if ( strlen( $value ) > 2000 && ! in_array( $part, array( 'body', 'head', 'footer' ), true ) ) {
		$value = 'sha256:' . hash( 'sha256', $value ) . ' ' . strlen( $value );
	}
	echo "$case\t$part\t$value\n";
}

preg_match( '/initializeEditor\( \'editor\', "([a-z_]+)", (\d+), /', $page, $start );
row( $case, 'post', "$start[1] $start[2]" );
$settings = json_after( $page, $start[0] );
foreach ( members( $settings ) as $key => $value ) {
	row( $case, "settings.$key", $value );
}
$at = strpos( $page, $start[0] ) + strlen( $start[0] ) + strlen( $settings ) + 2;
row( $case, 'edits', substr( $page, $at, json_end( $page, $at ) - $at ) );
row( $case, 'categories', json_after( $page, 'wp.blocks.setCategories( ' ) );
row( $case, 'bindings', json_after( $page, 'for ( const source of ' ) );
row( $case, 'blocks', json_after( $page, 'wp.blocks.unstable__bootstrapServerSideBlockDefinitions(' ) );
$from = strpos( $page, '<div class="block-editor">' );
$to   = strpos( $page, '<div class="clear"></div></div><!-- wpbody-content -->' );
row( $case, 'body', bin2hex( substr( $page, $from, $to - $from ) ) );

// The style and script handles printed, in order: a concatenated file's
// handles, a tag's handle, and an inline tag's handle and position.
function handles( $html ) {
	preg_match_all( '/<(link|style|script)\b[^>]*>/', $html, $tags );
	$out = array();
	foreach ( $tags[0] as $tag ) {
		if ( preg_match( '/load-(styles|scripts)\.php\?[^\'"]*/', $tag, $m ) ) {
			preg_match_all( '/load%5Bchunk_\d+%5D=([^&]*)/', html_entity_decode( $m[0] ), $chunks );
			$out[] = $m[1] . ':' . rtrim( implode( '', $chunks[1] ), ',' );
		} elseif ( preg_match( '/ id=[\'"]([^\'"]+?)(-css|-js|-inline-css|-js-extra|-js-before|-js-after|-js-module)[\'"]/', $tag, $m ) ) {
			$out[] = $m[1] . $m[2];
		}
	}
	return implode( ' ', $out );
}
$head = strpos( $page, '</head>' );
$foot = strpos( $page, '<div id="wpfooter"' );
row( $case, 'head', handles( substr( $page, 0, $head ) ) );
row( $case, 'footer', handles( substr( $page, $foot ) ) );

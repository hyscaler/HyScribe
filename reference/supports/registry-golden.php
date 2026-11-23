<?php
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Oracle for hyscribe-render's block type registry. Prints:
//   v<TAB>hex(schema JSON)<TAB>hex(value JSON)<TAB>0|1
//     whether rest_validate_value_from_schema() accepts the value, once
//     per distinct schema and value
//   p<TAB>hex(block)<TAB>hex(attrs JSON)<TAB>hex(serialize(prepared))
//     prepare_attributes_for_render() for the wrapper attribute cases
// Run through registry-golden.sh inside the PHP WordPress oracle.

require '/var/www/html/wp-load.php';

$seen   = array();
$values = array( null, true, false, 0, 1, 2, -1, 1.5, 0.0, '', '0', '1', '2', 'true', 'FALSE', 'abc', ' 7 ', '+3', '1.0', '1e2', '1,2', 'a, b', array(), array( 1, 2 ), array( 'a' => 1 ), array( 'x', 'y' ), array( array( 1 ) ) );
foreach ( WP_Block_Type_Registry::get_instance()->get_all_registered() as $name => $type ) {
	if ( ! str_starts_with( $name, 'core/' ) ) {
		continue;
	}
	foreach ( (array) $type->attributes as $attribute => $schema ) {
		$candidates = $values;
		foreach ( $schema['enum'] ?? array() as $enum ) {
			$candidates[] = $enum;
		}
		if ( isset( $schema['default'] ) ) {
			$candidates[] = $schema['default'];
		}
		foreach ( $candidates as $value ) {
			$key = wp_json_encode( $schema ) . "\0" . wp_json_encode( $value, JSON_PRESERVE_ZERO_FRACTION );
			if ( isset( $seen[ $key ] ) ) {
				continue;
			}
			$seen[ $key ] = true;
			$valid        = ! is_wp_error( @rest_validate_value_from_schema( $value, $schema, $attribute ) );
			echo "v\t", bin2hex( wp_json_encode( $schema ) ), "\t", bin2hex( wp_json_encode( $value, JSON_PRESERVE_ZERO_FRACTION ) ), "\t", $valid ? '1' : '0', "\n";
		}
	}
}
// Synthetic schemas for the types and keywords core blocks do not use.
$synthetic = array(
	array( 'type' => 'null' ),
	array( 'type' => array( 'string', 'null' ) ),
	array( 'type' => array( 'null', 'integer' ) ),
	array( 'type' => array( 'array', 'object' ) ),
	array( 'type' => array( 'object', 'array' ) ),
	array( 'type' => array( 'number', 'boolean' ) ),
	array( 'type' => array( 'boolean', 'string' ) ),
	array( 'type' => array( 'integer', 'rich-text' ) ),
	array( 'type' => array( 'rich-text' ) ),
	array( 'type' => array( 'integer' ) ),
	array( 'type' => 'number', 'minimum' => 1 ),
	array( 'type' => 'number', 'maximum' => 1 ),
	array( 'type' => 'integer', 'minimum' => 0, 'maximum' => 5 ),
	array( 'type' => 'number', 'enum' => array( 1, 2.5 ) ),
	array( 'type' => 'integer', 'enum' => array( 1, 2 ) ),
	array( 'type' => 'boolean', 'enum' => array( true ) ),
	array( 'type' => 'null', 'enum' => array( null ) ),
	array( 'type' => 'array', 'enum' => array( array( 1, 2 ), array( 'x' ) ) ),
	array( 'type' => 'array', 'items' => array( 'type' => 'integer' ) ),
	array( 'type' => 'array', 'items' => array( 'type' => 'string', 'enum' => array( 'x', 'y' ) ) ),
	array( 'type' => 'object', 'enum' => array( array( 'a' => 1 ) ) ),
	array( 'type' => 'string', 'enum' => array( '1', 'Array', '' ) ),
	array( 'type' => array( 'string', 'integer' ), 'enum' => array( 1, '2' ) ),
	array( 'type' => 'rich-text', 'enum' => array( 'x' ) ),
	array( 'enum' => array( 'x' ) ),
	array( 'type' => 'integer', 'enum' => array( 1.0, 2.5 ) ),
	array( 'type' => 'number', 'enum' => array( 1, 3 ) ),
	array( 'type' => array( 1, 'string' ) ),
	array( 'type' => 'string', 'enum' => array() ),
	array( 'type' => 'array', 'enum' => array( array( 1 ), array( '1', '2' ) ) ),
	array( 'type' => 'number', 'enum' => array( 1.5, 2.0 ) ),
);
$extra = array( '.', '-.', '+', '1e+2', '1e-2', '2E3', ' 1e2 ', '1e+', array( 2 => 1 ), 3, 5, 6, -0.5, 2.5, '2.5', ' 1', '1 ', '0x1A', '.5', '5.', '-', '1e', 'Array', array( 'x' ), array( 1, 'a' ), array( 2 => 1 ), array( 1, 2.0 ) );
foreach ( $synthetic as $schema ) {
	foreach ( array_merge( $values, $extra ) as $value ) {
		$valid = ! is_wp_error( @rest_validate_value_from_schema( $value, $schema, 'x' ) );
		echo "v\t", bin2hex( wp_json_encode( $schema ) ), "\t", bin2hex( wp_json_encode( $value, JSON_PRESERVE_ZERO_FRACTION ) ), "\t", $valid ? '1' : '0', "\n";
	}
}

$golden = file( '/tmp/wrapper-attributes.tsv', FILE_IGNORE_NEW_LINES );
foreach ( $golden as $line ) {
	list( $name, $json ) = explode( "\t", $line );
	$name  = hex2bin( $name );
	$attrs = json_decode( hex2bin( $json ), true );
	$type  = WP_Block_Type_Registry::get_instance()->get_registered( $name );
	$prepared = $type ? @$type->prepare_attributes_for_render( $attrs ) : $attrs;
	echo "p\t", bin2hex( $name ), "\t", bin2hex( hex2bin( $json ) ), "\t", bin2hex( serialize( $prepared ) ), "\n";
}
